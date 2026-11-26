import { afterEach, describe, expect, it, vi } from "vitest";

// Web Awesome's components rely on browser features happy-dom lacks (form
// associated elements), so leave them as plain unknown elements.
vi.mock("@awesome.me/webawesome/dist/components/button/button.js", () => ({}));
vi.mock(
  "@awesome.me/webawesome/dist/components/callout/callout.js",
  () => ({}),
);
vi.mock(
  "@awesome.me/webawesome/dist/components/checkbox/checkbox.js",
  () => ({}),
);
vi.mock("@awesome.me/webawesome/dist/components/dialog/dialog.js", () => ({}));
vi.mock(
  "@awesome.me/webawesome/dist/components/divider/divider.js",
  () => ({}),
);
vi.mock("@awesome.me/webawesome/dist/components/input/input.js", () => ({}));
vi.mock("@awesome.me/webawesome/dist/components/option/option.js", () => ({}));
vi.mock("@awesome.me/webawesome/dist/components/select/select.js", () => ({}));

import type { Account, SourceType } from "../api.ts";
import { mockFetch } from "../test-helpers.ts";
import type { SourceDialog } from "./source-dialog.ts";
import "./source-dialog.ts";

let tasks: SourceType = {
  id: "google-tasks",
  name: "Google Tasks",
  due_from: [{ value: "modified", label: "Last modified" }],
  fields: [
    {
      key: "account",
      label: "Google account",
      kind: "account",
      required: true,
      placeholder: null,
      help: null,
      provider: "google",
    },
    {
      key: "list",
      label: "Task list",
      kind: "select",
      required: true,
      placeholder: null,
      help: null,
      provider: null,
    },
  ],
};

async function open() {
  mockFetch({
    "POST /api/source-types/google-tasks/fields/list/options": [
      { value: "*", label: "All lists" },
    ],
  });
  let dialog = document.createElement("source-dialog") as SourceDialog;
  document.body.append(dialog);
  dialog.show(
    tasks,
    null,
    [],
    [
      {
        id: "a",
        provider: "google",
        name: "me",
        api_url: null,
        missing_scopes: false,
        revoked: false,
      },
    ],
    null,
    async () => {},
  );
  await dialog.updateComplete;
  let waDialog = dialog.shadowRoot!.querySelector("wa-dialog")!;
  return { dialog, waDialog };
}

afterEach(() => {
  document.body.replaceChildren();
  vi.unstubAllGlobals();
});

describe("source-dialog", () => {
  it("stays open when a select's menu closes", async () => {
    let { dialog, waDialog } = await open();
    let select = waDialog.querySelector("wa-select")!;
    select.dispatchEvent(
      new Event("wa-after-hide", { bubbles: true, composed: true }),
    );
    await dialog.updateComplete;
    expect(waDialog.hasAttribute("open")).toBe(true);
  });

  it("closes when the dialog is hidden", async () => {
    let { dialog, waDialog } = await open();
    waDialog.dispatchEvent(new Event("wa-after-hide", { bubbles: true }));
    await dialog.updateComplete;
    expect(waDialog.hasAttribute("open")).toBe(false);
  });

  it("loads the select options", async () => {
    let { dialog, waDialog } = await open();
    await vi.waitFor(async () => {
      await dialog.updateComplete;
      let options = [...waDialog.querySelectorAll("wa-option")];
      expect(options.map((o) => o.textContent)).toContain("All lists");
    });
  });

  it("offers to add an account, then chooses it", async () => {
    mockFetch({
      "POST /api/source-types/google-tasks/fields/list/options": [],
    });
    let dialog = document.createElement("source-dialog") as SourceDialog;
    document.body.append(dialog);
    let added: ((account: Account) => void) | null = null;
    let addAccount = vi.fn((_provider: string, done: (a: Account) => void) => {
      added = done;
    });
    dialog.show(tasks, null, [], [], null, async () => {}, addAccount);
    await dialog.updateComplete;
    let select = dialog.shadowRoot!.querySelector<HTMLSelectElement>(
      "wa-select[label='Google account']",
    )!;
    // Usable with no accounts yet, offering only to add one.
    expect(select.hasAttribute("disabled")).toBe(false);
    expect(
      [...select.querySelectorAll("wa-option")].map((o) => o.textContent),
    ).toEqual(["Add an account…"]);

    select.value = "+add";
    select.dispatchEvent(new Event("change"));
    expect(addAccount).toHaveBeenCalledWith("google", expect.any(Function));
    // Not chosen as a value.
    expect(select.value).toBe("");

    added!({
      id: "new",
      provider: "google",
      name: "me@example.com",
      api_url: null,
      missing_scopes: false,
      revoked: false,
    });
    await dialog.updateComplete;
    expect(select.value).toBe("new");
    expect(
      [...select.querySelectorAll("wa-option")].map((o) => o.textContent),
    ).toEqual(["me@example.com", "Add an account…"]);
  });

  it("can't add accounts without a way to", async () => {
    let { waDialog } = await open();
    let select = waDialog.querySelector("wa-select[label='Google account']")!;
    expect(
      [...select.querySelectorAll("wa-option")].map((o) => o.textContent),
    ).toEqual(["me"]);
  });

  it("offers a default due date counted from the type's times", async () => {
    let dialog = document.createElement("source-dialog") as SourceDialog;
    document.body.append(dialog);
    dialog.show(
      {
        ...tasks,
        due_from: [
          { value: "requested", label: "Request made" },
          { value: "modified", label: "Last modified" },
        ],
      },
      null,
      [],
      [],
      null,
      async () => {},
    );
    await dialog.updateComplete;
    let root = dialog.shadowRoot!;
    let input = root.querySelector("wa-input[type='number']")!;
    expect(input.getAttribute("hint")).toBe("Leave empty for none.");
    let select = root.querySelector<HTMLSelectElement>(
      "wa-select[label='Count from']",
    )!;
    // The first time is the default.
    expect(select.value).toBe("requested");
    expect(
      [...select.querySelectorAll("wa-option")].map((o) => o.textContent),
    ).toEqual(["Request made", "Last modified"]);
  });
});
