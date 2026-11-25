import { describe, expect, it } from "vitest";

import { parseRoute, routeUrl } from "./route.ts";

describe("parseRoute", () => {
  it("parses context routes", () => {
    expect(parseRoute("/c/work")).toEqual({ context: "work", snoozed: false });
    expect(parseRoute("/c/work/")).toEqual({ context: "work", snoozed: false });
    expect(parseRoute("/c/work/snoozed")).toEqual({
      context: "work",
      snoozed: true,
    });
    expect(parseRoute("/c/a%20b")).toEqual({ context: "a b", snoozed: false });
  });

  it("returns no context for other paths", () => {
    expect(parseRoute("/")).toEqual({ context: null, snoozed: false });
    expect(parseRoute("/elsewhere")).toEqual({ context: null, snoozed: false });
    expect(parseRoute("/c/work/other")).toEqual({
      context: null,
      snoozed: false,
    });
  });
});

describe("routeUrl", () => {
  it("builds urls", () => {
    expect(routeUrl({ context: null, snoozed: false })).toBe("/");
    expect(routeUrl({ context: "work", snoozed: false })).toBe("/c/work");
    expect(routeUrl({ context: "work", snoozed: true })).toBe(
      "/c/work/snoozed",
    );
    expect(routeUrl({ context: "a b", snoozed: false })).toBe("/c/a%20b");
  });
});
