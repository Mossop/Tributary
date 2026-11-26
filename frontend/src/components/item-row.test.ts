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

/** The menu opened by the button titled `title`. */
function menu(row: ItemRow, title: string): Element {
  return row.shadowRoot!.querySelector(`wa-button[title='${title}']`)!
    .parentElement!;
}

/** The due date menu opened by the calendar button. */
function dueMenu(row: ItemRow): Element {
  return menu(row, "Due date");
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

  it("doesn't show the source name", async () => {
    let row = await render({ item: makeItem() });
    expect(row.shadowRoot!.querySelector(".meta")!.textContent).not.toContain(
      "GitHub",
    );
  });

  it("links URLs in the description", async () => {
    let row = await render({
      item: makeItem({ description: "Notes: https://example.com/doc." }),
    });
    let description = row.shadowRoot!.querySelector(".description")!;
    expect(description.textContent).toBe("Notes: https://example.com/doc.");
    let link = description.querySelector("a")!;
    expect(link.getAttribute("href")).toBe("https://example.com/doc");
    expect(link.target).toBe("_blank");
    expect(link.rel).toBe("noopener noreferrer");
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

    let dropdown = menu(row, "Snooze");
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

  it("queues items for today and tomorrow", async () => {
    let local = new Date(now);
    let today = new Date(
      local.getFullYear(),
      local.getMonth(),
      local.getDate(),
    );
    let tomorrow = new Date(
      local.getFullYear(),
      local.getMonth(),
      local.getDate() + 1,
    );
    // Choices for an item queued for `queuedFor`.
    let choose = async (queuedFor: Date | null) => {
      let item = makeItem({ queued_for: queuedFor?.toISOString() ?? null });
      let row = await render({ item });
      let actions: ItemAction[] = [];
      row.addEventListener("item-action", (e) => actions.push(e.detail));
      let queue = menu(row, "Queue");
      expect(
        [...queue.querySelectorAll("wa-dropdown-item")].map((i) =>
          i.textContent!.trim(),
        ),
      ).toEqual(["Today", "Tomorrow"]);
      for (let value of ["today", "tomorrow"]) {
        queue.dispatchEvent(
          new CustomEvent("wa-select", { detail: { item: { value } } }),
        );
      }
      return actions.map((a) => (a as { day: Date | null }).day);
    };

    expect(await choose(null)).toEqual([today, tomorrow]);
    // Choosing the ticked one takes the item out of the queue.
    expect(await choose(today)).toEqual([null, tomorrow]);
    expect(await choose(tomorrow)).toEqual([today, null]);
  });

  it("shows where an item is queued", async () => {
    let today = makeItem({ queued_for: "2026-01-01T00:00:00Z" });
    let row = await render({ item: today });
    let checked = () =>
      [...menu(row, "Queue").querySelectorAll("wa-dropdown-item")]
        .filter((i) => (i as HTMLElement & { checked: boolean }).checked)
        .map((i) => i.textContent!.trim());
    expect(checked()).toEqual(["Today"]);
    expect(row.shadowRoot!.textContent).not.toContain("Queued for tomorrow");

    row = await render({
      item: makeItem({ queued_for: "2026-01-11T00:00:00Z" }),
    });
    expect(checked()).toEqual(["Tomorrow"]);
    expect(row.shadowRoot!.textContent).toContain("Queued for tomorrow");
  });

  it("doesn't keep a ticked queue choice when showing another item", async () => {
    let row = await render({ item: makeItem({ id: "1" }) });
    // Choosing Today ticks it, then the row is reused for an unqueued item.
    let today = menu(row, "Queue").querySelector(
      "wa-dropdown-item",
    )! as HTMLElement & { checked: boolean };
    today.checked = true;
    row.item = makeItem({ id: "2" });
    await row.updateComplete;
    expect(today.checked).toBe(false);
  });

  it("offers snoozing until before the due date", async () => {
    // Midday UTC: the 20th in practically every time zone.
    let item = makeItem({ due_override: "2026-01-20T12:00:00Z" });
    let row = await render({ item });
    let actions: ItemAction[] = [];
    row.addEventListener("item-action", (e) => actions.push(e.detail));

    let menu = row.shadowRoot!.querySelector(
      "wa-button[title='Snooze']",
    )!.parentElement!;
    let items = [...menu.querySelectorAll("wa-dropdown-item")];
    let dayBefore = items.find((i) =>
      i.textContent!.includes("One day before due"),
    )!;
    expect(
      items.some((i) => i.textContent!.includes("One week before due")),
    ).toBe(true);

    menu.dispatchEvent(
      new CustomEvent("wa-select", {
        detail: { item: { value: dayBefore.getAttribute("value") } },
      }),
    );
    // A fixed time: 8am local time the day before the due date.
    expect(actions).toEqual([
      {
        action: "snooze",
        item,
        until: new Date(2026, 0, 19, 8, 0),
      },
    ]);
  });

  it("fires due date actions from the menu", async () => {
    let item = makeItem();
    let row = await render({ item });
    let actions: ItemAction[] = [];
    row.addEventListener("item-action", (e) => actions.push(e.detail));

    let menu = dueMenu(row);
    let labels = [...menu.querySelectorAll("wa-dropdown-item")].map(
      (i) => i.textContent!.trim().split("\n")[0],
    );
    // A Saturday: tomorrow, then next week, then next weekend.
    expect(labels).toEqual([
      "Tomorrow",
      "Next week",
      "Next weekend",
      "Pick a date…",
    ]);

    let select = (value: string) =>
      menu.dispatchEvent(
        new CustomEvent("wa-select", { detail: { item: { value } } }),
      );
    select("0");
    select("custom");
    expect(actions[0]).toMatchObject({ action: "due", item });
    expect((actions[0] as { due: Date }).due).toEqual(
      new Date(2026, 0, 11, 9, 0),
    );
    expect(actions[1]).toEqual({ action: "due-custom", item });
  });

  it("opens the due date menu from the badge", async () => {
    let item = makeItem({ due: "2026-01-20T00:00:00Z" });
    let row = await render({ item });
    let actions: ItemAction[] = [];
    row.addEventListener("item-action", (e) => actions.push(e.detail));

    let trigger = row.shadowRoot!.querySelector("button.due-trigger")!;
    expect(trigger.getAttribute("slot")).toBe("trigger");
    expect(trigger.querySelector("wa-tag")!.textContent).toContain("Due");
    let menu = trigger.parentElement!;
    expect(menu.localName).toBe("wa-dropdown");
    let values = [...menu.querySelectorAll("wa-dropdown-item")].map((i) =>
      i.getAttribute("value"),
    );
    expect(values).toEqual(
      [...dueMenu(row).querySelectorAll("wa-dropdown-item")].map((i) =>
        i.getAttribute("value"),
      ),
    );
    menu.dispatchEvent(
      new CustomEvent("wa-select", { detail: { item: { value: "custom" } } }),
    );
    expect(actions).toEqual([{ action: "due-custom", item }]);
  });

  it("offers clearing an overridden due date", async () => {
    let item = makeItem({ due_override: "2026-01-20T00:00:00Z" });
    let row = await render({ item });
    let actions: ItemAction[] = [];
    row.addEventListener("item-action", (e) => actions.push(e.detail));
    let menu = dueMenu(row);
    expect(
      menu.querySelector("wa-dropdown-item[value='clear']"),
    ).not.toBeNull();
    menu.dispatchEvent(
      new CustomEvent("wa-select", { detail: { item: { value: "clear" } } }),
    );
    expect(actions).toEqual([{ action: "clear-due", item }]);
  });

  it("fires quick actions", async () => {
    let item = makeItem({
      actions: [
        { id: "complete", label: "Mark complete", icon: "symbol:check_circle" },
      ],
    });
    let row = await render({ item });
    let actions: ItemAction[] = [];
    row.addEventListener("item-action", (e) => actions.push(e.detail));

    let button = row.shadowRoot!.querySelector<HTMLElement>(
      ".actions wa-button[title='Mark complete']",
    )!;
    expect(button.querySelector("wa-icon")!.getAttribute("name")).toBe(
      "check_circle",
    );
    button.click();
    expect(actions).toEqual([{ action: "quick", item, id: "complete" }]);
  });

  it("opens link actions in a new tab", async () => {
    let item = makeItem({
      actions: [
        {
          id: "lando",
          label: "Open in Lando",
          icon: "symbol:flight_takeoff",
          url: "https://lando.moz.tools/D1/",
        },
      ],
    });
    let row = await render({ item });
    let actions: ItemAction[] = [];
    row.addEventListener("item-action", (e) => actions.push(e.detail));

    let button = row.shadowRoot!.querySelector<HTMLElement>(
      "wa-button[title='Open in Lando']",
    )!;
    expect(button.getAttribute("href")).toBe("https://lando.moz.tools/D1/");
    expect(button.getAttribute("target")).toBe("_blank");
    expect(button.getAttribute("rel")).toBe("noopener noreferrer");
    button.click();
    expect(actions).toEqual([]);
  });

  it("reflects being nested in a group", async () => {
    let row = await render({ item: makeItem(), nested: true });
    expect(row.hasAttribute("nested")).toBe(true);
    // Nested rows keep their own actions.
    expect(
      row.shadowRoot!.querySelector("wa-button[title='Snooze']"),
    ).not.toBeNull();
    row = await render({ item: makeItem() });
    expect(row.hasAttribute("nested")).toBe(false);
  });

  it("shows quick actions in progress", async () => {
    let item = makeItem({
      actions: [
        { id: "complete", label: "Mark complete", icon: "symbol:check" },
      ],
    });
    let row = await render({ item, busy: true });
    let button = row.shadowRoot!.querySelector(
      "wa-button[title='Mark complete']",
    )!;
    expect(button.hasAttribute("loading")).toBe(true);
    expect(button.hasAttribute("disabled")).toBe(true);
  });

  it("doesn't offer quick actions in the snoozed view", async () => {
    let item = makeItem({
      snoozed_forever: true,
      actions: [
        { id: "complete", label: "Mark complete", icon: "symbol:check" },
      ],
    });
    let row = await render({ item, snoozed: true });
    expect(
      row.shadowRoot!.querySelector("wa-button[title='Mark complete']"),
    ).toBeNull();
  });
});
