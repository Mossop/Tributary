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
    revoked: false,
  },
];

let providers: AccountProvider[] = [
  {
    id: "google",
    name: "Google",
    icon: "https://example.com/google.png",
    configured: true,
  },
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

  it("performs quick actions, marking the item busy", async () => {
    routes();
    await store.start(work);
    let item = makeItem({ source_id: "gh", id: "inbox/t1" });

    let calls = routes();
    let done = store.performAction(item, "complete");
    expect(store.state.busy).toEqual(["gh/inbox/t1"]);
    await done;
    expect(store.state.busy).toEqual([]);
    let requests = calls.map((c) => `${c.method} ${c.url}`);
    expect(requests).toEqual([
      "POST /api/items/gh/inbox%2Ft1/actions/complete",
      "GET /api/contexts/work/items",
    ]);
  });

  it("reports failed quick actions", async () => {
    routes();
    await store.start(work);
    let calls = routes({
      "POST /api/items/gh/1/actions/complete": new Response(
        JSON.stringify({ error: "insufficient scopes" }),
        { status: 502 },
      ),
    });
    await store.performAction(
      makeItem({ source_id: "gh", id: "1" }),
      "complete",
    );
    expect(store.state.error).toBe("insufficient scopes");
    expect(store.state.busy).toEqual([]);
    expect(calls.map((c) => `${c.method} ${c.url}`)).toEqual([
      "POST /api/items/gh/1/actions/complete",
    ]);
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

  it("keeps the error when a change fails", async () => {
    routes();
    await store.start(work);
    let calls = routes({
      "PUT /api/items/gh/owner%2Frepo%231/snooze": new Response(
        JSON.stringify({ error: "unknown item" }),
        { status: 404 },
      ),
    });
    await store.snooze(makeItem(), null);
    expect(store.state.error).toBe("unknown item");
    // Reloading would have cleared the error.
    expect(calls.map((c) => c.method)).toEqual(["PUT"]);
  });

  it("flags failing sources for attention in settings", async () => {
    routes();
    await store.start(work);
    expect(store.settingsAttention()).toBeNull();

    let failing = makeSource({
      id: "other",
      contexts: ["home"],
      status: { last_attempt: null, last_success: null, last_error: "403" },
    });
    routes({ "GET /api/sources": [makeSource(), failing] });
    await store.loadSettings();
    expect(store.settingsAttention()).toBe("A source couldn't update");

    routes({
      "GET /api/sources": [makeSource(), failing],
      "GET /api/accounts": [{ ...accounts[0], revoked: true }],
    });
    await store.loadSettings();
    expect(store.settingsAttention()).toBe("An account needs reconnecting");
  });

  it("knows when accounts need reconnecting", async () => {
    routes();
    await store.start(work);
    expect(store.accountsNeedReconnecting()).toBe(false);

    for (let change of [{ missing_scopes: true }, { revoked: true }]) {
      routes({ "GET /api/accounts": [{ ...accounts[0], ...change }] });
      await store.loadSettings();
      expect(store.accountsNeedReconnecting()).toBe(true);
    }
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
