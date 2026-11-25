import { vi } from "vitest";

import type { Item, Source } from "./api.ts";

export function makeItem(fields: Partial<Item> = {}): Item {
  return {
    source_id: "gh",
    id: "owner/repo#1",
    kind: "patch",
    title: "Fix the thing",
    url: "https://github.com/owner/repo/pull/1",
    icon: null,
    description: "owner/repo #1 · PR by alice",
    modified: "2026-01-01T00:00:00Z",
    due: null,
    due_override: null,
    snoozed_until: null,
    snoozed_forever: false,
    actions: [],
    ...fields,
  };
}

export function makeSource(fields: Partial<Source> = {}): Source {
  return {
    id: "gh",
    type: "github",
    name: "GitHub",
    icon: "https://github.com/favicon.ico",
    contexts: ["work"],
    config: {},
    secrets_set: [],
    status: { last_attempt: null, last_success: null, last_error: null },
    ...fields,
  };
}

export interface Call {
  method: string;
  url: string;
  body: unknown;
}

/**
 * Replaces `fetch` with a stub that answers from `routes`, keyed by
 * "METHOD /path". Unmatched requests get a 204. Returns the recorded calls.
 */
export function mockFetch(routes: Record<string, unknown> = {}): Call[] {
  let calls: Call[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init: RequestInit = {}) => {
      let method = init.method ?? "GET";
      calls.push({
        method,
        url,
        body: init.body ? JSON.parse(String(init.body)) : undefined,
      });
      let key = `${method} ${url}`;
      if (key in routes) {
        let value = routes[key];
        if (value instanceof Response) {
          return value.clone();
        }
        return new Response(JSON.stringify(value), {
          status: 200,
          headers: { "content-type": "application/json" },
        });
      }
      return new Response(null, { status: 204 });
    }),
  );
  return calls;
}
