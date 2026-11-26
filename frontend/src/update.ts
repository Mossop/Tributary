/** Detects a new build of the frontend on the server.
 *
 * Built assets have content-hashed names, so a new build's `index.html`
 * references different scripts or stylesheets from the page that's loaded.
 */

/** The scripts and stylesheets a page loads, as written in its HTML. */
export function assets(doc: Document): string[] {
  return [
    ...doc.querySelectorAll("script[src], link[rel=stylesheet][href]"),
  ].map((el) => el.getAttribute("src") ?? el.getAttribute("href") ?? "");
}

/** Whether the server's `index.html` loads different assets from
 * `current`. Pages that load no scripts (such as error pages) don't count. */
export function changed(current: Document, html: string): boolean {
  let latest = assets(new DOMParser().parseFromString(html, "text/html"));
  return (
    latest.some((a) => a.endsWith(".js")) &&
    latest.join("\n") != assets(current).join("\n")
  );
}

/** Fetches the server's `index.html` and checks whether it has changed. */
export async function frontendChanged(): Promise<boolean> {
  let response = await fetch("/", { cache: "no-store" });
  if (!response.ok) {
    return false;
  }
  return changed(document, await response.text());
}
