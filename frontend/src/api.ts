/** Client for the server's JSON API. Timestamps are RFC 3339 strings. */

/** What kind of thing an item is. */
export type ItemKind = "task" | "issue" | "patch";

/** The settings edited when creating or updating a context. */
export interface ContextSettings {
  id: string;
  name: string;
  /** Material Symbol name. */
  icon: string | null;
}

export interface Context extends ContextSettings {
  /** Item kinds not shown in the context. */
  hidden_kinds: ItemKind[];
}

export interface SourceStatus {
  last_attempt: string | null;
  last_success: string | null;
  last_error: string | null;
}

export interface Source {
  id: string;
  /** The source type id. */
  type: string;
  name: string;
  /** Empty if the source isn't running, e.g. because its settings are invalid. */
  icon: string;
  contexts: string[];
  /** Type specific settings, excluding secrets. */
  config: Record<string, unknown>;
  /** Keys of secret settings that have a value. */
  secrets_set: string[];
  status: SourceStatus;
}

export interface Field {
  key: string;
  label: string;
  /** `account` holds the id of an account with `provider`; `select` holds
   * one of the options from `getFieldOptions`. */
  kind: "text" | "url" | "secret" | "account" | "select";
  required: boolean;
  placeholder: string | null;
  help: string | null;
  provider: string | null;
}

export interface FieldOption {
  value: string;
  label: string;
}

/** A connected account with an external service, e.g. Google. */
export interface Account {
  id: string;
  provider: string;
  /** e.g. the email address or login. */
  name: string;
  /** The API the account belongs to, if not the provider's public one. */
  api_url: string | null;
  /** Source types need access the account hasn't granted; connect it again. */
  missing_scopes: boolean;
  /** Access was revoked or expired; connect it again. */
  revoked: boolean;
}

export interface AccountProvider {
  id: string;
  name: string;
  /** A URL, or `symbol:<name>` for a Material Symbol. */
  icon: string;
  /** `oauth` accounts are connected by visiting `connectUrl`; `token`
   * accounts are added with `addTokenAccount`. */
  connect: "oauth" | "token";
  /** Whether the server is set up to connect accounts. */
  configured: boolean;
}

export interface SourceType {
  id: string;
  name: string;
  fields: Field[];
}

/** The settings sent when creating or updating a source. */
export interface SourceSettings {
  /** Only used when creating. */
  type?: string;
  name: string;
  contexts: string[];
  config: Record<string, unknown>;
}

/** Something that can be done to an item from the dashboard. */
export interface QuickAction {
  id: string;
  label: string;
  /** A URL, or `symbol:<name>` for a Material Symbol. */
  icon: string;
}

export interface Item {
  source_id: string;
  id: string;
  kind: ItemKind;
  title: string;
  url: string;
  icon: string | null;
  description: string | null;
  modified: string;
  due: string | null;
  due_override: string | null;
  snoozed_until: string | null;
  snoozed_forever: boolean;
  actions: QuickAction[];
}

export type ServerEvent =
  | { type: "source-updated"; source_id: string }
  | { type: "item-state-changed"; source_id: string; item_id: string }
  | { type: "settings-changed" };

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}

async function request<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  let init: RequestInit = { method };
  if (body !== undefined) {
    init.headers = { "content-type": "application/json" };
    init.body = JSON.stringify(body);
  }
  let response = await fetch(path, init);
  if (!response.ok) {
    let message = response.statusText;
    try {
      message = (await response.json()).error ?? message;
    } catch {
      // Not JSON; keep the status text.
    }
    throw new ApiError(response.status, message);
  }
  if (response.status == 204) {
    return undefined as T;
  }
  return response.json();
}

function contextPath(context: string) {
  return `/api/contexts/${encodeURIComponent(context)}`;
}

function itemPath(item: Item) {
  return `/api/items/${encodeURIComponent(item.source_id)}/${encodeURIComponent(item.id)}`;
}

export function getContexts() {
  return request<Context[]>("GET", "/api/contexts");
}

export function getSources() {
  return request<Source[]>("GET", "/api/sources");
}

export function getItems(context: string) {
  return request<Item[]>("GET", `${contextPath(context)}/items`);
}

export function getSnoozed(context: string) {
  return request<Item[]>("GET", `${contextPath(context)}/snoozed`);
}

/** Refreshes every source in a context, resolving once they have finished. */
export function refreshContext(context: string) {
  return request<void>("POST", `${contextPath(context)}/refresh`);
}

/** Snoozes an item until a time, or forever when `until` is null. */
export function snooze(item: Item, until: Date | null) {
  return request<void>("PUT", `${itemPath(item)}/snooze`, {
    until: until?.toISOString() ?? null,
  });
}

export function unsnooze(item: Item) {
  return request<void>("DELETE", `${itemPath(item)}/snooze`);
}

export function setDue(item: Item, due: Date) {
  return request<void>("PUT", `${itemPath(item)}/due`, {
    due: due.toISOString(),
  });
}

export function clearDue(item: Item) {
  return request<void>("DELETE", `${itemPath(item)}/due`);
}

/** Performs a quick action, resolving once the item's source has been
 * refreshed. */
export function performAction(item: Item, action: string) {
  return request<void>(
    "POST",
    `${itemPath(item)}/actions/${encodeURIComponent(action)}`,
  );
}

/** Tells the server the dashboard is in use. */
export function pingActivity() {
  return request<void>("POST", "/api/activity");
}

export function createContext(context: ContextSettings) {
  return request<Context>("POST", "/api/contexts", context);
}

/** Updates a context. `context.id` may differ from `id` to change its URL. */
export function updateContext(id: string, context: ContextSettings) {
  return request<Context>("PUT", contextPath(id), context);
}

/** Sets which item kinds a context hides. */
export function setHiddenKinds(context: string, kinds: ItemKind[]) {
  return request<Context>("PUT", `${contextPath(context)}/hidden-kinds`, kinds);
}

export function deleteContext(id: string) {
  return request<void>("DELETE", contextPath(id));
}

export function getSourceTypes() {
  return request<SourceType[]>("GET", "/api/source-types");
}

/** The options of a select field, given the source's other settings. */
export function getFieldOptions(
  type: string,
  field: string,
  config: Record<string, unknown>,
) {
  return request<FieldOption[]>(
    "POST",
    `/api/source-types/${encodeURIComponent(type)}/fields/${encodeURIComponent(field)}/options`,
    { config },
  );
}

export function getAccountProviders() {
  return request<AccountProvider[]>("GET", "/api/account-providers");
}

export function getAccounts() {
  return request<Account[]>("GET", "/api/accounts");
}

export function deleteAccount(id: string) {
  return request<void>("DELETE", `/api/accounts/${encodeURIComponent(id)}`);
}

/** Adds an account from an access token, or updates the token of an
 * account that was already added. */
export function addTokenAccount(
  provider: string,
  body: { token: string; api_url: string | null },
) {
  return request<Account>(
    "POST",
    `/api/accounts/${encodeURIComponent(provider)}`,
    body,
  );
}

/** Where to send the browser to connect an account, or to reconnect
 * `accountId`. The server redirects back to the settings page. */
export function connectUrl(provider: string, accountId?: string) {
  let url = `/api/accounts/${encodeURIComponent(provider)}/connect`;
  return accountId ? `${url}?account=${encodeURIComponent(accountId)}` : url;
}

function sourcePath(id: string) {
  return `/api/sources/${encodeURIComponent(id)}`;
}

export function createSource(settings: SourceSettings) {
  return request<Source>("POST", "/api/sources", settings);
}

/** Updates a source. Secrets left empty keep their saved value. */
export function updateSource(id: string, settings: SourceSettings) {
  return request<Source>("PUT", sourcePath(id), settings);
}

export function deleteSource(id: string) {
  return request<void>("DELETE", sourcePath(id));
}
