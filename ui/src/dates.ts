// Journal dates for display. Rust owns the data: a journal's page title stays what
// the vault's title pattern makes it (Logseq's `MMM do, yyyy` by default), and links
// keep resolving by that name. The screen shows the date itself instead, read from
// the journal's file name, so a Chinese vault reads in Chinese.

export type Day = { year: number; month: number; day: number };

const JOURNAL_FILE = /^journals\/(\d{4})[-_.](\d{2})[-_.](\d{2})\.md$/;

const MONTHS = ["一月", "二月", "三月", "四月", "五月", "六月", "七月", "八月", "九月", "十月", "十一月", "十二月"];
const WEEKDAYS = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];

/** The day a journal file stands for, or `null` for a page that is not a journal. */
export function dayOf(rel: string): Day | null {
  const m = JOURNAL_FILE.exec(rel);
  if (!m) return null;
  const year = Number(m[1]);
  const month = Number(m[2]);
  const day = Number(m[3]);
  if (month < 1 || month > 12 || day < 1 || day > 31) return null;
  return { year, month, day };
}

export function monthName(month: number): string {
  return MONTHS[month - 1] ?? "";
}

export function weekdayOf(d: Day): string {
  return WEEKDAYS[new Date(Date.UTC(d.year, d.month - 1, d.day)).getUTCDay()] ?? "";
}

/** "11月1日 周日" */
export function dayLabel(d: Day): string {
  return `${d.month}月${d.day}日 ${weekdayOf(d)}`;
}

/** What the screen calls a page: a journal's date, or any other page's title. */
export function pageLabel(rel: string, title: string): string {
  const d = dayOf(rel);
  return d ? dayLabel(d) : title;
}
