import clsx from "clsx";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  Archive,
  ArrowLeft,
  ChevronDown,
  ChevronUp,
  Clock,
  Download,
  FileIcon,
  Forward,
  MailOpen,
  Paperclip,
  RefreshCw,
  Reply,
  ReplyAll,
  Sparkles,
  Star,
  Trash2,
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useStore } from "../store";
import { aiStream, api, errorMessage } from "../lib/api";
import { displayName, formatBytes, longDate, shortDate } from "../lib/format";
import type { Message, ThreadDetail } from "../lib/types";
import { Avatar, IconButton, Kbd, Spinner } from "./ui";
import { EmailFrame } from "./EmailFrame";
import { Composer } from "./Composer";

function Recipients({ m, me }: { m: Message; me: string[] }) {
  const list = [...m.to, ...m.cc];
  if (!list.length) return null;
  return (
    <span className="truncate text-fg-3">
      to {list.map((a) => displayName(a, me)).join(", ")}
    </span>
  );
}

function Attachments({ m }: { m: Message }) {
  const toast = useStore((s) => s.toast);
  const [busy, setBusy] = useState<number | null>(null);
  const files = m.attachments.filter((a) => !a.inline);
  if (!files.length) return null;
  const run = async (index: number, save: boolean) => {
    setBusy(index);
    try {
      const path = await api.openAttachment(m.id, index, save);
      if (save) toast(`Saved to ${path}`, { kind: "success" });
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
  if (!expanded) {
    return (
      <button
        onClick={onToggle}
        className="flex w-full items-center gap-3 border-b border-line px-6 py-2.5 text-left hover:bg-hover"
      >
        <Avatar address={m.from} size={24} />
        <span className={clsx("w-40 shrink-0 truncate text-[13px]", !m.isRead ? "font-semibold" : "text-fg-2")}>
          {displayName(m.from, me)}
        </span>
        <span className="min-w-0 flex-1 truncate text-[13px] text-fg-3">{m.snippet}</span>
        {m.attachments.some((a) => !a.inline) && <Paperclip size={12} className="text-fg-3" />}
        <span className="text-xs tabular-nums text-fg-3">{shortDate(m.date)}</span>
      </button>
    );
  }
  return (
    <article className="border-b border-line px-6 pb-6 pt-4">
      <header className="mb-4 flex items-start gap-3">
        <Avatar address={m.from} size={32} />
        <div className="min-w-0 flex-1 cursor-pointer" onClick={onToggle}>
          <div className="flex items-baseline gap-2">
            <span className="truncate text-[13.5px] font-semibold">{displayName(m.from, me)}</span>
            <span className="truncate text-xs text-fg-3 selectable">{m.from.email}</span>
          </div>
          <div className="flex gap-2 text-xs">
            <Recipients m={m} me={me} />
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2 text-xs text-fg-3">
          {unsubscribe && (
            <button className="rounded px-1.5 py-0.5 hover:bg-hover hover:text-fg-2" onClick={() => openUrl(unsubscribe)}>
              Unsubscribe
            </button>
          )}
          <span title={longDate(m.date)}>{longDate(m.date)}</span>
        </div>
      </header>
      <div className="pl-11 selectable">
        {m.bodyHtml ? (
          <EmailFrame html={m.bodyHtml} sender={m.from.email} />
        ) : (
          <div className="prose-plain text-fg">{m.bodyText ?? m.snippet}</div>
        )}
        <Attachments m={m} />
      </div>
    </article>
  );
}

function AiSummary({ detail }: { detail: ThreadDetail }) {
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
  const auto = settings.ai_auto_summary !== "false";

  useEffect(() => {
    setText(detail.thread.aiSummary);
    setState("idle");
    if (ai?.connected && auto && long) run(false);
    return () => cancel.current?.();
    // Re-run only when the thread or its message count changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [threadId, detail.messages.length, ai?.connected]);

  useEffect(() => {
    const h = () => (ai?.connected ? run(true) : set({ settingsSection: "ai" }));
    window.addEventListener("cm:summarize", h);
    return () => window.removeEventListener("cm:summarize", h);
  }, [run, ai?.connected, set]);

  if (!ai?.connected || (!text && state === "idle" && !long)) return null;
  if (!text && state === "idle") {
    return (
      <button
        onClick={() => run(false)}
        className="mx-6 mt-4 flex items-center gap-2 rounded-lg border border-line px-3 py-2 text-xs text-fg-3 hover:bg-hover hover:text-fg-2"
      >
        <Sparkles size={13} className="text-accent" /> Summarize thread <Kbd keys={["Shift", "S"]} />
      </button>
    );
  }

  const lines = (text ?? "").split("\n").map((l) => l.trim()).filter(Boolean);
  const [tldr, ...rest] = lines;
  return (
    <section className="mx-6 mt-4 overflow-hidden rounded-lg border border-accent/25 bg-accent-soft/40">
      <div className="flex items-center gap-2 px-3.5 pt-2.5 text-[11.5px] font-medium text-accent">
        <Sparkles size={12} />
        <span className="flex-1">Summary</span>
        {state === "loading" && <Spinner size={11} className="text-accent" />}
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
                    <li key={i} className="mt-2 flex gap-2 font-medium text-fg">
                      <span className="rounded bg-warning/15 px-1.5 text-[11px] leading-5 text-warning">Action</span>
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

export function ThreadView({ id }: { id: string }) {
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
  const me = accounts.map((a) => a.email);
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
    <div className="flex h-full flex-col">
      <div className="flex h-11 shrink-0 items-center gap-1 border-b border-line px-3">
        <IconButton label="Back" shortcut={["Esc"]} onClick={closeThread}>
          <ArrowLeft size={15} />
        </IconButton>
        <div className="mx-1 h-4 w-px bg-line-strong" />
        <IconButton label="Done" shortcut={["E"]} onClick={() => act("archive")}>
          <Archive size={15} />
        </IconButton>
        <IconButton label="Snooze" shortcut={["H"]} onClick={() => set({ snoozeFor: [id] })}>
          <Clock size={15} />
        </IconButton>
        <IconButton label="Trash" shortcut={["#"]} onClick={() => act("trash")}>
          <Trash2 size={15} />
        </IconButton>
        <IconButton label="Star" shortcut={["S"]} onClick={toggleStar}>
          <Star size={15} className={t?.starred ? "fill-warning text-warning" : ""} />
        </IconButton>
        <IconButton label="Mark unread" shortcut={["U"]} onClick={toggleRead}>
          <MailOpen size={15} />
        </IconButton>
        <div className="flex-1" />
        {threads.length > 0 && (
          <span className="mr-2 text-xs tabular-nums text-fg-3">
            {cursor + 1} / {threads.length}
          </span>
        )}
        <span className="flex items-center gap-1 text-[11px] text-fg-4">
          <Kbd keys={["J"]} />
          <Kbd keys={["K"]} />
        </span>
      </div>

      <div ref={scrollRef} className="flex-1 overflow-y-auto">
        <div className="mx-auto max-w-[920px] pb-10">
          <div className="px-6 pb-1 pt-6">
            <h2 className="text-[19px] font-semibold leading-snug tracking-[-0.01em] selectable">
              {t?.subject || "(no subject)"}
            </h2>
            {account && (
              <div className="mt-1.5 flex items-center gap-2 text-xs text-fg-3">
                <span className="h-1.5 w-1.5 rounded-full" style={{ background: account.color }} />
                {account.label ? `${account.label} · ${account.email}` : account.email}
                {t && t.category !== "primary" && (
                  <span className="rounded-full border border-line-strong px-1.5 py-px text-[10.5px] capitalize">{t.category}</span>
                )}
              </div>
            )}
          </div>

          {detail && <AiSummary detail={detail} />}

          <div className="mt-4 border-t border-line">
            {!detail &&
              Array.from({ length: 2 }).map((_, i) => (
                <div key={i} className="flex flex-col gap-2 border-b border-line px-6 py-5">
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
            <div className="px-6 pt-5">
              <Composer key={composer.key} />
            </div>
          ) : (
            latest && (
              <div className="flex gap-2 px-6 pt-5">
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
