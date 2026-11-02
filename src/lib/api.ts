import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  Account,
  AccountKind,
  AiEvent,
  AiStatus,
  Contact,
  Draft,
  ImapConfig,
  ModelInfo,
  Split,
  ThreadAction,
  ThreadDetail,
  ThreadFilter,
  ThreadSummary,
} from "./types";

export const api = {
  listAccounts: () => invoke<Account[]>("list_accounts"),
  addOAuthAccount: (kind: AccountKind, label?: string) =>
    invoke<Account>("add_oauth_account", { req: { kind, label } }),
  addImapAccount: (req: { email: string; name: string; label?: string; config: ImapConfig; password?: string }) =>
    invoke<Account>("add_imap_account", { req }),
  detectServers: (email: string) =>
    invoke<Pick<ImapConfig, "imapHost" | "imapPort" | "imapSecurity" | "smtpHost" | "smtpPort" | "smtpSecurity"> | null>(
      "detect_servers",
      { email },
    ),
  testImap: (config: ImapConfig, password: string) => invoke<void>("test_imap", { config, password }),
  reconnectAccount: (id: string, password?: string) => invoke<void>("reconnect_account", { id, password }),
  updateAccount: (update: { id: string; name: string; label: string | null; color: string; signature: string | null }) =>
    invoke<Account>("update_account", { update }),
  removeAccount: (id: string) => invoke<void>("remove_account", { id }),
  cancelOAuth: () => invoke<void>("cancel_oauth"),
  syncNow: (accountId?: string) => invoke<void>("sync_now", { accountId }),
  resyncAccount: (id: string) => invoke<void>("resync_account", { id }),

  listThreads: (filter: ThreadFilter) => invoke<ThreadSummary[]>("list_threads", { filter }),
  countUnread: (filters: Record<string, ThreadFilter>) => invoke<Record<string, number>>("count_unread", { filters }),
  getThread: (id: string) => invoke<ThreadDetail>("get_thread", { id }),
  threadAction: (threadIds: string[], action: ThreadAction) => invoke<void>("thread_action", { threadIds, action }),
  snooze: (threadIds: string[], until: number | null) => invoke<void>("snooze", { threadIds, until }),

  replyDraft: (messageId: string, mode: "reply" | "replyAll" | "forward") =>
    invoke<Draft>("reply_draft", { messageId, mode }),
  sendMessage: (draft: Draft) => invoke<{ id: string; sendAt: number }>("send_message", { draft }),
  cancelSend: (id: string) => invoke<Draft | null>("cancel_send", { id }),
  failedSends: () => invoke<{ id: string; draft: Draft; error: string }[]>("failed_sends"),
  saveDraft: (draft: Draft) => invoke<string>("save_draft", { draft }),
  listDrafts: () => invoke<Draft[]>("list_drafts"),
  deleteDraft: (id: string) => invoke<void>("delete_draft", { id }),
  suggestContacts: (query: string) => invoke<Contact[]>("suggest_contacts", { query }),
  openAttachment: (messageId: string, index: number, save: boolean) =>
    invoke<string>("open_attachment", { messageId, index, save }),

  getSettings: () => invoke<Record<string, string>>("get_settings"),
  setSetting: (key: string, value: string) => invoke<void>("set_setting", { key, value }),
  getSplits: () => invoke<Split[]>("get_splits"),
  setSplits: (splits: Split[]) => invoke<void>("set_splits", { splits }),
  oauthConfigStatus: () => invoke<{ google: boolean; microsoft: boolean }>("oauth_config_status"),

  aiStatus: () => invoke<AiStatus>("ai_status"),
  aiLogin: () => invoke<AiStatus>("ai_login"),
  aiLogout: () => invoke<void>("ai_logout"),
  aiModels: () => invoke<ModelInfo[]>("ai_models"),
  aiCancel: (requestId: string) => invoke<void>("ai_cancel", { requestId }),
};

/** Streams an AI job. Returns a cancel function. */
export function aiStream(
  command: "ai_summarize" | "ai_compose" | "ai_rewrite",
  args: Record<string, unknown>,
  handlers: { onDelta?: (t: string) => void; onDone?: (t: string) => void; onError?: (m: string) => void },
): () => void {
  const requestId = crypto.randomUUID();
  const channel = new Channel<AiEvent>();
  let finished = false;
  channel.onmessage = (ev) => {
    if (finished) return;
    if (ev.type === "delta") handlers.onDelta?.(ev.text);
    else if (ev.type === "done") {
      finished = true;
      handlers.onDone?.(ev.text);
    } else {
      finished = true;
      handlers.onError?.(ev.message);
    }
  };
  invoke(command, { ...args, requestId, channel }).catch((e) => {
    finished = true;
    handlers.onError?.(String(e));
  });
  return () => {
    finished = true;
    api.aiCancel(requestId).catch(() => {});
  };
}

export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e);
}
