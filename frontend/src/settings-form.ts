import type { Source, SourceSettings, SourceType } from "./api.ts";

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
  if (creating) {
    settings.type = type.id;
  }
  return settings;
}
