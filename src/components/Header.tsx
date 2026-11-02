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

interface Suggestion {
  /** Replaces the token being typed. */
  insert: string;
  label: string;
  hint?: string;
}

const OPERATORS: Suggestion[] = [
  { insert: "from:", label: "from:", hint: "Sender name or address · from:me" },
  { insert: "to:", label: "to:", hint: "Recipient · to:me" },
  { insert: "subject:", label: "subject:", hint: "Words in the subject" },
  { insert: "account:", label: "account:", hint: "Account label, address or gmail/outlook/imap" },
  { insert: "is:unread ", label: "is:unread" },
  { insert: "is:starred ", label: "is:starred" },
  { insert: "has:attachment ", label: "has:attachment" },
  { insert: "category:", label: "category:", hint: "primary · newsletter · notification" },
  { insert: "in:", label: "in:", hint: "inbox · done · sent · drafts" },
];

const VALUES: Record<string, string[]> = {
  "is:": ["unread", "read", "starred", "snoozed"],
  "has:": ["attachment"],
  "category:": ["primary", "newsletter", "notification"],
  "in:": ["inbox", "done", "sent", "drafts", "trash", "spam"],
};

function SearchBox() {
  const query = useStore((s) => s.query);
  const setQuery = useStore((s) => s.setQuery);
  const closeSearch = useStore((s) => s.closeSearch);
  const accounts = useStore((s) => s.accounts);
  const [value, setValue] = useState(query);
  const [suggestions, setSuggestions] = useState<Suggestion[]>([]);
  const [active, setActive] = useState(0);
  const [open, setOpen] = useState(true);
  const ref = useRef<HTMLInputElement>(null);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => ref.current?.focus(), []);

  // Suggest operators, then values for the operator being typed.
  useEffect(() => {
    const token = value.split(/\s+/).pop() ?? "";
    const neg = token.startsWith("-") ? "-" : "";
    const tok = token.slice(neg.length).toLowerCase();
    let stale = false;
    const colon = tok.indexOf(":");
    if (colon < 0) {
      setSuggestions(tok ? OPERATORS.filter((o) => o.label.startsWith(tok) && o.label !== tok).map((o) => ({ ...o, insert: neg + o.insert })) : []);
    } else {
      const key = tok.slice(0, colon + 1);
      const part = tok.slice(colon + 1);
      if (VALUES[key]) {
        setSuggestions(VALUES[key].filter((v) => v.startsWith(part) && v !== part).map((v) => ({ insert: `${neg}${key}${v} `, label: `${key}${v}` })));
      } else if (key === "account:") {
        setSuggestions(
          accounts
            .map((a) => ({ value: (a.label ?? a.email).toLowerCase().replace(/\s+/g, ""), a }))
            .filter((x) => x.value.includes(part) || x.a.email.includes(part))
            .map((x) => ({ insert: `${neg}account:${x.a.label ? x.value : x.a.email} `, label: x.a.label ?? x.a.email, hint: x.a.email })),
        );
      } else if ((key === "from:" || key === "to:") && part.length >= 1) {
        api
          .suggestContacts(part)
          .then((cs) => {
            if (stale) return;
            setSuggestions(cs.slice(0, 6).map((c) => ({ insert: `${neg}${key}${c.email} `, label: c.name ?? c.email, hint: c.email })));
          })
          .catch(() => {});
      } else {
        setSuggestions([]);
      }
    }
    setActive(0);
    return () => {
      stale = true;
    };
  }, [value, accounts]);

  const apply = (v: string) => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setQuery(v), 160);
  };

  const accept = (sug: Suggestion) => {
    const parts = value.split(/(\s+)/);
    parts[parts.length - 1] = sug.insert;
    const next = parts.join("");
    setValue(next);
    setOpen(true);
    apply(next);
    ref.current?.focus();
  };

  const showing = open && suggestions.length > 0;

  return (
    <div className="relative flex h-8 min-w-0 flex-1 items-center gap-2 rounded-md border border-line-strong bg-input px-2.5 focus-within:border-fg-4">
      <Search size={14} className="shrink-0 text-fg-3" />
      <input
        ref={ref}
        value={value}
        placeholder="Search all mail…  try from:, to:, subject:, account:, is:unread"
        className="min-w-0 flex-1 bg-transparent text-[13px] placeholder:text-fg-4"
        onChange={(e) => {
          setValue(e.target.value);
          setOpen(true);
          apply(e.target.value);
        }}
        onBlur={() => window.setTimeout(() => setOpen(false), 120)}
        onFocus={() => setOpen(true)}
        onKeyDown={(e) => {
          if (showing && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
            e.preventDefault();
            setActive((a) => (a + (e.key === "ArrowDown" ? 1 : -1) + suggestions.length) % suggestions.length);
          } else if (showing && (e.key === "Tab" || e.key === "Enter")) {
            e.preventDefault();
            accept(suggestions[active]);
          } else if (e.key === "Escape") {
            e.preventDefault();
            if (showing) setOpen(false);
            else closeSearch();
          } else if (e.key === "Enter" || e.key === "ArrowDown") {
            e.preventDefault();
            window.clearTimeout(timer.current);
            setQuery(value);
            ref.current?.blur();
          }
        }}
      />
      <button onClick={closeSearch} className="text-fg-3 hover:text-fg" aria-label="Close search">
        <X size={14} />
      </button>
      {showing && (
        <div className="absolute left-0 top-full z-30 mt-1 w-[420px] overflow-hidden rounded-lg border border-line-strong bg-elev p-1 shadow-pop animate-in">
          {suggestions.map((sug, i) => (
            <button
              key={sug.insert}
              onMouseDown={(e) => {
                e.preventDefault();
                accept(sug);
              }}
              onMouseEnter={() => setActive(i)}
              className={clsx("flex h-8 w-full items-center gap-3 rounded-md px-2 text-left text-[13px]", i === active ? "bg-active text-fg" : "text-fg-2")}
            >
              <span className="font-mono text-[12px]">{sug.label}</span>
              {sug.hint && <span className="truncate text-xs text-fg-3">{sug.hint}</span>}
            </button>
          ))}
          <div className="flex gap-3 px-2 pb-1 pt-1.5 text-[11px] text-fg-4">
            <span>Tab or Enter to complete</span>
            <span>Esc to dismiss</span>
          </div>
        </div>
      )}
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
              {splits.map((s, i) => (
                <SplitTab
                  key={s.id}
                  hint={i < 9 ? `${i + 1}  ·  Tab / Shift+Tab to cycle` : undefined}
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
        <span className="rounded-md bg-active px-2 py-0.5 text-xs font-medium text-fg-2">
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

function SplitTab({
  active,
  label,
  count,
  hint,
  onClick,
}: {
  active: boolean;
  label: string;
  count?: number;
  hint?: string;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      title={hint ? `${label}  ·  ${hint}` : label}
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
