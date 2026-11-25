import { LitElement, css, html, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import "@awesome.me/webawesome/dist/components/button/button.js";
import "@awesome.me/webawesome/dist/components/callout/callout.js";
import "@awesome.me/webawesome/dist/components/dialog/dialog.js";

export interface ConfirmOptions {
  title: string;
  message: string;
  /** The confirming button's label, e.g. "Delete". */
  confirm: string;
  /** Marks the action as destructive. */
  danger?: boolean;
}

/**
 * Asks before doing something, then does it. If the action fails, its
 * error is shown and the dialog stays open. Also explains why something
 * can't be done, with only a button to close it.
 */
@customElement("confirm-dialog")
export class ConfirmDialog extends LitElement {
  @state() private open = false;
  @state() private options: ConfirmOptions | null = null;
  @state() private error: string | null = null;
  @state() private working = false;
  private action: (() => Promise<unknown>) | null = null;

  static styles = css`
    p {
      margin: 0;
    }

    wa-callout {
      margin-top: var(--wa-space-m);
    }
  `;

  /** Asks to confirm `options`, running `action` if confirmed. */
  ask(options: ConfirmOptions, action: () => Promise<unknown>) {
    this.options = options;
    this.action = action;
    this.error = null;
    this.open = true;
  }

  /** Explains why something can't be done. */
  explain(title: string, message: string) {
    this.options = { title, message, confirm: "" };
    this.action = null;
    this.error = null;
    this.open = true;
  }

  private async run() {
    if (!this.action) {
      return;
    }
    this.working = true;
    this.error = null;
    try {
      await this.action();
      this.open = false;
    } catch (error) {
      this.error = error instanceof Error ? error.message : `${error}`;
    } finally {
      this.working = false;
    }
  }

  render() {
    let options = this.options;
    return html`
      <wa-dialog
        label=${options?.title ?? ""}
        ?open=${this.open}
        @wa-after-hide=${(e: Event) => {
          if (e.target == e.currentTarget) {
            this.open = false;
          }
        }}
      >
        <p>${options?.message}</p>
        ${
          this.error
            ? html`<wa-callout variant="danger">${this.error}</wa-callout>`
            : nothing
        }
        <wa-button
          slot="footer"
          appearance=${this.action ? "plain" : "accent"}
          @click=${() => (this.open = false)}
          >${this.action ? "Cancel" : "OK"}</wa-button
        >
        ${
          this.action
            ? html`<wa-button
                slot="footer"
                variant=${options?.danger ? "danger" : "brand"}
                ?loading=${this.working}
                @click=${this.run}
                >${options?.confirm}</wa-button
              >`
            : nothing
        }
      </wa-dialog>
    `;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "confirm-dialog": ConfirmDialog;
  }
}
