import clsx from "clsx";
import {
  AlarmClock,
  Archive,
  FileText,
  Inbox,
  PenSquare,
  Plus,
  Search,
  Send,
  Settings,
  ShieldAlert,
  Star,
  Trash2,
  AlertCircle,
  Layers,
  type LucideIcon,
} from "lucide-react";
import { useStore } from "../store";
import type { View } from "../lib/types";
import { AccountDot, Kbd, Spinner } from "./ui";
import { accountLabel } from "../lib/format";

const NAV: { view: View; label: string; icon: LucideIcon; keys: string[] }[] = [
  { view: "inbox", label: "Inbox", icon: Inbox, keys: ["G", "I"] },
  { view: "starred", label: "Starred", icon: Star, keys: ["G", "S"] },
  { view: "snoozed", label: "Snoozed", icon: AlarmClock, keys: ["G", "H"] },
  { view: "done", label: "Done", icon: Archive, keys: ["G", "E"] },
  { view: "sent", label: "Sent", icon: Send, keys: ["G", "T"] },
  { view: "drafts", label: "Drafts", icon: FileText, keys: ["G", "D"] },
];

const MORE: { view: View; label: string; icon: LucideIcon }[] = [
  { view: "spam", label: "Spam", icon: ShieldAlert },
  { view: "trash", label: "Trash", icon: Trash2 },
];

function NavItem({
  active,
  icon: Icon,
  label,
  count,
  onClick,
  title,
  children,
}: {
  active?: boolean;
  icon?: LucideIcon;
  label: string;
  count?: number;
  onClick: () => void;
  title?: string;
  children?: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      title={title}
      className={clsx(
        "group flex h-7 w-full items-center gap-2 rounded-md px-2 text-[13px] transition-colors",
        active ? "bg-active text-fg" : "text-fg-2 hover:bg-hover hover:text-fg",
      )}
    >
      {Icon && <Icon size={15} strokeWidth={1.8} className={active ? "text-fg" : "text-fg-3 group-hover:text-fg-2"} />}
      {children}
      <span className="flex-1 truncate text-left">{label}</span>
      {!!count && <span className="text-xs tabular-nums text-fg-3">{count}</span>}
    </button>
  );
}

export function Sidebar() {
  const view = useStore((s) => s.view);
  const accounts = useStore((s) => s.accounts);
  const accountFilter = useStore((s) => s.accountFilter);
  const counts = useStore((s) => s.counts);
  const sync = useStore((s) => s.sync);
  const set = useStore((s) => s.set);
  const setView = useStore((s) => s.setView);
  const setAccountFilter = useStore((s) => s.setAccountFilter);
  const compose = useStore((s) => s.compose);
  const openSearch = useStore((s) => s.openSearch);
  const collapsed = useStore((s) => s.sidebarCollapsed);

  if (collapsed) return null;

  return (
    <aside className="flex w-[232px] shrink-0 flex-col px-2.5 pb-2.5 pt-3">
      <div className="mb-3 flex items-center gap-2 px-1.5">
        <div className="flex h-[22px] w-[22px] items-center justify-center rounded-md bg-gradient-to-br from-[#7c86e6] to-[#4f5bc4] text-[11px] font-bold text-white shadow-[inset_0_1px_0_rgb(255_255_255/0.25)]">
          C
        </div>
        <span className="flex-1 text-[13.5px] font-semibold tracking-tight">CigMail</span>
        <button
          onClick={() => compose()}
          title="New message · C"
          className="flex h-7 w-7 items-center justify-center rounded-md border border-line-strong bg-elev-2 text-fg-2 hover:text-fg hover:bg-active"
        >
          <PenSquare size={14} />
        </button>
      </div>

      <button
        onClick={openSearch}
        className="mb-3 flex h-7 items-center gap-2 rounded-md px-2 text-[13px] text-fg-3 hover:bg-hover hover:text-fg-2"
      >
        <Search size={14} />
        <span className="flex-1 text-left">Search</span>
        <Kbd keys={["/"]} />
      </button>

      <nav className="flex flex-col gap-px">
        {NAV.map((n) => (
          <NavItem
            key={n.view}
            icon={n.icon}
            label={n.label}
            active={view === n.view}
            count={n.view === "inbox" ? counts["filtered"] : undefined}
            onClick={() => setView(n.view)}
            title={`${n.label} · ${n.keys.join(" then ")}`}
          />
        ))}
        {MORE.map((n) => (
          <NavItem key={n.view} icon={n.icon} label={n.label} active={view === n.view} onClick={() => setView(n.view)} />
        ))}
      </nav>

      <div className="mt-5 mb-1 flex items-center px-2 text-[11.5px] font-medium text-fg-3">
        <span className="flex-1">Accounts</span>
        <button
          title="Add account"
          onClick={() => set({ settingsSection: "accounts" })}
          className="rounded p-0.5 hover:bg-hover hover:text-fg"
        >
          <Plus size={13} />
        </button>
      </div>
      <div className="flex flex-col gap-px overflow-y-auto">
        {accounts.length > 1 && (
          <NavItem
            icon={Layers}
            label="All accounts"
            active={accountFilter === null}
            count={counts["inbox"]}
            onClick={() => setAccountFilter(null)}
          />
        )}
        {accounts.map((a) => {
          const st = sync[a.id];
          const err = st?.state === "error" || st?.state === "auth" || (!st && a.lastError);
          return (
            <NavItem
              key={a.id}
              label={accountLabel(a)}
              title={`${a.email}${err ? `\n${st?.message ?? a.lastError}` : ""}`}
              active={accountFilter === a.id}
              count={counts[`inbox:${a.id}`]}
              onClick={() => setAccountFilter(accountFilter === a.id ? null : a.id)}
            >
              <span className="flex w-[15px] justify-center">
                {st?.state === "syncing" ? (
                  <Spinner size={11} />
                ) : err ? (
                  <AlertCircle size={13} className="text-danger" />
                ) : (
                  <AccountDot color={a.color} />
                )}
              </span>
            </NavItem>
          );
        })}
        {accounts.length === 0 && (
          <button
            onClick={() => set({ settingsSection: "accounts" })}
            className="mx-2 mt-1 rounded-md border border-dashed border-line-strong px-2 py-2 text-left text-xs text-fg-3 hover:text-fg-2"
          >
            Add your first account
          </button>
        )}
      </div>

      <div className="flex-1" />
      <div className="flex items-center gap-1 px-0.5">
        <NavItem icon={Settings} label="Settings" onClick={() => set({ settingsSection: "general" })} />
        <button
          onClick={() => set({ help: true })}
          title="Keyboard shortcuts"
          className="flex h-7 items-center rounded-md px-1.5 text-fg-3 hover:bg-hover"
        >
          <Kbd keys={["?"]} />
        </button>
      </div>
    </aside>
  );
}
