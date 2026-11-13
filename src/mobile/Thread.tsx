import clsx from "clsx";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  Archive,
  ArrowLeft,
  Clock,
  Copy,
  Download,
  FileIcon,
  Forward,
  Image,
  Inbox,
  Mail,
  MailMinus,
  Moon,
  MoreVertical,
  Paperclip,
  Reply,
  ReplyAll,
  ShieldAlert,
  Star,
  Sun,
  Trash2,
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { canRestore, useStore } from "../store";
import { api, errorMessage } from "../lib/api";
import { displayName, formatBytes, longDate, shortDate } from "../lib/format";
import type { Message, ThreadDetail } from "../lib/types";
import { emailIsDark, toggleEmailTheme } from "../lib/commands";
import { Avatar, Spinner } from "../components/ui";
import { EmailFrame } from "../components/EmailFrame";
import { AiSummary } from "../components/ThreadView";
import { ActionSheet, type SheetAction } from "./Sheets";
import { BarButton } from "./Inbox";
import { AddressChip, MessageDetails, RecipientsLine, selectingText } from "../components/People";
import { writeText } from "../lib/clipboard";

function Attachments({ m }: { m: Message }) {
  const toast = useStore((s) => s.toast);
  const [busy, setBusy] = useState<number | null>(null);
  const files = m.attachments.filter((a) => !a.inline);
  if (!files.length) return null;
  const save = async (index: number) => {
    setBusy(index);
    try {
      const path = await api.openAttachment(m.id, index, true);
      toast(`Saved ${path.split(/[\\/]/).pop()} to Downloads`, { kind: "success" });
    } catch (e) {
      toast(errorMessage(e), { kind: "error" });
    } finally {
      setBusy(null);
    }
  };
  return (
    <div className="mt-3 flex flex-col gap-2">
      {files.map((a) => (
        <button
          key={a.index}
          onClick={() => save(a.index)}
          className="flex h-14 items-center gap-3 rounded-xl border border-line-strong bg-elev-2 px-3.5 text-left active:bg-active"
        >
          <FileIcon size={20} className="shrink-0 text-fg-3" />
          <span className="min-w-0 flex-1">
            <span className="block truncate text-[14px] font-medium">{a.filename}</span>
            <span className="block text-[12px] text-fg-3">{formatBytes(a.size)}</span>
          </span>
          {busy === a.index ? <Spinner size={16} /> : <Download size={18} className="text-fg-3" />}
        </button>
      ))}
    </div>
  );
}

function MessageCard({ m, expanded, onToggle, me }: { m: Message; expanded: boolean; onToggle: () => void; me: string[] }) {
  const [details, setDetails] = useState(false);
  const [actions, setActions] = useState<SheetAction[] | null>(null);
  // Android fires contextmenu on long-press: the touch version of the desktop right-click menu.
  const onLongPress = (e: React.MouseEvent) => {
    if (selectingText()) return;
    e.preventDefault();
    const { reply, toast } = useStore.getState();
    setActions([
      { label: "Reply to this message", icon: Reply, run: () => reply("reply", m.id, false) },
      { label: "Reply all", icon: ReplyAll, run: () => reply("replyAll", m.id, false) },
      { label: "Forward this message", icon: Forward, run: () => reply("forward", m.id, false) },
      { label: `Copy ${m.from.email}`, icon: Copy, run: () => writeText(m.from.email).then(() => toast("Copied")) },
    ]);
  };
  const sheet = <ActionSheet actions={actions} onClose={() => setActions(null)} title={`${displayName(m.from, me)} · ${shortDate(m.date)}`} />;
  if (!expanded) {
    return (
      <>
      {sheet}
      <button onClick={onToggle} onContextMenu={onLongPress} className="flex w-full items-center gap-3 border-b border-line px-4 py-3 text-left active:bg-hover">
        <Avatar address={m.from} size={32} />
        <span className="min-w-0 flex-1">
          <span className="flex items-baseline gap-2">
            <span className={clsx("min-w-0 flex-1 truncate text-[14.5px]", !m.isRead ? "font-semibold" : "text-fg-2")}>{displayName(m.from, me)}</span>
            {m.attachments.some((a) => !a.inline) && <Paperclip size={12} className="shrink-0 text-fg-3" />}
            <span className="shrink-0 text-[12px] tabular-nums text-fg-3">{shortDate(m.date)}</span>
          </span>
          <span className="block truncate text-[13.5px] text-fg-3">{m.snippet}</span>
        </span>
      </button>
      </>
    );
  }
  return (
    <article className="border-b border-line px-4 pb-5 pt-4">
      {sheet}
      <header className="mb-3 flex items-start gap-3" onClick={() => !selectingText() && onToggle()} onContextMenu={onLongPress}>
        <Avatar address={m.from} size={36} />
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline gap-2">
            <span className="min-w-0 flex-1">
              <AddressChip a={m.from} me={me} className="text-[15px] font-semibold" />
            </span>
            <span className="shrink-0 text-[12px] tabular-nums text-fg-3" title={longDate(m.date)}>
              {shortDate(m.date)}
            </span>
          </div>
          <div className="flex">
            <span className="selectable min-w-0 truncate text-[12.5px] text-fg-3" onClick={(e) => e.stopPropagation()} onContextMenu={(e) => e.stopPropagation()}>
              {m.from.email}
            </span>
          </div>
          <RecipientsLine m={m} me={me} open={details} onToggle={() => setDetails(!details)} className="text-[12.5px]" />
          {details && <MessageDetails m={m} me={me} />}
        </div>
      </header>
      {/* EmailFrame pulls itself out by the paper's padding; this leaves an 8px margin. */}
      <div className="selectable -mx-2 px-[18px]">
        <EmailFrame html={m.bodyHtml} text={m.bodyText ?? m.snippet} sender={m.from.email} />
      </div>
      <Attachments m={m} />
    </article>
  );
}

export function ThreadScreen({ id }: { id: string }) {
  const [detail, setDetail] = useState<ThreadDetail | null>(null);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [error, setError] = useState("");
  const [more, setMore] = useState<SheetAction[] | null>(null);
  const accounts = useStore((s) => s.accounts);
  const threads = useStore((s) => s.threads);
  const view = useStore((s) => s.view);
  useStore((s) => s.settings.email_theme);
  const { closeThread, act, set, reply, toggleStar, toggleRead } = useStore.getState();
  const scrollRef = useRef<HTMLDivElement>(null);
  const me = accounts.map((a) => a.email);

  const load = useCallback(
    async (initial: boolean) => {
      try {
        const d = await api.getThread(id);
        setDetail(d);
        if (initial) {
          // Expand unread messages plus the latest one, like on desktop.
          const ex = new Set(d.messages.filter((m) => !m.isRead).map((m) => m.id));
          const last = d.messages[d.messages.length - 1];
          if (last) ex.add(last.id);
          setExpanded(ex);
        }
      } catch (e) {
        setError(errorMessage(e));
      }
    },
    [id],
  );

  useEffect(() => {
    setDetail(null);
    setError("");
    load(true);
    scrollRef.current?.scrollTo(0, 0);
    const un = listen("mail:changed", () => load(false));
    return () => {
      un.then((f) => f());
    };
  }, [id, load]);

  const t = detail?.thread ?? threads.find((x) => x.id === id);
  const account = accounts.find((a) => a.id === t?.accountId);
  const latest = detail?.messages.filter((m) => !m.isDraft).slice(-1)[0];
  const restorable = canRestore(t, view);
  const unsubscribe = [...(detail?.messages ?? [])]
    .reverse()
    .map((m) => m.listUnsubscribe?.match(/<(https?:[^>]+)>/)?.[1])
    .find(Boolean);
  const emailDark = emailIsDark();

  const openMore = () =>
    setMore([
      { label: t?.starred ? "Unstar" : "Star", icon: Star, run: toggleStar },
      { label: "Mark as unread", icon: Mail, run: toggleRead },
      { label: emailDark ? "Show emails light" : "Show emails dark", icon: emailDark ? Sun : Moon, run: toggleEmailTheme },
      { label: "Load remote images", icon: Image, run: () => window.dispatchEvent(new CustomEvent("cm:load-images")) },
      ...(unsubscribe ? [{ label: "Unsubscribe", icon: MailMinus, run: () => openUrl(unsubscribe) }] : []),
      ...(view === "spam" ? [] : [{ label: "Mark as spam", icon: ShieldAlert, run: () => act("spam", [id]), danger: true }]),
    ]);

  return (
    <div className="flex h-full flex-col bg-panel animate-fade">
      <header className="flex shrink-0 items-center gap-0.5 border-b border-line px-1.5 pt-[env(safe-area-inset-top)]">
        <div className="flex h-14 flex-1 items-center gap-0.5">
          <BarButton label="Back" onClick={closeThread}>
            <ArrowLeft size={22} />
          </BarButton>
          <div className="flex-1" />
          {restorable ? (
            <BarButton label="Move to inbox" onClick={() => act("inbox", [id])}>
              <Inbox size={21} />
            </BarButton>
          ) : (
            t?.inInbox !== false && (
              <BarButton label="Done" onClick={() => act("archive", [id])}>
                <Archive size={21} />
              </BarButton>
            )
          )}
          <BarButton label="Snooze" onClick={() => set({ snoozeFor: [id] })}>
            <Clock size={21} />
          </BarButton>
          <BarButton label="Trash" onClick={() => act("trash", [id])}>
            <Trash2 size={21} />
          </BarButton>
          <BarButton label="More" onClick={openMore}>
            <MoreVertical size={21} />
          </BarButton>
        </div>
      </header>

      <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto overscroll-y-contain">
        {error ? (
          <div className="p-8 text-center text-fg-3">{error}</div>
        ) : (
          <>
            <div className="px-4 pb-1 pt-4">
              <h2 className="text-[21px] font-semibold leading-snug tracking-[-0.01em] selectable">
                {t?.subject || "(no subject)"}
                {t?.starred && <Star size={16} className="ml-2 inline -translate-y-0.5 fill-warning text-warning" />}
              </h2>
              {account && (
                <div className="mt-1.5 flex min-w-0 items-center gap-2 text-[12.5px] text-fg-3">
                  <span className="h-1.5 w-1.5 shrink-0 rounded-full" style={{ background: account.color }} />
                  <span className="truncate">{account.label ? `${account.label} · ${account.email}` : account.email}</span>
                  {t && t.category !== "primary" && (
                    <span className="shrink-0 rounded-full border border-line-strong px-1.5 py-px text-[11px] capitalize">{t.category}</span>
                  )}
                </div>
              )}
            </div>
            {detail && <AiSummary detail={detail} preview={false} />}
            <div className="mt-3 border-t border-line">
              {!detail &&
                Array.from({ length: 2 }).map((_, i) => (
                  <div key={i} className="flex flex-col gap-2 border-b border-line px-4 py-5">
                    <div className="skeleton h-3 w-40" />
                    <div className="skeleton h-3 w-[80%]" />
                    <div className="skeleton h-3 w-[60%]" />
                  </div>
                ))}
              {detail?.messages.map((m) => (
                <MessageCard
                  key={m.id}
                  m={m}
                  me={me}
                  expanded={expanded.has(m.id)}
                  onToggle={() =>
                    setExpanded((s) => {
                      const n = new Set(s);
                      if (n.has(m.id)) n.delete(m.id);
                      else n.add(m.id);
                      return n;
                    })
                  }
                />
              ))}
            </div>
            <div className="h-6" />
          </>
        )}
      </div>

      {latest && (
        <footer className="flex shrink-0 gap-2 border-t border-line bg-panel px-3 pb-[max(env(safe-area-inset-bottom),10px)] pt-2.5">
          <ReplyButton icon={<Reply size={18} />} label="Reply" onClick={() => reply("reply", latest.id, false)} />
          <ReplyButton icon={<ReplyAll size={18} />} label="Reply all" onClick={() => reply("replyAll", latest.id, false)} />
          <ReplyButton icon={<Forward size={18} />} label="Forward" onClick={() => reply("forward", latest.id, false)} />
        </footer>
      )}
      <ActionSheet actions={more} onClose={() => setMore(null)} title={t?.subject} />
    </div>
  );
}

function ReplyButton({ icon, label, onClick }: { icon: React.ReactNode; label: string; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="flex h-11 flex-1 items-center justify-center gap-2 rounded-xl border border-line-strong text-[14px] text-fg-2 active:bg-active"
    >
      {icon}
      {label}
    </button>
  );
}
