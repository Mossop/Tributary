import type { Item, ItemKind } from "./api.ts";

export interface KindInfo {
  kind: ItemKind;
  /** Plural, e.g. "Tasks". */
  label: string;
  /** Material Symbol name. */
  icon: string;
}

/** Every item kind, in the order they're shown. */
export const ITEM_KINDS: KindInfo[] = [
  { kind: "task", label: "Tasks", icon: "task_alt" },
  { kind: "issue", label: "Issues", icon: "bug_report" },
  { kind: "patch", label: "Patches", icon: "merge" },
];

/** The items whose kind isn't hidden. */
export function visibleItems(items: Item[], hidden: ItemKind[]): Item[] {
  return items.filter((item) => !hidden.includes(item.kind));
}

/** Hides a shown kind, or shows a hidden one. Kept in `ITEM_KINDS` order. */
export function toggleKind(hidden: ItemKind[], kind: ItemKind): ItemKind[] {
  let hide = !hidden.includes(kind);
  return ITEM_KINDS.map((k) => k.kind).filter((k) =>
    k == kind ? hide : hidden.includes(k),
  );
}

/** The kinds to hide so only `kind` is shown. */
export function onlyKind(kind: ItemKind): ItemKind[] {
  return ITEM_KINDS.map((k) => k.kind).filter((k) => k != kind);
}
