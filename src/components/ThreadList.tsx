import clsx from "clsx";
import { useEffect, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Archive, Clock, Inbox, Paperclip, Star, Sparkles, Trash2 } from "lucide-react";
import { useStore } from "../store";
import { displayName, relativeFuture, shortDate } from "../lib/format";
import type { ThreadSummary } from "../lib/types";
import { openThreadMenu } from "../lib/commands";

const ROW = 40;
const ROW_COMPACT = 76;

function participantsLabel(t: ThreadSummary, me: string[]): string {
  const names = t.participants.map((p) => displayName(p, me));
  const unique = [...new Set(names)];
  if (unique.length <= 2) return unique.join(", ");
  return `${unique[0]} … ${unique[unique.length - 1]}`;
}

function Row({
  t,
  index,
  active,
  selected,
  multiAccount,
  me,
  compact,
}: {
  t: ThreadSummary;
  index: number;
  active: boolean;
  selected: boolean;
  multiAccount: boolean;
  me: string[];
  compact: boolean;
}) {
  const account = useStore((s) => s.accounts.find((a) => a.id === t.accountId));
  const openThread = useStore((s) => s.openThread);
  const setCursor = useStore((s) => s.setCursor);
  const toggleSelect = useStore((s) => s.toggleSelect);
  const act = useStore((s) => s.act);
  const set = useStore((s) => s.set);
  const view = useStore((s) => s.view);
  const showAi = useStore((s) => s.settings.ai_list_summaries !== "false");

  const preview = showAi && t.aiSummary ? t.aiSummary.split("\n")[0] : t.snippet;
  const restorable = !t.inInbox || view === "trash" || view === "spam";

  const date =
    view === "snoozed" && t.snoozedUntil ? (
      <span className="flex items-center gap-1 whitespace-nowrap text-xs text-warning">
        <Clock size={12} />
        {relativeFuture(t.snoozedUntil)}
      </span>
    ) : (
      <span className={clsx("min-w-[52px] whitespace-nowrap text-right text-xs tabular-nums group-hover:hidden", t.unread && "text-fg-2")}>
        {shortDate(t.lastDate)}
      </span>
    );

  const hoverActions = (
    <span className="hidden items-center gap-0.5 group-hover:flex" onClick={(e) => e.stopPropagation()} onDoubleClick={(e) => e.stopPropagation()}>
      {restorable ? (
        <HoverAction label="Move to inbox (E)" onClick={() => act("inbox", [t.id])}>
          <Inbox size={14} />
        </HoverAction>
      ) : (
        <HoverAction label="Done (E)" onClick={() => act("archive", [t.id])}>
          <Archive size={14} />
        </HoverAction>
      )}
      <HoverAction label="Snooze (H)" onClick={() => set({ snoozeFor: [t.id] })}>
        <Clock size={14} />
      </HoverAction>
      <HoverAction label="Star (S)" onClick={() => act(t.starred ? "unstar" : "star", [t.id])}>
        <Star size={14} className={t.starred ? "fill-warning text-warning" : ""} />
      </HoverAction>
      {view !== "trash" && (
        <HoverAction label="Trash (⌫)" onClick={() => act("trash", [t.id])}>
          <Trash2 size={14} />
        </HoverAction>
      )}
    </span>
  );

  const marker = (
    <span className="flex w-2 shrink-0 justify-center">
      {selected ? (
        <span className="h-2 w-2 rounded-[3px] bg-fg-2" />
      ) : t.unread ? (
        <span className="h-[7px] w-[7px] rounded-full bg-accent" />
      ) : null}
    </span>
  );
  const accountBar = multiAccount && account && (
    <span className="h-3.5 w-[3px] shrink-0 rounded-full opacity-80" style={{ background: account.color }} title={account.email} />
  );
  const sender = (
    <>
      {participantsLabel(t, me) || "(no sender)"}
      {t.messageCount > 1 && <span className="ml-1.5 text-[11.5px] font-normal text-fg-3">{t.messageCount}</span>}
    </>
  );

  return (
    <div
      onClick={(e) => {
        if (e.metaKey || e.ctrlKey || e.shiftKey) toggleSelect(t.id);
        // With the preview pane, a click previews and a double-click opens.
        else if (compact) setCursor(index);
        else openThread(t.id);
      }}
      onDoubleClick={() => compact && openThread(t.id)}
      onMouseMove={() => !compact && !active && setCursor(index)}
      onContextMenu={(e) => {
        e.preventDefault();
        openThreadMenu(t.id, e.clientX, e.clientY);
      }}
      className={clsx(
        "group relative flex h-full gap-3 pl-4 pr-3 text-[13px]",
        compact ? "items-start border-b border-line py-2.5 hover:bg-hover" : "items-center",
        active && (compact ? "bg-active hover:bg-active" : "bg-hover"),
        selected && "bg-active",
      )}
    >
      {active && <span className="absolute inset-y-2 left-0 w-[2px] rounded-full bg-fg-4" />}
      {compact ? (
        <>
          <span className="flex flex-col items-center gap-2 pt-[5px]">
            {marker}
          </span>
          <span className="flex min-w-0 flex-1 flex-col gap-0.5">
            <span className="flex items-center gap-2">
              {accountBar}
              <span className={clsx("min-w-0 flex-1 truncate", t.unread ? "font-semibold text-fg" : "text-fg-2")}>{sender}</span>
              <span className="flex shrink-0 items-center gap-1.5 text-fg-3">
                {t.hasAttachments && <Paperclip size={12} />}
                {t.starred && <Star size={12} className="fill-warning text-warning" />}
                {date}
                {hoverActions}
              </span>
            </span>
            <span className={clsx("truncate", t.unread ? "font-medium text-fg" : "text-fg")}>{t.subject || "(no subject)"}</span>
            <span className="flex min-w-0 items-center gap-1 text-[12.5px] text-fg-3">
              {showAi && t.aiSummary && <Sparkles size={11} className="shrink-0 text-fg-4" />}
              <span className="truncate">{preview}</span>
            </span>
          </span>
        </>
      ) : (
        <>
          {marker}
          {accountBar}
          <span className={clsx("w-[188px] shrink-0 truncate", t.unread ? "font-semibold text-fg" : "text-fg-2")}>{sender}</span>
          <span className="flex min-w-0 flex-1 items-baseline gap-2 truncate">
            <span className={clsx("shrink-0 truncate max-w-[60%]", t.unread ? "font-medium text-fg" : "text-fg")}>
              {t.subject || "(no subject)"}
            </span>
            <span className="flex min-w-0 items-center gap-1 truncate text-fg-3">
              {showAi && t.aiSummary && <Sparkles size={11} className="shrink-0 text-fg-4" />}
              <span className="truncate">{preview}</span>
            </span>
          </span>
          <span className="flex shrink-0 items-center gap-2 text-fg-3">
            {t.hasAttachments && <Paperclip size={13} />}
            {t.starred && <Star size={13} className="fill-warning text-warning" />}
            {date}
            {hoverActions}
          </span>
        </>
      )}
    </div>
  );
}

function HoverAction({ label, onClick, children }: { label: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <button title={label} onClick={onClick} className="flex h-6 w-6 items-center justify-center rounded text-fg-3 hover:bg-active hover:text-fg">
      {children}
    </button>
  );
}

function Empty() {
  const view = useStore((s) => s.view);
  const accounts = useStore((s) => s.accounts);
  const sync = useStore((s) => s.sync);
  const syncing = Object.values(sync).some((s) => s.state === "syncing");
  const firstSync = accounts.some((a) => !a.lastSync);

  if (syncing && firstSync) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-fg-3">
        <div className="text-[13px]">Downloading your mail…</div>
        <div className="text-xs text-fg-4">The first sync can take a few minutes. New mail shows up as it arrives.</div>
      </div>
    );
  }
  if (view === "inbox") {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 animate-fade">
        <div className="relative h-20 w-20">
          <div className="absolute inset-0 rounded-full bg-gradient-to-br from-fg-4/30 to-transparent blur-xl" />
          <div className="relative flex h-20 w-20 items-center justify-center rounded-full border border-line-strong bg-elev">
            <Archive size={26} className="text-fg-2" strokeWidth={1.5} />
          </div>
        </div>
        <div className="text-[15px] font-medium">You're all done</div>
        <div className="text-xs text-fg-3">Nothing left in this split. Enjoy the quiet.</div>
      </div>
    );
  }
  if (view === "search") {
    return <div className="flex h-full items-center justify-center text-fg-3">No conversations match.</div>;
  }
  return <div className="flex h-full items-center justify-center text-fg-3">Nothing here.</div>;
}

export function ThreadList({ compact = false }: { compact?: boolean }) {
  const threads = useStore((s) => s.threads);
  const cursor = useStore((s) => s.cursor);
  const selected = useStore((s) => s.selected);
  const loading = useStore((s) => s.loading);
  const accounts = useStore((s) => s.accounts);
  const accountFilter = useStore((s) => s.accountFilter);
  const hasMore = useStore((s) => s.hasMore);
  const refresh = useStore((s) => s.refresh);
  const parentRef = useRef<HTMLDivElement>(null);
  const me = accounts.map((a) => a.email);
  const multiAccount = accounts.length > 1 && !accountFilter;
  const rowHeight = compact ? ROW_COMPACT : ROW;

  const virtualizer = useVirtualizer({
    count: threads.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => rowHeight,
    overscan: 12,
  });

  useEffect(() => {
    if (threads.length) virtualizer.scrollToIndex(cursor, { align: "auto" });
  }, [cursor, threads.length, virtualizer]);

  if (loading && !threads.length) {
    return (
      <div className="flex flex-col gap-0 pt-1">
        {Array.from({ length: 12 }).map((_, i) => (
          <div key={i} className="flex h-10 items-center gap-3 px-7">
            <div className="skeleton h-3 w-36" />
            <div className="skeleton h-3 flex-1" style={{ maxWidth: `${40 + ((i * 37) % 45)}%` }} />
            <div className="skeleton h-3 w-10" />
          </div>
        ))}
      </div>
    );
  }
  if (!threads.length) return <Empty />;

  return (
    <div
      ref={parentRef}
      className="h-full overflow-y-auto"
      onScroll={(e) => {
        const el = e.currentTarget;
        if (hasMore && el.scrollTop + el.clientHeight > el.scrollHeight - rowHeight * 15) refresh({ more: true });
      }}
    >
      <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
        {virtualizer.getVirtualItems().map((v) => {
          const t = threads[v.index];
          return (
            <div
              key={t.id}
              style={{ position: "absolute", top: 0, left: 0, right: 0, height: rowHeight, transform: `translateY(${v.start}px)` }}
            >
              <Row
                t={t}
                index={v.index}
                active={v.index === cursor}
                selected={selected.has(t.id)}
                multiAccount={multiAccount}
                me={me}
                compact={compact}
              />
            </div>
          );
        })}
      </div>
      <div className="h-6" />
    </div>
  );
}
