# CigMail

A fast, keyboard-first email client that pulls every account into one unified inbox. It looks like Linear, works with the speed of Superhuman, and is built with Tauri 2, Rust and React.

- **One inbox** for Gmail (Gmail API), Outlook / Microsoft 365 (Graph) and any IMAP/SMTP server
- **Split inbox**: Important / News / Updates, or any saved search (`account:school is:unread`). Press <kbd>Tab</kbd> to cycle splits.
- **Keyboard everything**: <kbd>J</kbd>/<kbd>K</kbd>, <kbd>E</kbd> done, <kbd>H</kbd> snooze, <kbd>R</kbd>/<kbd>A</kbd>/<kbd>F</kbd>, <kbd>C</kbd> compose, <kbd>/</kbd> search, <kbd>⌘K</kbd> command palette, <kbd>Z</kbd> undo, <kbd>?</kbd> for the full list
- **Instant and offline**: everything reads from a local SQLite cache with FTS5 full-text search; changes are applied locally first and synced in the background
- **Undo send**, snooze with natural language ("fri 9am", "3d"), autosaved drafts, contact autocomplete
- **AI on your ChatGPT plan**: thread summaries (TL;DR + key facts + action items) and <kbd>⌘J</kbd> write / rewrite in your own tone
- **Privacy**: remote images and tracking pixels are blocked by default; emails render in a sandboxed iframe

## Getting started

```bash
pnpm install
pnpm tauri dev
```

Then follow [SETUP.md](SETUP.md) to register the Google / Microsoft OAuth apps. IMAP accounts work right away.

`pnpm dev` runs the UI in a normal browser with sample data, which is handy for design work.

## Layout

```
src-tauri/src
  db.rs          SQLite schema, threads, FTS, ops queue, outbox
  query.rs       search / split query language → SQL
  mime.rs        parse incoming MIME, classify, build outgoing mail, reply drafts
  providers/     gmail.rs (history API), graph.rs (delta + immutable ids), imap.rs (IMAP + SMTP)
  sync.rs        per-account sync workers, undo-send outbox, snooze wake-ups, notifications
  oauth.rs       PKCE loopback OAuth for Google, Microsoft and ChatGPT
  ai.rs          ChatGPT (Codex backend) streaming + prompts
  commands.rs    Tauri command API
src
  store.ts       app state, optimistic actions, undo
  lib/commands.ts  command registry shared by hotkeys and ⌘K
  components/    Sidebar, ThreadList, ThreadView, Composer, CommandPalette, Settings, …
```

## Tests

```bash
cd src-tauri && cargo test
```
