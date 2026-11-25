import { describe, expect, it } from "vitest";

import type { Item } from "./api.ts";
import {
  effectiveDue,
  fromLocalInput,
  isOverdue,
  relativeTime,
  snoozePresets,
  toLocalInput,
} from "./format.ts";

function item(fields: Partial<Item> = {}): Item {
  return {
    source_id: "s",
    id: "1",
    title: "Item",
    url: "https://example.com",
    icon: null,
    description: null,
    modified: "2026-01-01T00:00:00Z",
    due: null,
    due_override: null,
    snoozed_until: null,
    snoozed_forever: false,
    actions: [],
    ...fields,
  };
}

describe("effectiveDue", () => {
  it("is null without a due date", () => {
    expect(effectiveDue(item())).toBeNull();
  });

  it("uses the source's due date", () => {
    expect(effectiveDue(item({ due: "2026-01-05T00:00:00Z" }))).toEqual(
      new Date("2026-01-05T00:00:00Z"),
    );
  });

  it("prefers the override", () => {
    expect(
      effectiveDue(
        item({
          due: "2026-01-05T00:00:00Z",
          due_override: "2026-02-01T00:00:00Z",
        }),
      ),
    ).toEqual(new Date("2026-02-01T00:00:00Z"));
  });
});

describe("isOverdue", () => {
  let now = new Date("2026-01-10T00:00:00Z");

  it("is false without a due date", () => {
    expect(isOverdue(item(), now)).toBe(false);
  });

  it("compares the effective due date", () => {
    expect(isOverdue(item({ due: "2026-01-05T00:00:00Z" }), now)).toBe(true);
    expect(
      isOverdue(
        item({
          due: "2026-01-05T00:00:00Z",
          due_override: "2026-01-11T00:00:00Z",
        }),
        now,
      ),
    ).toBe(false);
  });
});

describe("relativeTime", () => {
  let now = new Date("2026-01-10T12:00:00Z");
  let at = (ms: number) =>
    relativeTime(new Date(now.getTime() + ms), now, "en");

  it("formats past and future times", () => {
    expect(at(-3 * 60 * 60 * 1000)).toBe("3 hours ago");
    expect(at(2 * 24 * 60 * 60 * 1000)).toBe("in 2 days");
    expect(at(-24 * 60 * 60 * 1000)).toBe("yesterday");
    expect(at(-5 * 60 * 1000)).toBe("5 minutes ago");
  });

  it("treats very recent times as now", () => {
    expect(at(-10 * 1000)).toBe("this minute");
  });
});

describe("snoozePresets", () => {
  it("offers later today, tomorrow morning and next Monday morning", () => {
    // A Wednesday afternoon, local time.
    let now = new Date(2026, 0, 7, 14, 30);
    let [later, tomorrow, nextWeek] = snoozePresets(now);
    expect(later.until).toEqual(new Date(2026, 0, 7, 17, 30));
    expect(tomorrow.until).toEqual(new Date(2026, 0, 8, 9, 0));
    expect(nextWeek.until).toEqual(new Date(2026, 0, 12, 9, 0));
  });

  it("goes to the following Monday when today is Monday", () => {
    let now = new Date(2026, 0, 12, 8, 0);
    expect(snoozePresets(now)[2].until).toEqual(new Date(2026, 0, 19, 9, 0));
  });

  it("goes to Monday when today is Sunday", () => {
    let now = new Date(2026, 0, 11, 8, 0);
    expect(snoozePresets(now)[2].until).toEqual(new Date(2026, 0, 12, 9, 0));
  });
});

describe("local input conversion", () => {
  it("round trips", () => {
    let date = new Date(2026, 4, 3, 7, 9);
    expect(toLocalInput(date)).toBe("2026-05-03T07:09");
    expect(fromLocalInput("2026-05-03T07:09")).toEqual(date);
  });

  it("rejects invalid input", () => {
    expect(fromLocalInput("")).toBeNull();
    expect(fromLocalInput("nonsense")).toBeNull();
  });
});
