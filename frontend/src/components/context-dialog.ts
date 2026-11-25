import { LitElement, css, html, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import "@awesome.me/webawesome/dist/components/button/button.js";
import "@awesome.me/webawesome/dist/components/callout/callout.js";
import "@awesome.me/webawesome/dist/components/dialog/dialog.js";
import "@awesome.me/webawesome/dist/components/icon/icon.js";
import "@awesome.me/webawesome/dist/components/input/input.js";

import type { Context, ContextSettings } from "../api.ts";
import { slugify } from "../settings-form.ts";

type Save = (context: ContextSettings) => Promise<void>;

/** Adds or edits a context. */
@customElement("context-dialog")
export class ContextDialog extends LitElement {
  @state() private open = false;
  @state() private creating = true;
  @state() private name = "";
  @state() private slug = "";
  @state() private icon = "";
  @state() private error: string | null = null;
  @state() private saving = false;
  /** Whether the URL name follows the name, until edited by hand. */
  private slugFollowsName = true;
  private save: Save = async () => {};

  static styles = css`
    form {
      display: flex;
      flex-direction: column;
      gap: var(--wa-space-m);
    }
  `;

  /** Opens the dialog. `save` is called with the entered context; if it
   * throws, its message is shown and the dialog stays open. */
  show(context: Context | null, save: Save) {
    this.creating = context == null;
    this.name = context?.name ?? "";
    this.slug = context?.id ?? "";
    this.icon = context?.icon ?? "";
    this.slugFollowsName = context == null;
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
        id: this.slug,
        name: this.name,
        icon: this.icon || null,
      });
      this.open = false;
    } catch (error) {
      this.error = error instanceof Error ? error.message : `${error}`;
    } finally {
      this.saving = false;
    }
  }

  private onName(event: Event) {
    this.name = (event.target as HTMLInputElement).value;
    if (this.slugFollowsName) {
      this.slug = slugify(this.name);
    }
  }

  private onSlug(event: Event) {
    this.slug = (event.target as HTMLInputElement).value;
    this.slugFollowsName = false;
  }

  render() {
    return html`
      <wa-dialog
        label=${this.creating ? "Add context" : "Edit context"}
        ?open=${this.open}
        @wa-after-hide=${() => (this.open = false)}
      >
        <form @submit=${this.submit}>
          ${
            this.error
              ? html`<wa-callout variant="danger">${this.error}</wa-callout>`
              : nothing
          }
          <wa-input
            label="Name"
            required
            .value=${this.name}
            @input=${this.onName}
          ></wa-input>
          <wa-input
            label="URL name"
            hint="Shown in the address: /c/${this.slug || "…"}"
            required
            .value=${this.slug}
            @input=${this.onSlug}
          ></wa-input>
          <wa-input
            label="Icon"
            hint="Optional Material Symbol name, e.g. work or home"
            .value=${this.icon}
            @input=${(e: Event) =>
              (this.icon = (e.target as HTMLInputElement).value)}
          >
            ${
              this.icon
                ? html`<wa-icon slot="start" name=${this.icon}></wa-icon>`
                : nothing
            }
          </wa-input>
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
          >Save</wa-button
        >
      </wa-dialog>
    `;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "context-dialog": ContextDialog;
  }
}
