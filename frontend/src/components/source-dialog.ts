import { LitElement, css, html, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import "@awesome.me/webawesome/dist/components/button/button.js";
import "@awesome.me/webawesome/dist/components/callout/callout.js";
import "@awesome.me/webawesome/dist/components/checkbox/checkbox.js";
import "@awesome.me/webawesome/dist/components/dialog/dialog.js";
import "@awesome.me/webawesome/dist/components/divider/divider.js";
import "@awesome.me/webawesome/dist/components/input/input.js";
import "@awesome.me/webawesome/dist/components/option/option.js";
import "@awesome.me/webawesome/dist/components/select/select.js";

import * as api from "../api.ts";
import type {
  Account,
  Context,
  Field,
  FieldOption,
  Source,
  SourceSettings,
  SourceType,
} from "../api.ts";
import {
  type SourceFormValues,
  accountsFor,
  defaultAccounts,
  dueFrom,
  initialValues,
  sourceSettings,
} from "../settings-form.ts";

/** The options of a select field, as loaded from the server. */
interface Options {
  loading: boolean;
  options: FieldOption[];
  error: string | null;
}

type Save = (settings: SourceSettings) => Promise<void>;

/** Starts adding a `provider` account, calling `added` with it once it's
 * added. (Accounts connected by redirecting elsewhere never call it.) */
export type AddAccount = (
  provider: string,
  added: (account: Account) => void,
) => void;

/** The value of the account select's "Add an account…" option. */
const ADD_ACCOUNT = "+add";

/** Adds or edits a source. The type specific fields come from the source
 * type's field list. */
@customElement("source-dialog")
export class SourceDialog extends LitElement {
  @state() private open = false;
  @state() private type: SourceType | null = null;
  @state() private source: Source | null = null;
  @state() private contexts: Context[] = [];
  @state() private accounts: Account[] = [];
  @state() private options: Record<string, Options> = {};
  @state() private values: SourceFormValues = initialValues(null, null);
  @state() private error: string | null = null;
  @state() private saving = false;
  private save: Save = async () => {};
  private addAccount: AddAccount | null = null;
  private optionsSequence = 0;

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

    .due {
      display: flex;
      flex-direction: column;
      gap: var(--wa-space-xs);
    }
  `;

  /**
   * Opens the dialog to add a source of `type`, or to edit `source`.
   * `save` is called with the settings; if it throws, its message is shown
   * and the dialog stays open. With `addAccount`, account fields offer to
   * add an account.
   */
  show(
    type: SourceType,
    source: Source | null,
    contexts: Context[],
    accounts: Account[],
    defaultContext: string | null,
    save: Save,
    addAccount: AddAccount | null = null,
  ) {
    this.addAccount = addAccount;
    this.type = type;
    this.source = source;
    this.contexts = contexts;
    this.accounts = accounts;
    this.values = defaultAccounts(
      type,
      initialValues(source, defaultContext),
      accounts,
    );
    this.error = null;
    this.save = save;
    this.options = {};
    this.open = true;
    this.loadOptions();
  }

  /** Loads the options of select fields, which can depend on the other
   * settings (e.g. the chosen account). */
  private async loadOptions() {
    let type = this.type;
    if (!type) {
      return;
    }
    let sequence = ++this.optionsSequence;
    let config = sourceSettings(type, this.values, false).config;
    let selects = type.fields.filter((f) => f.kind == "select");
    for (let field of selects) {
      this.options = {
        ...this.options,
        [field.key]: {
          loading: true,
          options: this.options[field.key]?.options ?? [],
          error: null,
        },
      };
    }
    await Promise.all(
      selects.map(async (field) => {
        let result: Options;
        try {
          let options = await api.getFieldOptions(type.id, field.key, config);
          result = { loading: false, options, error: null };
        } catch (error) {
          let message = error instanceof Error ? error.message : `${error}`;
          result = { loading: false, options: [], error: message };
        }
        // Ignore results overtaken by a later load.
        if (sequence == this.optionsSequence) {
          this.options = { ...this.options, [field.key]: result };
        }
      }),
    );
  }

  private set(changes: Partial<SourceFormValues>) {
    this.values = { ...this.values, ...changes };
  }

  private setField(key: string, value: string) {
    this.set({ fields: { ...this.values.fields, [key]: value } });
  }

  private setAccount(key: string, value: string) {
    if (value == this.values.fields[key]) {
      return;
    }
    this.setField(key, value);
    this.loadOptions();
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

  /** Adds a `field.provider` account, then chooses it for `field`. */
  private startAddingAccount(field: Field) {
    let provider = field.provider;
    if (!this.addAccount || !provider) {
      return;
    }
    this.addAccount(provider, (account) => {
      this.accounts = [
        ...this.accounts.filter((a) => a.id != account.id),
        account,
      ];
      this.setAccount(field.key, account.id);
    });
  }

  private renderAccountField(field: Field) {
    let accounts = accountsFor(field.provider, this.accounts);
    let canAdd = this.addAccount != null && field.provider != null;
    let hint =
      accounts.length == 0 && !canAdd
        ? "Connect an account in the Accounts section of settings first."
        : (field.help ?? "");
    let value = this.values.fields[field.key] ?? "";
    let onChange = (e: Event) => {
      let select = e.target as HTMLInputElement;
      if (select.value == ADD_ACCOUNT) {
        // Not a choice: keep the current one until the account is added.
        select.value = value;
        this.startAddingAccount(field);
      } else {
        this.setAccount(field.key, select.value);
      }
    };
    return html`<wa-select
      label=${field.label}
      hint=${hint}
      ?required=${field.required}
      ?disabled=${accounts.length == 0 && !canAdd}
      .value=${value}
      @change=${onChange}
    >
      ${accounts.map(
        (account) =>
          html`<wa-option value=${account.id}>${account.name}</wa-option>`,
      )}
      ${
        canAdd
          ? html`${accounts.length > 0 ? html`<wa-divider></wa-divider>` : nothing}
              <wa-option value=${ADD_ACCOUNT}>Add an account…</wa-option>`
          : nothing
      }
    </wa-select>`;
  }

  private renderSelectField(field: Field) {
    let options = this.options[field.key];
    let value = this.values.fields[field.key] ?? "";
    let choices = options?.options ?? [];
    let hint = options?.error
      ? `Couldn't load the options: ${options.error}`
      : options?.loading
        ? "Loading…"
        : (field.help ?? "");
    return html`<wa-select
      label=${field.label}
      hint=${hint}
      ?required=${field.required}
      ?disabled=${options?.loading || choices.length == 0}
      .value=${value}
      @change=${(e: Event) =>
        this.setField(field.key, (e.target as HTMLInputElement).value)}
    >
      ${choices.map(
        (option) =>
          html`<wa-option value=${option.value}>${option.label}</wa-option>`,
      )}
    </wa-select>`;
  }

  /** A due date for items that don't have one, counted from one of their
   * times. */
  private renderDefaultDue() {
    let type = this.type;
    if (!type) {
      return nothing;
    }
    return html`<div class="due">
      <wa-input
        label="Default due date (days)"
        hint="Leave empty for none."
        type="number"
        min="1"
        max="365"
        .value=${this.values.dueDays}
        @input=${(e: Event) =>
          this.set({ dueDays: (e.target as HTMLInputElement).value })}
      ></wa-input>
      <wa-select
        label="Count from"
        .value=${dueFrom(type, this.values)}
        @change=${(e: Event) =>
          this.set({ dueFrom: (e.target as HTMLInputElement).value })}
      >
        ${type.due_from.map(
          (choice) =>
            html`<wa-option value=${choice.value}>${choice.label}</wa-option>`,
        )}
      </wa-select>
      <wa-checkbox
        .checked=${this.values.businessDays}
        @change=${(e: Event) =>
          this.set({ businessDays: (e.target as HTMLInputElement).checked })}
        >Business days</wa-checkbox
      >
    </div>`;
  }

  private renderField(field: Field) {
    if (field.kind == "account") {
      return this.renderAccountField(field);
    }
    if (field.kind == "select") {
      return this.renderSelectField(field);
    }
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
        @wa-after-hide=${(e: Event) => {
          // Selects inside the dialog fire this too when their menu closes.
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
          ${this.renderDefaultDue()}
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
