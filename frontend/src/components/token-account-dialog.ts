import { LitElement, css, html, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import "@awesome.me/webawesome/dist/components/button/button.js";
import "@awesome.me/webawesome/dist/components/callout/callout.js";
import "@awesome.me/webawesome/dist/components/dialog/dialog.js";
import "@awesome.me/webawesome/dist/components/input/input.js";

import type { Account, AccountProvider } from "../api.ts";

export interface TokenAccountSettings {
  token: string;
  /** Null for the provider's default instance. */
  api_url: string | null;
}

type Save = (account: TokenAccountSettings) => Promise<void>;

/** Adds an account from an access token (e.g. GitHub, Phabricator), or gives
 * an account a new token. */
@customElement("token-account-dialog")
export class TokenAccountDialog extends LitElement {
  @state() private open = false;
  @state() private provider: AccountProvider | null = null;
  @state() private account: Account | null = null;
  @state() private token = "";
  @state() private apiUrl = "";
  @state() private error: string | null = null;
  @state() private saving = false;
  private save: Save = async () => {};

  static styles = css`
    form {
      display: flex;
      flex-direction: column;
      gap: var(--wa-space-m);
    }
  `;

  /**
   * Opens the dialog to add a `provider` account, or to replace `account`'s
   * token. `save` is called with what was entered; if it throws, its
   * message is shown and the dialog stays open.
   */
  show(provider: AccountProvider, account: Account | null, save: Save) {
    this.provider = provider;
    this.account = account;
    this.token = "";
    this.apiUrl = account?.api_url ?? "";
    this.error = null;
    this.save = save;
    this.open = true;
  }

  private async submit(event: Event) {
    event.preventDefault();
    this.saving = true;
    this.error = null;
    try {
      await this.save({
        token: this.token.trim(),
        api_url: this.apiUrl.trim() || null,
      });
      this.open = false;
    } catch (error) {
      this.error = error instanceof Error ? error.message : `${error}`;
    } finally {
      this.saving = false;
    }
  }

  render() {
    let provider = this.provider;
    let label = this.account
      ? `Reconnect ${this.account.name}`
      : `Add ${provider?.name ?? ""} account`;
    let placeholder = provider?.api_url_placeholder ?? "";
    return html`
      <wa-dialog
        label=${label}
        ?open=${this.open}
        @wa-after-hide=${(e: Event) => {
          if (e.target == e.currentTarget) {
            this.open = false;
          }
        }}
      >
        <form @submit=${this.submit}>
          ${
            this.error
              ? html`<wa-callout variant="danger">${this.error}</wa-callout>`
              : nothing
          }
          <wa-input
            label="Access token"
            hint=${provider?.token_hint ?? ""}
            type="password"
            password-toggle
            required
            autocomplete="off"
            .value=${this.token}
            @input=${(e: Event) =>
              (this.token = (e.target as HTMLInputElement).value)}
          ></wa-input>
          <wa-input
            label="API URL"
            hint=${
              placeholder
                ? `Only needed for an instance other than ${placeholder}.`
                : ""
            }
            type="url"
            placeholder=${placeholder}
            ?disabled=${this.account != null}
            .value=${this.apiUrl}
            @input=${(e: Event) =>
              (this.apiUrl = (e.target as HTMLInputElement).value)}
          ></wa-input>
          <button type="submit" hidden></button>
        </form>
        <wa-button
          slot="footer"
          appearance="plain"
          @click=${() => (this.open = false)}
          >Cancel</wa-button
        >
        <wa-button
          slot="footer"
          variant="brand"
          ?loading=${this.saving}
          @click=${this.submit}
          >${this.account ? "Reconnect" : "Add"}</wa-button
        >
      </wa-dialog>
    `;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "token-account-dialog": TokenAccountDialog;
  }
}
