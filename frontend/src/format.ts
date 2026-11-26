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

export interface DatePreset {
  label: string;
  date: Date;
}

/** 9am, `days` days after `now`'s date. */
function morning(now: Date, days: number): Date {
  let date = new Date(now);
  date.setDate(date.getDate() + days);
  date.setHours(9, 0, 0, 0);
  return date;
}

/**
 * Common future dates, all at 9am local time, in date order: tomorrow, the
 * weekend (Saturday; "Next weekend" during one), next week (Monday) and,
 * with `nextMonth`, the 1st of next month. A date that's the same as an
 * earlier one is left out, e.g. the weekend on a Friday, when Saturday is
 * tomorrow.
 */
export function datePresets(now: Date, nextMonth: boolean): DatePreset[] {
  let today = now.getDay();
  let weekend = today == 6 || today == 0;
  // The next Saturday and Monday at least a day away.
  let toSaturday = (6 - today + 7) % 7 || 7;
  let toMonday = (8 - today) % 7 || 7;

  let presets: DatePreset[] = [
    { label: "Tomorrow", date: morning(now, 1) },
    {
      label: weekend ? "Next weekend" : "This weekend",
      date: morning(now, toSaturday),
    },
    { label: "Next week", date: morning(now, toMonday) },
  ];
  if (nextMonth) {
    let first = new Date(now.getFullYear(), now.getMonth() + 1, 1, 9);
    presets.push({ label: "Next month", date: first });
  }
  return inDateOrder(presets);
}

/** `presets` in date order, leaving out any with the same date as an
 * earlier one. */
function inDateOrder(presets: DatePreset[]): DatePreset[] {
  let sorted = [...presets].sort((a, b) => a.date.getTime() - b.date.getTime());
  return sorted.filter(
    (preset, index) =>
      index == 0 || preset.date.getTime() != sorted[index - 1].date.getTime(),
  );
}

/** 8am local time, `days` days before `date`'s date. */
function daysBefore(date: Date, days: number): Date {
  let before = new Date(date);
  before.setDate(before.getDate() - days);
  before.setHours(8, 0, 0, 0);
  return before;
}

/**
 * Times to snooze until. For an item `due`, these include 8am a day and a
 * week before it, when those are still to come. They're fixed times: moving
 * the due date afterwards doesn't move the snooze.
 */
export function snoozePresets(
  now: Date,
  due: Date | null = null,
): DatePreset[] {
  let presets = datePresets(now, true);
  if (due) {
    for (let [label, days] of [
      ["One day before due", 1],
      ["One week before due", 7],
    ] as const) {
      let date = daysBefore(due, days);
      if (date > now) {
        presets.push({ label, date });
      }
    }
  }
  return inDateOrder(presets);
}

/** Dates to set as due. */
export function duePresets(now: Date): DatePreset[] {
  return datePresets(now, false);
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
