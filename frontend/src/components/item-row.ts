import { LitElement, css, html, nothing } from "lit";
import { customElement, property } from "lit/decorators.js";
import "@awesome.me/webawesome/dist/components/button/button.js";
import "@awesome.me/webawesome/dist/components/dropdown/dropdown.js";
import "@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js";
import "@awesome.me/webawesome/dist/components/divider/divider.js";
import "@awesome.me/webawesome/dist/components/icon/icon.js";
import "@awesome.me/webawesome/dist/components/tag/tag.js";

import type { Item, QuickAction, Source } from "../api.ts";
import {
  absoluteTime,
  effectiveDue,
  isOverdue,
  linkify,
  relativeTime,
  snoozePresets,
} from "../format.ts";
import { parseIcon } from "../icons.ts";

export type ItemAction =
  | { action: "snooze"; item: Item; until: Date | null }
  | { action: "snooze-custom"; item: Item }
  | { action: "unsnooze"; item: Item }
  | { action: "due-custom"; item: Item }
  | { action: "clear-due"; item: Item }
  | { action: "quick"; item: Item; id: string };

/** Fired (bubbling, composed) when the user picks an action for an item. */
export class ItemActionEvent extends Event {
  constructor(public detail: ItemAction) {
    super("item-action", { bubbles: true, composed: true });
  }
}

@customElement("item-row")
export class ItemRow extends LitElement {
  @property({ attribute: false }) item!: Item;
  @property({ attribute: false }) source?: Source;
  /** Shown in the snoozed view. */
  @property({ type: Boolean }) snoozed = false;
  /** A quick action is in progress. */
  @property({ type: Boolean }) busy = false;
  /** Shown under a related item, as part of its group. */
  @property({ type: Boolean, reflect: true }) nested = false;
  @property({ attribute: false }) now = new Date();

  static styles = css`
    :host {
      display: flex;
      align-items: center;
      gap: var(--wa-space-m);
      padding: var(--wa-space-s) var(--wa-space-m);
    }

    /* A related item, shown under its group's lead. */
    :host([nested]) {
      position: relative;
      padding-left: calc(2 * var(--wa-space-m) + 24px);
    }

    /* A divider above it, starting at its indent. */
    :host([nested])::before {
      content: "";
      position: absolute;
      top: 0;
      left: calc(2 * var(--wa-space-m) + 24px);
      right: 0;
      border-top: var(--wa-border-width-s) solid var(--wa-color-surface-border);
    }

    .icon {
      flex: none;
      width: 24px;
      height: 24px;
      font-size: 24px;
      color: var(--wa-color-text-quiet);
    }

    .main {
      flex: 1;
      min-width: 0;
    }

    .title {
      display: block;
      color: var(--wa-color-text-normal);
      font-weight: var(--wa-font-weight-semibold);
      text-decoration: none;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .title:hover {
      text-decoration: underline;
    }

    .meta {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: var(--wa-space-xs) var(--wa-space-s);
      color: var(--wa-color-text-quiet);
      font-size: var(--wa-font-size-s);
    }

    .description a {
      color: inherit;
      overflow-wrap: anywhere;
    }

    .actions {
      flex: none;
      display: flex;
    }

    /* wa-tag lays its content out inline with a tall line height, which
       leaves icons on the text's baseline rather than centred on it. */
    .tag-content {
      display: inline-flex;
      align-items: center;
      gap: var(--wa-space-2xs);
      line-height: 1;
    }
  `;

  private fire(action: ItemAction) {
    this.dispatchEvent(new ItemActionEvent(action));
  }

  private renderIcon() {
    let icon = this.item.icon ?? this.source?.icon;
    if (!icon) {
      return html`<span class="icon"></span>`;
    }
    let parsed = parseIcon(icon);
    if ("symbol" in parsed) {
      return html`<wa-icon class="icon" name=${parsed.symbol}></wa-icon>`;
    }
    return html`<img class="icon" src=${parsed.url} alt="" />`;
  }

  private renderDue() {
    let due = effectiveDue(this.item);
    if (!due) {
      return nothing;
    }
    let overdue = isOverdue(this.item, this.now);
    return html`<wa-tag
      size="s"
      variant=${overdue ? "danger" : "neutral"}
      title=${absoluteTime(due)}
    >
      <span class="tag-content">
        <wa-icon
          name=${this.item.due_override ? "edit_calendar" : "event"}
        ></wa-icon>
        Due ${relativeTime(due, this.now)}
      </span>
    </wa-tag>`;
  }

  private renderSnoozedUntil() {
    if (this.item.snoozed_forever) {
      return html`<span>Snoozed forever</span>`;
    }
    if (this.item.snoozed_until) {
      let until = new Date(this.item.snoozed_until);
      return html`<span title=${absoluteTime(until)}
        >Snoozed until ${absoluteTime(until)}</span
      >`;
    }
    return nothing;
  }

  private renderQuickAction(action: QuickAction) {
    let icon = parseIcon(action.icon);
    let content =
      "symbol" in icon
        ? html`<wa-icon name=${icon.symbol} label=${action.label}></wa-icon>`
        : html`<img class="icon" src=${icon.url} alt=${action.label} />`;
    if (action.url) {
      // A link: opened by the browser, nothing for the server to do.
      return html`<wa-button
        appearance="plain"
        title=${action.label}
        href=${action.url}
        target="_blank"
        rel="noopener noreferrer"
        >${content}</wa-button
      >`;
    }
    return html`<wa-button
      appearance="plain"
      title=${action.label}
      ?loading=${this.busy}
      ?disabled=${this.busy}
      @click=${() =>
        this.fire({ action: "quick", item: this.item, id: action.id })}
    >
      ${content}
    </wa-button>`;
  }

  private renderActions() {
    if (this.snoozed) {
      return html`<wa-button
        appearance="plain"
        @click=${() => this.fire({ action: "unsnooze", item: this.item })}
      >
        <wa-icon slot="start" name="notifications_active"></wa-icon>
        Unsnooze
      </wa-button>`;
    }

    let presets = snoozePresets(this.now);
    let onSnooze = (event: CustomEvent<{ item: { value: string } }>) => {
      let value = event.detail.item.value;
      if (value == "custom") {
        this.fire({ action: "snooze-custom", item: this.item });
      } else if (value == "forever") {
        this.fire({ action: "snooze", item: this.item, until: null });
      } else {
        this.fire({
          action: "snooze",
          item: this.item,
          until: presets[Number(value)].until,
        });
      }
    };
    let onDue = (event: CustomEvent<{ item: { value: string } }>) => {
      let value = event.detail.item.value;
      this.fire({
        action: value == "clear" ? "clear-due" : "due-custom",
        item: this.item,
      });
    };

    return html`
      ${this.item.actions.map((action) => this.renderQuickAction(action))}
      <wa-dropdown placement="bottom-end" @wa-select=${onSnooze}>
        <wa-button slot="trigger" appearance="plain" title="Snooze">
          <wa-icon name="snooze" label="Snooze"></wa-icon>
        </wa-button>
        ${presets.map(
          (preset, index) =>
            html`<wa-dropdown-item value=${String(index)}>
              ${preset.label}
              <span slot="details">${absoluteTime(preset.until)}</span>
            </wa-dropdown-item>`,
        )}
        <wa-dropdown-item value="custom">Pick a date…</wa-dropdown-item>
        <wa-divider></wa-divider>
        <wa-dropdown-item value="forever">Forever</wa-dropdown-item>
      </wa-dropdown>
      <wa-dropdown placement="bottom-end" @wa-select=${onDue}>
        <wa-button slot="trigger" appearance="plain" title="Due date">
          <wa-icon name="edit_calendar" label="Due date"></wa-icon>
        </wa-button>
        <wa-dropdown-item value="set">Set due date…</wa-dropdown-item>
        ${
          this.item.due_override
            ? html`<wa-dropdown-item value="clear"
                >Clear due date override</wa-dropdown-item
              >`
            : nothing
        }
      </wa-dropdown>
    `;
  }

  render() {
    let modified = new Date(this.item.modified);
    return html`
      ${this.renderIcon()}
      <div class="main">
        <a
          class="title"
          href=${this.item.url}
          target="_blank"
          rel="noopener noreferrer"
          >${this.item.title}</a
        >
        <div class="meta">
          ${
            this.item.description
              ? html`<span class="description"
                  >${linkify(this.item.description).map((part) =>
                    "url" in part
                      ? html`<a
                          href=${part.url}
                          target="_blank"
                          rel="noopener noreferrer"
                          >${part.url}</a
                        >`
                      : part.text,
                  )}</span
                >`
              : nothing
          }
          <span title=${absoluteTime(modified)}
            >Updated ${relativeTime(modified, this.now)}</span
          >
          ${this.renderDue()}
          ${this.snoozed ? this.renderSnoozedUntil() : nothing}
        </div>
      </div>
      <div class="actions">${this.renderActions()}</div>
    `;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "item-row": ItemRow;
  }
  interface HTMLElementEventMap {
    "item-action": ItemActionEvent;
  }
}
