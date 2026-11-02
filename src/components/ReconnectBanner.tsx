import { useState } from "react";
import { AlertCircle } from "lucide-react";
import { useStore } from "../store";
import { api, errorMessage } from "../lib/api";
import { accountLabel } from "../lib/format";
import { Button, Spinner } from "./ui";

/** Shown when an account's sign-in expired (e.g. Google "Testing" apps after 7 days). */
export function ReconnectBanner() {
  const accounts = useStore((s) => s.accounts);
  const sync = useStore((s) => s.sync);
  const reloadMeta = useStore((s) => s.reloadMeta);
  const set = useStore((s) => s.set);
  const toast = useStore((s) => s.toast);
  const [busy, setBusy] = useState<string | null>(null);

  const expired = accounts.filter((a) =>
    sync[a.id] ? sync[a.id].state === "auth" : !!a.lastError?.startsWith("Sign-in expired"),
  );
  if (!expired.length) return null;

  const reconnect = async (id: string, kind: string) => {
    // Password accounts need a new password, which lives in Settings.
    if (kind === "imap") {
      set({ settingsSection: "accounts" });
      return;
    }
    setBusy(id);
    try {
      await api.reconnectAccount(id);
      await reloadMeta();
      useStore.setState((s) => ({ sync: { ...s.sync, [id]: { accountId: id, state: "syncing", message: null, progress: null } } }));
      toast("Reconnected", { kind: "success" });
    } catch (e) {
      const m = errorMessage(e);
      if (!m.includes("cancelled")) toast(m, { kind: "error", duration: 8000 });
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="flex flex-col border-b border-line">
      {expired.map((a) => (
        <div key={a.id} className="flex h-10 items-center gap-2.5 bg-warning/10 px-4 text-[13px]">
          <AlertCircle size={14} className="shrink-0 text-warning" />
          <span className="h-2 w-2 shrink-0 rounded-full" style={{ background: a.color }} />
          <span className="min-w-0 flex-1 truncate text-fg-2">
            <span className="font-medium text-fg">{accountLabel(a)}</span> needs you to sign in again. Mail from this account isn't syncing.
          </span>
          {busy === a.id && (
            <Button size="sm" variant="ghost" onClick={() => api.cancelOAuth()}>
              Cancel
            </Button>
          )}
          <Button size="sm" variant="primary" disabled={busy === a.id} onClick={() => reconnect(a.id, a.kind)}>
            {busy === a.id ? <Spinner size={11} className="text-white" /> : null}
            {busy === a.id ? "Waiting for browser…" : "Reconnect"}
          </Button>
        </div>
      ))}
    </div>
  );
}
