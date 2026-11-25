import { LitElement, html } from "lit";
import { customElement, query, state } from "lit/decorators.js";
import "@awesome.me/webawesome/dist/components/button/button.js";
import "@awesome.me/webawesome/dist/components/dialog/dialog.js";
import "@awesome.me/webawesome/dist/components/input/input.js";
import type WaInput from "@awesome.me/webawesome/dist/components/input/input.js";

import { fromLocalInput, toLocalInput } from "../format.ts";

/** A dialog asking for a date and time. */
@customElement("date-dialog")
export class DateDialog extends LitElement {
  @state() private label = "";
  @state() private value = "";
  @state() private open = false;
  @query("wa-input") private input!: WaInput;

  private resolve: ((date: Date | null) => void) | null = null;

  /** Shows the dialog, resolving with the chosen date or null if cancelled. */
  pick(label: string, initial: Date): Promise<Date | null> {
    this.resolve?.(null);
    this.label = label;
    this.value = toLocalInput(initial);
    this.open = true;
    return new Promise((resolve) => (this.resolve = resolve));
  }

  private finish(date: Date | null) {
    this.open = false;
    this.resolve?.(date);
    this.resolve = null;
  }

  private submit(event: Event) {
    event.preventDefault();
    let date = fromLocalInput(String(this.input.value ?? ""));
    if (date) {
      this.finish(date);
    }
  }

  render() {
    return html`
      <wa-dialog
        label=${this.label}
        ?open=${this.open}
        @wa-after-hide=${() => this.finish(null)}
      >
        <form @submit=${this.submit}>
          <wa-input
            type="datetime-local"
            label="Date and time"
            required
            .value=${this.value}
          ></wa-input>
        </form>
        <wa-button
          slot="footer"
          appearance="plain"
          @click=${() => this.finish(null)}
          >Cancel</wa-button
        >
        <wa-button slot="footer" variant="brand" @click=${this.submit}
          >Save</wa-button
        >
      </wa-dialog>
    `;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "date-dialog": DateDialog;
  }
}
