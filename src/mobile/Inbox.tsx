import clsx from "clsx";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  Archive,
  ArrowLeft,
  Check,
  Clock,
  FileText,
  Inbox as InboxIcon,
  Mail,
  MailOpen,
  Menu,
  MoreVertical,
  Paperclip,
  PenSquare,
  Search,
  ShieldAlert,
  Star,
  Trash2,
  X,
} from "lucide-react";
import { canRestore, useStore } from "../store";
import { api } from "../lib/api";
import { accountLabel, displayName, relativeFuture, shortDate } from "../lib/format";
import type { Draft, ThreadSummary, View } from "../lib/types";
import { Avatar, Spinner } from "../components/ui";
import { ReconnectBanner } from "../components/ReconnectBanner";
import { ActionSheet, type SheetAction } from "./Sheets";
import { useBackLayer } from "./back";

export const TITLES: Record<View, string> = {
  inbox: "Inbox",
  starred: "Starred",
  snoozed: "Snoozed",
  sent: "Sent",
  drafts: "Drafts",
  done: "Done",
  trash: "Trash",
  spam: "Spam",
  search: "Search",
};

const ROW = 88;

/** Tap target for the app bars: 44px, as the platform guidelines ask. */
export function BarButton({ label, onClick, children, className }: { label: string; onClick: () => void; children: ReactNode; className?: string }) {
  return (
    <button
      aria-label={label}
      onClick={onClick}
      className={clsx("flex h-11 w-11 shrink-0 items-center justify-center rounded-full text-fg-2 active:bg-active", className)}
    >
      {children}
    </button>
  );
}

// ------------------------------------------------------------------ swipe row

const LONG_PRESS_MS = 450;

/**
 * A list row you can swipe sideways (right = done, left = snooze), tap, or
 * long-press. Vertical drags are left to the list so it still scrolls.
 */
function SwipeRow({
  onTap,
  onLongPress,
  right,
  left,
  children,
}: {
  onTap: () => void;
  onLongPress: () => void;
  right?: { icon: ReactNode; color: string; run: () => void };
  left?: { icon: ReactNode; color: string; run: () => void };
  children: ReactNode;
}) {
  const [dx, setDx] = useState(0);
  const [settling, setSettling] = useState(false);
  const g = useRef<{ x: number; y: number; id: number; swiping: boolean; moved: boolean; pressed: boolean; timer?: number } | null>(null);
  const el = useRef<HTMLDivElement>(null);

  const reset = () => {
    if (g.current?.timer) window.clearTimeout(g.current.timer);
    g.current = null;
  };

  return (
    <div className="relative h-full overflow-hidden">
      {dx !== 0 && (
        <div
          className="absolute inset-0 flex items-center px-7 text-white"
          style={{ background: (dx > 0 ? right : left)?.color, justifyContent: dx > 0 ? "flex-start" : "flex-end" }}
        >
          {(dx > 0 ? right : left)?.icon}
        </div>
      )}
      <div
        ref={el}
        className={clsx("relative h-full touch-pan-y bg-panel", settling && "transition-transform duration-200 ease-out")}
        style={{ transform: dx ? `translateX(${dx}px)` : undefined }}
        onPointerDown={(e) => {
          if (e.button !== 0) return;
          g.current = { x: e.clientX, y: e.clientY, id: e.pointerId, swiping: false, moved: false, pressed: false };
          g.current.timer = window.setTimeout(() => {
            if (!g.current || g.current.moved) return;
            g.current.pressed = true;
            navigator.vibrate?.(12);
            onLongPress();
          }, LONG_PRESS_MS);
        }}
        onPointerMove={(e) => {
          const s = g.current;
          if (!s || s.id !== e.pointerId) return;
          const mx = e.clientX - s.x;
          const my = e.clientY - s.y;
          if (!s.moved && Math.hypot(mx, my) > 8) {
            s.moved = true;
            window.clearTimeout(s.timer);
            if (Math.abs(mx) > Math.abs(my) * 1.3 && (mx > 0 ? right : left)) {
              s.swiping = true;
              el.current?.setPointerCapture(e.pointerId);
            }
          }
          if (s.swiping) {
            setSettling(false);
            // Only allow the directions that have an action.
            setDx(mx > 0 ? (right ? mx : 0) : left ? mx : 0);
          }
        }}
        onPointerUp={() => {
          const s = g.current;
          reset();
          if (!s) return;
          if (s.swiping) {
            const width = el.current?.offsetWidth ?? 360;
            const action = dx > 0 ? right : left;
            setSettling(true);
            if (action && Math.abs(dx) > Math.min(110, width * 0.3)) {
              setDx(dx > 0 ? width : -width);
              window.setTimeout(() => {
                action.run();
                setSettling(false);
                setDx(0);
              }, 180);
            } else {
              setDx(0);
            }
            return;
          }
          if (!s.moved && !s.pressed) onTap();
        }}
        onPointerCancel={() => {
          reset();
          setSettling(true);
          setDx(0);
        }}
        onContextMenu={(e) => e.preventDefault()}
      >
        {children}
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ rows

function ThreadRow({ t, selected, selecting, multiAccount, me }: { t: ThreadSummary; selected: boolean; selecting: boolean; multiAccount: boolean; me: string[] }) {
  const account = useStore((s) => s.accounts.find((a) => a.id === t.accountId));
  const view = useStore((s) => s.view);
  const showAi = useStore((s) => s.settings.ai_list_summaries !== "false");
  const { openThread, toggleSelect, act, set } = useStore.getState();
  const restorable = canRestore(t, view);
  const names = [...new Set(t.participants.map((p) => displayName(p, me)))];
  const who = names.length <= 2 ? names.join(", ") : `${names[0]} … ${names[names.length - 1]}`;
  const preview = showAi && t.aiSummary ? t.aiSummary.split("\n")[0] : t.snippet;

  return (
    <SwipeRow
      onTap={() => (selecting ? toggleSelect(t.id) : openThread(t.id))}
      onLongPress={() => toggleSelect(t.id)}
      right={
        restorable
          ? { icon: <InboxIcon size={22} />, color: "var(--accent-strong)", run: () => act("inbox", [t.id]) }
          : t.inInbox
            ? { icon: <Archive size={22} />, color: "var(--success)", run: () => act("archive", [t.id]) }
            : undefined
      }
      left={view === "snoozed" || view === "trash" || view === "sent" || view === "drafts" ? undefined : { icon: <Clock size={22} />, color: "var(--warning)", run: () => set({ snoozeFor: [t.id] }) }}
    >
      <div className={clsx("flex h-full gap-3 border-b border-line px-4 pt-3", selected && "bg-accent-soft")}>
        <div className="relative mt-0.5 h-10 w-10 shrink-0">
          {selected ? (
            <div className="flex h-10 w-10 items-center justify-center rounded-full bg-accent-strong text-white">
              <Check size={20} />
            </div>
          ) : (
            <Avatar address={t.participants.find((p) => !me.includes(p.email)) ?? t.participants[0]} size={40} />
          )}
          {multiAccount && account && (
            <span className="absolute -bottom-0.5 -right-0.5 h-3.5 w-3.5 rounded-full border-2 border-panel" style={{ background: account.color }} />
          )}
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline gap-2">
            <span className={clsx("min-w-0 flex-1 truncate text-[15px]", t.unread ? "font-semibold text-fg" : "text-fg-2")}>
              {who || "(no sender)"}
              {t.messageCount > 1 && <span className="ml-1.5 text-[12px] font-normal text-fg-3">{t.messageCount}</span>}
            </span>
            {view === "snoozed" && t.snoozedUntil ? (
              <span className="flex shrink-0 items-center gap-1 text-[12px] text-warning">
                <Clock size={12} />
                {relativeFuture(t.snoozedUntil)}
              </span>
            ) : (
              <span className={clsx("shrink-0 text-[12px] tabular-nums", t.unread ? "font-medium text-accent" : "text-fg-3")}>{shortDate(t.lastDate)}</span>
            )}
          </div>
          <div className="flex items-center gap-1.5">
            <span className={clsx("min-w-0 flex-1 truncate text-[14px]", t.unread ? "font-medium text-fg" : "text-fg-2")}>{t.subject || "(no subject)"}</span>
            {t.hasAttachments && <Paperclip size={13} className="shrink-0 text-fg-3" />}
            {t.starred && <Star size={13} className="shrink-0 fill-warning text-warning" />}
          </div>
          <div className="truncate text-[13.5px] text-fg-3">{preview}</div>
        </div>
      </div>
    </SwipeRow>
  );
}

function Empty() {
  const view = useStore((s) => s.view);
  const sync = useStore((s) => s.sync);
  const accounts = useStore((s) => s.accounts);
  const firstSync = accounts.some((a) => !a.lastSync) && Object.values(sync).some((s) => s.state === "syncing");
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 px-10 text-center animate-fade">
      {firstSync ? (
        <>
          <Spinner size={20} />
          <div className="mt-2 text-[15px]">Downloading your mail…</div>
          <div className="text-[13px] text-fg-3">The first sync can take a few minutes. New mail shows up as it arrives.</div>
        </>
      ) : view === "inbox" ? (
        <>
          <div className="mb-2 flex h-20 w-20 items-center justify-center rounded-full border border-line-strong bg-elev">
            <Archive size={28} className="text-fg-2" strokeWidth={1.5} />
          </div>
          <div className="text-[16px] font-medium">You're all done</div>
          <div className="text-[13px] text-fg-3">Nothing left in this split. Enjoy the quiet.</div>
        </>
      ) : (
        <div className="text-[14px] text-fg-3">{view === "search" ? "No conversations match." : "Nothing here."}</div>
      )}
    </div>
  );
}

/** Locally saved drafts, shown above server drafts in the Drafts view. */
function LocalDrafts() {
  const accounts = useStore((s) => s.accounts);
  const composer = useStore((s) => s.composer);
  const compose = useStore((s) => s.compose);
  const [drafts, setDrafts] = useState<Draft[]>([]);
  useEffect(() => {
    api.listDrafts().then(setDrafts).catch(() => {});
  }, [composer]);
  if (!drafts.length) return null;
  return (
    <div className="border-b border-line">
      {drafts.map((d) => {
        const a = accounts.find((x) => x.id === d.accountId);
        return (
          <button key={d.id} onClick={() => compose(d)} className="flex w-full items-center gap-3 border-b border-line px-4 py-3 text-left last:border-0 active:bg-hover">
            <div className="relative flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-danger/15 text-danger">
              <FileText size={18} />
              {a && accounts.length > 1 && <span className="absolute -bottom-0.5 -right-0.5 h-3.5 w-3.5 rounded-full border-2 border-panel" style={{ background: a.color }} />}
            </div>
            <span className="min-w-0 flex-1">
              <span className="block truncate text-[15px] text-danger">Draft{d.to.length ? ` · ${d.to.map((x) => x.name || x.email).join(", ")}` : ""}</span>
              <span className="block truncate text-[14px] text-fg-2">{d.subject || "(no subject)"}</span>
            </span>
          </button>
        );
      })}
    </div>
  );
}

// ------------------------------------------------------------------ list

const PULL_TRIGGER = 72;

function List() {
  const threads = useStore((s) => s.threads);
  const selected = useStore((s) => s.selected);
  const loading = useStore((s) => s.loading);
  const accounts = useStore((s) => s.accounts);
  const accountFilter = useStore((s) => s.accountFilter);
  const hasMore = useStore((s) => s.hasMore);
  const refresh = useStore((s) => s.refresh);
  const syncing = useStore((s) => Object.values(s.sync).some((x) => x.state === "syncing"));
  const view = useStore((s) => s.view);
  const parentRef = useRef<HTMLDivElement>(null);
  const [pull, setPull] = useState(0);
  const [refreshing, setRefreshing] = useState(false);
  const pullStart = useRef<number | null>(null);
  const me = accounts.map((a) => a.email);
  const multiAccount = accounts.length > 1 && !accountFilter;

  const virtualizer = useVirtualizer({
    count: threads.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW,
    overscan: 8,
  });

  // The spinner stays until the sync it started settles.
  useEffect(() => {
    if (!refreshing || syncing) return;
    const t = window.setTimeout(() => setRefreshing(false), 600);
    return () => window.clearTimeout(t);
  }, [refreshing, syncing]);

  const indicator = refreshing ? PULL_TRIGGER * 0.75 : pull;

  return (
    <div
      ref={parentRef}
      className="h-full overflow-y-auto overscroll-y-contain"
      onScroll={(e) => {
        const el = e.currentTarget;
        if (hasMore && el.scrollTop + el.clientHeight > el.scrollHeight - ROW * 10) refresh({ more: true });
      }}
      onTouchStart={(e) => {
        pullStart.current = parentRef.current!.scrollTop <= 0 ? e.touches[0].clientY : null;
      }}
      onTouchMove={(e) => {
        if (pullStart.current === null) return;
        const d = e.touches[0].clientY - pullStart.current;
        setPull(d > 0 ? Math.min(110, d * 0.5) : 0);
      }}
      onTouchEnd={() => {
        if (pull >= PULL_TRIGGER * 0.75 && !refreshing) {
          setRefreshing(true);
          api.syncNow().catch(() => {});
          refresh();
        }
        pullStart.current = null;
        setPull(0);
      }}
    >
      <div
        className={clsx("flex items-end justify-center overflow-hidden", !pull && "transition-[height] duration-200")}
        style={{ height: indicator }}
      >
        <div className="mb-3 flex h-8 w-8 items-center justify-center rounded-full border border-line-strong bg-elev shadow-pop">
          {refreshing ? (
            <Spinner size={15} />
          ) : (
            <svg width="15" height="15" viewBox="0 0 24 24" style={{ transform: `rotate(${pull * 4}deg)`, opacity: pull / PULL_TRIGGER }}>
              <path d="M21 12a9 9 0 1 1-3-6.7L21 8" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" className="text-fg-2" />
              <path d="M21 3v5h-5" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" className="text-fg-2" />
            </svg>
          )}
        </div>
      </div>
      {view === "drafts" && <LocalDrafts />}
      {loading && !threads.length ? (
        Array.from({ length: 8 }).map((_, i) => (
          <div key={i} className="flex gap-3 border-b border-line px-4 py-3.5">
            <div className="skeleton h-10 w-10 rounded-full" />
            <div className="flex flex-1 flex-col gap-2 pt-1">
              <div className="skeleton h-3 w-32" />
              <div className="skeleton h-3" style={{ width: `${50 + ((i * 37) % 40)}%` }} />
              <div className="skeleton h-3 w-[80%]" />
            </div>
          </div>
        ))
      ) : !threads.length ? (
        <div className="h-[70%]">
          <Empty />
        </div>
      ) : (
        <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
          {virtualizer.getVirtualItems().map((v) => {
            const t = threads[v.index];
            return (
              <div key={t.id} style={{ position: "absolute", top: 0, left: 0, right: 0, height: ROW, transform: `translateY(${v.start}px)` }}>
                <ThreadRow t={t} selected={selected.has(t.id)} selecting={selected.size > 0} multiAccount={multiAccount} me={me} />
              </div>
            );
          })}
        </div>
      )}
      {/* Room for the compose button. */}
      <div className="h-24" />
    </div>
  );
}

// ------------------------------------------------------------------ bars

function SearchBar() {
  const query = useStore((s) => s.query);
  const setQuery = useStore((s) => s.setQuery);
  const closeSearch = useStore((s) => s.closeSearch);
  const [text, setText] = useState(query);
  useEffect(() => {
    const t = window.setTimeout(() => text !== query && setQuery(text), 220);
    return () => window.clearTimeout(t);
  }, [text, query, setQuery]);
  return (
    <>
      <BarButton label="Close search" onClick={closeSearch}>
        <ArrowLeft size={22} />
      </BarButton>
      <input
        autoFocus
        value={text}
        onChange={(e) => setText(e.target.value)}
        enterKeyHint="search"
        onKeyDown={(e) => e.key === "Enter" && (e.currentTarget.blur(), setQuery(text))}
        placeholder="Search mail, from:, is:unread…"
        className="h-11 min-w-0 flex-1 bg-transparent text-[16px] placeholder:text-fg-4"
      />
      {text && (
        <BarButton label="Clear" onClick={() => setText("")}>
          <X size={20} />
        </BarButton>
      )}
    </>
  );
}

function SelectionBar() {
  const selected = useStore((s) => s.selected);
  const threads = useStore((s) => s.threads);
  const view = useStore((s) => s.view);
  const { clearSelection, act, set, toggleStar, toggleRead, selectAll } = useStore.getState();
  const [more, setMore] = useState<SheetAction[] | null>(null);
  const ids = [...selected];
  const picked = threads.filter((t) => selected.has(t.id));
  const restorable = picked.length > 0 && picked.every((t) => canRestore(t, view));
  const doneable = picked.some((t) => t.inInbox);
  const anyUnread = picked.some((t) => t.unread);
  const allStarred = picked.length > 0 && picked.every((t) => t.starred);

  return (
    <>
      <BarButton label="Clear selection" onClick={clearSelection}>
        <X size={22} />
      </BarButton>
      <span className="flex-1 text-[17px] font-semibold tabular-nums">{selected.size}</span>
      {restorable ? (
        <BarButton label="Move to inbox" onClick={() => act("inbox", ids)}>
          <InboxIcon size={21} />
        </BarButton>
      ) : (
        doneable && (
          <BarButton label="Done" onClick={() => act("archive", ids)}>
            <Archive size={21} />
          </BarButton>
        )
      )}
      <BarButton label="Snooze" onClick={() => set({ snoozeFor: ids })}>
        <Clock size={21} />
      </BarButton>
      <BarButton label="Trash" onClick={() => act("trash", ids)}>
        <Trash2 size={21} />
      </BarButton>
      <BarButton
        label="More"
        onClick={() =>
          setMore([
            { label: anyUnread ? "Mark as read" : "Mark as unread", icon: anyUnread ? MailOpen : Mail, run: toggleRead },
            { label: allStarred ? "Unstar" : "Star", icon: Star, run: toggleStar },
            { label: "Select all", icon: Check, run: selectAll },
            ...(view === "spam" ? [] : [{ label: "Mark as spam", icon: ShieldAlert, run: () => act("spam", ids), danger: true }]),
          ])
        }
      >
        <MoreVertical size={21} />
      </BarButton>
      <ActionSheet actions={more} onClose={() => setMore(null)} title={`${selected.size} selected`} />
    </>
  );
}

function SplitChips() {
  const splits = useStore((s) => s.splits);
  const splitId = useStore((s) => s.splitId);
  const counts = useStore((s) => s.counts);
  const setSplit = useStore((s) => s.setSplit);
  const chip = (id: string | null, label: string, count?: number) => (
    <button
      key={id ?? "all"}
      onClick={() => setSplit(id)}
      className={clsx(
        "flex h-8 shrink-0 items-center gap-1.5 rounded-full border px-3.5 text-[13.5px] transition-colors",
        splitId === id ? "border-transparent bg-fg text-bg" : "border-line-strong text-fg-2 active:bg-active",
      )}
    >
      {label}
      {!!count && <span className={clsx("text-[12px] tabular-nums", splitId === id ? "opacity-70" : "text-fg-3")}>{count}</span>}
    </button>
  );
  return (
    <div className="flex gap-2 overflow-x-auto px-4 pb-2.5 [scrollbar-width:none]">
      {splits.map((s) => chip(s.id, s.name, counts[`split:${s.id}`]))}
      {chip(null, "All")}
    </div>
  );
}

// ------------------------------------------------------------------ screen

export function InboxScreen({ onMenu }: { onMenu: () => void }) {
  const view = useStore((s) => s.view);
  const searchOpen = useStore((s) => s.searchOpen);
  const selected = useStore((s) => s.selected);
  const accounts = useStore((s) => s.accounts);
  const accountFilter = useStore((s) => s.accountFilter);
  const counts = useStore((s) => s.counts);
  const { openSearch, closeSearch, clearSelection, compose } = useStore.getState();
  const searching = searchOpen || view === "search";
  const filterAccount = accounts.find((a) => a.id === accountFilter);
  const unread = counts[accountFilter ? "filtered" : "inbox"];

  useBackLayer(searching, closeSearch);
  useBackLayer(selected.size > 0, clearSelection);

  return (
    <div className="flex h-full flex-col bg-panel">
      <div className="shrink-0 bg-panel pt-[env(safe-area-inset-top)]">
        <header className="flex h-14 items-center gap-1 px-1.5">
          {selected.size > 0 ? (
            <SelectionBar />
          ) : searching ? (
            <SearchBar />
          ) : (
            <>
              <BarButton label="Menu" onClick={onMenu}>
                <Menu size={22} />
              </BarButton>
              <button onClick={onMenu} className="flex min-w-0 flex-1 flex-col items-start px-1 text-left">
                <span className="text-[18px] font-semibold leading-tight tracking-tight">
                  {TITLES[view]}
                  {view === "inbox" && !!unread && <span className="ml-2 text-[14px] font-normal text-fg-3 tabular-nums">{unread}</span>}
                </span>
                {filterAccount && (
                  <span className="flex max-w-full items-center gap-1.5 text-[12px] text-fg-3">
                    <span className="h-1.5 w-1.5 shrink-0 rounded-full" style={{ background: filterAccount.color }} />
                    <span className="truncate">{accountLabel(filterAccount)}</span>
                  </span>
                )}
              </button>
              <BarButton label="Search" onClick={openSearch}>
                <Search size={21} />
              </BarButton>
            </>
          )}
        </header>
        {view === "inbox" && !searching && selected.size === 0 && <SplitChips />}
        <ReconnectBanner />
      </div>
      <div className="min-h-0 flex-1 border-t border-line">
        <List />
      </div>
      {selected.size === 0 && (
        <button
          aria-label="Compose"
          onClick={() => compose()}
          className="fixed bottom-[calc(env(safe-area-inset-bottom)+20px)] right-5 z-30 flex h-14 items-center gap-2 rounded-2xl bg-accent-strong pl-4 pr-5 text-[15px] font-medium text-white shadow-[0_8px_24px_-6px_rgb(0_0_0/0.5)] active:scale-[0.97] transition-transform"
        >
          <PenSquare size={19} />
          Compose
        </button>
      )}
    </div>
  );
}
