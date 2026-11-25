import { LitElement, css, html, nothing } from "lit";
import { customElement, property, query, state } from "lit/decorators.js";
import "@awesome.me/webawesome/dist/components/button/button.js";
import "@awesome.me/webawesome/dist/components/callout/callout.js";
import "@awesome.me/webawesome/dist/components/dropdown/dropdown.js";
import "@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js";
import "@awesome.me/webawesome/dist/components/icon/icon.js";

import * as api from "../api.ts";
import type {
  Account,
  AccountProvider,
  Context,
  Source,
  SourceType,
} from "../api.ts";
import { parseIcon } from "../icons.ts";
import { connectError, sourcesUsing } from "../settings-form.ts";
import type { ConfirmDialog } from "./confirm-dialog.ts";
import "./confirm-dialog.ts";
import type { TokenAccountDialog } from "./token-account-dialog.ts";
import "./token-account-dialog.ts";
import type { Store } from "../store.ts";
import type { ContextDialog } from "./context-dialog.ts";
import "./context-dialog.ts";
import type { SourceDialog } from "./source-dialog.ts";
import "./source-dialog.ts";

/** The host of a URL, or the URL itself if it isn't valid. */
function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

/** Manages contexts, accounts and sources. */
@customElement("settings-page")
export class SettingsPage extends LitElement {
  @property({ attribute: false }) store!: Store;
  @property({ attribute: false }) contexts: Context[] = [];
  @property({ attribute: false }) sources: Source[] = [];
  @property({ attribute: false }) sourceTypes: SourceType[] = [];
  @property({ attribute: false }) accounts: Account[] = [];
  @property({ attribute: false }) providers: AccountProvider[] = [];
  @state() private error: string | null = null;
  @query("context-dialog") private contextDialog!: ContextDialog;
  @query("source-dialog") private sourceDialog!: SourceDialog;
  @query("confirm-dialog") private confirmDialog!: ConfirmDialog;
  @query("token-account-dialog") private tokenDialog!: TokenAccountDialog;

  static styles = css`
    :host {
      display: flex;
      flex-direction: column;
      gap: var(--wa-space-l);
    }

    section {
      background: var(--wa-color-surface-default);
      border-radius: var(--wa-border-radius-l);
      box-shadow: var(--wa-shadow-s);
      overflow: hidden;
    }

    .section-header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: var(--wa-space-s) var(--wa-space-m);
    }

    h2 {
      font-size: var(--wa-font-size-l);
      margin: 0;
    }

    .row {
      display: flex;
      align-items: center;
      gap: var(--wa-space-m);
      padding: var(--wa-space-s) var(--wa-space-m);
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
      font-weight: var(--wa-font-weight-semibold);
    }

    .meta {
      color: var(--wa-color-text-quiet);
      font-size: var(--wa-font-size-s);
    }

    .error {
      color: var(--wa-color-danger-on-quiet);
    }

    .warning {
      color: var(--wa-color-warning-on-quiet);
    }

    .empty {
      padding: var(--wa-space-m);
      border-top: var(--wa-border-width-s) solid var(--wa-color-surface-border);
      color: var(--wa-color-text-quiet);
    }
  `;

  connectedCallback() {
    super.connectedCallback();
    // Connecting an account comes back here with any error in the URL.
    let error = connectError(location.search);
    if (error) {
      this.error = error;
      history.replaceState(history.state, "", location.pathname);
    }
  }

  /** Runs a settings change, then reloads settings. Errors propagate to the
   * caller (dialogs show them) after being recorded. */
  private async apply(action: () => Promise<unknown>) {
    await action();
    await this.store.loadSettings();
  }

  private addContext() {
    this.contextDialog.show(null, (context) =>
      this.apply(() => api.createContext(context)),
    );
  }

  private editContext(context: Context) {
    this.contextDialog.show(context, (updated) =>
      this.apply(async () => {
        await api.updateContext(context.id, updated);
        this.store.contextRenamed(context.id, updated.id);
      }),
    );
  }

  private deleteContext(context: Context) {
    this.confirmDialog.ask(
      {
        title: "Delete context",
        message: `Delete the context “${context.name}”? Its sources stay, but are no longer shown in it.`,
        confirm: "Delete",
        danger: true,
      },
      () => this.apply(() => api.deleteContext(context.id)),
    );
  }

  /** Adds an account with `provider`, or reconnects `account`. */
  private connect(provider: string, account?: Account) {
    let info = this.providers.find((p) => p.id == provider);
    if (info?.connect == "token") {
      this.tokenDialog.show(info, account ?? null, (settings) =>
        this.apply(() => api.addTokenAccount(provider, settings)),
      );
    } else {
      location.assign(api.connectUrl(provider, account?.id));
    }
  }

  private deleteAccount(account: Account) {
    let users = sourcesUsing(account, this.sources, this.sourceTypes);
    if (users.length > 0) {
      let names = users.map((s) => `“${s.name}”`).join(", ");
      this.confirmDialog.explain(
        "Account in use",
        `${account.name} can't be removed while these sources use it: ${names}. Change or delete them first.`,
      );
      return;
    }
    this.confirmDialog.ask(
      {
        title: "Remove account",
        message: `Remove the account ${account.name}? You can connect it again later.`,
        confirm: "Remove",
        danger: true,
      },
      () => this.apply(() => api.deleteAccount(account.id)),
    );
  }

  private addSource(type: SourceType) {
    this.sourceDialog.show(
      type,
      null,
      this.contexts,
      this.accounts,
      this.store.homeRoute().context,
      (settings) => this.apply(() => api.createSource(settings)),
    );
  }

  private editSource(source: Source) {
    let type = this.sourceTypes.find((t) => t.id == source.type);
    if (!type) {
      this.error = `Sources of type “${source.type}” can't be edited here.`;
      return;
    }
    this.sourceDialog.show(
      type,
      source,
      this.contexts,
      this.accounts,
      null,
      (settings) => this.apply(() => api.updateSource(source.id, settings)),
    );
  }

  private deleteSource(source: Source) {
    this.confirmDialog.ask(
      {
        title: "Delete source",
        message: `Delete the source “${source.name}”? Its snoozes and due dates will be lost.`,
        confirm: "Delete",
        danger: true,
      },
      () => this.apply(() => api.deleteSource(source.id)),
    );
  }

  private renderContexts() {
    return html`<section>
      <div class="section-header">
        <h2>Contexts</h2>
        <wa-button appearance="plain" @click=${this.addContext}>
          <wa-icon slot="start" name="add"></wa-icon>
          Add context
        </wa-button>
      </div>
      ${this.contexts.map(
        (context) =>
          html`<div class="row">
            <wa-icon class="icon" name=${context.icon ?? "label"}></wa-icon>
            <div class="main">
              <div class="title">${context.name}</div>
            </div>
            <wa-button
              appearance="plain"
              title="Edit"
              @click=${() => this.editContext(context)}
            >
              <wa-icon name="edit" label="Edit"></wa-icon>
            </wa-button>
            <wa-button
              appearance="plain"
              title="Delete"
              ?disabled=${this.contexts.length == 1}
              @click=${() => this.deleteContext(context)}
            >
              <wa-icon name="delete" label="Delete"></wa-icon>
            </wa-button>
          </div>`,
      )}
    </section>`;
  }

  private renderAddAccount() {
    return html`<wa-dropdown
      placement="bottom-end"
      @wa-select=${(e: CustomEvent<{ item: { value: string } }>) =>
        this.connect(e.detail.item.value)}
    >
      <wa-button slot="trigger" appearance="plain" with-caret>
        <wa-icon slot="start" name="add"></wa-icon>
        Add account
      </wa-button>
      ${this.providers.map(
        (provider) =>
          html`<wa-dropdown-item
            value=${provider.id}
            ?disabled=${!provider.configured}
          >
            ${provider.name}
            ${
              provider.configured
                ? nothing
                : html`<span slot="details"
                    >Set up in the server configuration first</span
                  >`
            }
          </wa-dropdown-item>`,
      )}
    </wa-dropdown>`;
  }

  private renderAccountIcon(provider: AccountProvider | undefined) {
    if (!provider) {
      return html`<wa-icon class="icon" name="account_circle"></wa-icon>`;
    }
    let icon = parseIcon(provider.icon);
    return "symbol" in icon
      ? html`<wa-icon class="icon" name=${icon.symbol}></wa-icon>`
      : html`<img class="icon" src=${icon.url} alt="" />`;
  }

  private renderAccounts() {
    let providers = new Map(this.providers.map((p) => [p.id, p]));
    return html`<section>
      <div class="section-header">
        <h2>Accounts</h2>
        ${this.renderAddAccount()}
      </div>
      ${
        this.accounts.length == 0
          ? html`<div class="empty">
              No accounts yet. Some sources need one to get your data.
            </div>`
          : nothing
      }
      ${this.accounts.map(
        (account) =>
          html`<div class="row">
            ${this.renderAccountIcon(providers.get(account.provider))}
            <div class="main">
              <div class="title">${account.name}</div>
              <div class="meta">
                ${providers.get(account.provider)?.name ?? account.provider}
                ${account.api_url ? html` · ${hostOf(account.api_url)}` : nothing}
              </div>
              ${
                account.revoked
                  ? html`<div class="meta warning">
                      Access was revoked or has expired. Reconnect it to keep
                      its sources working.
                    </div>`
                  : account.missing_scopes
                    ? html`<div class="meta warning">
                        Needs reconnecting to give access to every source type.
                      </div>`
                    : nothing
              }
            </div>
            <wa-button
              appearance="plain"
              title="Reconnect"
              @click=${() => this.connect(account.provider, account)}
            >
              <wa-icon name="sync" label="Reconnect"></wa-icon>
            </wa-button>
            <wa-button
              appearance="plain"
              title="Remove"
              @click=${() => this.deleteAccount(account)}
            >
              <wa-icon name="delete" label="Remove"></wa-icon>
            </wa-button>
          </div>`,
      )}
    </section>`;
  }

  private renderSourceIcon(source: Source) {
    if (!source.icon) {
      return html`<wa-icon class="icon" name="error"></wa-icon>`;
    }
    let icon = parseIcon(source.icon);
    return "symbol" in icon
      ? html`<wa-icon class="icon" name=${icon.symbol}></wa-icon>`
      : html`<img class="icon" src=${icon.url} alt="" />`;
  }

  private renderSources() {
    let typeNames = new Map(this.sourceTypes.map((t) => [t.id, t.name]));
    let contextNames = new Map(this.contexts.map((c) => [c.id, c.name]));
    return html`<section>
      <div class="section-header">
        <h2>Sources</h2>
        <wa-dropdown
          placement="bottom-end"
          @wa-select=${(e: CustomEvent<{ item: { value: string } }>) => {
            let type = this.sourceTypes.find(
              (t) => t.id == e.detail.item.value,
            );
            if (type) {
              this.addSource(type);
            }
          }}
        >
          <wa-button slot="trigger" appearance="plain" with-caret>
            <wa-icon slot="start" name="add"></wa-icon>
            Add source
          </wa-button>
          ${this.sourceTypes.map(
            (type) =>
              html`<wa-dropdown-item value=${type.id}
                >${type.name}</wa-dropdown-item
              >`,
          )}
        </wa-dropdown>
      </div>
      ${
        this.sources.length == 0
          ? html`<div class="empty">No sources yet.</div>`
          : nothing
      }
      ${this.sources.map(
        (source) =>
          html`<div class="row">
            ${this.renderSourceIcon(source)}
            <div class="main">
              <div class="title">${source.name}</div>
              <div class="meta">
                ${typeNames.get(source.type) ?? source.type} · Shown in
                ${source.contexts
                  .map((c) => contextNames.get(c) ?? c)
                  .join(", ")}
              </div>
              ${
                source.status.last_error
                  ? html`<div class="meta error">
                      ${source.status.last_error}
                    </div>`
                  : nothing
              }
            </div>
            <wa-button
              appearance="plain"
              title="Edit"
              @click=${() => this.editSource(source)}
            >
              <wa-icon name="edit" label="Edit"></wa-icon>
            </wa-button>
            <wa-button
              appearance="plain"
              title="Delete"
              @click=${() => this.deleteSource(source)}
            >
              <wa-icon name="delete" label="Delete"></wa-icon>
            </wa-button>
          </div>`,
      )}
    </section>`;
  }

  render() {
    return html`
      ${
        this.error
          ? html`<wa-callout variant="danger">
              <wa-icon slot="icon" name="error"></wa-icon>
              ${this.error}
            </wa-callout>`
          : nothing
      }
      ${this.renderContexts()} ${this.renderAccounts()} ${this.renderSources()}
      <context-dialog></context-dialog>
      <source-dialog></source-dialog>
      <confirm-dialog></confirm-dialog>
      <token-account-dialog></token-account-dialog>
    `;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "settings-page": SettingsPage;
  }
}
