import { useStore } from "../store";
import { Button, Kbd, ProviderGlyph } from "./ui";

export function Onboarding() {
  const set = useStore((s) => s.set);
  return (
    <div className="flex h-full items-center justify-center">
      <div className="flex w-[440px] flex-col items-center text-center animate-in">
        <div className="relative mb-6">
          <div className="absolute inset-0 rounded-2xl bg-accent/25 blur-2xl" />
          <div className="relative flex h-14 w-14 items-center justify-center rounded-2xl bg-gradient-to-br from-[#7c86e6] to-[#4f5bc4] text-2xl font-bold text-white shadow-[inset_0_1px_0_rgb(255_255_255/0.25)]">
            C
          </div>
        </div>
        <h1 className="text-[22px] font-semibold tracking-tight">Welcome to CigMail</h1>
        <p className="mt-2 text-[13.5px] leading-relaxed text-fg-2">
          One fast, keyboard-first inbox for all your accounts. Connect Gmail, Outlook and any IMAP mailbox to get started.
        </p>
        <div className="mt-6 flex gap-2">
          <Button variant="primary" onClick={() => set({ settingsSection: "accounts" })}>
            Add an account
          </Button>
          <Button variant="ghost" onClick={() => set({ settingsSection: "connections" })}>
            Set up connections
          </Button>
        </div>
        <div className="mt-6 flex items-center gap-3 text-fg-3">
          <ProviderGlyph kind="gmail" size={18} />
          <ProviderGlyph kind="outlook" size={18} />
          <ProviderGlyph kind="imap" size={18} />
        </div>
        <div className="mt-10 flex items-center gap-2 text-xs text-fg-4">
          Press <Kbd keys={["Mod", "K"]} /> anytime for every command
        </div>
      </div>
    </div>
  );
}
