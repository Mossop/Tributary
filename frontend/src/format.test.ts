import { describe, expect, it } from "vitest";

import type { Item } from "./api.ts";
import {
  effectiveDue,
  fromLocalInput,
  isOverdue,
  linkify,
  relativeTime,
  duePresets,
  snoozePresets,
  toLocalInput,
} from "./format.ts";
import type { DatePreset } from "./format.ts";

function item(fields: Partial<Item> = {}): Item {
  return {
    source_id: "s",
    id: "1",
    kind: "task",
    title: "Item",
    url: "https://example.com",
    icon: null,
    description: null,
    modified: "2026-01-01T00:00:00Z",
    due: null,
    due_override: null,
    queued_for: null,
    snoozed_until: null,
    snoozed_forever: false,
    actions: [],
    references: [],
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

/** Labels and local dates of presets, e.g. "Tomorrow 1/8 9:00". */
function described(presets: DatePreset[]): string[] {
  return presets.map(
    ({ label, date }) =>
      `${label} ${date.getMonth() + 1}/${date.getDate()} ${date.getHours()}:${String(date.getMinutes()).padStart(2, "0")}`,
  );
}

describe("snoozePresets", () => {
  // January 2026: the 5th is a Monday.
  it("offers tomorrow, the weekend, next week and next month", () => {
    let wednesday = new Date(2026, 0, 7, 14, 30);
    expect(described(snoozePresets(wednesday))).toEqual([
      "Tomorrow 1/8 9:00",
      "This weekend 1/10 9:00",
      "Next week 1/12 9:00",
      "Next month 2/1 9:00",
    ]);
  });

  it("skips the weekend on a Friday", () => {
    let friday = new Date(2026, 0, 9, 8, 0);
    expect(described(snoozePresets(friday))).toEqual([
      "Tomorrow 1/10 9:00",
      "Next week 1/12 9:00",
      "Next month 2/1 9:00",
    ]);
  });

  it("offers next weekend during a weekend", () => {
    let saturday = new Date(2026, 0, 10, 8, 0);
    expect(described(snoozePresets(saturday))).toEqual([
      "Tomorrow 1/11 9:00",
      "Next week 1/12 9:00",
      "Next weekend 1/17 9:00",
      "Next month 2/1 9:00",
    ]);
    // On Sunday, next week is tomorrow.
    let sunday = new Date(2026, 0, 11, 8, 0);
    expect(described(snoozePresets(sunday))).toEqual([
      "Tomorrow 1/12 9:00",
      "Next weekend 1/17 9:00",
      "Next month 2/1 9:00",
    ]);
  });

  it("sorts by date", () => {
    // The 1st of next month is a Sunday, before next week's Monday.
    let thursday = new Date(2026, 0, 29, 8, 0);
    expect(described(snoozePresets(thursday))).toEqual([
      "Tomorrow 1/30 9:00",
      "This weekend 1/31 9:00",
      "Next month 2/1 9:00",
      "Next week 2/2 9:00",
    ]);
    // And on the last day of the month it's tomorrow.
    let lastDay = new Date(2026, 2, 31, 8, 0);
    expect(described(snoozePresets(lastDay))).toEqual([
      "Tomorrow 4/1 9:00",
      "This weekend 4/4 9:00",
      "Next week 4/6 9:00",
    ]);
  });

  it("offers a day and a week before a due date, in date order", () => {
    let wednesday = new Date(2026, 0, 7, 14, 30);
    let due = new Date(2026, 0, 20, 17, 0);
    expect(described(snoozePresets(wednesday, due))).toEqual([
      "Tomorrow 1/8 9:00",
      "This weekend 1/10 9:00",
      "Next week 1/12 9:00",
      "One week before due 1/13 8:00",
      "One day before due 1/19 8:00",
      "Next month 2/1 9:00",
    ]);
  });

  it("leaves out times before the due date that have passed", () => {
    let wednesday = new Date(2026, 0, 7, 14, 30);
    let labels = (due: Date) =>
      snoozePresets(wednesday, due).map((preset) => preset.label);
    // A week before has passed.
    expect(labels(new Date(2026, 0, 10, 9, 0))).toContain("One day before due");
    expect(labels(new Date(2026, 0, 10, 9, 0))).not.toContain(
      "One week before due",
    );
    // Both have passed, e.g. when overdue.
    let overdue = labels(new Date(2026, 0, 8, 9, 0));
    expect(overdue).not.toContain("One day before due");
    expect(overdue).not.toContain("One week before due");
  });

  it("goes to the following Monday on a Monday", () => {
    let monday = new Date(2026, 0, 12, 8, 0);
    expect(described(snoozePresets(monday))).toContain("Next week 1/19 9:00");
  });
});

describe("duePresets", () => {
  it("offers tomorrow, the weekend and next week", () => {
    let wednesday = new Date(2026, 0, 7, 14, 30);
    expect(described(duePresets(wednesday))).toEqual([
      "Tomorrow 1/8 9:00",
      "This weekend 1/10 9:00",
      "Next week 1/12 9:00",
    ]);
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

describe("linkify", () => {
  it("leaves text without URLs alone", () => {
    expect(linkify("owner/repo #1 · PR by alice")).toEqual([
      { text: "owner/repo #1 · PR by alice" },
    ]);
    expect(linkify("")).toEqual([]);
  });

  it("splits out URLs", () => {
    expect(
      linkify("See https://example.com/a?b=1#c and http://x.test too"),
    ).toEqual([
      { text: "See " },
      { url: "https://example.com/a?b=1#c" },
      { text: " and " },
      { url: "http://x.test" },
      { text: " too" },
    ]);
    expect(linkify("https://example.com")).toEqual([
      { url: "https://example.com" },
    ]);
  });

  it("leaves trailing punctuation out of URLs", () => {
    expect(linkify("Go to https://example.com/x.")).toEqual([
      { text: "Go to " },
      { url: "https://example.com/x" },
      { text: "." },
    ]);
    expect(linkify("(see https://example.com/x), then")).toEqual([
      { text: "(see " },
      { url: "https://example.com/x" },
      { text: "), then" },
    ]);
  });

  it("keeps brackets that belong to the URL", () => {
    expect(linkify("https://en.wikipedia.org/wiki/Rust_(language)")).toEqual([
      { url: "https://en.wikipedia.org/wiki/Rust_(language)" },
    ]);
  });

  it("only links http and https", () => {
    expect(linkify("javascript:alert(1) ftp://x")).toEqual([
      { text: "javascript:alert(1) ftp://x" },
    ]);
  });
});
