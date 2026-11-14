# CigMail

A fast, keyboard-first email client that pulls every account into one unified inbox. It looks like Linear, works with the speed of Superhuman, and is built with Tauri 2, Rust and React.

- **One inbox** for Gmail (Gmail API), Outlook / Microsoft 365 (Graph) and any IMAP/SMTP server
- **Split inbox**: Important / News / Updates, or any saved search (`account:school is:unread`). Press <kbd>Tab</kbd> to cycle splits.
- **Keyboard everything**: <kbd>J</kbd>/<kbd>K</kbd>, <kbd>E</kbd> done, <kbd>H</kbd> snooze, <kbd>R</kbd>/<kbd>A</kbd>/<kbd>F</kbd>, <kbd>C</kbd> compose, <kbd>/</kbd> search, <kbd>⌘K</kbd> command palette, <kbd>Z</kbd> undo, <kbd>?</kbd> for the full list
- **Instant and offline**: everything reads from a local SQLite cache with FTS5 full-text search; changes are applied locally first and synced in the background
- **Undo send**, snooze with natural language ("fri 9am", "3d"), autosaved drafts, contact autocomplete
- **AI on your ChatGPT plan**: thread summaries (TL;DR + key facts + action items) and <kbd>⌘J</kbd> write / rewrite in your own tone
- **Works with AI assistants**: Claude, Gemini and others can search and read your mail, tidy the inbox and write drafts through your own server. They can never send. See [docs/assistants.md](docs/assistants.md)
- **Privacy**: remote images and tracking pixels are blocked by default; emails render in a sandboxed iframe

## Getting started

```bash
pnpm install
pnpm tauri dev
```

To build the Linux desktop app and refresh the local Noctalia launcher entry:

```bash
pnpm desktop:build
```

This builds Tauri bundles, then installs the generated AppImage to
`~/.local/bin/cigmail.AppImage` and updates
`~/.local/share/applications/cigmail.desktop`.


Then follow [SETUP.md](SETUP.md) to register the Google / Microsoft OAuth apps. IMAP accounts work right away.

`pnpm dev` runs the UI in a normal browser with sample data, which is handy for design work. Add `?mobile` to the URL to get the phone UI.

## Android

The Android app is the same Rust core (sync, SQLite cache, providers) with a touch UI on top: swipe right for done and swipe left to snooze, long-press to select, pull to refresh, and the back button closes the topmost screen. See [SETUP.md](SETUP.md#android) to build it.

## Layout

```
crates/cigmail-core/src   the mail engine, shared by the app and the server
  db.rs          SQLite schema, threads, FTS, ops queue, outbox
  query.rs       search / split query language → SQL
  mime.rs        parse incoming MIME, classify, build outgoing mail, reply drafts
  providers/     gmail.rs (history API), graph.rs (delta + immutable ids), imap.rs (IMAP + SMTP)
  sync.rs        per-account sync workers, undo-send outbox, snooze wake-ups, notifications
  oauth.rs       PKCE loopback OAuth for Google, Microsoft and ChatGPT
crates/cigmail-server/src the server for AI assistants (docs/assistants.md)
  api.rs         pairing with the app, receiving its accounts
  oauth.rs       sign-in for assistants (OAuth 2.1)
  mcp.rs         the mail tools (Model Context Protocol)
src-tauri/src             the app
  ai.rs          ChatGPT (Codex backend) streaming + prompts
  server.rs      the link to a cigmail-server
  commands.rs    Tauri command API
src
  store.ts       app state, optimistic actions, undo
  lib/commands.ts  command registry shared by hotkeys and ⌘K
  components/    Sidebar, ThreadList, ThreadView, Composer, CommandPalette, Settings, …
  mobile/        phone shell: Inbox, Thread, Drawer, Sheets, back-button handling
```

## Tests

```bash
cargo test --workspace
```
