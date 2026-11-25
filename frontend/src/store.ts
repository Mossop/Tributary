import * as api from "./api.ts";
import type {
  Account,
  AccountProvider,
  Context,
  Item,
  ServerEvent,
  Source,
  SourceType,
} from "./api.ts";
import type { Route } from "./route.ts";

/** How often to tell the server we're active and reload while visible. */
export const ACTIVITY_INTERVAL_MS = 60_000;

export interface State {
  contexts: Context[];
  sources: Source[];
  sourceTypes: SourceType[];
  accounts: Account[];
  providers: AccountProvider[];
  /** Items for the current route (active or snoozed). */
  items: Item[];
  route: Route;
  loaded: boolean;
  refreshing: boolean;
  error: string | null;
}

export interface EventSourceLike {
  onmessage: ((event: MessageEvent) => void) | null;
  close(): void;
}

export interface StoreOptions {
  connect?: (url: string) => EventSourceLike;
}

/**
 * Holds the dashboard's state and keeps it in sync with the server.
 * Dispatches a `change` event whenever the state changes.
 */
export class Store extends EventTarget {
  state: State = {
    contexts: [],
    sources: [],
    sourceTypes: [],
    accounts: [],
    providers: [],
    items: [],
    route: { view: "items", context: null },
    loaded: false,
    refreshing: false,
    error: null,
  };

  private connect: (url: string) => EventSourceLike;
  private events: EventSourceLike | null = null;
  private timer: ReturnType<typeof setInterval> | null = null;
  private loadSequence = 0;
  /** The most recently viewed context, to return to from settings. */
  private lastContext: string | null = null;
  private onVisibilityChange = () => {
    if (document.visibilityState == "visible") {
      this.tick();
    }
  };

  constructor(options: StoreOptions = {}) {
    super();
    this.connect = options.connect ?? ((url) => new EventSource(url));
  }

  private update(changes: Partial<State>) {
    this.state = { ...this.state, ...changes };
    this.dispatchEvent(new Event("change"));
  }

  private fail(error: unknown) {
    this.update({ error: error instanceof Error ? error.message : `${error}` });
  }

  async start(route: Route) {
    this.events = this.connect("/api/events");
    this.events.onmessage = (event) =>
      this.handleEvent(JSON.parse(event.data) as ServerEvent);
    this.timer = setInterval(() => {
      if (document.visibilityState == "visible") {
        this.tick();
      }
    }, ACTIVITY_INTERVAL_MS);
    document.addEventListener("visibilitychange", this.onVisibilityChange);

    api.pingActivity().catch(() => {});
    try {
      let [contexts, sources, sourceTypes, accounts, providers] =
        await Promise.all([
          api.getContexts(),
          api.getSources(),
          api.getSourceTypes(),
          api.getAccounts(),
          api.getAccountProviders(),
        ]);
      this.update({ contexts, sources, sourceTypes, accounts, providers });
    } catch (error) {
      this.fail(error);
      return;
    }
    if (route.view != "settings" && route.context == null) {
      route = this.homeRoute();
    }
    await this.navigate(route);
    this.update({ loaded: true });
    await this.refresh();
  }

  stop() {
    this.events?.close();
    this.events = null;
    if (this.timer != null) {
      clearInterval(this.timer);
    }
    document.removeEventListener("visibilitychange", this.onVisibilityChange);
  }

  /** The context to show for a route without one. */
  defaultContext(): string | null {
    return this.state.contexts[0]?.id ?? null;
  }

  /** Where to go when leaving settings: the last context viewed, if it
   * still exists, otherwise the default. */
  homeRoute(): Route {
    let last = this.state.contexts.find((c) => c.id == this.lastContext);
    return { view: "items", context: last?.id ?? this.defaultContext() };
  }

  /** Records that a context's id changed, so returning to it still works. */
  contextRenamed(from: string, to: string) {
    if (this.lastContext == from) {
      this.lastContext = to;
    }
  }

  /** The sources shown in the current context. */
  contextSources(): Source[] {
    let context = this.state.route.context;
    return this.state.sources.filter(
      (s) => context != null && s.contexts.includes(context),
    );
  }

  async navigate(route: Route) {
    if (route.context != null) {
      this.lastContext = route.context;
    }
    this.update({ route, items: [] });
    await this.loadItems();
  }

  async loadItems() {
    let { context, view } = this.state.route;
    if (context == null || view == "settings") {
      return;
    }
    let sequence = ++this.loadSequence;
    try {
      let items = await (view == "snoozed"
        ? api.getSnoozed(context)
        : api.getItems(context));
      // Ignore responses that were overtaken by a later load.
      if (sequence == this.loadSequence) {
        this.update({ items, error: null });
      }
    } catch (error) {
      this.fail(error);
    }
  }

  async loadSources() {
    try {
      this.update({ sources: await api.getSources() });
    } catch (error) {
      this.fail(error);
    }
  }

  async refresh() {
    let context = this.state.route.context;
    if (context == null || this.state.refreshing) {
      return;
    }
    this.update({ refreshing: true });
    try {
      await api.refreshContext(context);
    } catch (error) {
      this.fail(error);
    }
    this.update({ refreshing: false });
    await Promise.all([this.loadSources(), this.loadItems()]);
  }

  private async mutate(action: Promise<void>) {
    try {
      await action;
    } catch (error) {
      this.fail(error);
    }
    await this.loadItems();
  }

  snooze(item: Item, until: Date | null) {
    return this.mutate(api.snooze(item, until));
  }

  unsnooze(item: Item) {
    return this.mutate(api.unsnooze(item));
  }

  setDue(item: Item, due: Date) {
    return this.mutate(api.setDue(item, due));
  }

  clearDue(item: Item) {
    return this.mutate(api.clearDue(item));
  }

  /** Reloads contexts, sources and accounts, leaving a context that no
   * longer exists. */
  async loadSettings() {
    try {
      let [contexts, sources, accounts] = await Promise.all([
        api.getContexts(),
        api.getSources(),
        api.getAccounts(),
      ]);
      this.update({ contexts, sources, accounts });
    } catch (error) {
      this.fail(error);
      return;
    }
    let { context } = this.state.route;
    if (context != null && !this.state.contexts.some((c) => c.id == context)) {
      await this.navigate(this.homeRoute());
    } else {
      await this.loadItems();
    }
  }

  handleEvent(event: ServerEvent) {
    if (event.type == "settings-changed") {
      this.loadSettings();
      return;
    }
    let inContext = this.contextSources().some((s) => s.id == event.source_id);
    if (event.type == "source-updated") {
      this.loadSources();
    }
    if (inContext) {
      this.loadItems();
    }
  }

  /** Periodic work while the dashboard is visible. */
  private tick() {
    api.pingActivity().catch(() => {});
    // Picks up snoozes that have expired.
    this.loadItems();
  }
}
