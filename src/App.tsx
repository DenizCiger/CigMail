import { useEffect, useState } from "react";
import { FileText, Trash2 } from "lucide-react";
import { useStore } from "./store";
import { handleGlobalKey } from "./lib/commands";
import { api } from "./lib/api";
import type { Draft } from "./lib/types";
import { Sidebar } from "./components/Sidebar";
import { Header } from "./components/Header";
import { ThreadList } from "./components/ThreadList";
import { ThreadView } from "./components/ThreadView";
import { Composer } from "./components/Composer";
import { CommandPalette } from "./components/CommandPalette";
import { SnoozePicker } from "./components/SnoozePicker";
import { Settings } from "./components/Settings";
import { HelpOverlay } from "./components/HelpOverlay";
import { Toasts } from "./components/Toasts";
import { ContextMenu } from "./components/ContextMenu";
import { Onboarding } from "./components/Onboarding";
import { ReconnectBanner } from "./components/ReconnectBanner";
import { IconButton } from "./components/ui";

export function useTheme() {
  const theme = useStore((s) => s.settings.theme ?? "system");
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const effective = theme === "system" ? (mq.matches ? "dark" : "light") : theme;
      document.documentElement.dataset.theme = effective;
    };
    apply();
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme]);
}

/** Locally saved drafts, shown above server drafts in the Drafts view. */
function LocalDrafts() {
  const compose = useStore((s) => s.compose);
  const accounts = useStore((s) => s.accounts);
  const [drafts, setDrafts] = useState<Draft[]>([]);
  const composer = useStore((s) => s.composer);
  useEffect(() => {
    api.listDrafts().then(setDrafts).catch(() => {});
  }, [composer]);
  if (!drafts.length) return null;
  return (
    <div className="border-b border-line py-1">
      {drafts.map((d) => {
        const a = accounts.find((x) => x.id === d.accountId);
        return (
          <div
            key={d.id}
            onClick={() => compose(d)}
            className="group flex h-10 items-center gap-3 px-6 text-[13px] hover:bg-hover"
          >
            <FileText size={14} className="text-fg-3" />
            {a && <span className="h-3.5 w-[3px] rounded-full" style={{ background: a.color }} />}
            <span className="w-[188px] truncate text-danger/90">
              Draft{d.to.length ? ` · ${d.to.map((x) => x.name || x.email).join(", ")}` : ""}
            </span>
            <span className="flex-1 truncate">{d.subject || "(no subject)"}</span>
            <IconButton
              label="Delete draft"
              className="opacity-0 group-hover:opacity-100"
              onClick={async (e) => {
                e.stopPropagation();
                await api.deleteDraft(d.id!);
                setDrafts((x) => x.filter((y) => y.id !== d.id));
              }}
            >
              <Trash2 size={13} />
            </IconButton>
          </div>
        );
      })}
    </div>
  );
}

export default function App() {
  const ready = useStore((s) => s.ready);
  const init = useStore((s) => s.init);
  const accounts = useStore((s) => s.accounts);
  const openThreadId = useStore((s) => s.openThreadId);
  const composer = useStore((s) => s.composer);
  const view = useStore((s) => s.view);
  const previewPane = useStore((s) => s.previewPane);
  const previewId = useStore((s) => s.threads[s.cursor]?.id ?? null);
  useTheme();

  useEffect(() => {
    init();
  }, [init]);

  useEffect(() => {
    window.addEventListener("keydown", handleGlobalKey);
    return () => window.removeEventListener("keydown", handleGlobalKey);
  }, []);

  if (!ready) return <div className="h-full bg-bg" />;

  return (
    <div
      className="flex h-full bg-bg text-fg"
      onContextMenu={(e) => {
        // Native menu only where it's useful: text fields and selected text.
        const t = e.target as HTMLElement;
        if (t.closest("input, textarea, [contenteditable='true']") || window.getSelection()?.toString()) return;
        e.preventDefault();
      }}
    >
      <Sidebar />
      <main className="my-2 mr-2 flex min-w-0 flex-1 flex-col overflow-hidden rounded-lg border border-line bg-panel shadow-[0_1px_2px_rgb(0_0_0/0.1)] first:ml-2">
        <ReconnectBanner />
        {accounts.length === 0 ? (
          <Onboarding />
        ) : openThreadId ? (
          <ThreadView id={openThreadId} />
        ) : (
          <>
            <Header />
            {view === "drafts" && <LocalDrafts />}
            {previewPane ? (
              <div className="flex min-h-0 flex-1">
                <div className="min-h-0 w-[38%] min-w-[290px] max-w-[520px] shrink-0 border-r border-line">
                  <ThreadList compact />
                </div>
                <div className="min-w-0 flex-1">
                  {previewId ? (
                    <ThreadView key={previewId} id={previewId} preview />
                  ) : (
                    <div className="flex h-full items-center justify-center text-[13px] text-fg-4">No conversation selected</div>
                  )}
                </div>
              </div>
            ) : (
              <div className="min-h-0 flex-1">
                <ThreadList />
              </div>
            )}
          </>
        )}
      </main>
      {composer && !composer.inline && <Composer key={composer.key} />}
      <CommandPalette />
      <SnoozePicker />
      <Settings />
      <HelpOverlay />
      <Toasts />
      <ContextMenu />
    </div>
  );
}
