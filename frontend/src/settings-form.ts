import type { Account, Source, SourceSettings, SourceType } from "./api.ts";

/** Derives a URL name from a context name, e.g. "Side Projects" → "side-projects". */
export function slugify(name: string): string {
  return name
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

/** The values edited in the source form. */
export interface SourceFormValues {
  name: string;
  contexts: string[];
  /** Type specific settings by field key, as entered. */
  fields: Record<string, string>;
  /** Days for the default due date, as entered; blank for none. */
  dueDays: string;
  businessDays: boolean;
  /** What the default due date counts from; blank for the type's default. */
  dueFrom: string;
}

/** What the default due date counts from, given the form's values. */
export function dueFrom(type: SourceType, values: SourceFormValues): string {
  return values.dueFrom || (type.due_from[0]?.value ?? "");
}

/** Initial form values for a new source, or for editing an existing one. */
export function initialValues(
  source: Source | null,
  defaultContext: string | null,
): SourceFormValues {
  if (!source) {
    return {
      name: "",
      contexts: defaultContext ? [defaultContext] : [],
      fields: {},
      dueDays: "",
      businessDays: false,
      dueFrom: "",
    };
  }
  let fields: Record<string, string> = {};
  for (let [key, value] of Object.entries(source.config)) {
    fields[key] = value == null ? "" : String(value);
  }
  return {
    name: source.name,
    contexts: [...source.contexts],
    fields,
    dueDays: source.default_due ? String(source.default_due.days) : "",
    businessDays: source.default_due?.business_days ?? false,
    dueFrom: source.default_due?.from ?? "",
  };
}

/**
 * Builds the settings to send to the server. Empty optional fields are
 * omitted, and so are empty secrets so that the saved value is kept.
 */
export function sourceSettings(
  type: SourceType,
  values: SourceFormValues,
  creating: boolean,
): SourceSettings {
  let config: Record<string, unknown> = {};
  for (let field of type.fields) {
    let value = (values.fields[field.key] ?? "").trim();
    if (value != "") {
      config[field.key] = value;
    }
  }
  let settings: SourceSettings = {
    name: values.name.trim(),
    contexts: values.contexts,
    config,
  };
  let days = values.dueDays.trim();
  // Anything that isn't a whole number is sent as 0 so the server explains
  // the allowed range.
  let parsed = Number(days);
  settings.default_due =
    days == ""
      ? null
      : {
          days: Number.isInteger(parsed) ? parsed : 0,
          business_days: values.businessDays,
          from: dueFrom(type, values),
        };
  if (creating) {
    settings.type = type.id;
  }
  return settings;
}

/** The sources configured to use an account. */
export function sourcesUsing(
  account: Account,
  sources: Source[],
  types: SourceType[],
): Source[] {
  return sources.filter((source) => {
    let type = types.find((t) => t.id == source.type);
    return type?.fields.some(
      (f) => f.kind == "account" && source.config[f.key] == account.id,
    );
  });
}

/** The accounts that can be chosen for an account field. */
export function accountsFor(
  provider: string | null,
  accounts: Account[],
): Account[] {
  return accounts.filter((a) => a.provider == provider);
}

/** Chooses the account for empty account fields when there is only one
 * to choose from. */
export function defaultAccounts(
  type: SourceType,
  values: SourceFormValues,
  accounts: Account[],
): SourceFormValues {
  let fields = { ...values.fields };
  for (let field of type.fields) {
    let choices = accountsFor(field.provider, accounts);
    if (field.kind == "account" && !fields[field.key] && choices.length == 1) {
      fields[field.key] = choices[0].id;
    }
  }
  return { ...values, fields };
}

/** The error the server reports in the settings URL after connecting an
 * account failed, e.g. `?error=Access+denied`. */
export function connectError(search: string): string | null {
  return new URLSearchParams(search).get("error");
}
