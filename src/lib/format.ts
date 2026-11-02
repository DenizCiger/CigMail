import type { Account, Address } from "./types";

const timeFmt = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" });
const dayFmt = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" });
const fullFmt = new Intl.DateTimeFormat(undefined, { year: "numeric", month: "short", day: "numeric" });
const longFmt = new Intl.DateTimeFormat(undefined, {
  weekday: "short",
  year: "numeric",
  month: "short",
  day: "numeric",
  hour: "2-digit",
  minute: "2-digit",
});
const weekdayFmt = new Intl.DateTimeFormat(undefined, { weekday: "short", hour: "2-digit", minute: "2-digit" });

export function shortDate(unix: number): string {
  const d = new Date(unix * 1000);
  const now = new Date();
  if (d.toDateString() === now.toDateString()) return timeFmt.format(d);
  if (d.getFullYear() === now.getFullYear()) return dayFmt.format(d);
  return fullFmt.format(d);
}

export function longDate(unix: number): string {
  return longFmt.format(new Date(unix * 1000));
}

export function relativeFuture(unix: number): string {
  const d = new Date(unix * 1000);
  const now = new Date();
  const tomorrow = new Date(now);
  tomorrow.setDate(now.getDate() + 1);
  if (d.toDateString() === now.toDateString()) return `Today ${timeFmt.format(d)}`;
  if (d.toDateString() === tomorrow.toDateString()) return `Tomorrow ${timeFmt.format(d)}`;
  if (d.getTime() - now.getTime() < 6 * 86400_000) return weekdayFmt.format(d);
  return `${dayFmt.format(d)} ${timeFmt.format(d)}`;
}

export function displayName(a: Address | undefined, me?: string[]): string {
  if (!a) return "";
  if (me?.some((m) => m.toLowerCase() === a.email.toLowerCase())) return "me";
  if (a.name && a.name.trim()) return a.name.trim();
  return a.email.split("@")[0];
}

export function firstName(a: Address | undefined, me?: string[]): string {
  const n = displayName(a, me);
  if (n === "me") return n;
  return n.includes("@") ? n : n.split(/\s+/)[0];
}

export function initials(a: Address | undefined): string {
  if (!a) return "?";
  const n = (a.name || a.email).replace(/["']/g, "").trim();
  const parts = n.split(/[\s.@_-]+/).filter(Boolean);
  if (parts.length >= 2 && a.name) return (parts[0][0] + parts[1][0]).toUpperCase();
  return (parts[0]?.[0] ?? "?").toUpperCase();
}

const AVATAR_COLORS = ["#5e6ad2", "#26b5ce", "#4cb782", "#d9a62e", "#f2994a", "#eb5757", "#a77cf0", "#e06eb1", "#3fa7a0"];

export function colorFor(s: string): string {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) | 0;
  return AVATAR_COLORS[Math.abs(h) % AVATAR_COLORS.length];
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

export function accountLabel(a: Account): string {
  return a.label || a.email;
}

export function formatAddress(a: Address): string {
  return a.name ? `${a.name} <${a.email}>` : a.email;
}

/** Parses "Name <a@b.c>, x@y.z" style input. */
export function parseAddresses(input: string): Address[] {
  return input
    .split(/[,;\n]/)
    .map((s) => s.trim())
    .filter(Boolean)
    .map((s) => {
      const m = s.match(/^(.*)<([^>]+)>$/);
      if (m) return { name: m[1].trim().replace(/^"|"$/g, "") || null, email: m[2].trim() };
      return { name: null, email: s };
    });
}

export const isMac = navigator.userAgent.includes("Mac");
export const mod = isMac ? "⌘" : "Ctrl";

export function textToHtml(t: string): string {
  const esc = t.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  return esc
    .split(/\n{2,}/)
    .map((p) => `<p>${p.replace(/\n/g, "<br>")}</p>`)
    .join("");
}
