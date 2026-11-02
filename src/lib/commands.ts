import {
  Archive,
  ArrowLeft,
  Clock,
  Command as CommandIcon,
  Forward,
  Inbox,
  Keyboard,
  Mail,
  MailOpen,
  Moon,
  PanelLeft,
  PenSquare,
  RefreshCw,
  Reply,
  ReplyAll,
  Search,
  Send,
  Settings,
  Sparkles,
  Star,
  Trash2,
  Undo2,
  CheckCheck,
  FileText,
  AlarmClock,
  ShieldAlert,
  type LucideIcon,
} from "lucide-react";
import { api } from "./api";
import { useStore } from "../store";
import type { View } from "./types";

export interface Command {
  id: string;
  title: string;
  section: "Conversation" | "Navigation" | "Compose" | "Accounts" | "App";
  keys?: string[];
  icon?: LucideIcon;
  run: () => void;
  enabled?: () => boolean;
  /** Reachable by hotkey only; not listed in the palette or help. */
  hidden?: boolean;
}

const s = () => useStore.getState();
const hasTarget = () => s().targets().length > 0;

/** The thread rows an action would apply to. */
function targetThreads() {
  const st = s();
  const ids = st.targets();
  return st.threads.filter((t) => ids.includes(t.id));
}
const inTrashOrSpam = () => s().view === "trash" || s().view === "spam";
/** "Mark done" only makes sense for threads that are still in the inbox. */
const canDone = () => !inTrashOrSpam() && targetThreads().some((t) => t.inInbox);
const canRestore = () => hasTarget() && (inTrashOrSpam() || targetThreads().some((t) => !t.inInbox));

/** E toggles: done when in the inbox, back to the inbox otherwise (like Superhuman). */
function toggleDone() {
  const st = s();
  if (canDone()) st.act("archive");
  else if (canRestore()) st.act("inbox");
}

function restoreTitle() {
  const v = s().view;
  return v === "trash" ? "Restore from trash" : v === "spam" ? "Not spam" : "Move to inbox";
}

function go(view: View) {
  return () => s().setView(view);
}

/** Every command in the app. Hotkeys and the command palette both read this. */
export function allCommands(): Command[] {
  const st = s();
  const cmds: Command[] = [
    // Conversation
    { id: "done", title: "Mark done", section: "Conversation", keys: ["E"], icon: Archive, run: () => st.act("archive"), enabled: canDone },
    { id: "inbox", title: restoreTitle(), section: "Conversation", keys: canDone() ? ["Shift", "E"] : ["E"], icon: Inbox, run: () => st.act("inbox"), enabled: canRestore },
    { id: "toggleDone", title: "Mark done / move to inbox", section: "Conversation", run: toggleDone, hidden: true },
    { id: "trash", title: "Move to trash", section: "Conversation", keys: ["#"], icon: Trash2, run: () => st.act("trash"), enabled: () => hasTarget() && s().view !== "trash" },
    { id: "spam", title: "Mark as spam", section: "Conversation", keys: ["!"], icon: ShieldAlert, run: () => st.act("spam"), enabled: () => hasTarget() && s().view !== "spam" },
    { id: "unsubscribe", title: "Unsubscribe", section: "Conversation", keys: ["Mod", "U"], icon: MailOpen, run: () => window.dispatchEvent(new CustomEvent("cm:unsubscribe")), enabled: () => !!s().openThreadId },
    { id: "expandAll", title: "Expand / collapse all messages", section: "Conversation", keys: [";"], icon: Mail, run: () => window.dispatchEvent(new CustomEvent("cm:expand-all")), enabled: () => !!s().openThreadId },
    { id: "star", title: "Star / unstar", section: "Conversation", keys: ["S"], icon: Star, run: () => st.toggleStar(), enabled: hasTarget },
    { id: "read", title: "Mark read / unread", section: "Conversation", keys: ["U"], icon: MailOpen, run: () => st.toggleRead(), enabled: hasTarget },
    { id: "snooze", title: "Snooze…", section: "Conversation", keys: ["H"], icon: Clock, run: () => st.set({ snoozeFor: st.targets() }), enabled: () => hasTarget() && !inTrashOrSpam() },
    { id: "reply", title: "Reply", section: "Conversation", keys: ["R"], icon: Reply, run: () => replyFromTarget("reply"), enabled: hasTarget },
    { id: "replyAll", title: "Reply all", section: "Conversation", keys: ["A"], icon: ReplyAll, run: () => replyFromTarget("replyAll"), enabled: hasTarget },
    { id: "forward", title: "Forward", section: "Conversation", keys: ["F"], icon: Forward, run: () => replyFromTarget("forward"), enabled: hasTarget },
    { id: "select", title: "Select conversation", section: "Conversation", keys: ["X"], icon: CheckCheck, run: () => st.toggleSelect(), enabled: () => !s().openThreadId },
    { id: "selectAll", title: "Select all", section: "Conversation", keys: ["Mod", "A"], icon: CheckCheck, run: () => st.selectAll(), enabled: () => !s().openThreadId },
    { id: "summarize", title: "Summarize with AI", section: "Conversation", keys: ["Shift", "S"], icon: Sparkles, run: () => window.dispatchEvent(new CustomEvent("cm:summarize")), enabled: () => !!s().openThreadId },
    { id: "undo", title: "Undo", section: "Conversation", keys: ["Z"], icon: Undo2, run: () => st.undo() },

    // Navigation
    { id: "goInbox", title: "Go to Inbox", section: "Navigation", keys: ["G", "I"], icon: Inbox, run: go("inbox") },
    { id: "goStarred", title: "Go to Starred", section: "Navigation", keys: ["G", "S"], icon: Star, run: go("starred") },
    { id: "goSnoozed", title: "Go to Snoozed", section: "Navigation", keys: ["G", "H"], icon: AlarmClock, run: go("snoozed") },
    { id: "goDone", title: "Go to Done", section: "Navigation", keys: ["G", "E"], icon: Archive, run: go("done") },
    { id: "goSent", title: "Go to Sent", section: "Navigation", keys: ["G", "T"], icon: Send, run: go("sent") },
    { id: "goDrafts", title: "Go to Drafts", section: "Navigation", keys: ["G", "D"], icon: FileText, run: go("drafts") },
    { id: "goTrash", title: "Go to Trash", section: "Navigation", keys: ["G", "X"], icon: Trash2, run: go("trash") },
    { id: "goSpam", title: "Go to Spam", section: "Navigation", keys: ["G", "!"], icon: ShieldAlert, run: go("spam") },
    { id: "search", title: "Search mail", section: "Navigation", keys: ["/"], icon: Search, run: () => st.openSearch() },
    { id: "nextSplit", title: "Next split", section: "Navigation", keys: ["Tab"], icon: Mail, run: () => st.cycleSplit(1) },
    { id: "prevSplit", title: "Previous split", section: "Navigation", keys: ["Shift", "Tab"], icon: Mail, run: () => st.cycleSplit(-1) },
    { id: "back", title: "Back to list", section: "Navigation", keys: ["Esc"], icon: ArrowLeft, run: () => st.closeThread(), enabled: () => !!s().openThreadId },

    // Compose
    { id: "compose", title: "New message", section: "Compose", keys: ["C"], icon: PenSquare, run: () => st.compose() },

    // App
    { id: "palette", title: "Command palette", section: "App", keys: ["Mod", "K"], icon: CommandIcon, run: () => st.set({ palette: true }) },
    { id: "sync", title: "Sync all accounts", section: "App", icon: RefreshCw, run: () => { api.syncNow(); st.toast("Syncing…"); } },
    { id: "settings", title: "Settings", section: "App", keys: ["Mod", ","], icon: Settings, run: () => st.set({ settingsSection: "general" }) },
    { id: "accounts", title: "Manage accounts", section: "App", icon: Settings, run: () => st.set({ settingsSection: "accounts" }) },
    { id: "aiSettings", title: "AI settings", section: "App", icon: Sparkles, run: () => st.set({ settingsSection: "ai" }) },
    { id: "theme", title: "Toggle light / dark", section: "App", icon: Moon, run: toggleTheme },
    { id: "sidebar", title: "Toggle sidebar", section: "App", keys: ["["], icon: PanelLeft, run: () => st.set({ sidebarCollapsed: !s().sidebarCollapsed }) },
    { id: "help", title: "Keyboard shortcuts", section: "App", keys: ["?"], icon: Keyboard, run: () => st.set({ help: true }) },
  ];

  cmds.push({
    id: "account:all",
    title: "All accounts (unified inbox)",
    section: "Accounts",
    keys: ["G", "0"],
    icon: Mail,
    run: () => st.setAccountFilter(null),
    enabled: () => !!s().accountFilter,
  });
  st.accounts.forEach((a, i) => {
    cmds.push({
      id: `account:${i + 1}`,
      title: `Show only ${a.label ? `${a.label} (${a.email})` : a.email}`,
      section: "Accounts",
      keys: i < 9 ? ["G", String(i + 1)] : undefined,
      icon: Mail,
      run: () => st.setAccountFilter(a.id),
    });
  });
  if (st.accounts.length > 1) {
    cmds.push({ id: "nextAccount", title: "Next account", section: "Accounts", keys: ["]"], icon: Mail, run: () => cycleAccount(1) });
    cmds.push({ id: "prevAccount", title: "Previous account", section: "Accounts", keys: ["Shift", "]"], icon: Mail, run: () => cycleAccount(-1) });
  }
  st.splits.forEach((sp, i) => {
    if (i < 9) {
      cmds.push({ id: `split:${i + 1}`, title: `Show split: ${sp.name}`, section: "Navigation", keys: [String(i + 1)], icon: Inbox, run: () => st.setSplit(sp.id) });
    }
  });
  return cmds;
}

/** Cycles All accounts → account 1 → account 2 … */
function cycleAccount(dir: 1 | -1) {
  const st = s();
  const ids: (string | null)[] = [null, ...st.accounts.map((a) => a.id)];
  const i = ids.indexOf(st.accountFilter);
  st.setAccountFilter(ids[(i + dir + ids.length) % ids.length]);
}

export function toggleTheme() {
  const st = s();
  const current = st.settings.theme ?? "system";
  const effective =
    current === "system" ? (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light") : current;
  st.setSetting("theme", effective === "dark" ? "light" : "dark");
}

async function replyFromTarget(mode: "reply" | "replyAll" | "forward") {
  const st = s();
  const id = st.targets()[0];
  if (!id) return;
  // The thread view knows which message is the latest; let it start the reply.
  if (st.openThreadId === id) {
    window.dispatchEvent(new CustomEvent("cm:reply", { detail: mode }));
  } else {
    st.set({ pendingReply: mode });
    st.openThread(id);
  }
}

// ------------------------------------------------------------------ hotkeys

const bindings: Record<string, string> = {
  e: "toggleDone",
  "Shift+e": "inbox",
  "#": "trash",
  Delete: "trash",
  "Mod+Backspace": "trash",
  "!": "spam",
  "Mod+u": "unsubscribe",
  ";": "expandAll",
  "]": "nextAccount",
  "}": "prevAccount",
  "Shift+]": "prevAccount",
  "g 0": "account:all",
  "g !": "goSpam",
  "g x": "goTrash",
  s: "star",
  u: "read",
  h: "snooze",
  r: "reply",
  a: "replyAll",
  f: "forward",
  x: "select",
  "Mod+a": "selectAll",
  "Shift+s": "summarize",
  z: "undo",
  "Mod+z": "undo",
  c: "compose",
  "/": "search",
  "Mod+k": "palette",
  "Mod+,": "settings",
  "[": "sidebar",
  "?": "help",
  Tab: "nextSplit",
  "Shift+Tab": "prevSplit",
  "g i": "goInbox",
  "g s": "goStarred",
  "g h": "goSnoozed",
  "g e": "goDone",
  "g t": "goSent",
  "g d": "goDrafts",
  "g #": "goTrash",
};
for (let i = 1; i <= 9; i++) {
  bindings[String(i)] = `split:${i}`;
  bindings[`g ${i}`] = `account:${i}`;
}

const MODIFIERS = new Set(["Shift", "Control", "Alt", "AltGraph", "Meta", "OS", "CapsLock", "Hyper", "Super"]);

function normalize(e: KeyboardEvent): string {
  const parts: string[] = [];
  if (e.ctrlKey || e.metaKey) parts.push("Mod");
  if (e.altKey) parts.push("Alt");
  // WebKitGTK reports Shift+Tab as "ISO_Left_Tab"/"Unidentified" on some setups.
  let key = e.code === "Tab" ? "Tab" : e.key;
  // Symbols already encode Shift ("#", "?"); letters don't.
  const isLetter = key.length === 1 && /[a-z]/i.test(key);
  if (e.shiftKey && (isLetter || key.length > 1)) parts.push("Shift");
  if (isLetter) key = key.toLowerCase();
  parts.push(key);
  return parts.join("+");
}

function isEditable(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false;
  return t.isContentEditable || t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT";
}

let pending: string | null = null;
let pendingTimer: number | undefined;

export function handleGlobalKey(e: KeyboardEvent) {
  const st = s();
  const overlayOpen = st.palette || st.snoozeFor || st.settingsSection || st.help;
  if (e.defaultPrevented) return;
  // A bare Shift press must not end a "g" sequence (e.g. "g" then Shift+3 for "#").
  if (MODIFIERS.has(e.key)) return;

  const combo = normalize(e);

  if (combo === "Mod+k") {
    e.preventDefault();
    st.set({ palette: !st.palette });
    return;
  }
  if (overlayOpen) return;
  if (isEditable(e.target)) return;
  // Leave the floating composer alone unless it's minimized.
  if (st.composer && !st.composer.inline && document.activeElement?.closest("[data-composer]")) return;

  if (combo === "Escape") {
    if (st.selected.size) st.clearSelection();
    else if (st.openThreadId) st.closeThread();
    else if (st.searchOpen || st.view === "search") st.closeSearch();
    return;
  }

  // List / thread navigation
  if (combo === "j" || combo === "ArrowDown") {
    e.preventDefault();
    st.move(1);
    return;
  }
  if (combo === "k" || combo === "ArrowUp") {
    e.preventDefault();
    st.move(-1);
    return;
  }
  if ((combo === "Enter" || combo === "o") && !st.openThreadId) {
    e.preventDefault();
    st.openThread();
    return;
  }

  const key = pending ? `${pending} ${combo}` : combo;
  if (pending) {
    pending = null;
    window.clearTimeout(pendingTimer);
  } else if (combo === "g") {
    pending = "g";
    pendingTimer = window.setTimeout(() => (pending = null), 900);
    return;
  }

  const id = bindings[key];
  if (!id) return;
  const cmd = allCommands().find((c) => c.id === id);
  if (!cmd || (cmd.enabled && !cmd.enabled())) return;
  e.preventDefault();
  cmd.run();
}
