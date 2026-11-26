import { LitElement, css, html, nothing } from "lit";
import { customElement, query, state } from "lit/decorators.js";
import "@awesome.me/webawesome/dist/components/badge/badge.js";
import "@awesome.me/webawesome/dist/components/button/button.js";
import "@awesome.me/webawesome/dist/components/callout/callout.js";
import "@awesome.me/webawesome/dist/components/dropdown/dropdown.js";
import "@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js";
import "@awesome.me/webawesome/dist/components/icon/icon.js";
import "@awesome.me/webawesome/dist/components/spinner/spinner.js";

import type { Item } from "../api.ts";
import { type Route, parseRoute, routeUrl } from "../route.ts";
import { groupItems } from "../groups.ts";
import { ITEM_KINDS, visibleItems } from "../kinds.ts";
import { Store, type State, itemKey } from "../store.ts";
import type { DateDialog } from "./date-dialog.ts";
import "./date-dialog.ts";
import type { ItemActionEvent } from "./item-row.ts";
import "./item-row.ts";
import "./settings-page.ts";

@customElement("app-shell")
export class AppShell extends LitElement {
  store = new Store();
  @state() private state: State = this.store.state;
  @query("date-dialog") private dateDialog!: DateDialog;

  static styles = css`
    :host {
      display: block;
      max-width: 960px;
      margin: 0 auto;
      padding: var(--wa-space-m);
    }

    header {
      display: flex;
      align-items: center;
      gap: var(--wa-space-s);
      margin-bottom: var(--wa-space-m);
    }

    header h1 {
      font-size: var(--wa-font-size-xl);
      margin: 0;
    }

    .spacer {
      flex: 1;
    }

    .settings {
      position: relative;
    }

    /* Shown on the settings button when something there needs a look. */
    .dot {
      position: absolute;
      top: 6px;
      right: 6px;
      width: 8px;
      height: 8px;
      border-radius: 50%;
      background: var(--wa-color-danger-fill-loud);
      pointer-events: none;
    }

    .list {
      background: var(--wa-color-surface-default);
      border-radius: var(--wa-border-radius-l);
      box-shadow: var(--wa-shadow-s);
      overflow: hidden;
    }

    .group,
    .list > .empty {
      border-top: var(--wa-border-width-s) solid var(--wa-color-surface-border);
    }

    .kinds {
      display: flex;
      gap: var(--wa-space-2xs);
      padding: var(--wa-space-2xs) var(--wa-space-s);
    }

    /* A hidden kind's button is dimmed. */
    .kinds wa-button[aria-pressed="false"] {
      opacity: 0.4;
    }

    .empty {
      padding: var(--wa-space-2xl);
      text-align: center;
      color: var(--wa-color-text-quiet);
    }

    wa-callout {
      margin-bottom: var(--wa-space-m);
    }

    h2 {
      font-size: var(--wa-font-size-l);
      margin: 0 0 var(--wa-space-s);
    }
  `;

  private onChange = () => {
    this.state = this.store.state;
    // Reflect a defaulted or redirected route (e.g. "/" -> "/c/work") in the
    // URL.
    let { route } = this.state;
    let url = routeUrl(route);
    if (
      (route.context != null || route.view == "settings") &&
      url != location.pathname
    ) {
      history.replaceState(null, "", url);
    }
  };

  private onPopState = () => {
    this.store.navigate(this.currentRoute());
  };

  private currentRoute(): Route {
    let route = parseRoute(location.pathname);
    if (route.view != "settings" && route.context == null) {
      return this.store.homeRoute();
    }
    return route;
  }

  connectedCallback() {
    super.connectedCallback();
    this.store.addEventListener("change", this.onChange);
    window.addEventListener("popstate", this.onPopState);
    // A route without a context is defaulted by the store; onChange then
    // updates the URL.
    this.store.start(parseRoute(location.pathname));
  }

  disconnectedCallback() {
    super.disconnectedCallback();
    this.store.removeEventListener("change", this.onChange);
    window.removeEventListener("popstate", this.onPopState);
    this.store.stop();
  }

  private async go(route: Route) {
    history.pushState(null, "", routeUrl(route));
    await this.store.navigate(route);
  }

  private async onItemAction(event: ItemActionEvent) {
    let detail = event.detail;
    let { item } = detail;
    switch (detail.action) {
      case "snooze":
        await this.store.snooze(item, detail.until);
        break;
      case "unsnooze":
        await this.store.unsnooze(item);
        break;
      case "clear-due":
        await this.store.clearDue(item);
        break;
      case "quick":
        await this.store.performAction(item, detail.id);
        break;
      case "snooze-custom": {
        let tomorrow = new Date(Date.now() + 24 * 60 * 60 * 1000);
        let until = await this.dateDialog.pick("Snooze until", tomorrow);
        if (until) {
          await this.store.snooze(item, until);
        }
        break;
      }
      case "due-custom": {
        let current = item.due_override ?? item.due;
        let initial = current
          ? new Date(current)
          : new Date(Date.now() + 24 * 60 * 60 * 1000);
        let due = await this.dateDialog.pick("Due date", initial);
        if (due) {
          await this.store.setDue(item, due);
        }
        break;
      }
    }
  }

  private renderContextSwitcher() {
    let { contexts, route } = this.state;
    let current = contexts.find((c) => c.id == route.context);
    let onSelect = (event: CustomEvent<{ item: { value: string } }>) =>
      this.go({ view: route.view, context: event.detail.item.value });

    return html`<wa-dropdown @wa-select=${onSelect}>
      <wa-button slot="trigger" appearance="plain" with-caret>
        ${
          current?.icon
            ? html`<wa-icon slot="start" name=${current.icon}></wa-icon>`
            : nothing
        }
        ${current?.name ?? "Tributary"}
      </wa-button>
      ${contexts.map(
        (context) =>
          html`<wa-dropdown-item
            value=${context.id}
            type="checkbox"
            ?checked=${context.id == route.context}
          >
            ${
              context.icon
                ? html`<wa-icon slot="icon" name=${context.icon}></wa-icon>`
                : nothing
            }
            ${context.name}
          </wa-dropdown-item>`,
      )}
    </wa-dropdown>`;
  }

  private renderHeader() {
    let { route, refreshing } = this.state;
    if (route.view == "settings") {
      return html`<header>
        <wa-button
          appearance="plain"
          title="Back"
          @click=${() => this.go(this.store.homeRoute())}
        >
          <wa-icon name="arrow_back" label="Back"></wa-icon>
        </wa-button>
        <h1>Settings</h1>
      </header>`;
    }
    return html`<header>
      ${this.renderContextSwitcher()}
      <span class="spacer"></span>
      ${
        route.view == "snoozed"
          ? html`<wa-button
              appearance="plain"
              @click=${() => this.go({ ...route, view: "items" })}
            >
              <wa-icon slot="start" name="arrow_back"></wa-icon>
              Back
            </wa-button>`
          : html`<wa-button
              appearance="plain"
              @click=${() => this.go({ ...route, view: "snoozed" })}
            >
              <wa-icon slot="start" name="snooze"></wa-icon>
              Snoozed
              ${
                this.store.snoozedCount() > 0
                  ? html`<wa-badge slot="end" pill variant="neutral"
                      >${this.store.snoozedCount()}</wa-badge
                    >`
                  : nothing
              }
            </wa-button>`
      }
      <wa-button
        appearance="plain"
        title="Refresh"
        ?loading=${refreshing}
        @click=${() => this.store.refresh()}
      >
        <wa-icon name="refresh" label="Refresh"></wa-icon>
      </wa-button>
      ${this.renderSettingsButton()}
    </header>`;
  }

  private renderSettingsButton() {
    let attention = this.store.settingsAttention();
    let label = attention ? `Settings: ${attention}` : "Settings";
    return html`<span class="settings">
      <wa-button
        appearance="plain"
        title=${label}
        @click=${() => this.go({ view: "settings", context: null })}
      >
        <wa-icon name="settings" label=${label}></wa-icon>
      </wa-button>
      ${attention ? html`<span class="dot"></span>` : nothing}
    </span>`;
  }

  /** Source errors are shown in settings instead, flagged by the dot on the
   * settings button. */
  private renderErrors() {
    if (!this.state.error) {
      return nothing;
    }
    return html`<wa-callout variant="danger">
      <wa-icon slot="icon" name="error"></wa-icon>
      ${this.state.error}
    </wa-callout>`;
  }

  /** Buttons showing and hiding each kind of item. */
  private renderKinds() {
    let hidden = this.store.hiddenKinds();
    return html`<div class="kinds">
      ${ITEM_KINDS.map(({ kind, label, icon }) => {
        let shown = !hidden.includes(kind);
        let title = `${shown ? "Hide" : "Show"} ${label.toLowerCase()}`;
        return html`<wa-button
          appearance="plain"
          size="small"
          title=${title}
          aria-pressed=${shown ? "true" : "false"}
          data-kind=${kind}
          @click=${() => this.store.toggleKind(kind)}
        >
          <wa-icon name=${icon} label=${title}></wa-icon>
        </wa-button>`;
      })}
    </div>`;
  }

  private renderItems() {
    let { items, sources, route, loaded } = this.state;
    let snoozed = route.view == "snoozed";
    if (!loaded) {
      return html`<div class="empty"><wa-spinner></wa-spinner></div>`;
    }
    let shown = visibleItems(items, this.store.hiddenKinds());
    if (items.length > 0 && shown.length == 0) {
      return html`<div class="empty">
        Everything here is hidden by the filters above.
      </div>`;
    }
    if (items.length == 0) {
      if (!snoozed && this.store.contextSources().length == 0) {
        return html`<div class="empty">
          No sources are shown in this context yet. Add one in
          <a
            href="/settings"
            @click=${(e: Event) => {
              e.preventDefault();
              this.go({ view: "settings", context: null });
            }}
            >settings</a
          >.
        </div>`;
      }
      return html`<div class="empty">
        ${snoozed ? "Nothing is snoozed." : "Nothing to do."}
      </div>`;
    }
    let now = new Date();
    let sourcesById = new Map(sources.map((s) => [s.id, s]));
    let row = (item: Item, nested: boolean) =>
      html`<item-row
        .item=${item}
        .source=${sourcesById.get(item.source_id)}
        .now=${now}
        ?snoozed=${snoozed}
        ?nested=${nested}
        ?busy=${this.state.busy.includes(itemKey(item))}
      ></item-row>`;
    // Related items are shown under the group's lead.
    return groupItems(shown).map(
      ([lead, ...related]) =>
        html`<div class="group">
          ${row(lead, false)} ${related.map((item) => row(item, true))}
        </div>`,
    );
  }

  private renderBody() {
    let { route } = this.state;
    if (route.view == "settings") {
      return html`<settings-page
        .store=${this.store}
        .contexts=${this.state.contexts}
        .sources=${this.state.sources}
        .sourceTypes=${this.state.sourceTypes}
        .accounts=${this.state.accounts}
        .providers=${this.state.providers}
      ></settings-page>`;
    }
    return html`
      ${route.view == "snoozed" ? html`<h2>Snoozed</h2>` : nothing}
      <div class="list" @item-action=${this.onItemAction}>
        ${this.renderKinds()} ${this.renderItems()}
      </div>
    `;
  }

  render() {
    return html`
      ${this.renderHeader()} ${this.renderErrors()} ${this.renderBody()}
      <date-dialog></date-dialog>
    `;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "app-shell": AppShell;
  }
}
