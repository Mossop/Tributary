import * as api from "./api.ts";
import type { Context, Item, ServerEvent, Source } from "./api.ts";
import type { Route } from "./route.ts";

/** How often to tell the server we're active and reload while visible. */
export const ACTIVITY_INTERVAL_MS = 60_000;

export interface State {
  contexts: Context[];
  sources: Source[];
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
    items: [],
    route: { context: null, snoozed: false },
    loaded: false,
    refreshing: false,
    error: null,
  };

  private connect: (url: string) => EventSourceLike;
  private events: EventSourceLike | null = null;
  private timer: ReturnType<typeof setInterval> | null = null;
  private loadSequence = 0;
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
      let [contexts, sources] = await Promise.all([
        api.getContexts(),
        api.getSources(),
      ]);
      this.update({ contexts, sources });
    } catch (error) {
      this.fail(error);
      return;
    }
    if (route.context == null) {
      route = { context: this.defaultContext(), snoozed: false };
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

  /** The sources shown in the current context. */
  contextSources(): Source[] {
    let context = this.state.route.context;
    return this.state.sources.filter(
      (s) => context != null && s.contexts.includes(context),
    );
  }

  async navigate(route: Route) {
    this.update({ route, items: [] });
    await this.loadItems();
  }

  async loadItems() {
    let { context, snoozed } = this.state.route;
    if (context == null) {
      return;
    }
    let sequence = ++this.loadSequence;
    try {
      let items = await (snoozed
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

  handleEvent(event: ServerEvent) {
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
