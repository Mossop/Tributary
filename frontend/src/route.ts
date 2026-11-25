/**
 * URL scheme:
 *   /c/<context>          items in a context
 *   /c/<context>/snoozed  snoozed items in a context
 *   /settings             contexts and sources
 */

export type View = "items" | "snoozed" | "settings";

export interface Route {
  view: View;
  /** Null for settings, or when no context is in the URL. */
  context: string | null;
}

export function parseRoute(pathname: string): Route {
  if (/^\/settings\/?$/.test(pathname)) {
    return { view: "settings", context: null };
  }
  let match = /^\/c\/([^/]+)(\/snoozed)?\/?$/.exec(pathname);
  if (!match) {
    return { view: "items", context: null };
  }
  return {
    view: match[2] ? "snoozed" : "items",
    context: decodeURIComponent(match[1]),
  };
}

export function routeUrl(route: Route): string {
  if (route.view == "settings") {
    return "/settings";
  }
  if (route.context == null) {
    return "/";
  }
  let base = `/c/${encodeURIComponent(route.context)}`;
  return route.view == "snoozed" ? `${base}/snoozed` : base;
}
