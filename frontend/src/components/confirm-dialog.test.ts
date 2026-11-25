import { afterEach, describe, expect, it, vi } from "vitest";

// Web Awesome's components rely on browser features happy-dom lacks (form
// associated elements), so leave them as plain unknown elements.
vi.mock("@awesome.me/webawesome/dist/components/button/button.js", () => ({}));
vi.mock(
  "@awesome.me/webawesome/dist/components/callout/callout.js",
  () => ({}),
);
vi.mock("@awesome.me/webawesome/dist/components/dialog/dialog.js", () => ({}));

import type { ConfirmDialog } from "./confirm-dialog.ts";
import "./confirm-dialog.ts";

async function render() {
  let dialog = document.createElement("confirm-dialog") as ConfirmDialog;
  document.body.append(dialog);
  await dialog.updateComplete;
  return dialog;
}

function parts(dialog: ConfirmDialog) {
  let root = dialog.shadowRoot!;
  let waDialog = root.querySelector("wa-dialog")!;
  let buttons = [...root.querySelectorAll<HTMLElement>("wa-button")];
  return { waDialog, buttons };
}

let options = {
  title: "Delete source",
  message: "Delete it?",
  confirm: "Delete",
  danger: true,
};

afterEach(() => {
  document.body.replaceChildren();
});

describe("confirm-dialog", () => {
  it("runs the action when confirmed", async () => {
    let dialog = await render();
    let action = vi.fn(async () => {});
    dialog.ask(options, action);
    await dialog.updateComplete;

    let { waDialog, buttons } = parts(dialog);
    expect(waDialog.getAttribute("label")).toBe("Delete source");
    expect(waDialog.textContent).toContain("Delete it?");
    expect(buttons.map((b) => b.textContent!.trim())).toEqual([
      "Cancel",
      "Delete",
    ]);
    expect(buttons[1].getAttribute("variant")).toBe("danger");

    buttons[1].click();
    await vi.waitFor(() => expect(waDialog.hasAttribute("open")).toBe(false));
    expect(action).toHaveBeenCalledOnce();
  });

  it("doesn't run the action when cancelled", async () => {
    let dialog = await render();
    let action = vi.fn(async () => {});
    dialog.ask(options, action);
    await dialog.updateComplete;

    let { waDialog, buttons } = parts(dialog);
    buttons[0].click();
    await dialog.updateComplete;
    expect(waDialog.hasAttribute("open")).toBe(false);
    expect(action).not.toHaveBeenCalled();
  });

  it("shows errors and stays open", async () => {
    let dialog = await render();
    dialog.ask(options, async () => {
      throw new Error("these sources use this account");
    });
    await dialog.updateComplete;

    let { waDialog, buttons } = parts(dialog);
    buttons[1].click();
    await vi.waitFor(() =>
      expect(waDialog.querySelector("wa-callout")?.textContent).toContain(
        "these sources use this account",
      ),
    );
    expect(waDialog.hasAttribute("open")).toBe(true);
  });

  it("explains without offering an action", async () => {
    let dialog = await render();
    dialog.explain("Account in use", "Change the sources first.");
    await dialog.updateComplete;

    let { waDialog, buttons } = parts(dialog);
    expect(waDialog.textContent).toContain("Change the sources first.");
    expect(buttons.map((b) => b.textContent!.trim())).toEqual(["OK"]);
    buttons[0].click();
    await dialog.updateComplete;
    expect(waDialog.hasAttribute("open")).toBe(false);
  });
});
