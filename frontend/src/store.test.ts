import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Context } from "./api.ts";
import { type EventSourceLike, Store } from "./store.ts";
import { makeItem, makeSource, mockFetch } from "./test-helpers.ts";

let contexts: Context[] = [
  { id: "work", name: "Work", icon: "work" },
  { id: "home", name: "Home", icon: null },
];

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
    await store.start({ context: null, snoozed: false });

    expect(store.state.route).toEqual({ context: "work", snoozed: false });
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
    await store.start({ context: "work", snoozed: true });
    expect(store.state.items[0].snoozed_forever).toBe(true);
  });

  it("filters sources to the current context", async () => {
    routes();
    await store.start({ context: "home", snoozed: false });
    expect(store.contextSources().map((s) => s.id)).toEqual(["other"]);
  });

  it("reloads items when a source in the context updates", async () => {
    let calls = routes();
    await store.start({ context: "work", snoozed: false });
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
    await store.start({ context: "work", snoozed: false });
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
    await store.start({ context: "work", snoozed: false });
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
    await store.start({ context: "work", snoozed: false });
    expect(store.state.error).toBe("database error");
  });

  it("closes the event stream when stopped", async () => {
    routes();
    await store.start({ context: "work", snoozed: false });
    store.stop();
    expect(events.closed).toBe(true);
  });
});
