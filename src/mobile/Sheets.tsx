import clsx from "clsx";
import { useMemo, useState, type ReactNode } from "react";
import { Clock, type LucideIcon } from "lucide-react";
import { useStore } from "../store";
import { relativeFuture } from "../lib/format";
import { parseWhen, presets } from "../components/SnoozePicker";
import { useBackLayer } from "./back";

/** A panel that slides up from the bottom edge; tap outside or press back to close. */
export function Sheet({ open, onClose, children, className }: { open: boolean; onClose: () => void; children: ReactNode; className?: string }) {
  useBackLayer(open, onClose);
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-end bg-black/45 animate-fade" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div
        className={clsx(
          "w-full rounded-t-2xl border-t border-line-strong bg-elev pb-[max(env(safe-area-inset-bottom),12px)] shadow-pop animate-slide-up",
          className,
        )}
      >
        <div className="mx-auto mb-1 mt-2.5 h-1 w-9 rounded-full bg-line-strong" />
        {children}
      </div>
    </div>
  );
}

export interface SheetAction {
  label: string;
  icon?: LucideIcon;
  run: () => void;
  danger?: boolean;
}

/** A list of actions in a bottom sheet: the touch version of a context menu. */
export function ActionSheet({ title, actions, onClose }: { title?: ReactNode; actions: SheetAction[] | null; onClose: () => void }) {
  return (
    <Sheet open={!!actions} onClose={onClose}>
      {title && <div className="truncate px-5 pb-1 pt-2 text-[13px] text-fg-3">{title}</div>}
      <div className="px-2 pt-1">
        {actions?.map((a) => (
          <button
            key={a.label}
            onClick={() => {
              onClose();
              a.run();
            }}
            className={clsx("flex h-12 w-full items-center gap-4 rounded-lg px-3 text-left text-[15px] active:bg-active", a.danger ? "text-danger" : "text-fg")}
          >
            {a.icon && <a.icon size={19} className={a.danger ? "" : "text-fg-3"} />}
            {a.label}
          </button>
        ))}
      </div>
    </Sheet>
  );
}

export function SnoozeSheet() {
  const ids = useStore((s) => s.snoozeFor);
  const set = useStore((s) => s.set);
  const snoozeThreads = useStore((s) => s.snoozeThreads);
  const [text, setText] = useState("");
  const list = useMemo(() => presets(), [ids]);
  const parsed = parseWhen(text);
  const close = () => {
    set({ snoozeFor: null });
    setText("");
  };
  const choose = (d: Date) => {
    const target = ids!;
    close();
    snoozeThreads(target, Math.floor(d.getTime() / 1000));
  };

  return (
    <Sheet open={!!ids} onClose={close}>
      <div className="px-5 pb-2 pt-2 text-[15px] font-semibold">Snooze until</div>
      <div className="px-2">
        {list.map((o) => (
          <button key={o.label} onClick={() => choose(o.date)} className="flex h-12 w-full items-center gap-3 rounded-lg px-3 text-left active:bg-active">
            <span className="flex-1 text-[15px]">{o.label}</span>
            <span className="text-[13px] text-fg-3">{relativeFuture(Math.floor(o.date.getTime() / 1000))}</span>
          </button>
        ))}
      </div>
      <form
        className="mx-4 mt-2 flex h-11 items-center gap-2.5 rounded-xl border border-line-strong bg-input px-3"
        onSubmit={(e) => {
          e.preventDefault();
          if (parsed) choose(parsed);
        }}
      >
        <Clock size={16} className="shrink-0 text-fg-3" />
        <input
          value={text}
          onChange={(e) => setText(e.target.value)}
          enterKeyHint="done"
          placeholder="Or type: fri 9am, 3d, tomorrow 14:00"
          className="h-full min-w-0 flex-1 bg-transparent text-[15px] placeholder:text-fg-4"
        />
        {text && (
          <button type="submit" disabled={!parsed} className="shrink-0 text-[13px] font-medium text-accent disabled:text-fg-4">
            {parsed ? relativeFuture(Math.floor(parsed.getTime() / 1000)) : "?"}
          </button>
        )}
      </form>
    </Sheet>
  );
}
