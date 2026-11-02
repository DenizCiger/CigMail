import clsx from "clsx";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { EditorContent, useEditor, type Editor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import Placeholder from "@tiptap/extension-placeholder";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import DOMPurify from "dompurify";
import { CornerDownLeft, MoreHorizontal, Paperclip, Sparkles, Trash2, X, Wand2 } from "lucide-react";
import { useStore } from "../store";
import { aiStream, api } from "../lib/api";
import { formatAddress, parseAddresses, textToHtml } from "../lib/format";
import type { Address, Contact, Draft } from "../lib/types";
import { Avatar, Button, IconButton, Kbd, Spinner } from "./ui";

// ------------------------------------------------------------------ recipients

function isEmail(s: string) {
  return /^[^\s@<>]+@[^\s@<>]+\.[^\s@<>]+$/.test(s.trim());
}

function RecipientInput({
  label,
  value,
  onChange,
  autoFocus,
  trailing,
}: {
  label: string;
  value: Address[];
  onChange: (v: Address[]) => void;
  autoFocus?: boolean;
  trailing?: React.ReactNode;
}) {
  const [text, setText] = useState("");
  const [suggestions, setSuggestions] = useState<Contact[]>([]);
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!text.trim()) {
      setSuggestions([]);
      return;
    }
    const t = window.setTimeout(async () => {
      const res = await api.suggestContacts(text).catch(() => []);
      setSuggestions(res.filter((c) => !value.some((v) => v.email.toLowerCase() === c.email.toLowerCase())));
      setActive(0);
    }, 80);
    return () => window.clearTimeout(t);
  }, [text, value]);

  const commit = (addr?: Address) => {
    const a = addr ?? (suggestions[active] ? { name: suggestions[active].name, email: suggestions[active].email } : null);
    if (a) {
      onChange([...value, a]);
      setText("");
      setSuggestions([]);
      return true;
    }
    const parsed = parseAddresses(text).filter((p) => isEmail(p.email));
    if (parsed.length) {
      onChange([...value, ...parsed]);
      setText("");
      return true;
    }
    return false;
  };

  return (
    <div className="relative flex min-h-9 items-start gap-2 border-b border-line px-4 py-1.5">
      <span className="w-10 shrink-0 pt-[5px] text-[13px] text-fg-3">{label}</span>
      <div className="flex min-w-0 flex-1 flex-wrap items-center gap-1" onClick={() => inputRef.current?.focus()}>
        {value.map((a, i) => (
          <span
            key={`${a.email}-${i}`}
            title={a.email}
            className={clsx(
              "flex h-6 items-center gap-1 rounded-md border pl-2 pr-1 text-xs",
              isEmail(a.email) ? "border-line-strong bg-elev-2" : "border-danger/60 bg-danger/10 text-danger",
            )}
          >
            {a.name || a.email}
            <button
              onClick={(e) => {
                e.stopPropagation();
                onChange(value.filter((_, j) => j !== i));
              }}
              className="rounded p-0.5 text-fg-3 hover:text-fg"
            >
              <X size={11} />
            </button>
          </span>
        ))}
        <input
          ref={inputRef}
          autoFocus={autoFocus}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onPaste={(e) => {
            const t = e.clipboardData.getData("text");
            if (/[,;\n]/.test(t)) {
              e.preventDefault();
              onChange([...value, ...parseAddresses(t).filter((p) => isEmail(p.email))]);
            }
          }}
          onBlur={() => {
            if (isEmail(text)) commit({ name: null, email: text.trim() });
            window.setTimeout(() => setSuggestions([]), 120);
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown" && suggestions.length) {
              e.preventDefault();
              setActive((a) => Math.min(a + 1, suggestions.length - 1));
            } else if (e.key === "ArrowUp" && suggestions.length) {
              e.preventDefault();
              setActive((a) => Math.max(a - 1, 0));
            } else if ((e.key === "Enter" || e.key === "Tab" || e.key === "," || e.key === ";") && text.trim()) {
              if (commit()) e.preventDefault();
            } else if (e.key === "Backspace" && !text && value.length) {
              onChange(value.slice(0, -1));
            }
          }}
          className="h-6 min-w-[120px] flex-1 bg-transparent text-[13px]"
        />
      </div>
      {trailing}
      {suggestions.length > 0 && (
        <div className="absolute left-14 top-full z-30 mt-1 w-[360px] overflow-hidden rounded-lg border border-line bg-elev p-1 shadow-pop animate-in">
          {suggestions.map((c, i) => (
            <button
              key={c.email}
              onMouseDown={(e) => {
                e.preventDefault();
                commit({ name: c.name, email: c.email });
              }}
              onMouseEnter={() => setActive(i)}
              className={clsx("flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left", i === active && "bg-active")}
            >
              <Avatar address={c} size={22} />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[13px]">{c.name || c.email}</span>
                {c.name && <span className="block truncate text-[11.5px] text-fg-3">{c.email}</span>}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

// ------------------------------------------------------------------ AI assist

const COMPOSE_CHIPS = ["Accept", "Decline politely", "Ask for more details", "Say thanks", "Propose a time to meet"];
const REWRITE_CHIPS = ["Improve", "Shorten", "Make it friendlier", "More formal", "Fix spelling & grammar", "Translate to English", "Translate to German"];

function AiAssist({
  editor,
  draft,
  onClose,
}: {
  editor: Editor;
  draft: Draft;
  onClose: () => void;
}) {
  const ai = useStore((s) => s.ai);
  const set = useStore((s) => s.set);
  const [prompt, setPrompt] = useState("");
  const [output, setOutput] = useState("");
  const [status, setStatus] = useState<"idle" | "running" | "done" | "error">("idle");
  const [error, setError] = useState("");
  const cancel = useRef<(() => void) | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const { from, to, empty } = editor.state.selection;
  const selection = empty ? "" : editor.state.doc.textBetween(from, to, "\n\n");
  const range = useRef({ from, to, hasSelection: !empty });
  const hasText = editor.getText().trim().length > 0;
  const mode: "rewriteSelection" | "rewriteAll" | "compose" = selection
    ? "rewriteSelection"
    : hasText
      ? "rewriteAll"
      : "compose";

  useEffect(() => {
    inputRef.current?.focus();
    return () => cancel.current?.();
  }, []);

  const run = (instruction: string) => {
    if (!instruction.trim()) return;
    cancel.current?.();
    setOutput("");
    setError("");
    setStatus("running");
    let acc = "";
    const handlers = {
      onDelta: (d: string) => {
        acc += d;
        setOutput(acc);
      },
      onDone: (t: string) => {
        setOutput(t);
        setStatus("done");
      },
      onError: (m: string) => {
        setError(m);
        setStatus("error");
      },
    };
    const isRewriteChip = REWRITE_CHIPS.includes(instruction);
    if (mode === "rewriteSelection" || (mode === "rewriteAll" && isRewriteChip)) {
      cancel.current = aiStream(
        "ai_rewrite",
        { instruction, text: mode === "rewriteSelection" ? selection : editor.getText({ blockSeparator: "\n\n" }) },
        handlers,
      );
    } else {
      cancel.current = aiStream(
        "ai_compose",
        {
          req: {
            accountId: draft.accountId,
            instruction,
            draftText: editor.getText({ blockSeparator: "\n\n" }),
            threadId: draft.threadId ?? null,
            recipients: draft.to,
          },
        },
        handlers,
      );
    }
  };

  const accept = () => {
    if (!output) return;
    const html = textToHtml(output);
    if (mode === "rewriteSelection" && range.current.hasSelection) {
      editor.chain().focus().insertContentAt({ from: range.current.from, to: range.current.to }, html).run();
    } else {
      editor.chain().focus().setContent(html).run();
    }
    onClose();
  };

  if (!ai?.connected) {
    return (
      <div className="mx-4 mb-3 flex items-center gap-3 rounded-lg border border-line bg-hover px-3 py-2.5 text-[13px]">
        <Sparkles size={14} className="text-accent" />
        <span className="flex-1 text-fg-2">Connect your ChatGPT account to write with AI.</span>
        <Button size="sm" variant="primary" onClick={() => set({ settingsSection: "ai" })}>
          Connect
        </Button>
        <IconButton label="Close" onClick={onClose}>
          <X size={13} />
        </IconButton>
      </div>
    );
  }

  const chips = mode === "compose" ? (draft.threadId ? COMPOSE_CHIPS : []) : REWRITE_CHIPS;

  return (
    <div
      className="mx-4 mb-3 overflow-hidden rounded-lg border border-line-strong bg-elev-2 animate-in"
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          onClose();
        } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && status === "done") {
          e.preventDefault();
          e.stopPropagation();
          accept();
        }
      }}
    >
      <div className="flex items-center gap-2 px-3 py-2">
        <Wand2 size={14} className="shrink-0 text-accent" />
        <input
          ref={inputRef}
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.metaKey && !e.ctrlKey) {
              e.preventDefault();
              run(prompt);
            }
          }}
          placeholder={
            mode === "rewriteSelection"
              ? "How should the selection change?"
              : mode === "rewriteAll"
                ? "Improve, shorten, or tell AI what to add…"
                : draft.threadId
                  ? "What should the reply say?"
                  : "Describe the email you want to write…"
          }
          className="h-7 flex-1 bg-transparent text-[13px] placeholder:text-fg-4"
        />
        {status === "running" ? (
          <button onClick={() => { cancel.current?.(); setStatus("idle"); }} className="text-xs text-fg-3 hover:text-fg">
            Stop
          </button>
        ) : (
          <Kbd keys={["↵"]} />
        )}
      </div>
      {chips.length > 0 && status === "idle" && (
        <div className="flex flex-wrap gap-1.5 px-3 pb-2.5">
          {chips.map((c) => (
            <button
              key={c}
              onClick={() => {
                setPrompt(c);
                run(c);
              }}
              className="h-6 rounded-full border border-line-strong px-2.5 text-xs text-fg-2 hover:border-fg-4 hover:text-fg"
            >
              {c}
            </button>
          ))}
        </div>
      )}
      {(output || status === "running" || status === "error") && (
        <div className="border-t border-line px-3 py-2.5">
          {status === "error" ? (
            <div className="text-[13px] text-danger">{error}</div>
          ) : (
            <div className="prose-plain max-h-64 overflow-y-auto text-[13.5px] text-fg selectable">
              {output || <Spinner size={13} />}
            </div>
          )}
          {status === "done" && (
            <div className="mt-2.5 flex items-center gap-2">
              <Button size="sm" variant="primary" onClick={accept}>
                {mode === "rewriteSelection" ? "Replace selection" : "Use this"} <Kbd keys={["Mod", "↵"]} className="ml-1" />
              </Button>
              <Button size="sm" variant="ghost" onClick={() => run(prompt || "Try again with a different wording")}>
                Try again
              </Button>
              <div className="flex-1" />
              <Button size="sm" variant="ghost" onClick={onClose}>
                Discard
              </Button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// ------------------------------------------------------------------ composer

export function Composer() {
  const composer = useStore((s) => s.composer)!;
  const accounts = useStore((s) => s.accounts);
  const closeComposer = useStore((s) => s.closeComposer);
  const send = useStore((s) => s.send);
  const toast = useStore((s) => s.toast);
  const [draft, setDraft] = useState<Draft>(composer.draft);
  const [showCc, setShowCc] = useState(draft.cc.length > 0 || draft.bcc.length > 0);
  const [showQuote, setShowQuote] = useState(false);
  const [aiOpen, setAiOpen] = useState(false);
  const [sending, setSending] = useState(false);
  const dirty = useRef(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const account = accounts.find((a) => a.id === draft.accountId) ?? accounts[0];
  const isReply = draft.mode === "reply" || draft.mode === "replyAll";

  const initialHtml = useMemo(() => {
    if (draft.html) return draft.html;
    const sig = account?.signature?.trim();
    return sig ? `<p></p><p></p><p>-- <br>${sig.replace(/\n/g, "<br>")}</p>` : "";
    // Only computed once per composer instance.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const editor = useEditor({
    extensions: [
      StarterKit.configure({ link: { openOnClick: false, autolink: true } }),
      Placeholder.configure({ placeholder: isReply ? "Write a reply…  (⌘J to write with AI)" : "Write your message…  (⌘J to write with AI)" }),
    ],
    content: initialHtml,
    autofocus: isReply || draft.mode === "forward" ? "start" : false,
    onUpdate: () => {
      dirty.current = true;
    },
  });

  const update = (patch: Partial<Draft>) => {
    dirty.current = true;
    setDraft((d) => ({ ...d, ...patch }));
  };

  const currentDraft = useCallback((): Draft => {
    return { ...draft, html: editor?.getHTML() ?? draft.html, text: editor?.getText({ blockSeparator: "\n\n" }) ?? "" };
  }, [draft, editor]);

  // Autosave locally so nothing is lost.
  useEffect(() => {
    const t = window.setInterval(async () => {
      if (!dirty.current) return;
      dirty.current = false;
      const d = currentDraft();
      if (!d.text.trim() && !d.subject.trim() && !d.to.length) return;
      const id = await api.saveDraft(d).catch(() => null);
      if (id && !draft.id) setDraft((x) => ({ ...x, id }));
    }, 2000);
    return () => window.clearInterval(t);
  }, [currentDraft, draft.id]);

  const doSend = async () => {
    if (sending) return;
    const d = currentDraft();
    if (!d.to.length && !d.cc.length && !d.bcc.length) {
      toast("Add a recipient first", { kind: "error" });
      return;
    }
    if (!d.subject.trim() && !window.confirm("Send without a subject?")) return;
    setSending(true);
    const ok = await send(d);
    if (!ok) setSending(false);
  };

  const discard = async () => {
    if (draft.id) await api.deleteDraft(draft.id).catch(() => {});
    closeComposer();
  };

  const close = async () => {
    const d = currentDraft();
    if (d.text.trim() || d.attachments.length) {
      const id = await api.saveDraft(d).catch(() => null);
      if (id) toast("Draft saved", { duration: 2000 });
    }
    closeComposer();
  };

  const attach = async () => {
    const picked = await openDialog({ multiple: true, directory: false });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    update({ attachments: [...draft.attachments, ...paths.map((p) => ({ path: String(p) }))] });
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const mod = e.metaKey || e.ctrlKey;
    if (mod && e.key === "Enter" && !aiOpen) {
      e.preventDefault();
      doSend();
    } else if (mod && (e.key === "j" || e.key === "J")) {
      e.preventDefault();
      setAiOpen((o) => !o);
    } else if (mod && e.shiftKey && (e.key === "c" || e.key === "C" || e.key === "b" || e.key === "B")) {
      e.preventDefault();
      setShowCc(true);
    } else if (mod && e.shiftKey && (e.key === "a" || e.key === "A")) {
      e.preventDefault();
      attach();
    } else if (e.key === "Escape" && !aiOpen) {
      e.preventDefault();
      e.stopPropagation();
      close();
    }
  };

  const body = (
    <div
      ref={rootRef}
      data-composer
      onKeyDown={onKeyDown}
      className={clsx(
        "flex flex-col overflow-visible rounded-xl border border-line-strong bg-elev",
        composer.inline ? "shadow-sm" : "shadow-pop",
      )}
    >
      {!composer.inline && (
        <div className="flex h-10 items-center gap-2 border-b border-line px-4">
          <span className="flex-1 text-[13px] font-medium">
            {draft.mode === "forward" ? "Forward" : draft.mode === "new" || !draft.mode ? "New message" : "Reply"}
          </span>
          <IconButton label="Close and save draft" shortcut={["Esc"]} onClick={close}>
            <X size={14} />
          </IconButton>
        </div>
      )}

      {accounts.length > 1 && (
        <div className="flex h-9 items-center gap-2 border-b border-line px-4">
          <span className="w-10 shrink-0 text-[13px] text-fg-3">From</span>
          <select
            value={draft.accountId}
            onChange={(e) => update({ accountId: e.target.value })}
            className="h-7 flex-1 bg-transparent text-[13px]"
          >
            {accounts.map((a) => (
              <option key={a.id} value={a.id} className="bg-elev">
                {formatAddress({ name: a.name, email: a.email })}
                {a.label ? `  ·  ${a.label}` : ""}
              </option>
            ))}
          </select>
          <span className="h-2 w-2 rounded-full" style={{ background: account?.color }} />
        </div>
      )}

      <RecipientInput
        label="To"
        value={draft.to}
        onChange={(to) => update({ to })}
        autoFocus={!isReply && draft.mode !== "forward" ? true : draft.mode === "forward"}
        trailing={
          !showCc && (
            <button onClick={() => setShowCc(true)} className="pt-[5px] text-xs text-fg-3 hover:text-fg">
              Cc / Bcc
            </button>
          )
        }
      />
      {showCc && (
        <>
          <RecipientInput label="Cc" value={draft.cc} onChange={(cc) => update({ cc })} />
          <RecipientInput label="Bcc" value={draft.bcc} onChange={(bcc) => update({ bcc })} />
        </>
      )}
      {!isReply && (
        <div className="flex h-9 items-center gap-2 border-b border-line px-4">
          <span className="w-10 shrink-0 text-[13px] text-fg-3">Subject</span>
          <input
            value={draft.subject}
            onChange={(e) => update({ subject: e.target.value })}
            className="h-7 flex-1 bg-transparent text-[13px] font-medium"
          />
        </div>
      )}

      <div className="cm-editor px-4 pb-2 pt-3" onClick={() => editor?.commands.focus()}>
        <EditorContent editor={editor} />
      </div>

      {draft.quotedHtml && (
        <div className="px-4 pb-2">
          <button
            onClick={() => setShowQuote(!showQuote)}
            title={showQuote ? "Hide quoted text" : "Show quoted text"}
            className="flex h-5 items-center rounded border border-line-strong px-1.5 text-fg-3 hover:text-fg"
          >
            <MoreHorizontal size={13} />
          </button>
          {showQuote && (
            <div
              className="mt-2 max-h-64 overflow-y-auto rounded-md border border-line p-3 text-xs text-fg-3 [&_img]:max-w-full"
              dangerouslySetInnerHTML={{ __html: stripScripts(draft.quotedHtml) }}
            />
          )}
        </div>
      )}

      {draft.attachments.length > 0 && (
        <div className="flex flex-wrap gap-1.5 px-4 pb-2">
          {draft.attachments.map((a, i) => (
            <span key={a.path} className="flex h-7 items-center gap-1.5 rounded-md border border-line-strong bg-elev-2 pl-2 pr-1 text-xs">
              <Paperclip size={12} className="text-fg-3" />
              {a.path.split(/[\\/]/).pop()}
              <button onClick={() => update({ attachments: draft.attachments.filter((_, j) => j !== i) })} className="p-0.5 text-fg-3 hover:text-fg">
                <X size={11} />
              </button>
            </span>
          ))}
        </div>
      )}

      {aiOpen && editor && <AiAssist editor={editor} draft={draft} onClose={() => { setAiOpen(false); editor.commands.focus(); }} />}

      <div className="flex items-center gap-1.5 border-t border-line px-3 py-2">
        <Button variant="primary" onClick={doSend} disabled={sending}>
          {sending ? <Spinner size={12} className="text-white" /> : <CornerDownLeft size={13} />}
          Send
          <span className="ml-1 text-[11px] opacity-70">{navigator.userAgent.includes("Mac") ? "⌘↵" : "Ctrl ↵"}</span>
        </Button>
        <Button variant="ghost" onClick={() => setAiOpen((o) => !o)} className={aiOpen ? "bg-active text-fg" : ""}>
          <Sparkles size={13} className="text-accent" />
          Write with AI
          <Kbd keys={["Mod", "J"]} className="ml-0.5" />
        </Button>
        <div className="flex-1" />
        <IconButton label="Attach files" shortcut={["Mod", "Shift", "A"]} onClick={attach}>
          <Paperclip size={14} />
        </IconButton>
        <IconButton label="Discard draft" onClick={discard}>
          <Trash2 size={14} />
        </IconButton>
      </div>
    </div>
  );

  if (composer.inline) return body;
  return (
    <div className="fixed inset-0 z-40 flex items-end justify-center bg-black/30 p-6 animate-fade" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="w-full max-w-[760px] animate-slide-up">{body}</div>
    </div>
  );
}

function stripScripts(html: string): string {
  const clean = DOMPurify.sanitize(html, { FORBID_TAGS: ["style", "script", "iframe", "form", "link", "meta"] });
  // Quoted mail is shown inside the app document, so keep remote content out.
  return clean.replace(/\ssrc="https?:[^"]*"/gi, "");
}
