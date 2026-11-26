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
vi.mock("@awesome.me/webawesome/dist/components/input/input.js", () => ({}));
vi.mock("@awesome.me/webawesome/dist/components/option/option.js", () => ({}));
vi.mock("@awesome.me/webawesome/dist/components/select/select.js", () => ({}));

import type { SourceType } from "../api.ts";
import { mockFetch } from "../test-helpers.ts";
import type { SourceDialog } from "./source-dialog.ts";
import "./source-dialog.ts";

let tasks: SourceType = {
  id: "google-tasks",
  name: "Google Tasks",
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

  it("offers a default due date only for types that count one", async () => {
    let { waDialog } = await open();
    expect(waDialog.querySelector("wa-input[type='number']")).toBeNull();

    document.body.replaceChildren();
    let dialog = document.createElement("source-dialog") as SourceDialog;
    document.body.append(dialog);
    dialog.show(
      { ...tasks, default_due: "after the request was made" },
      null,
      [],
      [],
      null,
      async () => {},
    );
    await dialog.updateComplete;
    let input = dialog.shadowRoot!.querySelector("wa-input[type='number']")!;
    expect(input.getAttribute("hint")).toBe(
      "Counted after the request was made. Leave empty for none.",
    );
  });
});
