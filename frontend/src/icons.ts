import { registerIconLibrary } from "@awesome.me/webawesome";

const SYMBOLS_PATH = import.meta.env.DEV
  ? "/node_modules/@material-symbols/svg-400/outlined/"
  : "/material-symbols/";

/** Makes Material Symbols the default icon library for `<wa-icon>`. */
export function registerMaterialSymbols() {
  registerIconLibrary("default", {
    resolver: (name) => `${SYMBOLS_PATH}${name}.svg`,
    mutator: (svg) => svg.setAttribute("fill", "currentColor"),
  });
}

/**
 * Source and item icons are either a URL or `symbol:<name>` for a Material
 * Symbol.
 */
export type ParsedIcon = { symbol: string } | { url: string };

export function parseIcon(icon: string): ParsedIcon {
  if (icon.startsWith("symbol:")) {
    return { symbol: icon.slice("symbol:".length) };
  }
  return { url: icon };
}
