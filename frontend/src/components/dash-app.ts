import { LitElement, css, html, nothing } from "lit";
import { customElement, query, state } from "lit/decorators.js";
import "@awesome.me/webawesome/dist/components/button/button.js";
import "@awesome.me/webawesome/dist/components/callout/callout.js";
import "@awesome.me/webawesome/dist/components/dropdown/dropdown.js";
import "@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js";
import "@awesome.me/webawesome/dist/components/icon/icon.js";
import "@awesome.me/webawesome/dist/components/spinner/spinner.js";

import { type Route, parseRoute, routeUrl } from "../route.ts";
import { Store, type State } from "../store.ts";
import type { DateDialog } from "./date-dialog.ts";
import "./date-dialog.ts";
import type { ItemActionEvent } from "./item-row.ts";
import "./item-row.ts";

@customElement("dash-app")
export class DashApp extends LitElement {
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

    .spacer {
      flex: 1;
    }

    .list {
      background: var(--wa-color-surface-default);
      border-radius: var(--wa-border-radius-l);
      box-shadow: var(--wa-shadow-s);
      overflow: hidden;
    }

    item-row + item-row {
      border-top: var(--wa-border-width-s) solid var(--wa-color-surface-border);
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
    // Reflect a defaulted route (e.g. "/" -> "/c/work") in the URL.
    let url = routeUrl(this.state.route);
    if (this.state.route.context != null && url != location.pathname) {
      history.replaceState(null, "", url);
    }
  };

  private onPopState = () => {
    this.store.navigate(this.currentRoute());
  };

  private currentRoute(): Route {
    let route = parseRoute(location.pathname);
    if (route.context == null) {
      return { context: this.store.defaultContext(), snoozed: false };
    }
    return route;
  }

  connectedCallback() {
    super.connectedCallback();
    this.store.addEventListener("change", this.onChange);
    window.addEventListener("popstate", this.onPopState);
    this.start();
  }

  disconnectedCallback() {
    super.disconnectedCallback();
    this.store.removeEventListener("change", this.onChange);
    window.removeEventListener("popstate", this.onPopState);
    this.store.stop();
  }

  private start() {
    // A route without a context is defaulted by the store; onChange then
    // updates the URL.
    this.store.start(parseRoute(location.pathname));
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
      this.go({ context: event.detail.item.value, snoozed: route.snoozed });

    return html`<wa-dropdown @wa-select=${onSelect}>
      <wa-button slot="trigger" appearance="plain" with-caret>
        ${
          current?.icon
            ? html`<wa-icon slot="start" name=${current.icon}></wa-icon>`
            : nothing
        }
        ${current?.name ?? "Dashboard"}
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
    return html`<header>
      ${this.renderContextSwitcher()}
      <span class="spacer"></span>
      ${
        route.snoozed
          ? html`<wa-button
              appearance="plain"
              @click=${() => this.go({ ...route, snoozed: false })}
            >
              <wa-icon slot="start" name="arrow_back"></wa-icon>
              Back
            </wa-button>`
          : html`<wa-button
              appearance="plain"
              @click=${() => this.go({ ...route, snoozed: true })}
            >
              <wa-icon slot="start" name="snooze"></wa-icon>
              Snoozed
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
    </header>`;
  }

  private renderErrors() {
    let failing = this.store
      .contextSources()
      .filter((s) => s.status.last_error);
    return html`
      ${
        this.state.error
          ? html`<wa-callout variant="danger">
              <wa-icon slot="icon" name="error"></wa-icon>
              ${this.state.error}
            </wa-callout>`
          : nothing
      }
      ${failing.map(
        (source) =>
          html`<wa-callout variant="warning">
            <wa-icon slot="icon" name="warning"></wa-icon>
            Couldn't update ${source.name}: ${source.status.last_error}
          </wa-callout>`,
      )}
    `;
  }

  private renderItems() {
    let { items, sources, route, loaded } = this.state;
    if (!loaded) {
      return html`<div class="empty"><wa-spinner></wa-spinner></div>`;
    }
    if (items.length == 0) {
      return html`<div class="empty">
        ${route.snoozed ? "Nothing is snoozed." : "Nothing to do."}
      </div>`;
    }
    let now = new Date();
    let sourcesById = new Map(sources.map((s) => [s.id, s]));
    return items.map(
      (item) =>
        html`<item-row
          .item=${item}
          .source=${sourcesById.get(item.source_id)}
          .now=${now}
          ?snoozed=${route.snoozed}
        ></item-row>`,
    );
  }

  render() {
    return html`
      ${this.renderHeader()} ${this.renderErrors()}
      ${this.state.route.snoozed ? html`<h2>Snoozed</h2>` : nothing}
      <div class="list" @item-action=${this.onItemAction}>
        ${this.renderItems()}
      </div>
      <date-dialog></date-dialog>
    `;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "dash-app": DashApp;
  }
}
