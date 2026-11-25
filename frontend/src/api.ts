/** Client for the server's JSON API. Timestamps are RFC 3339 strings. */

export interface Context {
  id: string;
  name: string;
  /** Material Symbol name. */
  icon: string | null;
}

export interface SourceStatus {
  last_attempt: string | null;
  last_success: string | null;
  last_error: string | null;
}

export interface Source {
  id: string;
  name: string;
  icon: string;
  contexts: string[];
  status: SourceStatus;
}

export interface Item {
  source_id: string;
  id: string;
  title: string;
  url: string;
  icon: string | null;
  description: string | null;
  modified: string;
  due: string | null;
  due_override: string | null;
  snoozed_until: string | null;
  snoozed_forever: boolean;
}

export type ServerEvent =
  | { type: "source-updated"; source_id: string }
  | { type: "item-state-changed"; source_id: string; item_id: string };

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

/** Tells the server the dashboard is in use. */
export function pingActivity() {
  return request<void>("POST", "/api/activity");
}
