import { afterEach, describe, expect, it, vi } from "vitest";

import * as api from "./api.ts";
import { makeItem, mockFetch } from "./test-helpers.ts";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("settings api", () => {
  it("manages contexts", async () => {
    let context = { id: "work", name: "Work", icon: null };
    let calls = mockFetch({
      "POST /api/contexts": context,
      "PUT /api/contexts/default": context,
    });
    expect(await api.createContext(context)).toEqual(context);
    await api.updateContext("default", context);
    await api.deleteContext("a b");
    expect(calls.map((c) => [c.method, c.url, c.body])).toEqual([
      ["POST", "/api/contexts", context],
      ["PUT", "/api/contexts/default", context],
      ["DELETE", "/api/contexts/a%20b", undefined],
    ]);
  });

  it("manages sources", async () => {
    let settings = {
      type: "github",
      name: "PRs",
      contexts: ["work"],
      config: { query: "is:pr" },
    };
    let calls = mockFetch({ "GET /api/source-types": [] });
    expect(await api.getSourceTypes()).toEqual([]);
    await api.createSource(settings);
    await api.updateSource("abc", settings);
    await api.deleteSource("abc");
    expect(calls.map((c) => `${c.method} ${c.url}`)).toEqual([
      "GET /api/source-types",
      "POST /api/sources",
      "PUT /api/sources/abc",
      "DELETE /api/sources/abc",
    ]);
    expect(calls[1].body).toEqual(settings);
  });
});

describe("api", () => {
  it("fetches items for a context", async () => {
    let items = [makeItem()];
    let calls = mockFetch({ "GET /api/contexts/work/items": items });
    expect(await api.getItems("work")).toEqual(items);
    expect(calls).toEqual([
      { method: "GET", url: "/api/contexts/work/items", body: undefined },
    ]);
  });

  it("encodes item ids in paths", async () => {
    let calls = mockFetch();
    await api.unsnooze(makeItem({ source_id: "gh", id: "owner/repo#1" }));
    expect(calls[0].url).toBe("/api/items/gh/owner%2Frepo%231/snooze");
    expect(calls[0].method).toBe("DELETE");
  });

  it("sends snooze times", async () => {
    let calls = mockFetch();
    await api.snooze(makeItem(), new Date("2026-02-01T09:00:00Z"));
    await api.snooze(makeItem(), null);
    expect(calls.map((c) => c.body)).toEqual([
      { until: "2026-02-01T09:00:00.000Z" },
      { until: null },
    ]);
  });

  it("performs quick actions", async () => {
    let calls = mockFetch();
    await api.performAction(
      makeItem({ source_id: "gh", id: "inbox/t1" }),
      "complete",
    );
    expect(calls[0].method).toBe("POST");
    expect(calls[0].url).toBe("/api/items/gh/inbox%2Ft1/actions/complete");
  });

  it("sets hidden kinds", async () => {
    let calls = mockFetch();
    await api.setHiddenKinds("a b", ["task", "patch"]);
    expect(calls[0]).toEqual({
      method: "PUT",
      url: "/api/contexts/a%20b/hidden-kinds",
      body: ["task", "patch"],
    });
  });

  it("sends due overrides", async () => {
    let calls = mockFetch();
    await api.setDue(makeItem(), new Date("2026-02-01T09:00:00Z"));
    await api.clearDue(makeItem());
    expect(calls.map((c) => [c.method, c.body])).toEqual([
      ["PUT", { due: "2026-02-01T09:00:00.000Z" }],
      ["DELETE", undefined],
    ]);
  });

  it("raises server errors", async () => {
    mockFetch({
      "GET /api/contexts/nope/items": new Response(
        JSON.stringify({ error: "unknown context `nope`" }),
        { status: 404 },
      ),
    });
    await expect(api.getItems("nope")).rejects.toMatchObject({
      status: 404,
      message: "unknown context `nope`",
    });
  });

  it("falls back to the status text for non-JSON errors", async () => {
    mockFetch({
      "GET /api/contexts": new Response("oops", {
        status: 500,
        statusText: "Internal Server Error",
      }),
    });
    await expect(api.getContexts()).rejects.toMatchObject({
      status: 500,
      message: "Internal Server Error",
    });
  });
});
