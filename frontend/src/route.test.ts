import { describe, expect, it } from "vitest";

import { parseRoute, routeUrl } from "./route.ts";

describe("parseRoute", () => {
  it("parses context routes", () => {
    expect(parseRoute("/c/work")).toEqual({ view: "items", context: "work" });
    expect(parseRoute("/c/work/")).toEqual({ view: "items", context: "work" });
    expect(parseRoute("/c/work/snoozed")).toEqual({
      view: "snoozed",
      context: "work",
    });
    expect(parseRoute("/c/a%20b")).toEqual({ view: "items", context: "a b" });
  });

  it("parses settings", () => {
    expect(parseRoute("/settings")).toEqual({
      view: "settings",
      context: null,
    });
  });

  it("returns no context for other paths", () => {
    expect(parseRoute("/")).toEqual({ view: "items", context: null });
    expect(parseRoute("/elsewhere")).toEqual({ view: "items", context: null });
    expect(parseRoute("/c/work/other")).toEqual({
      view: "items",
      context: null,
    });
  });
});

describe("routeUrl", () => {
  it("builds urls", () => {
    expect(routeUrl({ view: "items", context: null })).toBe("/");
    expect(routeUrl({ view: "items", context: "work" })).toBe("/c/work");
    expect(routeUrl({ view: "snoozed", context: "work" })).toBe(
      "/c/work/snoozed",
    );
    expect(routeUrl({ view: "items", context: "a b" })).toBe("/c/a%20b");
    expect(routeUrl({ view: "settings", context: null })).toBe("/settings");
  });
});
