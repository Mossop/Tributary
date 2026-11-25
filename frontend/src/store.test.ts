import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Account, AccountProvider, Context, SourceType } from "./api.ts";
import type { Route } from "./route.ts";
import { type EventSourceLike, Store } from "./store.ts";
import { makeItem, makeSource, mockFetch } from "./test-helpers.ts";

let contexts: Context[] = [
  { id: "work", name: "Work", icon: "work" },
  { id: "home", name: "Home", icon: null },
];

let sourceTypes: SourceType[] = [
  { id: "github", name: "GitHub search", fields: [] },
];

let accounts: Account[] = [
  {
    id: "a1",
    provider: "google",
    name: "me@example.com",
    missing_scopes: false,
  },
];

let providers: AccountProvider[] = [
  { id: "google", name: "Google", configured: true },
];

let work: Route = { view: "items", context: "work" };

class FakeEventSource implements EventSourceLike {
  onmessage: ((event: MessageEvent) => void) | null = null;
  closed = false;

  close() {
    this.closed = true;
  }

  emit(data: unknown) {
    this.onmessage?.(
      new MessageEvent("message", { data: JSON.stringify(data) }),
    );
  }
}

let events: FakeEventSource;
let store: Store;

function routes(extra: Record<string, unknown> = {}) {
  return mockFetch({
    "GET /api/contexts": contexts,
    "GET /api/source-types": sourceTypes,
    "GET /api/accounts": accounts,
    "GET /api/account-providers": providers,
    "GET /api/sources": [
      makeSource({ id: "gh", contexts: ["work"] }),
      makeSource({ id: "other", contexts: ["home"] }),
    ],
    "GET /api/contexts/work/items": [makeItem()],
    "GET /api/contexts/work/snoozed": [makeItem({ snoozed_forever: true })],
    "GET /api/contexts/home/items": [],
    ...extra,
  });
}

beforeEach(() => {
  events = new FakeEventSource();
  store = new Store({ connect: () => events });
});

afterEach(() => {
  store.stop();
  vi.unstubAllGlobals();
});

describe("Store", () => {
  it("loads, defaults the context and refreshes on start", async () => {
    let calls = routes();
    await store.start({ view: "items", context: null });

    expect(store.state.route).toEqual({ view: "items", context: "work" });
    expect(store.state.contexts).toEqual(contexts);
    expect(store.state.items).toEqual([makeItem()]);
    expect(store.state.loaded).toBe(true);
    expect(store.state.refreshing).toBe(false);

    let requests = calls.map((c) => `${c.method} ${c.url}`);
    expect(requests).toContain("POST /api/activity");
    expect(requests).toContain("POST /api/contexts/work/refresh");
  });

  it("loads snoozed items", async () => {
    routes();
    await store.start({ view: "snoozed", context: "work" });
    expect(store.state.items[0].snoozed_forever).toBe(true);
  });

  it("filters sources to the current context", async () => {
    routes();
    await store.start({ view: "items", context: "home" });
    expect(store.contextSources().map((s) => s.id)).toEqual(["other"]);
  });

  it("reloads items when a source in the context updates", async () => {
    let calls = routes();
    await store.start(work);
    calls.length = 0;

    events.emit({ type: "source-updated", source_id: "gh" });
    await vi.waitFor(() =>
      expect(calls.map((c) => c.url)).toEqual(
        expect.arrayContaining(["/api/sources", "/api/contexts/work/items"]),
      ),
    );
  });

  it("ignores item changes for sources outside the context", async () => {
    let calls = routes();
    await store.start(work);
    calls.length = 0;

    events.emit({
      type: "item-state-changed",
      source_id: "other",
      item_id: "1",
    });
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(calls).toEqual([]);
  });

  it("snoozes and reloads", async () => {
    let calls = routes();
    await store.start(work);
    calls.length = 0;

    await store.snooze(makeItem(), null);
    expect(calls.map((c) => `${c.method} ${c.url}`)).toEqual([
      "PUT /api/items/gh/owner%2Frepo%231/snooze",
      "GET /api/contexts/work/items",
    ]);
  });

  it("reports errors", async () => {
    routes({
      "GET /api/contexts/work/items": new Response(
        JSON.stringify({ error: "database error" }),
        { status: 500 },
      ),
    });
    await store.start(work);
    expect(store.state.error).toBe("database error");
  });

  it("closes the event stream when stopped", async () => {
    routes();
    await store.start(work);
    store.stop();
    expect(events.closed).toBe(true);
  });

  it("loads source types and accounts", async () => {
    routes();
    await store.start(work);
    expect(store.state.sourceTypes).toEqual(sourceTypes);
    expect(store.state.accounts).toEqual(accounts);
    expect(store.state.providers).toEqual(providers);
  });

  it("doesn't load items or refresh in settings", async () => {
    let calls = routes();
    await store.start({ view: "settings", context: null });
    expect(store.state.route).toEqual({ view: "settings", context: null });
    let urls = calls.map((c) => c.url);
    expect(urls.some((u) => u.includes("/items"))).toBe(false);
    expect(urls.some((u) => u.includes("/refresh"))).toBe(false);
  });

  it("returns from settings to the last context", async () => {
    routes();
    await store.start({ view: "items", context: "home" });
    await store.navigate({ view: "settings", context: null });
    expect(store.homeRoute()).toEqual({ view: "items", context: "home" });

    store.contextRenamed("home", "house");
    // "house" doesn't exist in the loaded contexts, so fall back.
    expect(store.homeRoute()).toEqual({ view: "items", context: "work" });
  });

  it("reloads settings when they change", async () => {
    routes();
    await store.start(work);

    let renamed = [{ id: "office", name: "Office", icon: null }, contexts[1]];
    let calls = routes({ "GET /api/contexts": renamed });
    events.emit({ type: "settings-changed" });
    await vi.waitFor(() => expect(store.state.contexts).toEqual(renamed));
    // The current context no longer exists, so move to the default.
    await vi.waitFor(() =>
      expect(store.state.route).toEqual({ view: "items", context: "office" }),
    );
    expect(calls.map((c) => c.url)).toContain("/api/sources");
    expect(calls.map((c) => c.url)).toContain("/api/accounts");
  });
});
