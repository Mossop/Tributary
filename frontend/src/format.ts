import type { Item } from "./api.ts";

/** The due date to use for an item: the user's override, else the source's. */
export function effectiveDue(item: Item): Date | null {
  let due = item.due_override ?? item.due;
  return due ? new Date(due) : null;
}

export function isOverdue(item: Item, now: Date): boolean {
  let due = effectiveDue(item);
  return due != null && due < now;
}

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 24 * 60 * 60 * 1000],
  ["month", 30 * 24 * 60 * 60 * 1000],
  ["week", 7 * 24 * 60 * 60 * 1000],
  ["day", 24 * 60 * 60 * 1000],
  ["hour", 60 * 60 * 1000],
  ["minute", 60 * 1000],
];

/** Formats a time relative to now, e.g. "3 hours ago" or "in 2 days". */
export function relativeTime(date: Date, now: Date, locale?: string): string {
  let format = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  let diff = date.getTime() - now.getTime();
  for (let [unit, ms] of UNITS) {
    if (Math.abs(diff) >= ms) {
      return format.format(Math.round(diff / ms), unit);
    }
  }
  return format.format(0, "minute");
}

/** Formats an absolute date and time for tooltips and labels. */
export function absoluteTime(date: Date, locale?: string): string {
  return date.toLocaleString(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

export interface SnoozePreset {
  label: string;
  until: Date;
}

/** Common times to snooze until, in local time. */
export function snoozePresets(now: Date): SnoozePreset[] {
  let laterToday = new Date(now.getTime() + 3 * 60 * 60 * 1000);

  let tomorrow = new Date(now);
  tomorrow.setDate(tomorrow.getDate() + 1);
  tomorrow.setHours(9, 0, 0, 0);

  // Next Monday, at least one day away.
  let nextWeek = new Date(now);
  let daysUntilMonday = (8 - nextWeek.getDay()) % 7 || 7;
  nextWeek.setDate(nextWeek.getDate() + daysUntilMonday);
  nextWeek.setHours(9, 0, 0, 0);

  return [
    { label: "Later today", until: laterToday },
    { label: "Tomorrow", until: tomorrow },
    { label: "Next week", until: nextWeek },
  ];
}

function pad(n: number) {
  return String(n).padStart(2, "0");
}

/** Formats a date as the local time value used by `datetime-local` inputs. */
export function toLocalInput(date: Date): string {
  return (
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}` +
    `T${pad(date.getHours())}:${pad(date.getMinutes())}`
  );
}

/** Parses a `datetime-local` input value, returning null if invalid. */
export function fromLocalInput(value: string): Date | null {
  if (!value) {
    return null;
  }
  let date = new Date(value);
  return isNaN(date.getTime()) ? null : date;
}
