/**
 * Browser-only IPC mock so the UI can be developed with `pnpm dev` outside
 * Tauri. Never loaded inside the real app.
 */
import { mockIPC } from "@tauri-apps/api/mocks";
import type { Account, Message, ThreadSummary } from "./types";

const now = Math.floor(Date.now() / 1000);
const H = 3600;

const accounts: Account[] = [
  { id: "g1", kind: "gmail", email: "deniz@gmail.com", name: "Deniz Ciger", label: "Personal", color: "#5e6ad2", createdAt: 0, lastSync: now, lastError: null, signature: null },
  { id: "o1", kind: "outlook", email: "deniz.ciger@uni.edu", name: "Deniz Ciger", label: "School", color: "#4cb782", createdAt: 0, lastSync: now, lastError: null, signature: null },
  { id: "o2", kind: "outlook", email: "deniz@outlook.com", name: "Deniz", label: null, color: "#f2994a", createdAt: 0, lastSync: now, lastError: null, signature: null },
  { id: "i1", kind: "imap", email: "deniz@ciger.dev", name: "Deniz Ciger", label: "Dev", color: "#bb87fc", createdAt: 0, lastSync: now, lastError: null, signature: null },
];

const seed: [string, string, string, string, string, number, string, Partial<ThreadSummary>?][] = [
  ["o1", "Prof. Anna Weber", "a.weber@uni.edu", "Exam schedule for Distributed Systems", "Hi all, the final exam will take place on December 14th in lecture hall H3. Please bring your student ID.", 0.3, "primary", { unread: true, messageCount: 3, aiSummary: "Final exam is Dec 14 in H3, bring your student ID; registration closes Nov 30.\n- Allowed: one handwritten A4 sheet\nAction: Register in the portal by Nov 30" }],
  ["g1", "Lena Hoffmann", "lena.h@gmail.com", "Dinner on Saturday?", "Hey! Are you free this Saturday? Thinking about trying that new ramen place near the station.", 1.2, "primary", { unread: true }],
  ["i1", "GitHub", "notifications@github.com", "[tauri-apps/tauri] Fix window decorations on Wayland (#12034)", "@deniz commented on this pull request. Looks good to me, can you rebase on dev?", 1.9, "notification", { unread: true, messageCount: 4 }],
  ["g1", "Stripe", "receipts@stripe.com", "Your receipt from Vercel Inc. #2291-4410", "Amount paid $20.00 · Date paid Nov 2, 2026 · Payment method Visa ending 4242", 3, "notification"],
  ["o1", "Student Services", "noreply@uni.edu", "Semester fee reminder", "This is a reminder that the semester contribution for the summer term is due by January 15.", 5, "notification"],
  ["g1", "Max Bauer", "max.bauer@posteo.de", "Re: Flat viewing next week", "Sounds good, I'll be there on Tuesday at 6pm. Should I bring the documents?", 7, "primary", { messageCount: 5, starred: true }],
  ["i1", "Linear", "changelog@linear.app", "Linear Changelog: Agents, Pulse 2.0 and more", "This month we're introducing a new way for agents to collaborate with your team inside issues.", 9, "newsletter"],
  ["o2", "Amazon.de", "versand@amazon.de", "Your package has been delivered", "Your order 302-1234567 has been delivered. It was handed to resident.", 22, "notification"],
  ["g1", "Figma", "no-reply@figma.com", "Jonas invited you to “CigMail Design”", "Jonas Klein has invited you to edit a file. Open file to start collaborating.", 26, "notification", { hasAttachments: false }],
  ["o1", "Tim Schulz", "tim.schulz@uni.edu", "Group project – slides draft", "Hey, I put together a first draft of our slides. Could you take a look at the architecture part?", 30, "primary", { hasAttachments: true, messageCount: 2 }],
  ["i1", "Hetzner", "info@hetzner.com", "Invoice 0012345678", "Dear customer, attached you will find your invoice for October 2026.", 46, "notification", { hasAttachments: true }],
  ["g1", "The Pragmatic Engineer", "pragmaticengineer@substack.com", "The state of AI coding tools in late 2026", "In today's issue: how engineering teams actually use agentic coding tools, with data from 900 respondents.", 50, "newsletter"],
  ["o2", "Mom", "mama@web.de", "Fotos vom Wochenende", "Hallo Schatz, hier sind die Fotos vom Wochenende. Das Wetter war so schön!", 70, "primary", { hasAttachments: true }],
  ["g1", "Spotify", "no-reply@spotify.com", "Your Wrapped is almost here", "Get ready. Your year in music is coming soon.", 80, "newsletter"],
  ["i1", "Cloudflare", "noreply@notify.cloudflare.com", "Certificate renewed for ciger.dev", "The edge certificate for ciger.dev has been renewed successfully.", 96, "notification"],
  ["o1", "Library", "library@uni.edu", "Loan period ending soon", "The loan period for “Designing Data-Intensive Applications” ends in 3 days.", 120, "notification"],
  ["g1", "Sarah Kim", "sarah@kim.studio", "Freelance project inquiry", "Hi Deniz, I found your portfolio and wanted to ask whether you'd be available for a two-week Rust project in December.", 140, "primary", { messageCount: 1 }],
];

const threads: ThreadSummary[] = seed.map(([acc, name, email, subject, snippet, hoursAgo, category, extra], i) => ({
  id: `${acc}:t${i}`,
  accountId: acc,
  subject,
  snippet,
  lastDate: now - Math.round(hoursAgo * H),
  participants: [{ name, email }],
  messageCount: 1,
  unread: false,
  starred: false,
  inInbox: true,
  hasAttachments: false,
  category,
  snoozedUntil: null,
  aiSummary: null,
  ...extra,
}));

function messagesFor(t: ThreadSummary): Message[] {
  const p = t.participants[0];
  const acc = accounts.find((a) => a.id === t.accountId)!;
  const n = Math.max(1, Math.min(t.messageCount, 3));
  return Array.from({ length: n }).map((_, i) => {
    const mine = i % 2 === 1;
    const from = mine ? { name: acc.name, email: acc.email } : p;
    const html =
      i === n - 1 && t.category !== "primary"
        ? `<table width="100%" bgcolor="#f4f4f5" style="padding:24px"><tr><td><table width="560" align="center" bgcolor="#ffffff" style="border-radius:8px;padding:32px;font-family:Helvetica,Arial"><tr><td><h2 style="margin:0 0 12px">${t.subject}</h2><p style="color:#52525b">${t.snippet}</p><p><a href="https://example.com" style="background:#5e6ad2;color:#fff;padding:10px 16px;border-radius:6px;text-decoration:none">View details</a></p><img src="https://example.com/pixel.gif" width="1" height="1"></td></tr></table></td></tr></table>`
        : null;
    return {
      id: `${t.id}:m${i}`,
      accountId: t.accountId,
      threadId: t.id,
      messageId: `${t.id}-${i}@mock`,
      references: [],
      subject: t.subject,
      from,
      to: [mine ? p : { name: acc.name, email: acc.email }],
      cc: [],
      bcc: [],
      replyTo: [],
      date: t.lastDate - (n - 1 - i) * 5 * H,
      snippet: t.snippet,
      bodyHtml: html,
      bodyText: mine
        ? "Thanks, that works for me.\n\nBest,\nDeniz"
        : `${t.snippet}\n\nLet me know if you have any questions.\n\nBest regards,\n${p.name}`,
      isRead: !t.unread || i < n - 1,
      isStarred: false,
      isDraft: false,
      inSent: mine,
      attachments: t.hasAttachments && i === n - 1 ? [
              { index: 0, filename: "slides-draft-v2.pdf", mime: "application/pdf", size: 2_400_000, inline: false, contentId: null },
              { index: 1, filename: "architecture.png", mime: "image/png", size: 310_000, inline: false, contentId: null },
            ] : [],
      listUnsubscribe: t.category === "newsletter" ? "<https://example.com/unsubscribe>" : null,
    };
  });
}

function filterThreads(filter: { view: string; accountId?: string | null; query?: string | null; limit?: number; offset?: number }) {
  let list = [...threads];
  if (filter.view === "inbox") list = list.filter((t) => t.inInbox && !(t.snoozedUntil && t.snoozedUntil > now));
  if (filter.view === "starred") list = list.filter((t) => t.starred);
  if (filter.view === "done") list = list.filter((t) => !t.inInbox);
  if (filter.view === "snoozed") list = list.filter((t) => t.snoozedUntil && t.snoozedUntil > now);
  if (["sent", "drafts", "trash", "spam"].includes(filter.view)) list = [];
  if (filter.accountId) list = list.filter((t) => t.accountId === filter.accountId);
  for (const term of (filter.query ?? "").split(/\s+/).filter(Boolean)) {
    const neg = term.startsWith("-");
    const tt = neg ? term.slice(1) : term;
    let pred: (t: ThreadSummary) => boolean;
    if (tt.startsWith("category:")) pred = (t) => t.category === tt.slice(9);
    else if (tt === "is:unread") pred = (t) => t.unread;
    else if (tt.startsWith("account:")) pred = (t) => accounts.find((a) => a.id === t.accountId)!.email.includes(tt.slice(8));
    else pred = (t) => (t.subject + t.snippet + t.participants[0].name).toLowerCase().includes(tt.toLowerCase());
    list = list.filter((t) => (neg ? !pred(t) : pred(t)));
  }
  list.sort((a, b) => b.lastDate - a.lastDate);
  return list.slice(filter.offset ?? 0, (filter.offset ?? 0) + (filter.limit ?? 300));
}

export function installDevMock() {
  const settings: Record<string, string> = {};
  let splits = [
    { id: "important", name: "Important", query: "category:primary" },
    { id: "news", name: "News", query: "category:newsletter" },
    { id: "updates", name: "Updates", query: "category:notification" },
  ];
  mockIPC(
    (cmd, payload) => {
      const args = (payload ?? {}) as Record<string, any>;
      switch (cmd) {
        case "list_accounts":
          return accounts;
        case "get_splits":
          return splits;
        case "set_splits":
          splits = args.splits;
          return null;
        case "get_settings":
          return settings;
        case "set_setting":
          settings[args.key] = args.value;
          return null;
        case "ai_status":
          return { connected: true, email: "deniz@gmail.com", plan: "plus", model: null };
        case "ai_models":
          return [{ slug: "gpt-5.4-mini", name: "GPT-5.4 mini" }, { slug: "gpt-5.5", name: "GPT-5.5" }];
        case "oauth_config_status":
          return { google: true, microsoft: false };
        case "list_threads":
          return filterThreads(args.filter);
        case "count_unread": {
          const out: Record<string, number> = {};
          for (const [k, f] of Object.entries(args.filters as Record<string, any>)) out[k] = filterThreads({ ...f, limit: 9999 }).filter((t) => t.unread).length;
          return out;
        }
        case "get_thread": {
          const t = threads.find((x) => x.id === args.id)!;
          return { thread: t, messages: messagesFor(t) };
        }
        case "thread_action": {
          for (const id of args.threadIds as string[]) {
            const t = threads.find((x) => x.id === id);
            if (!t) continue;
            if (args.action === "archive") t.inInbox = false;
            if (args.action === "inbox") t.inInbox = true;
            if (args.action === "read") t.unread = false;
            if (args.action === "unread") t.unread = true;
            if (args.action === "star") t.starred = true;
            if (args.action === "unstar") t.starred = false;
            if (args.action === "trash") threads.splice(threads.indexOf(t), 1);
          }
          return null;
        }
        case "snooze":
          for (const id of args.threadIds as string[]) {
            const t = threads.find((x) => x.id === id);
            if (t) t.snoozedUntil = args.until;
          }
          return null;
        case "suggest_contacts":
          return [
            { name: "Lena Hoffmann", email: "lena.h@gmail.com", count: 12 },
            { name: "Max Bauer", email: "max.bauer@posteo.de", count: 8 },
          ].filter((c) => (c.name + c.email).toLowerCase().includes(String(args.query).toLowerCase()));
        case "reply_draft": {
          const t = threads.find((x) => (args.messageId as string).startsWith(x.id))!;
          return { accountId: t.accountId, to: t.participants, cc: [], bcc: [], subject: `Re: ${t.subject}`, html: "", text: "", references: [], attachments: [], threadId: t.id, mode: args.mode, quotedHtml: `<p>On …, ${t.participants[0].name} wrote:</p><blockquote>${t.snippet}</blockquote>` };
        }
        case "send_message":
          return { id: "x", sendAt: now + 10 };
        case "list_drafts":
        case "failed_sends":
          return [];
        case "save_attachments":
          return (args.messageIds as string[]).flatMap((_: string, i: number) => [`/home/you/Downloads/file-${i + 1}.pdf`]);
        case "save_draft":
          return "d1";
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
}
