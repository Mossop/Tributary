import type { Item } from "./api.ts";

/** Midnight local time, `days` days after `now`'s date. */
export function startOfDay(now: Date, days = 0): Date {
  let date = new Date(now);
  date.setDate(date.getDate() + days);
  date.setHours(0, 0, 0, 0);
  return date;
}

/** Whether an item is one of today's: queued for today or an earlier day.
 * It stays one until it's taken out of the queue. */
export function isToday(item: Item, now: Date): boolean {
  return item.queued_for != null && new Date(item.queued_for) <= now;
}

/** Whether an item is queued for a day that hasn't started yet. */
export function isQueuedLater(item: Item, now: Date): boolean {
  return item.queued_for != null && new Date(item.queued_for) > now;
}

/** Splits items into today's and the rest, keeping their order. */
export function splitToday(items: Item[], now: Date): [Item[], Item[]] {
  let today: Item[] = [];
  let rest: Item[] = [];
  for (let item of items) {
    (isToday(item, now) ? today : rest).push(item);
  }
  return [today, rest];
}
