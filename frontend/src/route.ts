/**
 * URL scheme:
 *   /c/<context>          items in a context
 *   /c/<context>/snoozed  snoozed items in a context
 */

export interface Route {
  context: string | null;
  snoozed: boolean;
}

export function parseRoute(pathname: string): Route {
  let match = /^\/c\/([^/]+)(\/snoozed)?\/?$/.exec(pathname);
  if (!match) {
    return { context: null, snoozed: false };
  }
  return { context: decodeURIComponent(match[1]), snoozed: !!match[2] };
}

export function routeUrl(route: Route): string {
  if (route.context == null) {
    return "/";
  }
  let base = `/c/${encodeURIComponent(route.context)}`;
  return route.snoozed ? `${base}/snoozed` : base;
}
