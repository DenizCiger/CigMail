import { useEffect, useState } from "react";
import { isPermissionGranted, requestPermission } from "@tauri-apps/plugin-notification";
import { useStore } from "../store";
import { useTheme } from "../App";
import { Button, ProviderGlyph } from "../components/ui";
import { Composer } from "../components/Composer";
import { Settings } from "../components/Settings";
import { Toasts } from "../components/Toasts";
import { ContextMenu } from "../components/ContextMenu";
import { InboxScreen } from "./Inbox";
import { ThreadScreen } from "./Thread";
import { Drawer } from "./Drawer";
import { SnoozeSheet } from "./Sheets";
import { useBackLayer } from "./back";

function Welcome() {
  const set = useStore((s) => s.set);
  return (
    <div className="flex h-full flex-col items-center justify-center bg-panel px-8 pb-[env(safe-area-inset-bottom)] pt-[env(safe-area-inset-top)] text-center animate-in">
      <div className="relative mb-6">
        <div className="absolute inset-0 rounded-2xl bg-accent/25 blur-2xl" />
        <div className="relative flex h-16 w-16 items-center justify-center rounded-2xl bg-gradient-to-br from-[#7c86e6] to-[#4f5bc4] text-3xl font-bold text-white shadow-[inset_0_1px_0_rgb(255_255_255/0.25)]">
          C
        </div>
      </div>
      <h1 className="text-[24px] font-semibold tracking-tight">Welcome to CigMail</h1>
      <p className="mt-2 text-[15px] leading-relaxed text-fg-2">One fast inbox for all your accounts. Connect Gmail, Outlook and any IMAP mailbox to get started.</p>
      <Button variant="primary" className="mt-8 h-12 w-full max-w-[320px] rounded-xl text-[15px]" onClick={() => set({ settingsSection: "accounts" })}>
        Add an account
      </Button>
      <div className="mt-6 flex items-center gap-4 text-fg-3">
        <ProviderGlyph kind="gmail" size={22} />
        <ProviderGlyph kind="outlook" size={22} />
        <ProviderGlyph kind="imap" size={22} />
      </div>
    </div>
  );
}

/** Android 13+ asks before an app may post notifications. */
async function askForNotifications() {
  try {
    if (!(await isPermissionGranted())) await requestPermission();
  } catch {
    /* not available outside the app (e.g. the browser demo) */
  }
}

export default function MobileApp() {
  const ready = useStore((s) => s.ready);
  const init = useStore((s) => s.init);
  const accounts = useStore((s) => s.accounts);
  const openThreadId = useStore((s) => s.openThreadId);
  const composer = useStore((s) => s.composer);
  const closeThread = useStore((s) => s.closeThread);
  const [drawer, setDrawer] = useState(false);
  useTheme();

  useEffect(() => {
    init();
  }, [init]);

  useEffect(() => {
    if (ready && accounts.length) askForNotifications();
  }, [ready, accounts.length]);

  useBackLayer(!!openThreadId, closeThread);

  if (!ready) return <div className="h-full bg-panel" />;

  return (
    <div className="relative h-full overflow-hidden bg-panel text-fg">
      {accounts.length === 0 ? (
        <Welcome />
      ) : (
        <>
          {/* The list stays mounted under an open thread so its scroll position survives. */}
          <div className="absolute inset-0" aria-hidden={!!openThreadId} style={{ visibility: openThreadId ? "hidden" : undefined }}>
            <InboxScreen onMenu={() => setDrawer(true)} />
          </div>
          {openThreadId && (
            <div className="absolute inset-0 z-10">
              <ThreadScreen key={openThreadId} id={openThreadId} />
            </div>
          )}
        </>
      )}
      <Drawer open={drawer} onClose={() => setDrawer(false)} />
      {composer && <Composer key={composer.key} />}
      <SnoozeSheet />
      <Settings />
      <Toasts />
      <ContextMenu />
    </div>
  );
}
