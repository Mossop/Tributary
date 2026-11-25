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

export type TextPart = { text: string } | { url: string };

/** Matches http(s) URLs up to whitespace. */
const URL_PATTERN = /https?:\/\/[^\s<>"]+/g;

/**
 * Splits text into plain text and the URLs in it. Punctuation that ends a
 * sentence or closes a bracket is left off the end of a URL, unless the URL
 * opened that bracket itself.
 */
export function linkify(text: string): TextPart[] {
  let parts: TextPart[] = [];
  let last = 0;
  for (let match of text.matchAll(URL_PATTERN)) {
    let url = trimUrl(match[0]);
    let start = match.index;
    if (start > last) {
      parts.push({ text: text.slice(last, start) });
    }
    parts.push({ url });
    last = start + url.length;
  }
  if (last < text.length) {
    parts.push({ text: text.slice(last) });
  }
  return parts;
}

function trimUrl(url: string): string {
  while (url.length > 0) {
    let end = url[url.length - 1];
    if (".,;:!?'".includes(end)) {
      url = url.slice(0, -1);
    } else if (end == ")" && count(url, "(") < count(url, ")")) {
      url = url.slice(0, -1);
    } else {
      return url;
    }
  }
  return url;
}

function count(text: string, char: string): number {
  return text.split(char).length - 1;
}
