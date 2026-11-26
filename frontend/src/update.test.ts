import { describe, expect, it } from "vitest";

import { assets, changed } from "./update.ts";

function page(script: string, style = "/assets/index-a.css"): string {
  return `<!doctype html><html><head>
    <link rel="icon" href="/favicon.ico" />
    <script type="module" src="${script}"></script>
    <link rel="stylesheet" href="${style}" />
  </head><body></body></html>`;
}

function parse(html: string): Document {
  return new DOMParser().parseFromString(html, "text/html");
}

describe("update", () => {
  it("lists scripts and stylesheets", () => {
    expect(assets(parse(page("/assets/index-a.js")))).toEqual([
      "/assets/index-a.js",
      "/assets/index-a.css",
    ]);
  });

  it("notices new builds", () => {
    let current = parse(page("/assets/index-a.js"));
    expect(changed(current, page("/assets/index-a.js"))).toBe(false);
    expect(changed(current, page("/assets/index-b.js"))).toBe(true);
    expect(
      changed(current, page("/assets/index-a.js", "/assets/index-b.css")),
    ).toBe(true);
  });

  it("ignores pages without scripts", () => {
    let current = parse(page("/assets/index-a.js"));
    expect(changed(current, "")).toBe(false);
    expect(changed(current, "<h1>Bad gateway</h1>")).toBe(false);
  });
});
