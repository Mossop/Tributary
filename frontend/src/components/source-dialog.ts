import { LitElement, css, html, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import "@awesome.me/webawesome/dist/components/button/button.js";
import "@awesome.me/webawesome/dist/components/callout/callout.js";
import "@awesome.me/webawesome/dist/components/checkbox/checkbox.js";
import "@awesome.me/webawesome/dist/components/dialog/dialog.js";
import "@awesome.me/webawesome/dist/components/input/input.js";

import type {
  Context,
  Field,
  Source,
  SourceSettings,
  SourceType,
} from "../api.ts";
import {
  type SourceFormValues,
  initialValues,
  sourceSettings,
} from "../settings-form.ts";

type Save = (settings: SourceSettings) => Promise<void>;

/** Adds or edits a source. The type specific fields come from the source
 * type's field list. */
@customElement("source-dialog")
export class SourceDialog extends LitElement {
  @state() private open = false;
  @state() private type: SourceType | null = null;
  @state() private source: Source | null = null;
  @state() private contexts: Context[] = [];
  @state() private values: SourceFormValues = initialValues(null, null);
  @state() private error: string | null = null;
  @state() private saving = false;
  private save: Save = async () => {};

  static styles = css`
    form {
      display: flex;
      flex-direction: column;
      gap: var(--wa-space-m);
    }

    fieldset {
      border: none;
      margin: 0;
      padding: 0;
      display: flex;
      flex-wrap: wrap;
      gap: var(--wa-space-s) var(--wa-space-l);
    }

    legend {
      font-weight: var(--wa-font-weight-semibold);
      margin-bottom: var(--wa-space-xs);
    }
  `;

  /**
   * Opens the dialog to add a source of `type`, or to edit `source`.
   * `save` is called with the settings; if it throws, its message is shown
   * and the dialog stays open.
   */
  show(
    type: SourceType,
    source: Source | null,
    contexts: Context[],
    defaultContext: string | null,
    save: Save,
  ) {
    this.type = type;
    this.source = source;
    this.contexts = contexts;
    this.values = initialValues(source, defaultContext);
    this.error = null;
    this.save = save;
    this.open = true;
  }

  private set(changes: Partial<SourceFormValues>) {
    this.values = { ...this.values, ...changes };
  }

  private setField(key: string, value: string) {
    this.set({ fields: { ...this.values.fields, [key]: value } });
  }

  private toggleContext(id: string, checked: boolean) {
    let contexts = this.values.contexts.filter((c) => c != id);
    if (checked) {
      contexts.push(id);
    }
    this.set({ contexts });
  }

  private async submit(event: Event) {
    event.preventDefault();
    if (!this.type) {
      return;
    }
    this.saving = true;
    this.error = null;
    try {
      await this.save(sourceSettings(this.type, this.values, !this.source));
      this.open = false;
    } catch (error) {
      this.error = error instanceof Error ? error.message : `${error}`;
    } finally {
      this.saving = false;
    }
  }

  private renderField(field: Field) {
    let secretSet =
      field.kind == "secret" && this.source?.secrets_set.includes(field.key);
    let placeholder = secretSet
      ? "Saved. Leave blank to keep it."
      : (field.placeholder ?? "");
    return html`<wa-input
      label=${field.label}
      hint=${field.help ?? ""}
      type=${
        field.kind == "secret"
          ? "password"
          : field.kind == "url"
            ? "url"
            : "text"
      }
      ?password-toggle=${field.kind == "secret"}
      ?required=${field.required && !secretSet}
      placeholder=${placeholder}
      autocomplete="off"
      .value=${this.values.fields[field.key] ?? ""}
      @input=${(e: Event) =>
        this.setField(field.key, (e.target as HTMLInputElement).value)}
    ></wa-input>`;
  }

  render() {
    let label = this.source
      ? `Edit ${this.source.name}`
      : `Add ${this.type?.name ?? "source"}`;
    return html`
      <wa-dialog
        label=${label}
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
            .value=${this.values.name}
            @input=${(e: Event) =>
              this.set({ name: (e.target as HTMLInputElement).value })}
          ></wa-input>
          <fieldset>
            <legend>Show in</legend>
            ${this.contexts.map(
              (context) =>
                html`<wa-checkbox
                  .checked=${this.values.contexts.includes(context.id)}
                  @change=${(e: Event) =>
                    this.toggleContext(
                      context.id,
                      (e.target as HTMLInputElement).checked,
                    )}
                  >${context.name}</wa-checkbox
                >`,
            )}
          </fieldset>
          ${this.type?.fields.map((field) => this.renderField(field))}
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
    "source-dialog": SourceDialog;
  }
}
