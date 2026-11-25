import { afterEach, describe, expect, it, vi } from "vitest";

import * as api from "./api.ts";
import { makeItem, mockFetch } from "./test-helpers.ts";

afterEach(() => {
  vi.unstubAllGlobals();
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
