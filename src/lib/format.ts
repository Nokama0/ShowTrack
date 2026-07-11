const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const MONTHS_FULL = [
  "January", "February", "March", "April", "May", "June",
  "July", "August", "September", "October", "November", "December",
];

/** "2026-08-24" -> "Aug 24" (adds the year when it isn't the current one). */
export function fmtDate(iso: string | null | undefined): string {
  if (!iso) return "";
  const [y, m, d] = iso.split("-").map(Number);
  if (!y || !m || !d) return iso;
  const year = new Date().getFullYear() === y ? "" : ` ${y}`;
  return `${MONTHS[m - 1]} ${d}${year}`;
}

/** "2026-08-24" -> "August 2026". */
export function fmtMonth(iso: string): string {
  const [y, m] = iso.split("-").map(Number);
  if (!y || !m) return iso;
  return `${MONTHS_FULL[m - 1]} ${y}`;
}

/** 340 -> "5 h 40 m"; 45 -> "45 m"; 120 -> "2 h". */
export function fmtRuntime(min: number): string {
  if (min <= 0) return "";
  const h = Math.floor(min / 60);
  const m = min % 60;
  if (h === 0) return `${m} m`;
  if (m === 0) return `${h} h`;
  return `${h} h ${m} m`;
}

/** Days until an ISO date: "today", "tomorrow", "in N days"; "" for past or invalid. */
export function relDays(iso: string | null | undefined): string {
  if (!iso) return "";
  const [y, m, d] = iso.split("-").map(Number);
  if (!y || !m || !d) return "";
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const diff = Math.round((new Date(y, m - 1, d).getTime() - today.getTime()) / 86400000);
  if (diff < 0) return "";
  if (diff === 0) return "today";
  if (diff === 1) return "tomorrow";
  return `in ${diff} days`;
}

/** Shared label for an airing season (BingeCard.airing or AiringRow). */
export function airingLabel(a: {
  airedCount: number;
  nextAirDate: string | null;
  finaleDate: string | null;
}): string {
  if (a.airedCount === 0) {
    const d = a.nextAirDate ?? a.finaleDate;
    return d ? `starts ${fmtDate(d)}` : "not yet airing";
  }
  if (a.finaleDate) return `watchable ${fmtDate(a.finaleDate)}`;
  return "finale TBA";
}
