import { LitElement, css, html, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import "@awesome.me/webawesome/dist/components/button/button.js";
import "@awesome.me/webawesome/dist/components/callout/callout.js";
import "@awesome.me/webawesome/dist/components/dialog/dialog.js";
import "@awesome.me/webawesome/dist/components/input/input.js";

import type { Account } from "../api.ts";

export interface GithubAccountSettings {
  token: string;
  /** Null for github.com. */
  api_url: string | null;
}

type Save = (account: GithubAccountSettings) => Promise<void>;

/** Adds a GitHub account from an access token, or gives an account a new
 * token. */
@customElement("github-account-dialog")
export class GithubAccountDialog extends LitElement {
  @state() private open = false;
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
   * Opens the dialog to add an account, or to replace `account`'s token.
   * `save` is called with what was entered; if it throws, its message is
   * shown and the dialog stays open.
   */
  show(account: Account | null, save: Save) {
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
    let label = this.account
      ? `Reconnect ${this.account.name}`
      : "Add GitHub account";
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
            hint="A personal access token. For private repositories a classic token needs the repo scope; authorize it for any organisations that use SSO."
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
            hint="Only needed for GitHub Enterprise, e.g. https://github.example.com/api/v3."
            type="url"
            placeholder="https://api.github.com"
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
    "github-account-dialog": GithubAccountDialog;
  }
}
