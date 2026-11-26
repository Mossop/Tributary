import { describe, expect, it } from "vitest";

import type { Account, SourceType } from "./api.ts";
import {
  accountsFor,
  connectError,
  defaultAccounts,
  dueFrom,
  initialValues,
  slugify,
  sourceSettings,
  sourcesUsing,
} from "./settings-form.ts";
import { makeSource } from "./test-helpers.ts";

let github: SourceType = {
  id: "github",
  name: "GitHub search",
  due_from: [{ value: "modified", label: "Last modified" }],
  fields: [
    {
      key: "query",
      label: "Search query",
      kind: "text",
      required: true,
      placeholder: null,
      help: null,
      provider: null,
    },
    {
      key: "token",
      label: "Access token",
      kind: "secret",
      required: true,
      placeholder: null,
      help: null,
      provider: null,
    },
    {
      key: "api_url",
      label: "API URL",
      kind: "url",
      required: false,
      placeholder: null,
      help: null,
      provider: null,
    },
  ],
};

describe("slugify", () => {
  it("makes URL names", () => {
    expect(slugify("Work")).toBe("work");
    expect(slugify("  Side Projects!  ")).toBe("side-projects");
    expect(slugify("Café & Bar")).toBe("cafe-bar");
    expect(slugify("2026 Plans")).toBe("2026-plans");
    expect(slugify("!!!")).toBe("");
  });
});

describe("initialValues", () => {
  it("defaults a new source", () => {
    expect(initialValues(null, "work")).toEqual({
      name: "",
      contexts: ["work"],
      fields: {},
      dueDays: "",
      businessDays: false,
      dueFrom: "",
    });
    expect(initialValues(null, null).contexts).toEqual([]);
  });

  it("loads an existing source", () => {
    let source = makeSource({
      name: "PRs",
      contexts: ["work", "home"],
      config: { query: "is:pr", api_url: null },
    });
    expect(initialValues(source, "work")).toEqual({
      name: "PRs",
      contexts: ["work", "home"],
      fields: { query: "is:pr", api_url: "" },
      dueDays: "",
      businessDays: false,
      dueFrom: "",
    });
  });
});

describe("sourceSettings", () => {
  let values = {
    name: " PRs ",
    contexts: ["work"],
    fields: { query: " is:pr ", token: "", api_url: "", stray: "x" },
    dueDays: "",
    businessDays: false,
    dueFrom: "",
  };

  it("builds settings for a new source", () => {
    expect(sourceSettings(github, values, true)).toEqual({
      type: "github",
      name: "PRs",
      contexts: ["work"],
      config: { query: "is:pr" },
      default_due: null,
    });
  });

  it("omits the type and empty secrets when updating", () => {
    let settings = sourceSettings(
      github,
      { ...values, fields: { ...values.fields, token: "new" } },
      false,
    );
    expect(settings.type).toBeUndefined();
    expect(settings.config).toEqual({ query: "is:pr", token: "new" });
  });
});

let githubType: SourceType = {
  id: "github",
  name: "GitHub",
  fields: [],
  due_from: [],
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

function account(id: string, provider = "google"): Account {
  return {
    id,
    provider,
    name: `${id}@example.com`,
    api_url: null,
    missing_scopes: false,
    revoked: false,
  };
}

describe("accounts", () => {
  it("filters accounts by provider", () => {
    let accounts = [account("a"), account("b", "other")];
    expect(accountsFor("google", accounts)).toEqual([account("a")]);
    expect(accountsFor(null, accounts)).toEqual([]);
  });

  it("chooses the only account", () => {
    let values = initialValues(null, "work");
    expect(defaultAccounts(tasks, values, [account("a")]).fields).toEqual({
      account: "a",
    });
    expect(
      defaultAccounts(tasks, values, [account("a"), account("b")]).fields,
    ).toEqual({});
    expect(
      defaultAccounts(tasks, values, [account("a", "other")]).fields,
    ).toEqual({});
  });

  it("keeps a chosen account", () => {
    let values = { ...initialValues(null, null), fields: { account: "b" } };
    expect(defaultAccounts(tasks, values, [account("a")]).fields).toEqual({
      account: "b",
    });
  });

  it("sends account and list settings", () => {
    let values = {
      name: "Tasks",
      contexts: ["work"],
      fields: { account: "a", list: "*" },
      dueDays: "",
      businessDays: false,
      dueFrom: "",
    };
    expect(sourceSettings(tasks, values, true).config).toEqual({
      account: "a",
      list: "*",
    });
  });

  it("finds the sources using an account", () => {
    let uses = makeSource({
      id: "s1",
      type: "google-tasks",
      config: { account: "a", list: "*" },
    });
    let other = makeSource({
      id: "s2",
      type: "google-tasks",
      config: { account: "b", list: "*" },
    });
    let github = makeSource({ id: "s3", config: { query: "a" } });
    expect(
      sourcesUsing(account("a"), [uses, other, github], [tasks, githubType]),
    ).toEqual([uses]);
  });

  it("reads connect errors", () => {
    expect(connectError("?error=Access+wasn%27t+granted")).toBe(
      "Access wasn't granted",
    );
    expect(connectError("")).toBeNull();
  });
});

describe("default due dates", () => {
  let due: SourceType = {
    ...github,
    due_from: [
      { value: "requested", label: "Request made" },
      { value: "created", label: "Created" },
      { value: "modified", label: "Last modified" },
    ],
  };

  it("loads a source's default due date", () => {
    let source = makeSource({
      default_due: { days: 3, business_days: true, from: "created" },
    });
    expect(initialValues(source, null)).toMatchObject({
      dueDays: "3",
      businessDays: true,
      dueFrom: "created",
    });
    expect(initialValues(makeSource(), null)).toMatchObject({
      dueDays: "",
      businessDays: false,
      dueFrom: "",
    });
    expect(initialValues(null, null)).toMatchObject({
      dueDays: "",
      dueFrom: "",
    });
  });

  it("counts from the type's first time unless one is chosen", () => {
    let values = initialValues(null, null);
    expect(dueFrom(due, values)).toBe("requested");
    expect(dueFrom(due, { ...values, dueFrom: "modified" })).toBe("modified");
  });

  it("sends it", () => {
    let values = {
      ...initialValues(null, null),
      dueDays: " 5 ",
      businessDays: true,
    };
    expect(sourceSettings(due, values, true).default_due).toEqual({
      days: 5,
      business_days: true,
      from: "requested",
    });
    expect(
      sourceSettings(due, { ...values, dueFrom: "created" }, true).default_due,
    ).toEqual({ days: 5, business_days: true, from: "created" });
    // Blank means none.
    expect(
      sourceSettings(due, { ...values, dueDays: "" }, true).default_due,
    ).toBeNull();
    // Not a whole number: sent as 0 for the server to explain.
    expect(
      sourceSettings(due, { ...values, dueDays: "2.5" }, true).default_due,
    ).toEqual({ days: 0, business_days: true, from: "requested" });
  });
});
