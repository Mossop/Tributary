import { afterEach, describe, expect, it, vi } from "vitest";

// Web Awesome's components rely on browser features happy-dom lacks (form
// associated elements), so leave them as plain unknown elements.
vi.mock("@awesome.me/webawesome/dist/components/button/button.js", () => ({}));
vi.mock(
  "@awesome.me/webawesome/dist/components/dropdown/dropdown.js",
  () => ({}),
);
vi.mock(
  "@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js",
  () => ({}),
);
vi.mock(
  "@awesome.me/webawesome/dist/components/divider/divider.js",
  () => ({}),
);
vi.mock("@awesome.me/webawesome/dist/components/icon/icon.js", () => ({}));
vi.mock("@awesome.me/webawesome/dist/components/tag/tag.js", () => ({}));

import { makeItem, makeSource } from "../test-helpers.ts";
import type { ItemAction, ItemRow } from "./item-row.ts";
import "./item-row.ts";

let now = new Date("2026-01-10T12:00:00Z");

async function render(fields: Partial<ItemRow>) {
  let row = document.createElement("item-row");
  Object.assign(row, { now, source: makeSource() }, fields);
  document.body.append(row);
  await row.updateComplete;
  return row;
}

afterEach(() => {
  document.body.replaceChildren();
});

describe("item-row", () => {
  it("renders the item", async () => {
    let row = await render({ item: makeItem() });
    let root = row.shadowRoot!;

    let link = root.querySelector<HTMLAnchorElement>("a.title")!;
    expect(link.textContent).toBe("Fix the thing");
    expect(link.href).toBe("https://github.com/owner/repo/pull/1");
    expect(link.target).toBe("_blank");

    expect(root.querySelector(".meta")!.textContent).toContain(
      "owner/repo #1 · PR by alice",
    );
    expect(root.querySelector(".meta")!.textContent).toContain("Updated");
    expect(root.querySelector("wa-tag")).toBeNull();
  });

  it("uses the source icon unless the item has one", async () => {
    let row = await render({ item: makeItem() });
    expect(row.shadowRoot!.querySelector("img")!.getAttribute("src")).toBe(
      "https://github.com/favicon.ico",
    );

    row = await render({ item: makeItem({ icon: "symbol:bug_report" }) });
    expect(row.shadowRoot!.querySelector("img")).toBeNull();
    expect(
      row.shadowRoot!.querySelector("wa-icon.icon")!.getAttribute("name"),
    ).toBe("bug_report");
  });

  it("marks overdue items", async () => {
    let row = await render({ item: makeItem({ due: "2026-01-09T00:00:00Z" }) });
    let tag = row.shadowRoot!.querySelector("wa-tag")!;
    expect(tag.getAttribute("variant")).toBe("danger");
    expect(tag.textContent).toContain("Due");
  });

  it("shows upcoming due dates as neutral", async () => {
    let row = await render({ item: makeItem({ due: "2026-01-20T00:00:00Z" }) });
    expect(
      row.shadowRoot!.querySelector("wa-tag")!.getAttribute("variant"),
    ).toBe("neutral");
  });

  it("fires unsnooze in the snoozed view", async () => {
    let item = makeItem({ snoozed_forever: true });
    let row = await render({ item, snoozed: true });
    expect(row.shadowRoot!.textContent).toContain("Snoozed forever");

    let actions: ItemAction[] = [];
    row.addEventListener("item-action", (e) => actions.push(e.detail));
    row.shadowRoot!.querySelector<HTMLElement>(".actions wa-button")!.click();
    expect(actions).toEqual([{ action: "unsnooze", item }]);
  });

  it("fires snooze actions from the menu", async () => {
    let item = makeItem();
    let row = await render({ item });
    let actions: ItemAction[] = [];
    row.addEventListener("item-action", (e) => actions.push(e.detail));

    let dropdown = row.shadowRoot!.querySelector("wa-dropdown")!;
    let select = (value: string) =>
      dropdown.dispatchEvent(
        new CustomEvent("wa-select", { detail: { item: { value } } }),
      );
    select("forever");
    select("custom");
    select("1");

    expect(actions[0]).toEqual({ action: "snooze", item, until: null });
    expect(actions[1]).toEqual({ action: "snooze-custom", item });
    expect(actions[2]).toMatchObject({ action: "snooze", item });
    expect((actions[2] as { until: Date }).until.getHours()).toBe(9);
  });
});
