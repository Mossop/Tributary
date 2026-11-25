import { describe, expect, it } from "vitest";

import type { SourceType } from "./api.ts";
import {
  initialValues,
  minutesToMs,
  msToMinutes,
  slugify,
  sourceSettings,
} from "./settings-form.ts";
import { makeSource } from "./test-helpers.ts";

let github: SourceType = {
  id: "github",
  name: "GitHub search",
  fields: [
    {
      key: "query",
      label: "Search query",
      kind: "text",
      required: true,
      placeholder: null,
      help: null,
    },
    {
      key: "token",
      label: "Access token",
      kind: "secret",
      required: true,
      placeholder: null,
      help: null,
    },
    {
      key: "api_url",
      label: "API URL",
      kind: "url",
      required: false,
      placeholder: null,
      help: null,
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

describe("intervals", () => {
  it("converts minutes", () => {
    expect(minutesToMs(5)).toBe(300_000);
    expect(minutesToMs(0.5)).toBe(30_000);
    expect(msToMinutes(1_800_000)).toBe(30);
  });
});

describe("initialValues", () => {
  it("defaults a new source", () => {
    expect(initialValues(null, "work")).toEqual({
      name: "",
      contexts: ["work"],
      intervalMinutes: 5,
      idleIntervalMinutes: 30,
      fields: {},
    });
    expect(initialValues(null, null).contexts).toEqual([]);
  });

  it("loads an existing source", () => {
    let source = makeSource({
      name: "PRs",
      contexts: ["work", "home"],
      interval_ms: 120_000,
      idle_interval_ms: 600_000,
      config: { query: "is:pr", api_url: null },
    });
    expect(initialValues(source, "work")).toEqual({
      name: "PRs",
      contexts: ["work", "home"],
      intervalMinutes: 2,
      idleIntervalMinutes: 10,
      fields: { query: "is:pr", api_url: "" },
    });
  });
});

describe("sourceSettings", () => {
  let values = {
    name: " PRs ",
    contexts: ["work"],
    intervalMinutes: 5,
    idleIntervalMinutes: 30,
    fields: { query: " is:pr ", token: "", api_url: "", stray: "x" },
  };

  it("builds settings for a new source", () => {
    expect(sourceSettings(github, values, true)).toEqual({
      type: "github",
      name: "PRs",
      contexts: ["work"],
      interval_ms: 300_000,
      idle_interval_ms: 1_800_000,
      config: { query: "is:pr" },
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
