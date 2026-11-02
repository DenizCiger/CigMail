import clsx from "clsx";
import { useEffect, useMemo, useRef, useState } from "react";
import { Clock } from "lucide-react";
import { useStore } from "../store";
import { relativeFuture } from "../lib/format";
import { Kbd } from "./ui";

function at(d: Date, h: number, m = 0) {
  const x = new Date(d);
  x.setHours(h, m, 0, 0);
  return x;
}

function addDays(d: Date, n: number) {
  const x = new Date(d);
  x.setDate(x.getDate() + n);
  return x;
}

function presets(now = new Date()) {
  const list: { label: string; key: string; date: Date }[] = [];
  const laterToday = new Date(now.getTime() + 3 * 3600_000);
  if (laterToday.getDate() === now.getDate()) list.push({ label: "Later today", key: "1", date: laterToday });
  if (now.getHours() < 18) list.push({ label: "This evening", key: "2", date: at(now, 19) });
  list.push({ label: "Tomorrow", key: "3", date: at(addDays(now, 1), 8) });
  const day = now.getDay();
  const toSat = (6 - day + 7) % 7 || 7;
  if (day !== 6 && day !== 0) list.push({ label: "This weekend", key: "4", date: at(addDays(now, toSat), 9) });
  const toMon = (8 - day) % 7 || 7;
  list.push({ label: "Next week", key: "5", date: at(addDays(now, toMon), 8) });
  list.push({ label: "In a month", key: "6", date: at(addDays(now, 30), 8) });
  return list.map((p, i) => ({ ...p, key: String(i + 1) }));
}

const WEEKDAYS = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

/** Understands things like "2h", "3 days", "tomorrow 9", "fri 14:30", "next week", "18:00". */
export function parseWhen(input: string, now = new Date()): Date | null {
  const s = input.trim().toLowerCase();
  if (!s) return null;
  let m = s.match(/^(?:in\s+)?(\d+)\s*(m|min|mins|minutes?|h|hrs?|hours?|d|days?|w|weeks?)$/);
  if (m) {
    const n = parseInt(m[1], 10);
    const u = m[2][0];
    const ms = u === "m" ? 60_000 : u === "h" ? 3600_000 : u === "d" ? 86400_000 : 7 * 86400_000;
    const d = new Date(now.getTime() + n * ms);
    if (u === "d" || u === "w") return at(d, 8);
    return d;
  }
  const timeRe = /(\d{1,2})(?::(\d{2}))?\s*(am|pm)?$/;
  const parseTime = (t: string | undefined, fallback: number): [number, number] => {
    if (!t) return [fallback, 0];
    const tm = t.trim().match(timeRe);
    if (!tm) return [fallback, 0];
    let h = parseInt(tm[1], 10);
    if (tm[3] === "pm" && h < 12) h += 12;
    if (tm[3] === "am" && h === 12) h = 0;
    return [h, tm[2] ? parseInt(tm[2], 10) : 0];
  };
  m = s.match(/^(today|tonight|tomorrow|tmrw|next week|weekend|(?:next\s+)?(sun|mon|tue|wed|thu|fri|sat)[a-z]*)\s*(?:at\s*)?(.*)$/);
  if (m) {
    const word = m[1];
    let base: Date;
    let defaultHour = 8;
    if (word === "today") base = now;
    else if (word === "tonight") {
      base = now;
      defaultHour = 20;
    } else if (word === "tomorrow" || word === "tmrw") base = addDays(now, 1);
    else if (word === "next week") base = addDays(now, (8 - now.getDay()) % 7 || 7);
    else if (word === "weekend") base = addDays(now, (6 - now.getDay() + 7) % 7 || 7);
    else {
      const target = WEEKDAYS.indexOf(m[2]);
      let diff = (target - now.getDay() + 7) % 7 || 7;
      if (word.startsWith("next")) diff += diff < 7 ? 7 : 0;
      base = addDays(now, diff);
    }
    const [h, min] = parseTime(m[3] || undefined, defaultHour);
    const d = at(base, h, min);
    return d > now ? d : null;
  }
  const tm = s.match(/^(?:at\s+)?(\d{1,2}(?::\d{2})?\s*(?:am|pm)?)$/);
  if (tm) {
    const [h, min] = parseTime(tm[1], 8);
    let d = at(now, h, min);
    if (d <= now) d = addDays(d, 1);
    return d;
  }
  return null;
}

export function SnoozePicker() {
  const ids = useStore((s) => s.snoozeFor);
  const set = useStore((s) => s.set);
  const snoozeThreads = useStore((s) => s.snoozeThreads);
  const [text, setText] = useState("");
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const list = useMemo(() => presets(), [ids]);
  const parsed = parseWhen(text);
  const options = parsed ? [{ label: text, key: "↵", date: parsed }] : text ? [] : list;

  useEffect(() => {
    if (ids) {
      setText("");
      setActive(0);
    }
  }, [ids]);

  if (!ids) return null;
  const close = () => set({ snoozeFor: null });
  const choose = (d: Date) => {
    close();
    snoozeThreads(ids, Math.floor(d.getTime() / 1000));
  };

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center bg-black/40 pt-[18vh] animate-fade" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="w-[420px] overflow-hidden rounded-xl border border-line-strong bg-elev shadow-pop animate-in">
        <div className="flex items-center gap-2.5 border-b border-line px-4">
          <Clock size={15} className="text-fg-3" />
          <input
            ref={inputRef}
            autoFocus
            value={text}
            onChange={(e) => {
              setText(e.target.value);
              setActive(0);
            }}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                close();
              } else if (e.key === "ArrowDown") {
                e.preventDefault();
                setActive((a) => Math.min(a + 1, options.length - 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setActive((a) => Math.max(a - 1, 0));
              } else if (e.key === "Enter" && options[active]) {
                e.preventDefault();
                choose(options[active].date);
              } else if (!text && /^[1-9]$/.test(e.key)) {
                const p = list.find((x) => x.key === e.key);
                if (p) {
                  e.preventDefault();
                  choose(p.date);
                }
              }
            }}
            placeholder="Snooze until…  try “fri 9am”, “3d”, “tomorrow 14:00”"
            className="h-12 flex-1 bg-transparent text-[14px] placeholder:text-fg-4"
          />
        </div>
        <div className="p-1.5">
          {options.map((o, i) => (
            <button
              key={o.label}
              onMouseEnter={() => setActive(i)}
              onClick={() => choose(o.date)}
              className={clsx(
                "flex h-9 w-full items-center gap-3 rounded-md px-2.5 text-left text-[13px]",
                i === active ? "bg-active text-fg" : "text-fg-2",
              )}
            >
              <span className="flex-1">{o.label}</span>
              <span className="text-xs text-fg-3">{relativeFuture(Math.floor(o.date.getTime() / 1000))}</span>
              <Kbd keys={[o.key]} />
            </button>
          ))}
          {text && !parsed && <div className="px-3 py-3 text-[13px] text-fg-3">Couldn't understand that time.</div>}
        </div>
      </div>
    </div>
  );
}
