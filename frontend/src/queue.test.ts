import { describe, expect, it } from "vitest";

import { isQueuedLater, isToday, splitToday, startOfDay } from "./queue.ts";
import { makeItem } from "./test-helpers.ts";

describe("queue", () => {
  let now = new Date(2026, 10, 26, 15, 30);

  it("finds the start of days", () => {
    expect(startOfDay(now)).toEqual(new Date(2026, 10, 26));
    expect(startOfDay(now, 1)).toEqual(new Date(2026, 10, 27));
    // Across a month end.
    expect(startOfDay(new Date(2026, 10, 30, 23, 59), 1)).toEqual(
      new Date(2026, 11, 1),
    );
  });

  it("keeps items queued for today or earlier in today", () => {
    let item = (day: Date | null) =>
      makeItem({ queued_for: day?.toISOString() ?? null });
    expect(isToday(item(new Date(2026, 10, 26)), now)).toBe(true);
    // Queued for a day that has passed: still today's.
    expect(isToday(item(new Date(2026, 10, 20)), now)).toBe(true);
    expect(isToday(item(new Date(2026, 10, 27)), now)).toBe(false);
    expect(isQueuedLater(item(new Date(2026, 10, 27)), now)).toBe(true);
    expect(isToday(item(null), now)).toBe(false);
    expect(isQueuedLater(item(null), now)).toBe(false);
  });

  it("splits today's items from the rest", () => {
    let a = makeItem({ id: "a" });
    let b = makeItem({
      id: "b",
      queued_for: new Date(2026, 10, 26).toISOString(),
    });
    let c = makeItem({
      id: "c",
      queued_for: new Date(2026, 10, 27).toISOString(),
    });
    let d = makeItem({
      id: "d",
      queued_for: new Date(2026, 10, 25).toISOString(),
    });
    expect(splitToday([a, b, c, d], now)).toEqual([
      [b, d],
      [a, c],
    ]);
  });
});
