export type AccountKind = "gmail" | "outlook" | "imap";

export interface Account {
  id: string;
  kind: AccountKind;
  email: string;
  name: string;
  label: string | null;
  color: string;
  createdAt: number;
  lastSync: number | null;
  lastError: string | null;
  signature: string | null;
}

export interface Address {
  name: string | null;
  email: string;
}

export interface AttachmentMeta {
  index: number;
  filename: string;
  mime: string;
  size: number;
  inline: boolean;
  contentId: string | null;
}

export interface ThreadSummary {
  id: string;
  accountId: string;
  subject: string;
  snippet: string;
  lastDate: number;
  participants: Address[];
  messageCount: number;
  unread: boolean;
  starred: boolean;
  inInbox: boolean;
  hasReceived: boolean;
  hasAttachments: boolean;
  category: string;
  snoozedUntil: number | null;
  aiSummary: string | null;
}

export interface Message {
  id: string;
  accountId: string;
  threadId: string;
  messageId: string | null;
  references: string[];
  subject: string;
  from: Address;
  to: Address[];
  cc: Address[];
  bcc: Address[];
  replyTo: Address[];
  date: number;
  snippet: string;
  bodyHtml: string | null;
  bodyText: string | null;
  isRead: boolean;
  isStarred: boolean;
  isDraft: boolean;
  inSent: boolean;
  attachments: AttachmentMeta[];
  listUnsubscribe: string | null;
}

export interface ThreadDetail {
  thread: ThreadSummary;
  messages: Message[];
}

export interface Draft {
  id?: string | null;
  accountId: string;
  to: Address[];
  cc: Address[];
  bcc: Address[];
  subject: string;
  html: string;
  text: string;
  inReplyTo?: string | null;
  references: string[];
  threadId?: string | null;
  attachments: { path: string }[];
  mode?: "new" | "reply" | "replyAll" | "forward" | null;
  quotedHtml?: string | null;
}

export interface Split {
  id: string;
  name: string;
  query: string;
}

export interface Contact {
  name: string | null;
  email: string;
  count: number;
}

export interface SyncStatus {
  accountId: string;
  state: "idle" | "syncing" | "error" | "auth";
  message: string | null;
  progress: [number, number] | null;
}

export type Security = "tls" | "starttls" | "none";

export interface ImapConfig {
  imapHost: string;
  imapPort: number;
  imapSecurity: Security;
  smtpHost: string;
  smtpPort: number;
  smtpSecurity: Security;
  username: string;
  auth: "password" | "microsoftOAuth";
}

export type ThreadAction = "archive" | "inbox" | "trash" | "read" | "unread" | "star" | "unstar" | "spam";

export type View = "inbox" | "starred" | "snoozed" | "sent" | "drafts" | "done" | "trash" | "spam" | "search";

export interface ThreadFilter {
  view: View;
  accountId?: string | null;
  query?: string | null;
  limit?: number;
  offset?: number;
}

export interface AiStatus {
  connected: boolean;
  email: string | null;
  plan: string | null;
  model: string | null;
}

export interface ModelInfo {
  slug: string;
  name: string;
}

export type AiEvent =
  | { type: "delta"; text: string }
  | { type: "done"; text: string }
  | { type: "error"; message: string };

/** An app or assistant connected to the server for AI assistants. */
export interface ServerDevice {
  id: string;
  kind: "app" | "assistant";
  name: string;
  createdAt: number;
  lastSeenAt: number | null;
  /** Assistants only: whether it may change things or only read. */
  canWrite: boolean | null;
}

export interface ServerStatus {
  version: string;
  /** This app's own entry in `devices`. */
  deviceId: string;
  accounts: { id: string; email: string; kind: AccountKind; lastSync: number | null; lastError: string | null }[];
  devices: ServerDevice[];
}

/** The link to the user's cigmail-server. */
export interface ServerState {
  /** Null when the app isn't connected to a server. */
  url: string | null;
  /** The address to give an assistant. */
  mcpUrl: string | null;
  status: ServerStatus | null;
  /** Why the server couldn't be reached just now. */
  error: string | null;
}
