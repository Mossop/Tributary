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
  "@awesome.me/webawesome/dist/components/dropdown/dropdown.js",
  () => ({}),
);
vi.mock(
  "@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js",
  () => ({}),
);
vi.mock("@awesome.me/webawesome/dist/components/icon/icon.js", () => ({}));
vi.mock("@awesome.me/webawesome/dist/components/input/input.js", () => ({}));
vi.mock("@awesome.me/webawesome/dist/components/option/option.js", () => ({}));
vi.mock("@awesome.me/webawesome/dist/components/select/select.js", () => ({}));

import type { Account, SourceType } from "../api.ts";
import type { Store } from "../store.ts";
import { makeSource, mockFetch } from "../test-helpers.ts";
import type { SettingsPage } from "./settings-page.ts";
import "./settings-page.ts";

let account: Account = {
  id: "a1",
  provider: "google",
  name: "me@example.com",
  api_url: null,
  missing_scopes: false,
  revoked: false,
};

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
  ],
};

async function render(sources = [makeSource()]) {
  let page = document.createElement("settings-page") as SettingsPage;
  let store = { loadSettings: vi.fn(async () => {}) };
  Object.assign(page, {
    store: store as unknown as Store,
    contexts: [{ id: "work", name: "Work", icon: null, hidden_kinds: [] }],
    sources,
    sourceTypes: [tasks],
    accounts: [account],
    providers: [
      {
        id: "github",
        name: "GitHub",
        icon: "https://github.com/favicon.ico",
        connect: "token",
        configured: true,
        token_hint: "A personal access token.",
        api_url_placeholder: "https://api.github.com",
      },
      {
        id: "phabricator",
        name: "Phabricator",
        icon: "https://phab.test/favicon",
        connect: "token",
        configured: true,
        token_hint: "A Conduit API token.",
        api_url_placeholder: "https://phabricator.services.mozilla.com/api",
      },
    ],
  });
  document.body.append(page);
  await page.updateComplete;
  return { page, store };
}

/** Clicks the button with `title` in the row containing `text`. */
function clickInRow(page: SettingsPage, text: string, title: string) {
  let row = [...page.shadowRoot!.querySelectorAll(".row")].find((r) =>
    r.textContent!.includes(text),
  )!;
  row.querySelector<HTMLElement>(`wa-button[title='${title}']`)!.click();
}

async function confirmDialog(page: SettingsPage) {
  let dialog = page.shadowRoot!.querySelector("confirm-dialog")!;
  await dialog.updateComplete;
  let waDialog = dialog.shadowRoot!.querySelector("wa-dialog")!;
  let buttons = [
    ...dialog.shadowRoot!.querySelectorAll<HTMLElement>("wa-button"),
  ];
  return { waDialog, buttons };
}

afterEach(() => {
  document.body.replaceChildren();
  vi.unstubAllGlobals();
});

describe("settings-page", () => {
  it("refuses to remove an account a source uses", async () => {
    let calls = mockFetch();
    let source = makeSource({
      name: "My tasks",
      type: "google-tasks",
      config: { account: "a1" },
    });
    let { page } = await render([source]);

    clickInRow(page, "me@example.com", "Remove");
    let { waDialog, buttons } = await confirmDialog(page);
    expect(waDialog.hasAttribute("open")).toBe(true);
    expect(waDialog.getAttribute("label")).toBe("Account in use");
    expect(waDialog.textContent).toContain("“My tasks”");
    expect(buttons.map((b) => b.textContent!.trim())).toEqual(["OK"]);
    expect(calls).toEqual([]);
  });

  it("removes an unused account once confirmed", async () => {
    let calls = mockFetch();
    let { page, store } = await render();

    clickInRow(page, "me@example.com", "Remove");
    let { waDialog, buttons } = await confirmDialog(page);
    expect(waDialog.getAttribute("label")).toBe("Remove account");
    expect(calls).toEqual([]);

    buttons.find((b) => b.textContent!.trim() == "Remove")!.click();
    await vi.waitFor(() => expect(store.loadSettings).toHaveBeenCalled());
    expect(calls.map((c) => `${c.method} ${c.url}`)).toEqual([
      "DELETE /api/accounts/a1",
    ]);
  });

  it("adds GitHub accounts from a token", async () => {
    let calls = mockFetch();
    let { page, store } = await render();

    page
      .shadowRoot!.querySelector("wa-dropdown")!
      .dispatchEvent(
        new CustomEvent("wa-select", { detail: { item: { value: "github" } } }),
      );
    let dialog = page.shadowRoot!.querySelector("token-account-dialog")!;
    await dialog.updateComplete;
    let waDialog = dialog.shadowRoot!.querySelector("wa-dialog")!;
    expect(waDialog.getAttribute("label")).toBe("Add GitHub account");

    let [token, apiUrl] = [
      ...dialog.shadowRoot!.querySelectorAll<HTMLInputElement>("wa-input"),
    ];
    token.value = " ghp_secret ";
    token.dispatchEvent(new Event("input"));
    apiUrl.value = "";
    apiUrl.dispatchEvent(new Event("input"));
    dialog
      .shadowRoot!.querySelector<HTMLElement>("wa-button[variant='brand']")!
      .click();

    await vi.waitFor(() => expect(store.loadSettings).toHaveBeenCalled());
    expect(calls).toEqual([
      {
        method: "POST",
        url: "/api/accounts/github",
        body: { token: "ghp_secret", api_url: null },
      },
    ]);
  });

  it("shows when a GitHub token is rejected", async () => {
    mockFetch({
      "POST /api/accounts/github": new Response(
        JSON.stringify({ error: "GitHub didn't accept the token" }),
        { status: 400 },
      ),
    });
    let { page } = await render();
    page
      .shadowRoot!.querySelector("wa-dropdown")!
      .dispatchEvent(
        new CustomEvent("wa-select", { detail: { item: { value: "github" } } }),
      );
    let dialog = page.shadowRoot!.querySelector("token-account-dialog")!;
    await dialog.updateComplete;
    dialog
      .shadowRoot!.querySelector<HTMLElement>("wa-button[variant='brand']")!
      .click();
    await vi.waitFor(() =>
      expect(
        dialog.shadowRoot!.querySelector("wa-callout")?.textContent,
      ).toContain("GitHub didn't accept the token"),
    );
    expect(
      dialog.shadowRoot!.querySelector("wa-dialog")!.hasAttribute("open"),
    ).toBe(true);
  });

  it("adds Phabricator accounts with its own hints", async () => {
    let calls = mockFetch();
    let { page, store } = await render();
    page.shadowRoot!.querySelector("wa-dropdown")!.dispatchEvent(
      new CustomEvent("wa-select", {
        detail: { item: { value: "phabricator" } },
      }),
    );
    let dialog = page.shadowRoot!.querySelector("token-account-dialog")!;
    await dialog.updateComplete;
    let root = dialog.shadowRoot!;
    expect(root.querySelector("wa-dialog")!.getAttribute("label")).toBe(
      "Add Phabricator account",
    );
    let [token, apiUrl] = [
      ...root.querySelectorAll<HTMLInputElement>("wa-input"),
    ];
    expect(token.getAttribute("hint")).toBe("A Conduit API token.");
    expect(apiUrl.getAttribute("placeholder")).toBe(
      "https://phabricator.services.mozilla.com/api",
    );

    token.value = "api-abc";
    token.dispatchEvent(new Event("input"));
    root.querySelector<HTMLElement>("wa-button[variant='brand']")!.click();
    await vi.waitFor(() => expect(store.loadSettings).toHaveBeenCalled());
    expect(calls).toEqual([
      {
        method: "POST",
        url: "/api/accounts/phabricator",
        body: { token: "api-abc", api_url: null },
      },
    ]);
  });

  it("asks before deleting a source", async () => {
    let calls = mockFetch();
    let { page } = await render([makeSource({ id: "gh", name: "PRs" })]);

    clickInRow(page, "PRs", "Delete");
    let { waDialog, buttons } = await confirmDialog(page);
    expect(waDialog.getAttribute("label")).toBe("Delete source");
    buttons.find((b) => b.textContent!.trim() == "Cancel")!.click();
    expect(calls).toEqual([]);
  });
});
