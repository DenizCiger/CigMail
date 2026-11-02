import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { api, errorMessage } from "./lib/api";
import type {
  Account,
  AiStatus,
  Draft,
  Split,
  SyncStatus,
  ThreadAction,
  ThreadFilter,
  ThreadSummary,
  View,
} from "./lib/types";

export interface Toast {
  id: number;
  message: string;
  kind: "info" | "success" | "error";
  action?: { label: string; run: () => void };
  /** ms; 0 = sticky */
  duration: number;
}

export interface ComposerState {
  key: string;
  draft: Draft;
  /** Inline reply below the open thread vs. floating modal. */
  inline: boolean;
}

interface UndoEntry {
  label: string;
  run: () => Promise<void> | void;
}

interface State {
  ready: boolean;
  accounts: Account[];
  splits: Split[];
  settings: Record<string, string>;
  ai: AiStatus | null;
  oauth: { google: boolean; microsoft: boolean };

  view: View;
  splitId: string | null;
  accountFilter: string | null;
  query: string;
  searchOpen: boolean;
  sidebarCollapsed: boolean;

  threads: ThreadSummary[];
  loading: boolean;
  hasMore: boolean;
  cursor: number;
  selected: Set<string>;
  openThreadId: string | null;

  composer: ComposerState | null;
  /** Reply requested before the thread finished loading. */
  pendingReply: "reply" | "replyAll" | "forward" | null;
  palette: boolean;
  snoozeFor: string[] | null;
  settingsSection: string | null;
  help: boolean;

  toasts: Toast[];
  sync: Record<string, SyncStatus>;
  counts: Record<string, number>;
  undoStack: UndoEntry[];

  init: () => Promise<void>;
  reloadMeta: () => Promise<void>;
  refresh: (opts?: { more?: boolean }) => Promise<void>;
  refreshCounts: () => Promise<void>;
  setView: (v: View) => void;
  setSplit: (id: string | null) => void;
  cycleSplit: (dir: 1 | -1) => void;
  setAccountFilter: (id: string | null) => void;
  setQuery: (q: string) => void;
  openSearch: () => void;
  closeSearch: () => void;

  move: (delta: number) => void;
  setCursor: (i: number) => void;
  toggleSelect: (id?: string) => void;
  selectAll: () => void;
  clearSelection: () => void;
  openThread: (id?: string) => void;
  closeThread: () => void;

  targets: () => string[];
  act: (action: ThreadAction, ids?: string[], opts?: { silent?: boolean }) => Promise<void>;
  toggleStar: () => void;
  toggleRead: () => void;
  snoozeThreads: (ids: string[], until: number | null) => Promise<void>;

  compose: (draft?: Partial<Draft>, inline?: boolean) => void;
  reply: (mode: "reply" | "replyAll" | "forward", messageId: string, inline?: boolean) => Promise<void>;
  closeComposer: () => void;
  send: (draft: Draft) => Promise<boolean>;

  set: (patch: Partial<State>) => void;
  toast: (message: string, opts?: Partial<Omit<Toast, "id" | "message">>) => number;
  dismissToast: (id: number) => void;
  pushUndo: (label: string, run: () => Promise<void> | void) => void;
  undo: () => Promise<void>;
  setSetting: (key: string, value: string) => Promise<void>;
}

let toastId = 1;
let refreshTimer: number | undefined;
let refreshSeq = 0;
const PAGE = 300;

/** Whether an action makes a thread disappear from the current view. */
function removesFrom(view: View, action: ThreadAction): boolean {
  switch (action) {
    case "archive":
      return view === "inbox";
    case "trash":
      return view !== "trash";
    case "inbox":
      return ["done", "trash", "spam", "snoozed"].includes(view);
    case "unstar":
      return view === "starred";
    default:
      return false;
  }
}

const inverse: Partial<Record<ThreadAction, ThreadAction>> = {
  archive: "inbox",
  trash: "inbox",
  star: "unstar",
  unstar: "star",
  read: "unread",
  unread: "read",
};

const actionLabel: Record<ThreadAction, string> = {
  archive: "Marked as done",
  inbox: "Moved to inbox",
  trash: "Moved to trash",
  read: "Marked as read",
  unread: "Marked as unread",
  star: "Starred",
  unstar: "Unstarred",
};

export const useStore = create<State>((set, get) => ({
  ready: false,
  accounts: [],
  splits: [],
  settings: {},
  ai: null,
  oauth: { google: false, microsoft: false },

  view: "inbox",
  splitId: null,
  accountFilter: null,
  query: "",
  searchOpen: false,
  sidebarCollapsed: false,

  threads: [],
  loading: true,
  hasMore: false,
  cursor: 0,
  selected: new Set(),
  openThreadId: null,

  composer: null,
  pendingReply: null,
  palette: false,
  snoozeFor: null,
  settingsSection: null,
  help: false,

  toasts: [],
  sync: {},
  counts: {},
  undoStack: [],

  set: (patch) => set(patch),

  init: async () => {
    await get().reloadMeta();
    const { splits } = get();
    set({ splitId: splits[0]?.id ?? null, ready: true });
    await get().refresh();
    get().refreshCounts();

    const scheduleRefresh = () => {
      window.clearTimeout(refreshTimer);
      refreshTimer = window.setTimeout(() => {
        get().refresh();
        get().refreshCounts();
      }, 120);
    };
    listen<string>("mail:changed", scheduleRefresh);
    listen<SyncStatus>("sync:status", (e) => {
      const s = e.payload;
      set((st) => ({ sync: { ...st.sync, [s.accountId]: s } }));
      if (s.state === "idle" || s.state === "error" || s.state === "auth") {
        api.listAccounts().then((accounts) => set({ accounts }));
      }
    });
    listen<string>("outbox:sent", () => get().toast("Message sent", { kind: "success", duration: 2500 }));
    listen<{ id: string; error: string }>("outbox:failed", async (e) => {
      const failed = await api.failedSends();
      const f = failed.find((x) => x.id === e.payload.id);
      get().toast(`Sending failed: ${e.payload.error}`, {
        kind: "error",
        duration: 0,
        action: f
          ? {
              label: "Edit",
              run: async () => {
                const d = await api.cancelSend(f.id);
                if (d) get().compose(d);
              },
            }
          : undefined,
      });
    });
    // Snoozes can wake up while the app sits idle.
    window.setInterval(scheduleRefresh, 60_000);
  },

  reloadMeta: async () => {
    const [accounts, splits, settings, ai, oauth] = await Promise.all([
      api.listAccounts(),
      api.getSplits(),
      api.getSettings(),
      api.aiStatus().catch(() => null),
      api.oauthConfigStatus(),
    ]);
    set({ accounts, splits, settings, ai, oauth });
  },

  refresh: async (opts) => {
    const { view, splitId, splits, accountFilter, query, threads } = get();
    const seq = ++refreshSeq;
    const parts: string[] = [];
    if (view === "inbox" && splitId) {
      const s = splits.find((x) => x.id === splitId);
      if (s) parts.push(s.query);
    }
    if (view === "search" && query.trim()) parts.push(query.trim());
    const offset = opts?.more ? threads.length : 0;
    const limit = opts?.more ? PAGE : Math.max(PAGE, threads.length);
    try {
      const list = await api.listThreads({
        view,
        accountId: accountFilter,
        query: parts.join(" ") || null,
        limit,
        offset,
      });
      if (seq !== refreshSeq) return;
      const next = opts?.more ? [...threads, ...list] : list;
      set((st) => {
        // Keep the cursor on the same thread when the list changes under it.
        const currentId = st.threads[st.cursor]?.id;
        let cursor = currentId ? next.findIndex((t) => t.id === currentId) : -1;
        if (cursor < 0) cursor = Math.min(st.cursor, Math.max(0, next.length - 1));
        const ids = new Set(next.map((t) => t.id));
        const selected = new Set([...st.selected].filter((id) => ids.has(id)));
        return { threads: next, loading: false, cursor, selected, hasMore: list.length >= limit };
      });
    } catch (e) {
      set({ loading: false });
      get().toast(errorMessage(e), { kind: "error" });
    }
  },

  refreshCounts: async () => {
    const { accounts, splits, accountFilter } = get();
    const filters: Record<string, ThreadFilter> = { inbox: { view: "inbox" } };
    for (const a of accounts) filters[`inbox:${a.id}`] = { view: "inbox", accountId: a.id };
    for (const s of splits) filters[`split:${s.id}`] = { view: "inbox", accountId: accountFilter, query: s.query };
    filters["filtered"] = { view: "inbox", accountId: accountFilter };
    try {
      set({ counts: await api.countUnread(filters) });
    } catch {
      /* counts are cosmetic */
    }
  },

  setView: (view) => {
    set({ view, cursor: 0, selected: new Set(), openThreadId: null, loading: true, threads: [] });
    if (view !== "search") set({ searchOpen: false });
    get().refresh();
  },
  setSplit: (splitId) => {
    set({ splitId, cursor: 0, selected: new Set(), view: "inbox", openThreadId: null, threads: [], loading: true });
    get().refresh();
  },
  cycleSplit: (dir) => {
    const { splits, splitId } = get();
    const ids: (string | null)[] = [...splits.map((s) => s.id), null];
    const i = ids.indexOf(splitId);
    get().setSplit(ids[(i + dir + ids.length) % ids.length]);
  },
  setAccountFilter: (accountFilter) => {
    set({ accountFilter, cursor: 0, selected: new Set(), openThreadId: null, threads: [], loading: true });
    get().refresh();
    get().refreshCounts();
  },
  setQuery: (query) => {
    set({ query, view: "search", cursor: 0, openThreadId: null });
    get().refresh();
  },
  openSearch: () => set({ searchOpen: true }),
  closeSearch: () => {
    const wasSearch = get().view === "search";
    set({ searchOpen: false, query: "" });
    if (wasSearch) get().setView("inbox");
  },

  move: (delta) => {
    const { threads, cursor, openThreadId } = get();
    if (!threads.length) return;
    const next = Math.max(0, Math.min(threads.length - 1, cursor + delta));
    set({ cursor: next });
    if (openThreadId) get().openThread(threads[next].id);
    if (next >= threads.length - 20 && get().hasMore && !openThreadId) get().refresh({ more: true });
  },
  setCursor: (cursor) => set({ cursor }),
  toggleSelect: (id) => {
    const { threads, cursor, selected } = get();
    const target = id ?? threads[cursor]?.id;
    if (!target) return;
    const next = new Set(selected);
    if (next.has(target)) next.delete(target);
    else next.add(target);
    set({ selected: next });
  },
  selectAll: () => set((s) => ({ selected: new Set(s.threads.map((t) => t.id)) })),
  clearSelection: () => set({ selected: new Set() }),
  openThread: (id) => {
    const { threads, cursor } = get();
    const target = id ?? threads[cursor]?.id;
    if (!target) return;
    const idx = threads.findIndex((t) => t.id === target);
    set({ openThreadId: target, cursor: idx >= 0 ? idx : cursor, selected: new Set() });
    const t = threads.find((x) => x.id === target);
    if (t?.unread && get().settings.mark_read_on_open !== "false") {
      get().act("read", [target], { silent: true });
    }
  },
  closeThread: () => {
    const c = get().composer;
    set({ openThreadId: null, composer: c && c.inline ? null : c });
  },

  targets: () => {
    const { selected, openThreadId, threads, cursor } = get();
    if (selected.size) return [...selected];
    if (openThreadId) return [openThreadId];
    const t = threads[cursor];
    return t ? [t.id] : [];
  },

  act: async (action, ids, opts) => {
    const targets = ids ?? get().targets();
    if (!targets.length) return;
    const { view, threads, cursor, openThreadId } = get();
    const removes = removesFrom(view, action);

    // Optimistic UI: update or drop rows immediately.
    if (removes) {
      const remaining = threads.filter((t) => !targets.includes(t.id));
      const firstIdx = threads.findIndex((t) => targets.includes(t.id));
      const newCursor = Math.min(firstIdx >= 0 ? firstIdx : cursor, Math.max(0, remaining.length - 1));
      const patch: Partial<State> = { threads: remaining, cursor: newCursor, selected: new Set() };
      if (openThreadId && targets.includes(openThreadId)) {
        // Superhuman-style: go straight to the next conversation.
        const next = remaining[newCursor];
        patch.openThreadId = next && get().settings.after_action !== "list" ? next.id : null;
        patch.composer = get().composer?.inline ? null : get().composer;
      }
      set(patch);
      if (patch.openThreadId) {
        const t = remaining[newCursor];
        if (t?.unread && get().settings.mark_read_on_open !== "false") {
          api.threadAction([t.id], "read").catch(() => {});
        }
      }
    } else {
      set({
        threads: threads.map((t) =>
          targets.includes(t.id)
            ? {
                ...t,
                unread: action === "unread" ? true : action === "read" ? false : t.unread,
                starred: action === "star" ? true : action === "unstar" ? false : t.starred,
              }
            : t,
        ),
      });
    }

    try {
      await api.threadAction(targets, action);
    } catch (e) {
      get().toast(errorMessage(e), { kind: "error" });
      get().refresh();
      return;
    }
    if (opts?.silent) return;
    const inv = inverse[action];
    const n = targets.length;
    const label = `${actionLabel[action]}${n > 1 ? ` · ${n} conversations` : ""}`;
    if (inv) {
      get().pushUndo(label, () => api.threadAction(targets, inv));
    }
    get().toast(label, {
      action: inv ? { label: "Undo", run: () => get().undo() } : undefined,
    });
  },

  toggleStar: () => {
    const ids = get().targets();
    const t = get().threads.find((x) => x.id === ids[0]);
    get().act(t?.starred ? "unstar" : "star", ids);
  },
  toggleRead: () => {
    const ids = get().targets();
    const t = get().threads.find((x) => x.id === ids[0]);
    const open = get().openThreadId;
    get().act(t?.unread ? "read" : "unread", ids);
    // Marking an open thread unread means "deal with it later": go back.
    if (open && t && !t.unread) set({ openThreadId: null });
  },

  snoozeThreads: async (ids, until) => {
    const { threads, view, cursor, openThreadId } = get();
    if (until && view !== "snoozed") {
      const remaining = threads.filter((t) => !ids.includes(t.id));
      const newCursor = Math.min(cursor, Math.max(0, remaining.length - 1));
      set({
        threads: remaining,
        cursor: newCursor,
        selected: new Set(),
        openThreadId: openThreadId && ids.includes(openThreadId) ? remaining[newCursor]?.id ?? null : openThreadId,
      });
    }
    try {
      await api.snooze(ids, until);
    } catch (e) {
      get().toast(errorMessage(e), { kind: "error" });
      return;
    }
    if (until) {
      get().pushUndo("Snoozed", () => api.snooze(ids, null));
      get().toast(`Snoozed until ${new Date(until * 1000).toLocaleString(undefined, { weekday: "short", hour: "2-digit", minute: "2-digit", month: "short", day: "numeric" })}`, {
        action: { label: "Undo", run: () => get().undo() },
      });
    } else {
      get().toast("Unsnoozed");
    }
  },

  compose: (draft, inline = false) => {
    const { accounts, accountFilter, settings } = get();
    if (!accounts.length) {
      get().toast("Add an account first", { kind: "error" });
      return;
    }
    const accountId =
      draft?.accountId ?? accountFilter ?? settings.default_account ?? accounts[0].id;
    set({
      composer: {
        key: crypto.randomUUID(),
        inline,
        draft: {
          accountId: accounts.some((a) => a.id === accountId) ? accountId : accounts[0].id,
          to: [],
          cc: [],
          bcc: [],
          subject: "",
          html: "",
          text: "",
          references: [],
          attachments: [],
          mode: "new",
          ...draft,
        },
      },
    });
  },

  reply: async (mode, messageId, inline = true) => {
    try {
      const draft = await api.replyDraft(messageId, mode);
      get().compose(draft, inline && mode !== "forward" ? true : inline);
    } catch (e) {
      get().toast(errorMessage(e), { kind: "error" });
    }
  },

  closeComposer: () => set({ composer: null }),

  send: async (draft) => {
    try {
      const res = await api.sendMessage(draft);
      set({ composer: null });
      const secs = Math.max(0, res.sendAt - Math.floor(Date.now() / 1000));
      const cancel = async () => {
        const d = await api.cancelSend(res.id);
        if (d) {
          get().compose(d, false);
          get().toast("Sending cancelled");
        } else {
          get().toast("Too late, already sent", { kind: "error" });
        }
      };
      if (secs > 0) {
        get().pushUndo("Send", cancel);
        get().toast("Sending…", { duration: secs * 1000, action: { label: "Undo", run: cancel } });
      }
      return true;
    } catch (e) {
      get().toast(errorMessage(e), { kind: "error" });
      return false;
    }
  },

  toast: (message, opts) => {
    const id = toastId++;
    const t: Toast = { id, message, kind: opts?.kind ?? "info", action: opts?.action, duration: opts?.duration ?? 4000 };
    set((s) => ({ toasts: [...s.toasts.slice(-3), t] }));
    if (t.duration > 0) window.setTimeout(() => get().dismissToast(id), t.duration);
    return id;
  },
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),

  pushUndo: (label, run) => set((s) => ({ undoStack: [...s.undoStack.slice(-19), { label, run }] })),
  undo: async () => {
    const stack = get().undoStack;
    const last = stack[stack.length - 1];
    if (!last) {
      get().toast("Nothing to undo");
      return;
    }
    set({ undoStack: stack.slice(0, -1), toasts: [] });
    try {
      await last.run();
      get().refresh();
      if (last.label !== "Send") get().toast(`Undid: ${last.label.toLowerCase()}`);
    } catch (e) {
      get().toast(errorMessage(e), { kind: "error" });
    }
  },

  setSetting: async (key, value) => {
    await api.setSetting(key, value);
    set((s) => ({ settings: { ...s.settings, [key]: value } }));
  },
}));

/** Account lookup helper usable outside React. */
export function accountById(id: string | null | undefined): Account | undefined {
  return useStore.getState().accounts.find((a) => a.id === id);
}

export function myEmails(): string[] {
  return useStore.getState().accounts.map((a) => a.email);
}
