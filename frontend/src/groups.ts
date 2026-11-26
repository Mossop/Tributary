import type { Item, ItemKind } from "./api.ts";

/** Which kind of item leads a group: issues, then tasks, then patches. */
const LEAD_ORDER: ItemKind[] = ["issue", "task", "patch"];

/**
 * Groups items that share references, directly or through other items, so
 * a bug, a patch naming it and a task linking to the patch end up together.
 *
 * `items` must be in list order. Groups are ordered by their highest-placed
 * member. Each group starts with its lead, the item of the best kind (ties
 * go to the higher-placed item), followed by the rest in list order.
 */
export function groupItems(items: Item[]): Item[][] {
  // Union-find over item positions.
  let parent = items.map((_, index) => index);
  let find = (index: number): number => {
    while (parent[index] != index) {
      parent[index] = parent[parent[index]];
      index = parent[index];
    }
    return index;
  };
  let owner = new Map<string, number>();
  items.forEach((item, index) => {
    for (let reference of item.references) {
      let other = owner.get(reference);
      if (other == undefined) {
        owner.set(reference, index);
      } else {
        // Keep the higher-placed item as the root, so a group's root is its
        // first member.
        let [a, b] = [find(index), find(other)];
        parent[Math.max(a, b)] = Math.min(a, b);
      }
    }
  });

  let groups = new Map<number, Item[]>();
  items.forEach((item, index) => {
    let root = find(index);
    let group = groups.get(root);
    if (group) {
      group.push(item);
    } else {
      groups.set(root, [item]);
    }
  });

  // Maps preserve insertion order, which is the order of each group's first
  // member.
  return [...groups.values()].map((group) => {
    let lead = group.reduce((best, item) =>
      LEAD_ORDER.indexOf(item.kind) < LEAD_ORDER.indexOf(best.kind)
        ? item
        : best,
    );
    return [lead, ...group.filter((item) => item != lead)];
  });
}
