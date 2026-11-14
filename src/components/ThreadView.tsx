import clsx from "clsx";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  Archive,
  ArrowLeft,
  ChevronDown,
  ChevronUp,
  Clock,
  Copy,
  Download,
  FileIcon,
  Forward,
  Inbox,
  MailOpen,
  Moon,
  PanelRight,
  Sun,
  Paperclip,
  Pencil,
  RefreshCw,
  Reply,
  ReplyAll,
  Sparkles,
  Star,
  Trash2,
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { canRestore, useStore } from "../store";
import { aiStream, api, errorMessage } from "../lib/api";
import { isMobile } from "../lib/platform";
import { displayName, formatBytes, longDate, shortDate } from "../lib/format";
import type { Message, ThreadDetail } from "../lib/types";
import { Avatar, Button, IconButton, Kbd, Spinner } from "./ui";
import { EmailFrame } from "./EmailFrame";
import { writeText } from "../lib/clipboard";
import { emailIsDark, toggleEmailTheme } from "../lib/commands";
import { Composer } from "./Composer";
import { AddressChip, MessageDetails, RecipientsLine, selectingText } from "./People";

/** Saves all attachments of these messages to Downloads, with a toast to reveal them. */
async function saveAll(messageIds: string[]) {
  const { toast, dismissToast } = useStore.getState();
  const progress = toast("Saving attachments…", { duration: 0 });
  try {
    const paths = await api.saveAttachments(messageIds);
    dismissToast(progress);
    if (!paths.length) {
      toast("No attachments in this conversation");
      return;
    }
    toast(paths.length === 1 ? `Saved ${paths[0].split(/[\\/]/).pop()} to Downloads` : `Saved ${paths.length} files to Downloads`, {
      kind: "success",
      action: { label: "Show", run: () => revealItemInDir(paths[0]).catch(() => {}) },
    });
  } catch (e) {
    dismissToast(progress);
    toast(errorMessage(e), { kind: "error" });
  }
}

function Attachments({ m }: { m: Message }) {
  const toast = useStore((s) => s.toast);
  const [busy, setBusy] = useState<number | null>(null);
  const [savingAll, setSavingAll] = useState(false);
  const files = m.attachments.filter((a) => !a.inline);
  if (!files.length) return null;
  const run = async (index: number, save: boolean) => {
    setBusy(index);
    try {
      const path = await api.openAttachment(m.id, index, save);
      if (save) toast(`Saved to ${path}`, { kind: "success", action: { label: "Show", run: () => revealItemInDir(path).catch(() => {}) } });
    } catch (e) {
      toast(errorMessage(e), { kind: "error" });
    } finally {
      setBusy(null);
    }
  };
  return (
    <div className="mt-4 flex flex-wrap gap-2">
      {files.map((a) => (
        <div
          key={a.index}
          className="group flex h-10 max-w-[260px] items-center gap-2 rounded-lg border border-line-strong bg-elev-2 pl-2.5 pr-1 hover:border-fg-4"
        >
          <FileIcon size={15} className="shrink-0 text-fg-3" />
          <button onClick={() => run(a.index, false)} className="min-w-0 flex-1 text-left" title={`Open ${a.filename}`}>
            <div className="truncate text-xs font-medium">{a.filename}</div>
            <div className="text-[11px] text-fg-3">{formatBytes(a.size)}</div>
          </button>
          {busy === a.index ? (
            <Spinner size={13} className="mx-1.5" />
          ) : (
            <IconButton label="Save to Downloads" onClick={() => run(a.index, true)} className="h-7 w-7 opacity-0 group-hover:opacity-100">
              <Download size={13} />
            </IconButton>
          )}
        </div>
      ))}
      {files.length > 1 && (
        <button
          onClick={async () => {
            setSavingAll(true);
            await saveAll([m.id]);
            setSavingAll(false);
          }}
          disabled={savingAll}
          title="Save all attachments to Downloads  ·  Shift+D"
          className="flex h-10 items-center gap-2 rounded-lg border border-dashed border-line-strong px-3 text-xs text-fg-2 hover:border-fg-4 hover:text-fg"
        >
          {savingAll ? <Spinner size={13} /> : <Download size={13} />}
          Save all {files.length}
          <Kbd keys={["Shift", "D"]} />
        </button>
      )}
    </div>
  );
}

function MessageCard({
  m,
  expanded,
  onToggle,
  me,
}: {
  m: Message;
  expanded: boolean;
  onToggle: () => void;
  me: string[];
}) {
  const unsubscribe = m.listUnsubscribe?.match(/<(https?:[^>]+)>/)?.[1];
  const reply = useStore((s) => s.reply);
  const editDraft = useStore((s) => s.editDraft);
  const discardDraft = useStore((s) => s.discardDraft);
  const openMenu = useStore((s) => s.openMenu);
  const toast = useStore((s) => s.toast);
  const [details, setDetails] = useState(false);
  const onContextMenu = (e: React.MouseEvent) => {
    if (window.getSelection()?.toString()) return;
    e.preventDefault();
    if (m.isDraft) {
      openMenu(e.clientX, e.clientY, [
        { label: "Edit draft", icon: Pencil, run: () => editDraft(m.id) },
        { label: "Delete draft", icon: Trash2, run: () => discardDraft(m.id) },
        { label: expanded ? "Collapse" : "Expand", icon: expanded ? ChevronUp : ChevronDown, separator: true, run: onToggle },
      ]);
      return;
    }
    openMenu(e.clientX, e.clientY, [
      { label: "Reply to this message", icon: Reply, run: () => reply("reply", m.id) },
      { label: "Reply all", icon: ReplyAll, run: () => reply("replyAll", m.id) },
      { label: "Forward this message", icon: Forward, run: () => reply("forward", m.id, false) },
      { label: `Copy ${m.from.email}`, icon: Copy, separator: true, run: () => writeText(m.from.email).then(() => toast("Copied")) },
      { label: expanded ? "Collapse" : "Expand", icon: expanded ? ChevronUp : ChevronDown, run: onToggle },
    ]);
  };
  if (!expanded) {
    return (
      <button
        onClick={onToggle}
        onContextMenu={onContextMenu}
        className="flex w-full items-center gap-3 border-b border-line px-4 py-2.5 text-left hover:bg-hover @xl:px-6"
      >
        <Avatar address={m.from} size={24} />
        <span className={clsx("w-28 shrink-0 truncate text-[13px] @xl:w-40", m.isDraft ? "text-danger/90" : !m.isRead ? "font-semibold" : "text-fg-2")}>
          {m.isDraft ? "Draft" : displayName(m.from, me)}
        </span>
        <span className="min-w-0 flex-1 truncate text-[13px] text-fg-3">{m.snippet}</span>
        {m.attachments.some((a) => !a.inline) && <Paperclip size={12} className="text-fg-3" />}
        <span className="text-xs tabular-nums text-fg-3">{shortDate(m.date)}</span>
      </button>
    );
  }
  return (
    <article className="border-b border-line px-4 pb-6 pt-4 @xl:px-6">
      <header
        className="mb-4 flex cursor-pointer items-start gap-3"
        onClick={() => !selectingText() && onToggle()}
        onContextMenu={onContextMenu}
      >
        <Avatar address={m.from} size={32} />
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-baseline gap-2">
            <AddressChip a={m.from} me={me} className="max-w-[65%] shrink-0 text-[13.5px] font-semibold" />
            <span className="selectable min-w-0 cursor-text truncate text-xs text-fg-3" onClick={(e) => e.stopPropagation()}>
              {m.from.email}
            </span>
          </div>
          <RecipientsLine m={m} me={me} open={details} onToggle={() => setDetails(!details)} className="text-xs" />
          {details && <MessageDetails m={m} me={me} />}
        </div>
        <div className="flex shrink-0 items-center gap-2 text-xs text-fg-3">
          {unsubscribe && (
            <button className="rounded px-1.5 py-0.5 hover:bg-hover hover:text-fg-2" onClick={(e) => { e.stopPropagation(); openUrl(unsubscribe); }}>
              Unsubscribe
            </button>
          )}
          <span title={longDate(m.date)} className="hidden @2xl:inline">{longDate(m.date)}</span>
          <span title={longDate(m.date)} className="tabular-nums @2xl:hidden">{shortDate(m.date)}</span>
        </div>
      </header>
      <div className="selectable @xl:pl-11">
        {m.isDraft && <DraftBar onEdit={() => editDraft(m.id)} onDiscard={() => discardDraft(m.id)} />}
        <EmailFrame html={m.bodyHtml} text={m.bodyText ?? m.snippet} sender={m.from.email} />
        <Attachments m={m} />
      </div>
    </article>
  );
}

/** Shown on a draft from the Drafts folder: it hasn't been sent, and here is how to finish it. */
export function DraftBar({ onEdit, onDiscard }: { onEdit: () => void; onDiscard: () => void }) {
  return (
    <div className="mb-3 flex flex-wrap items-center gap-x-3 gap-y-2 rounded-lg border border-line-strong bg-elev px-3 py-2">
      <Pencil size={14} className="shrink-0 text-danger/90" />
      <span className="min-w-[10rem] flex-1 text-[13px]">
        <span className="font-medium text-danger/90">Draft</span> <span className="text-fg-3">· not sent yet</span>
      </span>
      <Button variant="ghost" onClick={onDiscard}>
        Delete
      </Button>
      <Button variant="primary" onClick={onEdit}>
        Edit &amp; send
      </Button>
    </div>
  );
}

export function AiSummary({ detail, preview }: { detail: ThreadDetail; preview: boolean }) {
  const ai = useStore((s) => s.ai);
  const settings = useStore((s) => s.settings);
  const set = useStore((s) => s.set);
  const [text, setText] = useState<string | null>(detail.thread.aiSummary);
  const [state, setState] = useState<"idle" | "loading" | "error">("idle");
  const [error, setError] = useState("");
  const [open, setOpen] = useState(true);
  const cancel = useRef<(() => void) | null>(null);
  const threadId = detail.thread.id;

  const run = useCallback(
    (force: boolean) => {
      cancel.current?.();
      setState("loading");
      setError("");
      if (force) setText("");
      let acc = "";
      cancel.current = aiStream(
        "ai_summarize",
        { threadId, force },
        {
          onDelta: (d) => {
            acc += d;
            setText(acc);
          },
          onDone: (t) => {
            setText(t);
            setState("idle");
          },
          onError: (m) => {
            setError(m);
            setState("error");
          },
        },
      );
    },
    [threadId],
  );

  const long =
    detail.messages.length >= 2 || detail.messages.some((m) => (m.bodyText?.length ?? 0) > 1200);
  // Previewing while skimming with J/K shouldn't burn through AI usage.
  const auto = settings.ai_auto_summary !== "false" && !preview;

  useEffect(() => {
    setText(detail.thread.aiSummary);
    setState("idle");
    if (ai?.connected && auto && long) run(false);
    return () => cancel.current?.();
    // Re-run only when the thread or its message count changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [threadId, detail.messages.length, ai?.connected]);

  // Shift+S: regenerate an existing summary, otherwise make one (cached if possible).
  const summarize = useCallback(() => {
    if (!ai?.connected) set({ settingsSection: "ai" });
    else run(!!text);
  }, [ai?.connected, run, set, text]);

  useEffect(() => {
    window.addEventListener("cm:summarize", summarize);
    return () => window.removeEventListener("cm:summarize", summarize);
  }, [summarize]);

  const pendingSummary = useStore((s) => s.pendingSummary);
  useEffect(() => {
    if (pendingSummary !== threadId) return;
    useStore.setState({ pendingSummary: null });
    // A long thread is summarized automatically on open; don't start a second run.
    if (!(ai?.connected && auto && long)) summarize();
  }, [pendingSummary, threadId, summarize, ai?.connected, auto, long]);

  if (!ai?.connected) return null;
  if (!text && state === "idle") {
    return (
      <button
        onClick={() => run(false)}
        className={clsx(
          "mx-4 mt-4 @xl:mx-6 flex items-center gap-2 rounded-lg border border-line px-3 text-fg-3 hover:bg-hover hover:text-fg-2",
          isMobile ? "h-11 text-[14px]" : "py-2 text-xs",
        )}
      >
        <Sparkles size={13} className="text-accent" /> Summarize thread
        {!isMobile && <Kbd keys={["Shift", "S"]} />}
      </button>
    );
  }

  const lines = (text ?? "").split("\n").map((l) => l.trim()).filter(Boolean);
  const [tldr, ...rest] = lines;
  return (
    <section className="mx-4 mt-4 @xl:mx-6 overflow-hidden rounded-lg border border-line bg-elev-2/60">
      <div className="flex items-center gap-2 px-3.5 pt-2.5 text-[11.5px] font-medium text-fg-3">
        <Sparkles size={12} />
        <span className="flex-1">Summary</span>
        {state === "loading" && <Spinner size={11} />}
        <IconButton label="Regenerate" className="h-5 w-5" onClick={() => run(true)}>
          <RefreshCw size={11} />
        </IconButton>
        <IconButton label={open ? "Collapse" : "Expand"} className="h-5 w-5" onClick={() => setOpen(!open)}>
          {open ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
        </IconButton>
      </div>
      <div className="px-3.5 pb-3 pt-1.5 text-[13px] leading-relaxed selectable">
        {state === "error" ? (
          <span className="text-danger">{error}</span>
        ) : (
          <>
            <p className="font-medium text-fg">{tldr ?? <span className="skeleton inline-block h-3 w-64" />}</p>
            {open && rest.length > 0 && (
              <ul className="mt-1.5 space-y-1 text-fg-2">
                {rest.map((l, i) =>
                  l.toLowerCase().startsWith("action:") ? (
                    <li key={i} className="mt-2 flex items-start gap-2 font-medium text-fg">
                      <span className="mt-px shrink-0 rounded bg-warning/15 px-1.5 text-[11px] leading-5 text-warning">Action</span>
                      {l.slice(7).trim()}
                    </li>
                  ) : (
                    <li key={i} className="flex gap-2">
                      <span className="text-fg-4">•</span>
                      {l.replace(/^[-•*]\s*/, "")}
                    </li>
                  ),
                )}
              </ul>
            )}
          </>
        )}
      </div>
    </section>
  );
}

export function ThreadView({ id, preview = false }: { id: string; preview?: boolean }) {
  const [detail, setDetail] = useState<ThreadDetail | null>(null);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [error, setError] = useState("");
  const accounts = useStore((s) => s.accounts);
  const composer = useStore((s) => s.composer);
  const closeThread = useStore((s) => s.closeThread);
  const act = useStore((s) => s.act);
  const set = useStore((s) => s.set);
  const reply = useStore((s) => s.reply);
  const toggleStar = useStore((s) => s.toggleStar);
  const toggleRead = useStore((s) => s.toggleRead);
  const threads = useStore((s) => s.threads);
  const cursor = useStore((s) => s.cursor);
  const scrollRef = useRef<HTMLDivElement>(null);
  const view = useStore((s) => s.view);
  useStore((s) => s.settings.email_theme);
  const emailDark = emailIsDark();
  const me = accounts.map((a) => a.email);

  // In the preview pane, count a thread as read once you've looked at it for a moment.
  useEffect(() => {
    if (!preview) return;
    const timer = window.setTimeout(() => {
      const st = useStore.getState();
      const t = st.threads.find((x) => x.id === id);
      if (t?.unread && st.settings.mark_read_on_open !== "false") st.act("read", [id], { silent: true });
    }, 1500);
    return () => window.clearTimeout(timer);
  }, [id, preview]);
  const account = accounts.find((a) => a.id === detail?.thread.accountId);

  const load = useCallback(
    async (initial: boolean) => {
      try {
        const d = await api.getThread(id);
        setDetail(d);
        if (initial) {
          // Expand unread messages plus the latest one, like Gmail.
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

  const latest = detail?.messages.filter((m) => !m.isDraft).slice(-1)[0];

  useEffect(() => {
    const h = (e: Event) => {
      const mode = (e as CustomEvent).detail as "reply" | "replyAll" | "forward";
      if (latest) reply(mode, latest.id, mode !== "forward");
    };
    window.addEventListener("cm:reply", h);
    return () => window.removeEventListener("cm:reply", h);
  }, [latest, reply]);

  useEffect(() => {
    const unsubscribe = () => {
      const link = [...(detail?.messages ?? [])]
        .reverse()
        .map((m) => m.listUnsubscribe?.match(/<(https?:[^>]+)>/)?.[1])
        .find(Boolean);
      if (link) openUrl(link);
      else useStore.getState().toast("This conversation has no unsubscribe link");
    };
    const expandAll = () =>
      setExpanded((cur) =>
        detail && cur.size < detail.messages.length ? new Set(detail.messages.map((m) => m.id)) : new Set(latest ? [latest.id] : []),
      );
    const downloadAll = () => {
      const ids = (detail?.messages ?? []).filter((m) => m.attachments.some((a) => !a.inline)).map((m) => m.id);
      if (ids.length) saveAll(ids);
      else useStore.getState().toast("No attachments in this conversation");
    };
    const trustImages = () => {
      const st = useStore.getState();
      let trusted: string[] = [];
      try {
        trusted = JSON.parse(st.settings.image_senders ?? "[]");
      } catch {
        /* start fresh */
      }
      const senders = [...new Set((detail?.messages ?? []).filter((m) => m.bodyHtml && !m.inSent).map((m) => m.from.email.toLowerCase()))];
      const added = senders.filter((x) => !trusted.includes(x));
      if (!added.length) {
        st.toast("Images from these senders already load automatically");
        return;
      }
      st.setSetting("image_senders", JSON.stringify([...trusted, ...added]));
      st.toast(`Images from ${added.join(", ")} will always load`, { kind: "success" });
    };
    window.addEventListener("cm:unsubscribe", unsubscribe);
    window.addEventListener("cm:expand-all", expandAll);
    window.addEventListener("cm:download-all", downloadAll);
    window.addEventListener("cm:trust-images", trustImages);
    return () => {
      window.removeEventListener("cm:unsubscribe", unsubscribe);
      window.removeEventListener("cm:expand-all", expandAll);
      window.removeEventListener("cm:download-all", downloadAll);
      window.removeEventListener("cm:trust-images", trustImages);
    };
  }, [detail, latest]);

  const pendingReply = useStore((s) => s.pendingReply);
  useEffect(() => {
    if (pendingReply && latest && detail?.thread.id === id) {
      useStore.setState({ pendingReply: null });
      reply(pendingReply, latest.id, pendingReply !== "forward");
    }
  }, [pendingReply, latest, detail, id, reply]);

  useEffect(() => {
    if (composer?.inline) {
      window.setTimeout(() => scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" }), 50);
    }
  }, [composer?.key, composer?.inline]);

  if (error) return <div className="flex h-full items-center justify-center text-fg-3">{error}</div>;

  const t = detail?.thread ?? threads.find((x) => x.id === id);

  return (
    <div className="@container flex h-full flex-col">
      <div className="flex h-11 shrink-0 items-center gap-1 border-b border-line px-3">
        {!preview && (
          <>
            <IconButton label="Back" shortcut={["Esc"]} onClick={closeThread}>
              <ArrowLeft size={15} />
            </IconButton>
            <div className="mx-1 h-4 w-px bg-line-strong" />
          </>
        )}
        {canRestore(t, view) ? (
          <IconButton label={view === "trash" ? "Restore from trash" : view === "spam" ? "Not spam" : "Move to inbox"} shortcut={["E"]} onClick={() => act("inbox", [id])}>
            <Inbox size={15} />
          </IconButton>
        ) : (
          t?.inInbox !== false && (
            <IconButton label="Done" shortcut={["E"]} onClick={() => act("archive", [id])}>
              <Archive size={15} />
            </IconButton>
          )
        )}
        <IconButton label="Snooze" shortcut={["H"]} onClick={() => set({ snoozeFor: [id] })}>
          <Clock size={15} />
        </IconButton>
        <IconButton label="Trash" shortcut={["⌫"]} onClick={() => act("trash", [id])}>
          <Trash2 size={15} />
        </IconButton>
        <IconButton label="Star" shortcut={["S"]} onClick={toggleStar}>
          <Star size={15} className={t?.starred ? "fill-warning text-warning" : ""} />
        </IconButton>
        <IconButton label="Mark unread" shortcut={["U"]} onClick={toggleRead}>
          <MailOpen size={15} />
        </IconButton>
        <div className="flex-1" />
        <IconButton label={emailDark ? "Show emails light" : "Show emails dark"} shortcut={["Shift", "L"]} onClick={toggleEmailTheme}>
          {emailDark ? <Sun size={15} /> : <Moon size={15} />}
        </IconButton>
        {!preview && (
          <IconButton label="Show preview pane" shortcut={["P"]} onClick={() => { useStore.getState().togglePreviewPane(); closeThread(); }}>
            <PanelRight size={15} />
          </IconButton>
        )}
        {threads.length > 0 && (
          <span className="mr-2 hidden whitespace-nowrap text-xs tabular-nums text-fg-3 @md:inline">
            {cursor + 1} / {threads.length}
          </span>
        )}
        <span className="hidden items-center gap-1 text-[11px] text-fg-4 @lg:flex">
          <Kbd keys={["J"]} />
          <Kbd keys={["K"]} />
        </span>
      </div>

      <div ref={scrollRef} className="flex-1 overflow-y-auto">
        <div className="mx-auto max-w-[920px] pb-10">
          <div className="px-4 pb-1 pt-6 @xl:px-6">
            <h2 className="text-[19px] font-semibold leading-snug tracking-[-0.01em] selectable">
              {t?.subject || "(no subject)"}
            </h2>
            {account && (
              <div className="mt-1.5 flex min-w-0 items-center gap-2 text-xs text-fg-3">
                <span className="h-1.5 w-1.5 rounded-full" style={{ background: account.color }} />
                {account.label ? `${account.label} · ${account.email}` : account.email}
                {t && t.category !== "primary" && (
                  <span className="rounded-full border border-line-strong px-1.5 py-px text-[10.5px] capitalize">{t.category}</span>
                )}
              </div>
            )}
          </div>

          {detail && <AiSummary detail={detail} preview={preview} />}

          <div className="mt-4 border-t border-line">
            {!detail &&
              Array.from({ length: 2 }).map((_, i) => (
                <div key={i} className="flex flex-col gap-2 border-b border-line px-4 py-5 @xl:px-6">
                  <div className="skeleton h-3 w-48" />
                  <div className="skeleton h-3 w-[70%]" />
                  <div className="skeleton h-3 w-[55%]" />
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

          {composer?.inline && composer.draft.threadId === id ? (
            <div className="px-4 pt-5 @xl:px-6">
              <Composer key={composer.key} />
            </div>
          ) : (
            latest && (
              <div className="flex flex-wrap gap-2 px-4 pt-5 @xl:px-6">
                <ReplyButton icon={<Reply size={14} />} label="Reply" k="R" onClick={() => reply("reply", latest.id)} />
                <ReplyButton icon={<ReplyAll size={14} />} label="Reply all" k="A" onClick={() => reply("replyAll", latest.id)} />
                <ReplyButton icon={<Forward size={14} />} label="Forward" k="F" onClick={() => reply("forward", latest.id, false)} />
              </div>
            )
          )}
        </div>
      </div>
    </div>
  );
}

function ReplyButton({ icon, label, k, onClick }: { icon: React.ReactNode; label: string; k: string; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="flex h-8 items-center gap-2 rounded-md border border-line-strong px-3 text-[13px] text-fg-2 hover:bg-hover hover:text-fg"
    >
      {icon}
      {label}
      <Kbd keys={[k]} className="ml-1" />
    </button>
  );
}
