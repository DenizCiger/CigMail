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
  Keyboard,
  type LucideIcon,
} from "lucide-react";
import { useStore } from "../store";
import type { View } from "../lib/types";
import { AccountDot, Spinner, useMediaQuery } from "./ui";
import { accountLabel, mod } from "../lib/format";

const NAV: { view: View; label: string; icon: LucideIcon; keys: string[] }[] = [
  { view: "inbox", label: "Inbox", icon: Inbox, keys: ["G", "I"] },
  { view: "starred", label: "Starred", icon: Star, keys: ["G", "S"] },
  { view: "snoozed", label: "Snoozed", icon: AlarmClock, keys: ["G", "H"] },
  { view: "done", label: "Done", icon: Archive, keys: ["G", "E"] },
  { view: "sent", label: "Sent", icon: Send, keys: ["G", "T"] },
  { view: "drafts", label: "Drafts", icon: FileText, keys: ["G", "D"] },
];

const MORE: { view: View; label: string; icon: LucideIcon; keys: string[] }[] = [
  { view: "spam", label: "Spam", icon: ShieldAlert, keys: ["G", "!"] },
  { view: "trash", label: "Trash", icon: Trash2, keys: ["G", "X"] },
];

function NavItem({
  active,
  icon: Icon,
  label,
  count,
  keys,
  onClick,
  title,
  children,
}: {
  active?: boolean;
  icon?: LucideIcon;
  label: string;
  count?: number;
  keys?: string[];
  onClick: () => void;
  title?: string;
  children?: React.ReactNode;
}) {
  const rail = useRail();
  if (rail)
    return (
      <button
        onClick={onClick}
        title={title ?? label}
        aria-label={label}
        className={clsx(
          "group relative flex h-8 w-8 shrink-0 items-center justify-center rounded-md transition-colors",
          active ? "bg-active text-fg" : "text-fg-3 hover:bg-hover hover:text-fg",
        )}
      >
        {Icon && <Icon size={16} strokeWidth={1.8} />}
        {children}
        {!!count && <span className="absolute right-1 top-1 h-1.5 w-1.5 rounded-full bg-accent" />}
      </button>
    );
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
      {keys && <KeyHint keys={keys} />}
    </button>
  );
}

/** Below this width the sidebar shrinks to an icon rail so the reading pane keeps its room. */
function useRail() {
  return useMediaQuery("(max-width: 1100px)");
}

/** A quiet shortcut label, e.g. "G I", that brightens on hover. */
function KeyHint({ keys }: { keys: string[] }) {
  return (
    <span className="flex shrink-0 items-center gap-[3px] text-fg-4 transition-colors group-hover:text-fg-3">
      {keys.map((k, i) => (
        <span key={i} className="min-w-[13px] rounded-[3px] bg-hover px-[3px] text-center text-[10px] font-medium leading-[15px]">
          {k === "Mod" ? mod : k}
        </span>
      ))}
    </span>
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
  const rail = useRail();

  if (collapsed) return null;
  if (rail) return <Rail />;

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
        <KeyHint keys={["/"]} />
      </button>

      <nav className="flex flex-col gap-px">
        {NAV.map((n) => (
          <NavItem
            key={n.view}
            icon={n.icon}
            label={n.label}
            active={view === n.view}
            count={n.view === "inbox" ? counts["filtered"] : undefined}
            keys={n.keys}
            onClick={() => setView(n.view)}
            title={`${n.label} · ${n.keys.join(" then ")}`}
          />
        ))}
        {MORE.map((n) => (
          <NavItem key={n.view} icon={n.icon} label={n.label} keys={n.keys} active={view === n.view} onClick={() => setView(n.view)} title={`${n.label} · ${n.keys.join(" then ")}`} />
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
            title="Unified inbox  ·  G then 0  ·  ] / Shift+] to cycle"
            active={accountFilter === null}
            count={counts["inbox"]}
            keys={["G", "0"]}
            onClick={() => setAccountFilter(null)}
          />
        )}
        {accounts.map((a, i) => {
          const st = sync[a.id];
          const err = st?.state === "error" || st?.state === "auth" || (!st && a.lastError);
          return (
            <NavItem
              key={a.id}
              label={accountLabel(a)}
              title={`${a.email}${i < 9 ? `  ·  G then ${i + 1}` : ""}${err ? `\n${st?.message ?? a.lastError}` : ""}`}
              active={accountFilter === a.id}
              count={counts[`inbox:${a.id}`]}
              keys={i < 9 ? ["G", String(i + 1)] : undefined}
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
        <NavItem icon={Settings} label="Settings" keys={["Mod", ","]} onClick={() => set({ settingsSection: "general" })} />
        <button
          onClick={() => set({ help: true })}
          title="Keyboard shortcuts  ·  ?"
          aria-label="Keyboard shortcuts"
          className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-fg-3 hover:bg-hover hover:text-fg"
        >
          <Keyboard size={15} strokeWidth={1.8} />
        </button>
      </div>
    </aside>
  );
}

/** Narrow-window sidebar: the same destinations as icons, names in tooltips. */
function Rail() {
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

  return (
    <aside className="flex w-[52px] shrink-0 flex-col items-center gap-px overflow-y-auto pb-2.5 pt-3">
      <button
        onClick={() => compose()}
        title="New message · C"
        aria-label="New message"
        className="mb-2 flex h-8 w-8 shrink-0 items-center justify-center rounded-md border border-line-strong bg-elev-2 text-fg-2 hover:bg-active hover:text-fg"
      >
        <PenSquare size={15} />
      </button>
      <NavItem icon={Search} label="Search" title="Search · /" onClick={openSearch} />
      <div className="my-1.5 h-px w-6 shrink-0 bg-line" />
      {[...NAV, ...MORE].map((n) => (
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
      <div className="my-1.5 h-px w-6 shrink-0 bg-line" />
      {accounts.length > 1 && (
        <NavItem
          icon={Layers}
          label="All accounts"
          title="Unified inbox  ·  G then 0"
          active={accountFilter === null}
          onClick={() => setAccountFilter(null)}
        />
      )}
      {accounts.map((a, i) => {
        const st = sync[a.id];
        const err = st?.state === "error" || st?.state === "auth" || (!st && a.lastError);
        return (
          <NavItem
            key={a.id}
            label={accountLabel(a)}
            title={`${accountLabel(a)} · ${a.email}${i < 9 ? `  ·  G then ${i + 1}` : ""}${err ? `\n${st?.message ?? a.lastError}` : ""}`}
            active={accountFilter === a.id}
            onClick={() => setAccountFilter(accountFilter === a.id ? null : a.id)}
          >
            {st?.state === "syncing" ? (
              <Spinner size={12} />
            ) : err ? (
              <AlertCircle size={14} className="text-danger" />
            ) : (
              <AccountDot color={a.color} size={9} />
            )}
          </NavItem>
        );
      })}
      <NavItem icon={Plus} label="Add account" onClick={() => set({ settingsSection: "accounts" })} />
      <div className="flex-1" />
      <NavItem icon={Keyboard} label="Keyboard shortcuts" title="Keyboard shortcuts  ·  ?" onClick={() => set({ help: true })} />
      <NavItem icon={Settings} label="Settings" title={`Settings  ·  ${mod} ,`} onClick={() => set({ settingsSection: "general" })} />
    </aside>
  );
}
