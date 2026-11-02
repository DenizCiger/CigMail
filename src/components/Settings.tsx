import clsx from "clsx";
import { useEffect, useState } from "react";
import {
  ArrowDown,
  ArrowUp,
  Check,
  KeyRound,
  Layers,
  Mail,
  Plus,
  RefreshCw,
  Settings2,
  Sparkles,
  Trash2,
  Keyboard,
} from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useStore } from "../store";
import { api, errorMessage } from "../lib/api";
import type { Account, AccountKind, ImapConfig, ModelInfo, Split } from "../lib/types";
import { Button, Field, IconButton, Input, Modal, ProviderGlyph, Select, Spinner, Switch } from "./ui";

const SECTIONS = [
  { id: "general", label: "General", icon: Settings2 },
  { id: "accounts", label: "Accounts", icon: Mail },
  { id: "splits", label: "Split inbox", icon: Layers },
  { id: "ai", label: "AI", icon: Sparkles },
  { id: "connections", label: "Connections", icon: KeyRound },
];

export function Settings() {
  const section = useStore((s) => s.settingsSection);
  const set = useStore((s) => s.set);
  if (!section) return null;
  const close = () => set({ settingsSection: null });
  return (
    <Modal open onClose={close} className="flex h-[min(680px,86vh)] w-[min(940px,92vw)]">
      <nav className="flex w-52 shrink-0 flex-col gap-px border-r border-line bg-elev-2 p-2.5">
        <div className="px-2 pb-3 pt-1.5 text-[13px] font-semibold">Settings</div>
        {SECTIONS.map((s) => (
          <button
            key={s.id}
            onClick={() => set({ settingsSection: s.id })}
            className={clsx(
              "flex h-7 items-center gap-2 rounded-md px-2 text-[13px]",
              section === s.id ? "bg-active text-fg" : "text-fg-2 hover:bg-hover hover:text-fg",
            )}
          >
            <s.icon size={14} className="text-fg-3" />
            {s.label}
          </button>
        ))}
        <button
          onClick={() => set({ settingsSection: null, help: true })}
          className="flex h-7 items-center gap-2 rounded-md px-2 text-[13px] text-fg-2 hover:bg-hover hover:text-fg"
        >
          <Keyboard size={14} className="text-fg-3" />
          Shortcuts
        </button>
      </nav>
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex-1 overflow-y-auto px-8 py-7">
          {section === "general" && <General />}
          {section === "accounts" && <Accounts />}
          {section === "splits" && <Splits />}
          {section === "ai" && <AiSettings />}
          {section === "connections" && <Connections />}
        </div>
      </div>
    </Modal>
  );
}

function H({ title, desc }: { title: string; desc?: string }) {
  return (
    <div className="mb-6">
      <h2 className="text-[17px] font-semibold tracking-tight">{title}</h2>
      {desc && <p className="mt-1 text-[13px] text-fg-3">{desc}</p>}
    </div>
  );
}

function Row({ label, desc, children }: { label: string; desc?: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center gap-6 border-b border-line py-3.5">
      <div className="flex-1">
        <div className="text-[13px]">{label}</div>
        {desc && <div className="mt-0.5 text-xs text-fg-3">{desc}</div>}
      </div>
      {children}
    </div>
  );
}

// ------------------------------------------------------------------ general

function General() {
  const settings = useStore((s) => s.settings);
  const setSetting = useStore((s) => s.setSetting);
  const accounts = useStore((s) => s.accounts);
  return (
    <>
      <H title="General" />
      <Row label="Theme">
        <Select
          value={settings.theme ?? "system"}
          onChange={(v) => setSetting("theme", v)}
          options={[
            { value: "system", label: "System" },
            { value: "dark", label: "Dark" },
            { value: "light", label: "Light" },
          ]}
        />
      </Row>
      <Row label="Undo send" desc="How long you can take a message back after hitting send.">
        <Select
          value={settings.undo_send_secs ?? "10"}
          onChange={(v) => setSetting("undo_send_secs", v)}
          options={["0", "5", "10", "20", "30"].map((v) => ({ value: v, label: v === "0" ? "Off" : `${v} seconds` }))}
        />
      </Row>
      <Row label="After marking done" desc="What to show after archiving, trashing or snoozing an open conversation.">
        <Select
          value={settings.after_action ?? "next"}
          onChange={(v) => setSetting("after_action", v)}
          options={[
            { value: "next", label: "Next conversation" },
            { value: "list", label: "Back to list" },
          ]}
        />
      </Row>
      <Row label="Mark as read when opened">
        <Switch checked={settings.mark_read_on_open !== "false"} onChange={(v) => setSetting("mark_read_on_open", String(v))} />
      </Row>
      <Row label="Remote images" desc="Blocking images stops senders from tracking when you open their mail.">
        <Select
          value={settings.remote_images ?? "ask"}
          onChange={(v) => setSetting("remote_images", v)}
          options={[
            { value: "ask", label: "Block (ask)" },
            { value: "always", label: "Always load" },
          ]}
        />
      </Row>
      <Row label="Desktop notifications" desc="Only for new mail in the Important split.">
        <Switch checked={settings.notifications !== "false"} onChange={(v) => setSetting("notifications", String(v))} />
      </Row>
      {accounts.length > 1 && (
        <Row label="Default account for new messages">
          <Select
            value={settings.default_account ?? accounts[0].id}
            onChange={(v) => setSetting("default_account", v)}
            options={accounts.map((a) => ({ value: a.id, label: a.label ? `${a.label} (${a.email})` : a.email }))}
          />
        </Row>
      )}
      <Row label="Download history" desc="How far back new accounts are synced. Applies to accounts added or resynced afterwards.">
        <Select
          value={settings.sync_days ?? "60"}
          onChange={(v) => setSetting("sync_days", v)}
          options={["30", "60", "90", "180", "365", "730"].map((v) => ({ value: v, label: `${v} days` }))}
        />
      </Row>
    </>
  );
}

// ------------------------------------------------------------------ accounts

const COLORS = ["#5e6ad2", "#26b5ce", "#4cb782", "#f2c94c", "#f2994a", "#eb5757", "#bb87fc", "#e86fb4"];

function AccountCard({ a }: { a: Account }) {
  const sync = useStore((s) => s.sync[a.id]);
  const reloadMeta = useStore((s) => s.reloadMeta);
  const toast = useStore((s) => s.toast);
  const [open, setOpen] = useState(false);
  const [name, setName] = useState(a.name);
  const [label, setLabel] = useState(a.label ?? "");
  const [color, setColor] = useState(a.color);
  const [signature, setSignature] = useState(a.signature ?? "");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState("");

  const error = sync?.state === "error" || sync?.state === "auth" ? sync.message : a.lastError;

  const save = async () => {
    await api.updateAccount({ id: a.id, name, label: label || null, color, signature: signature || null });
    await reloadMeta();
    toast("Account updated", { kind: "success" });
  };
  const reconnect = async () => {
    setBusy("reconnect");
    try {
      await api.reconnectAccount(a.id, password || undefined);
      await reloadMeta();
      toast("Reconnected", { kind: "success" });
    } catch (e) {
      toast(errorMessage(e), { kind: "error" });
    } finally {
      setBusy("");
    }
  };

  return (
    <div className="rounded-lg border border-line-strong">
      <button onClick={() => setOpen(!open)} className="flex w-full items-center gap-3 px-4 py-3 text-left">
        <span className="h-2.5 w-2.5 rounded-full" style={{ background: a.color }} />
        <ProviderGlyph kind={a.kind} size={16} />
        <div className="min-w-0 flex-1">
          <div className="truncate text-[13px] font-medium">
            {a.label ? `${a.label} · ` : ""}
            {a.email}
          </div>
          <div className={clsx("truncate text-xs", error ? "text-danger" : "text-fg-3")}>
            {error
              ? error
              : sync?.state === "syncing"
                ? "Syncing…"
                : a.lastSync
                  ? `Synced ${new Date(a.lastSync * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`
                  : "Waiting for first sync"}
          </div>
        </div>
        {sync?.state === "syncing" && <Spinner />}
      </button>
      {open && (
        <div className="flex flex-col gap-4 border-t border-line px-4 py-4">
          <div className="grid grid-cols-2 gap-3">
            <Field label="Sender name">
              <Input value={name} onChange={(e) => setName(e.target.value)} />
            </Field>
            <Field label="Label" hint="Shown in the sidebar, e.g. School">
              <Input value={label} onChange={(e) => setLabel(e.target.value)} placeholder="Personal" />
            </Field>
          </div>
          <Field label="Color">
            <div className="flex gap-2">
              {COLORS.map((c) => (
                <button
                  key={c}
                  onClick={() => setColor(c)}
                  className="flex h-6 w-6 items-center justify-center rounded-full"
                  style={{ background: c }}
                >
                  {color === c && <Check size={13} className="text-white" />}
                </button>
              ))}
            </div>
          </Field>
          <Field label="Signature">
            <textarea
              value={signature}
              onChange={(e) => setSignature(e.target.value)}
              rows={3}
              className="rounded-md border border-line-strong bg-input px-2.5 py-2 text-[13px] focus:border-accent"
            />
          </Field>
          {error && a.kind === "imap" && (
            <Field label="Password" hint="Enter a new password or app password to reconnect.">
              <Input type="password" value={password} onChange={(e) => setPassword(e.target.value)} />
            </Field>
          )}
          <div className="flex items-center gap-2">
            <Button variant="primary" onClick={save}>
              Save
            </Button>
            <Button onClick={reconnect} disabled={!!busy}>
              {busy === "reconnect" && <Spinner size={12} />}
              Reconnect
            </Button>
            <Button
              variant="ghost"
              icon={<RefreshCw size={13} />}
              onClick={async () => {
                if (!window.confirm("Delete the local copy of this account's mail and download it again?")) return;
                await api.resyncAccount(a.id);
                toast("Resync started");
              }}
            >
              Resync
            </Button>
            <div className="flex-1" />
            <Button
              variant="ghost"
              className="text-danger hover:text-danger"
              icon={<Trash2 size={13} />}
              onClick={async () => {
                if (!window.confirm(`Remove ${a.email} from CigMail? Mail on the server is not touched.`)) return;
                await api.removeAccount(a.id);
                await reloadMeta();
                useStore.getState().refresh();
                useStore.getState().refreshCounts();
              }}
            >
              Remove
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

function Accounts() {
  const accounts = useStore((s) => s.accounts);
  const oauth = useStore((s) => s.oauth);
  const set = useStore((s) => s.set);
  const reloadMeta = useStore((s) => s.reloadMeta);
  const toast = useStore((s) => s.toast);
  const [adding, setAdding] = useState<AccountKind | null>(null);
  const [waiting, setWaiting] = useState<AccountKind | null>(null);
  const [label, setLabel] = useState("");

  const startOAuth = async (kind: AccountKind) => {
    setWaiting(kind);
    try {
      const a = await api.addOAuthAccount(kind, label || undefined);
      await reloadMeta();
      toast(`Added ${a.email}`, { kind: "success" });
      setAdding(null);
      setLabel("");
      useStore.getState().refreshCounts();
    } catch (e) {
      const msg = errorMessage(e);
      if (!msg.includes("cancelled")) toast(msg, { kind: "error", duration: 8000 });
    } finally {
      setWaiting(null);
    }
  };

  return (
    <>
      <H title="Accounts" desc="Every account syncs into one unified inbox. Mail is cached locally for instant search and offline reading." />
      <div className="flex flex-col gap-2">
        {accounts.map((a) => (
          <AccountCard key={a.id} a={a} />
        ))}
      </div>

      <div className="mt-8 mb-3 text-[13px] font-medium">Add account</div>
      {!adding && (
        <div className="grid grid-cols-3 gap-2">
          <ProviderButton kind="gmail" title="Gmail" desc="Google Workspace or Gmail" onClick={() => setAdding("gmail")} />
          <ProviderButton kind="outlook" title="Outlook" desc="Microsoft 365, Outlook.com, Hotmail" onClick={() => setAdding("outlook")} />
          <ProviderButton kind="imap" title="IMAP / SMTP" desc="Any other provider" onClick={() => setAdding("imap")} />
        </div>
      )}

      {(adding === "gmail" || adding === "outlook") && (
        <div className="rounded-lg border border-line-strong p-4">
          {!(adding === "gmail" ? oauth.google : oauth.microsoft) ? (
            <div className="flex flex-col gap-3 text-[13px]">
              <p className="text-fg-2">
                {adding === "gmail" ? "Gmail" : "Outlook"} needs an OAuth client ID before you can sign in. It takes about five minutes to create
                one — see <span className="font-mono text-xs">SETUP.md</span> in the project.
              </p>
              <div className="flex gap-2">
                <Button variant="primary" onClick={() => set({ settingsSection: "connections" })}>
                  Configure connection
                </Button>
                <Button variant="ghost" onClick={() => setAdding(null)}>
                  Cancel
                </Button>
              </div>
            </div>
          ) : (
            <div className="flex flex-col gap-4">
              <Field label="Label (optional)" hint="Helps tell accounts apart, e.g. “School” or “Personal”.">
                <Input value={label} onChange={(e) => setLabel(e.target.value)} placeholder={adding === "outlook" ? "School" : "Personal"} />
              </Field>
              <div className="flex items-center gap-2">
                <Button variant="primary" onClick={() => startOAuth(adding)} disabled={!!waiting} icon={<ProviderGlyph kind={adding} />}>
                  {waiting ? "Waiting for browser…" : `Sign in with ${adding === "gmail" ? "Google" : "Microsoft"}`}
                </Button>
                {waiting ? (
                  <Button variant="ghost" onClick={() => api.cancelOAuth()}>
                    Cancel
                  </Button>
                ) : (
                  <Button variant="ghost" onClick={() => setAdding(null)}>
                    Back
                  </Button>
                )}
              </div>
              {adding === "outlook" && (
                <p className="text-xs text-fg-3">
                  School or work account blocked by your administrator? Use <b>IMAP / SMTP → Microsoft 365 (OAuth)</b> instead.
                </p>
              )}
            </div>
          )}
        </div>
      )}

      {adding === "imap" && <ImapForm onDone={() => setAdding(null)} />}
    </>
  );
}

function ProviderButton({ kind, title, desc, onClick }: { kind: string; title: string; desc: string; onClick: () => void }) {
  return (
    <button onClick={onClick} className="flex flex-col items-start gap-2 rounded-lg border border-line-strong p-3.5 text-left hover:border-fg-4 hover:bg-hover">
      <ProviderGlyph kind={kind} size={20} />
      <div>
        <div className="text-[13px] font-medium">{title}</div>
        <div className="text-xs text-fg-3">{desc}</div>
      </div>
    </button>
  );
}

const PRESETS: { id: string; label: string; match: RegExp; cfg: Partial<ImapConfig> }[] = [
  { id: "m365", label: "Microsoft 365 (OAuth)", match: /^$/, cfg: { imapHost: "outlook.office365.com", imapPort: 993, imapSecurity: "tls", smtpHost: "smtp.office365.com", smtpPort: 587, smtpSecurity: "starttls", auth: "microsoftOAuth" } },
  { id: "gmail", label: "Gmail (app password)", match: /@(gmail|googlemail)\.com$/, cfg: { imapHost: "imap.gmail.com", imapPort: 993, imapSecurity: "tls", smtpHost: "smtp.gmail.com", smtpPort: 465, smtpSecurity: "tls" } },
  { id: "outlook", label: "Outlook.com (app password)", match: /@(outlook|hotmail|live|msn)\.[a-z.]+$/, cfg: { imapHost: "outlook.office365.com", imapPort: 993, imapSecurity: "tls", smtpHost: "smtp-mail.outlook.com", smtpPort: 587, smtpSecurity: "starttls" } },
  { id: "icloud", label: "iCloud", match: /@(icloud|me|mac)\.com$/, cfg: { imapHost: "imap.mail.me.com", imapPort: 993, imapSecurity: "tls", smtpHost: "smtp.mail.me.com", smtpPort: 587, smtpSecurity: "starttls" } },
  { id: "gmx", label: "GMX", match: /@gmx\.[a-z]+$/, cfg: { imapHost: "imap.gmx.net", imapPort: 993, imapSecurity: "tls", smtpHost: "mail.gmx.net", smtpPort: 587, smtpSecurity: "starttls" } },
  { id: "webde", label: "WEB.DE", match: /@web\.de$/, cfg: { imapHost: "imap.web.de", imapPort: 993, imapSecurity: "tls", smtpHost: "smtp.web.de", smtpPort: 587, smtpSecurity: "starttls" } },
  { id: "posteo", label: "Posteo", match: /@posteo\.[a-z]+$/, cfg: { imapHost: "posteo.de", imapPort: 993, imapSecurity: "tls", smtpHost: "posteo.de", smtpPort: 465, smtpSecurity: "tls" } },
  { id: "mailbox", label: "mailbox.org", match: /@mailbox\.org$/, cfg: { imapHost: "imap.mailbox.org", imapPort: 993, imapSecurity: "tls", smtpHost: "smtp.mailbox.org", smtpPort: 465, smtpSecurity: "tls" } },
  { id: "yahoo", label: "Yahoo", match: /@yahoo\.[a-z.]+$/, cfg: { imapHost: "imap.mail.yahoo.com", imapPort: 993, imapSecurity: "tls", smtpHost: "smtp.mail.yahoo.com", smtpPort: 465, smtpSecurity: "tls" } },
  { id: "fastmail", label: "Fastmail", match: /@fastmail\.[a-z]+$/, cfg: { imapHost: "imap.fastmail.com", imapPort: 993, imapSecurity: "tls", smtpHost: "smtp.fastmail.com", smtpPort: 465, smtpSecurity: "tls" } },
];

function ImapForm({ onDone }: { onDone: () => void }) {
  const reloadMeta = useStore((s) => s.reloadMeta);
  const toast = useStore((s) => s.toast);
  const [email, setEmail] = useState("");
  const [name, setName] = useState("");
  const [label, setLabel] = useState("");
  const [password, setPassword] = useState("");
  const [preset, setPreset] = useState("custom");
  const [cfg, setCfg] = useState<ImapConfig>({
    imapHost: "",
    imapPort: 993,
    imapSecurity: "tls",
    smtpHost: "",
    smtpPort: 587,
    smtpSecurity: "starttls",
    username: "",
    auth: "password",
  });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    if (preset !== "custom") return;
    const p = PRESETS.find((x) => x.match.test(email.trim().toLowerCase()));
    if (p) {
      setCfg((c) => ({ ...c, ...p.cfg }));
    } else if (email.includes("@") && !cfg.imapHost) {
      const domain = email.split("@")[1];
      setCfg((c) => ({ ...c, imapHost: `imap.${domain}`, smtpHost: `smtp.${domain}` }));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [email]);

  const applyPreset = (id: string) => {
    setPreset(id);
    const p = PRESETS.find((x) => x.id === id);
    if (p) setCfg((c) => ({ ...c, auth: "password", ...p.cfg }));
  };

  const oauth = cfg.auth === "microsoftOAuth";

  const submit = async () => {
    setBusy(true);
    setError("");
    try {
      const a = await api.addImapAccount({
        email,
        name,
        label: label || undefined,
        config: { ...cfg, username: cfg.username || email },
        password: oauth ? undefined : password,
      });
      await reloadMeta();
      toast(`Added ${a.email}`, { kind: "success" });
      onDone();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const sec = [
    { value: "tls", label: "SSL/TLS" },
    { value: "starttls", label: "STARTTLS" },
    { value: "none", label: "None" },
  ];

  return (
    <div className="flex flex-col gap-4 rounded-lg border border-line-strong p-4">
      <Field label="Provider preset">
        <Select
          value={preset}
          onChange={applyPreset}
          options={[{ value: "custom", label: "Detect from email / custom" }, ...PRESETS.map((p) => ({ value: p.id, label: p.label }))]}
        />
      </Field>
      {oauth && (
        <p className="rounded-md bg-hover px-3 py-2 text-xs text-fg-2">
          Signs in with Microsoft and uses IMAP/SMTP with OAuth. Use this when your school or work tenant blocks third-party Graph access. Requires
          the Microsoft client ID under Connections, with the IMAP and SMTP permissions added.
        </p>
      )}
      <div className="grid grid-cols-2 gap-3">
        <Field label="Email address">
          <Input value={email} onChange={(e) => setEmail(e.target.value)} placeholder="you@example.com" autoFocus />
        </Field>
        <Field label="Your name">
          <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="Deniz" />
        </Field>
        {!oauth && (
          <Field label="Password" hint="Many providers require an app password.">
            <Input type="password" value={password} onChange={(e) => setPassword(e.target.value)} />
          </Field>
        )}
        <Field label="Label (optional)">
          <Input value={label} onChange={(e) => setLabel(e.target.value)} placeholder="Work" />
        </Field>
      </div>
      <details className="group">
        <summary className="cursor-default text-xs text-fg-3 hover:text-fg-2">Server settings</summary>
        <div className="mt-3 grid grid-cols-[1fr_90px_120px] gap-3">
          <Field label="IMAP server">
            <Input value={cfg.imapHost} onChange={(e) => setCfg({ ...cfg, imapHost: e.target.value })} />
          </Field>
          <Field label="Port">
            <Input value={cfg.imapPort} onChange={(e) => setCfg({ ...cfg, imapPort: Number(e.target.value) || 0 })} />
          </Field>
          <Field label="Security">
            <Select value={cfg.imapSecurity} onChange={(v) => setCfg({ ...cfg, imapSecurity: v as ImapConfig["imapSecurity"] })} options={sec} />
          </Field>
          <Field label="SMTP server">
            <Input value={cfg.smtpHost} onChange={(e) => setCfg({ ...cfg, smtpHost: e.target.value })} />
          </Field>
          <Field label="Port">
            <Input value={cfg.smtpPort} onChange={(e) => setCfg({ ...cfg, smtpPort: Number(e.target.value) || 0 })} />
          </Field>
          <Field label="Security">
            <Select value={cfg.smtpSecurity} onChange={(v) => setCfg({ ...cfg, smtpSecurity: v as ImapConfig["smtpSecurity"] })} options={sec} />
          </Field>
          <Field label="Username" hint="Defaults to the email address.">
            <Input value={cfg.username} onChange={(e) => setCfg({ ...cfg, username: e.target.value })} placeholder={email} />
          </Field>
        </div>
      </details>
      {error && <div className="rounded-md border border-danger/40 bg-danger/10 px-3 py-2 text-xs text-danger selectable">{error}</div>}
      <div className="flex gap-2">
        <Button variant="primary" onClick={submit} disabled={busy || (!oauth && (!email || !password))}>
          {busy && <Spinner size={12} className="text-white" />}
          {oauth ? "Sign in with Microsoft" : busy ? "Testing connection…" : "Add account"}
        </Button>
        <Button variant="ghost" onClick={onDone}>
          Cancel
        </Button>
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ splits

function Splits() {
  const splits = useStore((s) => s.splits);
  const reloadMeta = useStore((s) => s.reloadMeta);
  const toast = useStore((s) => s.toast);
  const accounts = useStore((s) => s.accounts);
  const [list, setList] = useState<Split[]>(splits);
  const dirty = JSON.stringify(list) !== JSON.stringify(splits);

  const save = async () => {
    await api.setSplits(list.filter((s) => s.name.trim()));
    await reloadMeta();
    useStore.getState().refresh();
    useStore.getState().refreshCounts();
    toast("Splits saved", { kind: "success" });
  };
  const move = (i: number, d: number) => {
    const n = [...list];
    const [x] = n.splice(i, 1);
    n.splice(i + d, 0, x);
    setList(n);
  };

  return (
    <>
      <H
        title="Split inbox"
        desc="Splits divide your inbox into focused tabs. Each one is a saved search; press Tab to cycle through them."
      />
      <div className="flex flex-col gap-2">
        {list.map((s, i) => (
          <div key={s.id} className="flex items-center gap-2">
            <Input className="w-40" value={s.name} onChange={(e) => setList(list.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)))} />
            <Input
              className="flex-1 font-mono text-xs"
              value={s.query}
              onChange={(e) => setList(list.map((x, j) => (j === i ? { ...x, query: e.target.value } : x)))}
            />
            <IconButton label="Move up" disabled={i === 0} onClick={() => move(i, -1)}>
              <ArrowUp size={13} />
            </IconButton>
            <IconButton label="Move down" disabled={i === list.length - 1} onClick={() => move(i, 1)}>
              <ArrowDown size={13} />
            </IconButton>
            <IconButton label="Delete" onClick={() => setList(list.filter((_, j) => j !== i))}>
              <Trash2 size={13} />
            </IconButton>
          </div>
        ))}
      </div>
      <div className="mt-3 flex flex-wrap gap-2">
        <Button icon={<Plus size={13} />} onClick={() => setList([...list, { id: crypto.randomUUID().slice(0, 8), name: "New split", query: "" }])}>
          Add split
        </Button>
        {accounts.map((a) => (
          <Button
            key={a.id}
            variant="ghost"
            icon={<Plus size={13} />}
            onClick={() =>
              setList([...list, { id: crypto.randomUUID().slice(0, 8), name: a.label || a.email.split("@")[0], query: `account:${a.email}` }])
            }
          >
            {a.label || a.email}
          </Button>
        ))}
      </div>
      {dirty && (
        <div className="mt-5 flex gap-2">
          <Button variant="primary" onClick={save}>
            Save splits
          </Button>
          <Button variant="ghost" onClick={() => setList(splits)}>
            Reset
          </Button>
        </div>
      )}
      <div className="mt-8 rounded-lg border border-line bg-hover p-4 text-xs leading-relaxed text-fg-2">
        <div className="mb-2 font-medium text-fg">Query syntax</div>
        <code className="font-mono">category:primary</code> · <code className="font-mono">category:newsletter</code> ·{" "}
        <code className="font-mono">category:notification</code> · <code className="font-mono">from:github.com</code> ·{" "}
        <code className="font-mono">to:team@</code> · <code className="font-mono">account:school</code> ·{" "}
        <code className="font-mono">is:unread</code> · <code className="font-mono">is:starred</code> ·{" "}
        <code className="font-mono">has:attachment</code> · <code className="font-mono">subject:invoice</code> · free text. Prefix with{" "}
        <code className="font-mono">-</code> to negate, e.g. <code className="font-mono">-from:noreply</code>.
      </div>
    </>
  );
}

// ------------------------------------------------------------------ AI

function AiSettings() {
  const ai = useStore((s) => s.ai);
  const settings = useStore((s) => s.settings);
  const setSetting = useStore((s) => s.setSetting);
  const reloadMeta = useStore((s) => s.reloadMeta);
  const toast = useStore((s) => s.toast);
  const [busy, setBusy] = useState(false);
  const [models, setModels] = useState<ModelInfo[]>([]);

  useEffect(() => {
    if (ai?.connected) api.aiModels().then(setModels).catch((e) => toast(errorMessage(e), { kind: "error" }));
  }, [ai?.connected, toast]);

  const connect = async () => {
    setBusy(true);
    try {
      await api.aiLogin();
      await reloadMeta();
      toast("ChatGPT connected", { kind: "success" });
    } catch (e) {
      const m = errorMessage(e);
      if (!m.includes("cancelled")) toast(m, { kind: "error", duration: 8000 });
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <H title="AI" desc="Thread summaries and writing help, powered by your ChatGPT subscription." />
      <div className="mb-6 flex items-center gap-4 rounded-lg border border-line-strong p-4">
        <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-accent-soft">
          <Sparkles size={18} className="text-accent" />
        </div>
        <div className="flex-1">
          <div className="text-[13px] font-medium">{ai?.connected ? "ChatGPT connected" : "Connect ChatGPT"}</div>
          <div className="text-xs text-fg-3">
            {ai?.connected
              ? `${ai.email ?? "Signed in"}${ai.plan ? ` · ${ai.plan} plan` : ""}`
              : "Sign in with your ChatGPT account. No API key needed."}
          </div>
        </div>
        {ai?.connected ? (
          <Button
            variant="ghost"
            onClick={async () => {
              await api.aiLogout();
              await reloadMeta();
            }}
          >
            Disconnect
          </Button>
        ) : (
          <>
            {busy && (
              <Button variant="ghost" onClick={() => api.cancelOAuth()}>
                Cancel
              </Button>
            )}
            <Button variant="primary" onClick={connect} disabled={busy}>
              {busy ? "Waiting for browser…" : "Connect"}
            </Button>
          </>
        )}
      </div>
      {ai?.connected && (
        <>
          <Row label="Model" desc="Faster models make summaries feel instant.">
            <Select
              value={settings.ai_model ?? ""}
              onChange={(v) => setSetting("ai_model", v)}
              options={[{ value: "", label: "Automatic" }, ...models.map((m) => ({ value: m.slug, label: m.name }))]}
            />
          </Row>
          <Row label="Reasoning effort">
            <Select
              value={settings.ai_effort ?? "low"}
              onChange={(v) => setSetting("ai_effort", v)}
              options={["low", "medium", "high"].map((v) => ({ value: v, label: v[0].toUpperCase() + v.slice(1) }))}
            />
          </Row>
          <Row label="Summarize threads automatically" desc="When you open a long conversation. Summaries are cached locally.">
            <Switch checked={settings.ai_auto_summary !== "false"} onChange={(v) => setSetting("ai_auto_summary", String(v))} />
          </Row>
          <Row label="Show summaries in the list" desc="Replace the preview text with the AI TL;DR once a thread has one.">
            <Switch checked={settings.ai_list_summaries !== "false"} onChange={(v) => setSetting("ai_list_summaries", String(v))} />
          </Row>
        </>
      )}
      <p className="mt-6 text-xs leading-relaxed text-fg-3">
        This uses the same sign-in as OpenAI's Codex CLI. It's unofficial: OpenAI may change or limit it, and usage counts toward your plan's
        limits. Only the conversation you summarize or reply to is sent.
      </p>
    </>
  );
}

// ------------------------------------------------------------------ connections

function Connections() {
  const settings = useStore((s) => s.settings);
  const setSetting = useStore((s) => s.setSetting);
  const reloadMeta = useStore((s) => s.reloadMeta);
  const toast = useStore((s) => s.toast);
  const [g, setG] = useState({ id: settings.google_client_id ?? "", secret: settings.google_client_secret ?? "" });
  const [m, setM] = useState({ id: settings.microsoft_client_id ?? "", tenant: settings.microsoft_tenant ?? "common" });

  const save = async () => {
    await setSetting("google_client_id", g.id.trim());
    await setSetting("google_client_secret", g.secret.trim());
    await setSetting("microsoft_client_id", m.id.trim());
    await setSetting("microsoft_tenant", m.tenant.trim() || "common");
    await reloadMeta();
    toast("Connections saved", { kind: "success" });
  };

  return (
    <>
      <H
        title="Connections"
        desc="Gmail and Outlook sign-in need OAuth app registrations that you own. They stay on this device."
      />
      <section className="mb-8 flex flex-col gap-3">
        <div className="flex items-center gap-2 text-[13px] font-medium">
          <ProviderGlyph kind="gmail" /> Google
        </div>
        <Field label="Client ID" hint="Google Cloud Console → APIs & Services → Credentials → OAuth client ID → Desktop app.">
          <Input value={g.id} onChange={(e) => setG({ ...g, id: e.target.value })} placeholder="1234-abc.apps.googleusercontent.com" />
        </Field>
        <Field label="Client secret" hint="Desktop clients have a secret too; Google doesn't treat it as confidential.">
          <Input type="password" value={g.secret} onChange={(e) => setG({ ...g, secret: e.target.value })} />
        </Field>
        <button className="self-start text-xs text-accent hover:underline" onClick={() => openUrl("https://console.cloud.google.com/apis/credentials")}>
          Open Google Cloud Console
        </button>
      </section>
      <section className="flex flex-col gap-3">
        <div className="flex items-center gap-2 text-[13px] font-medium">
          <ProviderGlyph kind="outlook" /> Microsoft
        </div>
        <Field label="Application (client) ID" hint="Entra admin center → App registrations → New. Platform: Mobile and desktop, redirect URI http://localhost.">
          <Input value={m.id} onChange={(e) => setM({ ...m, id: e.target.value })} placeholder="00000000-0000-0000-0000-000000000000" />
        </Field>
        <Field label="Tenant" hint="“common” works for personal and school accounts.">
          <Input value={m.tenant} onChange={(e) => setM({ ...m, tenant: e.target.value })} />
        </Field>
        <button
          className="self-start text-xs text-accent hover:underline"
          onClick={() => openUrl("https://entra.microsoft.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade")}
        >
          Open Entra app registrations
        </button>
      </section>
      <div className="mt-6">
        <Button variant="primary" onClick={save}>
          Save
        </Button>
      </div>
    </>
  );
}
