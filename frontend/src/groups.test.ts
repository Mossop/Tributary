import { describe, expect, it } from "vitest";

import type { Item, ItemKind } from "./api.ts";
import { groupItems } from "./groups.ts";
import { makeItem } from "./test-helpers.ts";

function item(id: string, kind: ItemKind, references: string[]): Item {
  return makeItem({ id, kind, references });
}

function ids(groups: Item[][]): string[][] {
  return groups.map((group) => group.map((i) => i.id));
}

describe("groupItems", () => {
  it("keeps unrelated items apart", () => {
    let items = [
      item("a", "issue", ["bugzilla:1"]),
      item("b", "patch", ["phab:D2"]),
      item("c", "task", []),
    ];
    expect(ids(groupItems(items))).toEqual([["a"], ["b"], ["c"]]);
  });

  it("groups items sharing a reference", () => {
    let items = [
      item("bug", "issue", ["bugzilla:1"]),
      item("other", "issue", ["bugzilla:9"]),
      item("patch", "patch", ["phab:D2", "bugzilla:1"]),
    ];
    expect(ids(groupItems(items))).toEqual([["bug", "patch"], ["other"]]);
  });

  it("chains groups through shared references", () => {
    let items = [
      item("task", "task", ["phab:D2"]),
      item("bug", "issue", ["bugzilla:1"]),
      item("unrelated", "issue", ["bugzilla:5"]),
      item("patch", "patch", ["phab:D2", "bugzilla:1"]),
    ];
    expect(ids(groupItems(items))).toEqual([
      ["bug", "task", "patch"],
      ["unrelated"],
    ]);
  });

  it("places groups by their highest-placed member", () => {
    let items = [
      item("x", "issue", []),
      item("patch", "patch", ["bugzilla:1"]),
      item("y", "issue", []),
      item("bug", "issue", ["bugzilla:1"]),
    ];
    // The group sits where the patch would, but the bug leads it.
    expect(ids(groupItems(items))).toEqual([["x"], ["bug", "patch"], ["y"]]);
  });

  it("leads with issues, then tasks, then patches", () => {
    let items = [item("patch", "patch", ["r"]), item("task", "task", ["r"])];
    expect(ids(groupItems(items))).toEqual([["task", "patch"]]);
  });

  it("breaks ties by list position", () => {
    let items = [
      item("patch", "patch", ["r"]),
      item("first", "issue", ["r"]),
      item("second", "issue", ["r"]),
    ];
    expect(ids(groupItems(items))).toEqual([["first", "patch", "second"]]);
  });

  it("handles no items", () => {
    expect(groupItems([])).toEqual([]);
  });
});
