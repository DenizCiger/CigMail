import clsx from "clsx";
import { useEffect, useRef, useState } from "react";
import { PanelLeft, RefreshCw, Search, X } from "lucide-react";
import { useStore } from "../store";
import { api } from "../lib/api";
import { IconButton, Kbd, Spinner } from "./ui";
import { accountLabel } from "../lib/format";

const TITLES: Record<string, string> = {
  inbox: "Inbox",
  starred: "Starred",
  snoozed: "Snoozed",
  done: "Done",
  sent: "Sent",
  drafts: "Drafts",
  trash: "Trash",
  spam: "Spam",
  search: "Search",
};

function SearchBox() {
  const query = useStore((s) => s.query);
  const setQuery = useStore((s) => s.setQuery);
  const closeSearch = useStore((s) => s.closeSearch);
  const [value, setValue] = useState(query);
  const ref = useRef<HTMLInputElement>(null);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => ref.current?.focus(), []);

  return (
    <div className="flex h-8 min-w-0 flex-1 items-center gap-2 rounded-md border border-line-strong bg-input px-2.5 focus-within:border-accent focus-within:ring-2 focus-within:ring-accent-soft">
      <Search size={14} className="shrink-0 text-fg-3" />
      <input
        ref={ref}
        value={value}
        placeholder="Search all mail…  from:  to:  subject:  has:attachment  is:unread  account:"
        className="min-w-0 flex-1 bg-transparent text-[13px] placeholder:text-fg-4"
        onChange={(e) => {
          const v = e.target.value;
          setValue(v);
          window.clearTimeout(timer.current);
          timer.current = window.setTimeout(() => setQuery(v), 160);
        }}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            closeSearch();
          } else if (e.key === "Enter" || e.key === "ArrowDown") {
            e.preventDefault();
            window.clearTimeout(timer.current);
            setQuery(value);
            ref.current?.blur();
          }
        }}
      />
      <button onClick={closeSearch} className="text-fg-3 hover:text-fg">
        <X size={14} />
      </button>
    </div>
  );
}

export function Header() {
  const view = useStore((s) => s.view);
  const splits = useStore((s) => s.splits);
  const splitId = useStore((s) => s.splitId);
  const setSplit = useStore((s) => s.setSplit);
  const counts = useStore((s) => s.counts);
  const searchOpen = useStore((s) => s.searchOpen);
  const accounts = useStore((s) => s.accounts);
  const accountFilter = useStore((s) => s.accountFilter);
  const sync = useStore((s) => s.sync);
  const collapsed = useStore((s) => s.sidebarCollapsed);
  const set = useStore((s) => s.set);
  const threads = useStore((s) => s.threads);
  const selected = useStore((s) => s.selected);

  const syncing = Object.values(sync).find((s) => s.state === "syncing");
  const filterAccount = accounts.find((a) => a.id === accountFilter);

  return (
    <header className="flex h-12 shrink-0 items-center gap-3 border-b border-line px-3">
      {collapsed && (
        <IconButton label="Show sidebar" shortcut={["["]} onClick={() => set({ sidebarCollapsed: false })}>
          <PanelLeft size={15} />
        </IconButton>
      )}
      {searchOpen || view === "search" ? (
        <SearchBox />
      ) : (
        <>
          <div className="flex items-center gap-2 pl-1">
            <h1 className="text-[13.5px] font-semibold">{TITLES[view]}</h1>
            {filterAccount && (
              <span className="flex items-center gap-1.5 rounded-full border border-line-strong px-2 py-0.5 text-[11.5px] text-fg-2">
                <span className="h-1.5 w-1.5 rounded-full" style={{ background: filterAccount.color }} />
                {accountLabel(filterAccount)}
              </span>
            )}
          </div>
          {view === "inbox" && (
            <nav className="flex items-center gap-0.5 overflow-x-auto">
              {splits.map((s) => (
                <SplitTab
                  key={s.id}
                  active={splitId === s.id}
                  label={s.name}
                  count={counts[`split:${s.id}`]}
                  onClick={() => setSplit(s.id)}
                />
              ))}
              <SplitTab active={splitId === null} label="All" onClick={() => setSplit(null)} />
            </nav>
          )}
          <div className="flex-1" />
        </>
      )}
      {selected.size > 0 && (
        <span className="rounded-md bg-accent-soft px-2 py-0.5 text-xs font-medium text-accent">
          {selected.size} selected
        </span>
      )}
      <div className="flex items-center gap-1 text-fg-3">
        {syncing ? (
          <span className="flex items-center gap-1.5 text-xs" title="Syncing">
            <Spinner size={12} />
            {syncing.progress && syncing.progress[1] > 0 && (
              <span className="tabular-nums">
                {syncing.progress[0]}/{syncing.progress[1]}
              </span>
            )}
          </span>
        ) : (
          <IconButton label="Sync now" onClick={() => api.syncNow()}>
            <RefreshCw size={14} />
          </IconButton>
        )}
        {!searchOpen && view !== "search" && (
          <span className="hidden text-xs tabular-nums text-fg-4 md:inline">{threads.length || ""}</span>
        )}
        {!searchOpen && view !== "search" && (
          <button
            onClick={() => set({ palette: true })}
            className="ml-1 flex h-7 items-center gap-1.5 rounded-md px-2 text-xs text-fg-3 hover:bg-hover hover:text-fg-2"
          >
            <Kbd keys={["Mod", "K"]} />
          </button>
        )}
      </div>
    </header>
  );
}

function SplitTab({ active, label, count, onClick }: { active: boolean; label: string; count?: number; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className={clsx(
        "flex h-7 items-center gap-1.5 rounded-md px-2.5 text-[13px] transition-colors",
        active ? "bg-active text-fg" : "text-fg-3 hover:text-fg-2 hover:bg-hover",
      )}
    >
      {label}
      {!!count && <span className={clsx("text-[11px] tabular-nums", active ? "text-fg-2" : "text-fg-4")}>{count}</span>}
    </button>
  );
}
