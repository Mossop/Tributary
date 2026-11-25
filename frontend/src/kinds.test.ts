import { describe, expect, it } from "vitest";

import { ITEM_KINDS, toggleKind, visibleItems } from "./kinds.ts";
import { makeItem } from "./test-helpers.ts";

describe("kinds", () => {
  it("lists every kind", () => {
    expect(ITEM_KINDS.map((k) => k.kind)).toEqual(["task", "issue", "patch"]);
  });

  it("toggles kinds, keeping their order", () => {
    expect(toggleKind([], "patch")).toEqual(["patch"]);
    expect(toggleKind(["patch"], "task")).toEqual(["task", "patch"]);
    expect(toggleKind(["task", "patch"], "task")).toEqual(["patch"]);
  });

  it("filters out hidden kinds", () => {
    let task = makeItem({ id: "1", kind: "task" });
    let issue = makeItem({ id: "2", kind: "issue" });
    let patch = makeItem({ id: "3", kind: "patch" });
    expect(visibleItems([task, issue, patch], ["issue"])).toEqual([
      task,
      patch,
    ]);
    expect(visibleItems([task, issue], [])).toEqual([task, issue]);
  });
});
