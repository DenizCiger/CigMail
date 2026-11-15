import clsx from "clsx";
import { AlarmClock, AlertCircle, Archive, FileText, Inbox, Send, Settings, ShieldAlert, Star, Trash2, type LucideIcon } from "lucide-react";
import { useStore } from "../store";
import { accountLabel } from "../lib/format";
import type { View } from "../lib/types";
import { Spinner } from "../components/ui";
import { AppIcon } from "../components/AppIcon";
import { useBackLayer } from "./back";

const NAV: { view: View; label: string; icon: LucideIcon }[] = [
  { view: "inbox", label: "Inbox", icon: Inbox },
  { view: "starred", label: "Starred", icon: Star },
  { view: "snoozed", label: "Snoozed", icon: AlarmClock },
  { view: "done", label: "Done", icon: Archive },
  { view: "sent", label: "Sent", icon: Send },
  { view: "drafts", label: "Drafts", icon: FileText },
  { view: "spam", label: "Spam", icon: ShieldAlert },
  { view: "trash", label: "Trash", icon: Trash2 },
];

function Item({ active, onClick, children }: { active?: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      onClick={onClick}
      className={clsx(
        "flex h-12 w-full items-center gap-4 rounded-full px-4 text-left text-[15px]",
        active ? "bg-accent-soft font-medium text-fg" : "text-fg-2 active:bg-active",
      )}
    >
      {children}
    </button>
  );
}

/** Slide-in navigation: views, the account filter and settings. */
export function Drawer({ open, onClose }: { open: boolean; onClose: () => void }) {
  const view = useStore((s) => s.view);
  const accounts = useStore((s) => s.accounts);
  const accountFilter = useStore((s) => s.accountFilter);
  const counts = useStore((s) => s.counts);
  const sync = useStore((s) => s.sync);
  const { setView, setAccountFilter, set } = useStore.getState();
  useBackLayer(open, onClose);

  const go = (fn: () => void) => {
    onClose();
    fn();
  };

  return (
    <div className={clsx("fixed inset-0 z-40", !open && "pointer-events-none")}>
      <div className={clsx("absolute inset-0 bg-black/45 transition-opacity duration-200", open ? "opacity-100" : "opacity-0")} onClick={onClose} />
      <nav
        className={clsx(
          "absolute inset-y-0 left-0 flex w-[min(320px,84vw)] flex-col overflow-y-auto bg-elev pb-[env(safe-area-inset-bottom)] pt-[env(safe-area-inset-top)] shadow-pop transition-transform duration-200 ease-out",
          open ? "translate-x-0" : "-translate-x-full",
        )}
      >
        <div className="flex items-center gap-3 px-6 pb-4 pt-5">
          <AppIcon className="h-10 w-10" />
          <span className="text-[17px] font-semibold tracking-tight">CigMail</span>
        </div>
        <div className="flex flex-col px-3">
          {NAV.map((n) => (
            <Item key={n.view} active={view === n.view} onClick={() => go(() => setView(n.view))}>
              <n.icon size={20} className="text-fg-3" />
              <span className="flex-1">{n.label}</span>
              {n.view === "inbox" && !!counts[accountFilter ? "filtered" : "inbox"] && (
                <span className="text-[13px] tabular-nums text-fg-3">{counts[accountFilter ? "filtered" : "inbox"]}</span>
              )}
            </Item>
          ))}
        </div>
        {accounts.length > 1 && (
          <>
            <div className="mx-6 my-3 h-px bg-line" />
            <div className="px-7 pb-1 text-[12px] font-medium uppercase tracking-wide text-fg-4">Accounts</div>
            <div className="flex flex-col px-3">
              <Item active={!accountFilter} onClick={() => go(() => setAccountFilter(null))}>
                <span className="flex w-5 justify-center">
                  <span className="h-2.5 w-2.5 rounded-full border border-fg-3" />
                </span>
                <span className="flex-1">All accounts</span>
              </Item>
              {accounts.map((a) => {
                const st = sync[a.id];
                return (
                  <Item key={a.id} active={accountFilter === a.id} onClick={() => go(() => setAccountFilter(a.id))}>
                    <span className="flex w-5 justify-center">
                      <span className="h-2.5 w-2.5 rounded-full" style={{ background: a.color }} />
                    </span>
                    <span className="min-w-0 flex-1 truncate">{accountLabel(a)}</span>
                    {st?.state === "syncing" ? (
                      <Spinner size={13} />
                    ) : st?.state === "error" || st?.state === "auth" ? (
                      <AlertCircle size={15} className="text-danger" />
                    ) : counts[`inbox:${a.id}`] ? (
                      <span className="text-[13px] tabular-nums text-fg-3">{counts[`inbox:${a.id}`]}</span>
                    ) : null}
                  </Item>
                );
              })}
            </div>
          </>
        )}
        <div className="flex-1" />
        <div className="mx-6 my-3 h-px bg-line" />
        <div className="px-3 pb-3">
          <Item onClick={() => go(() => set({ settingsSection: "index" }))}>
            <Settings size={20} className="text-fg-3" />
            Settings
          </Item>
        </div>
      </nav>
    </div>
  );
}
