import { useStore } from "../store";
import { allCommands } from "../lib/commands";
import { Kbd, Modal, ModalHeader } from "./ui";

const EXTRA: { section: string; title: string; keys: string[] }[] = [
  { section: "Navigation", title: "Next / previous conversation", keys: ["J", "K"] },
  { section: "Navigation", title: "Open conversation", keys: ["Enter"] },
  { section: "Navigation", title: "Switch to split 1–9", keys: ["1", "…", "9"] },
  { section: "Navigation", title: "Show account 1–9", keys: ["G", "1", "…", "9"] },
  { section: "Compose", title: "Send", keys: ["Mod", "Enter"] },
  { section: "Compose", title: "Show Cc / Bcc", keys: ["Mod", "Shift", "C"] },
  { section: "Compose", title: "Write with AI", keys: ["Mod", "J"] },
  { section: "Compose", title: "Attach files", keys: ["Mod", "Shift", "A"] },
  { section: "Compose", title: "Close & save draft", keys: ["Esc"] },
];

export function HelpOverlay() {
  const open = useStore((s) => s.help);
  const set = useStore((s) => s.set);
  if (!open) return null;
  const cmds = allCommands().filter((c) => c.keys && !c.hidden && !/^(split|account):\d/.test(c.id));
  const rows = [...EXTRA, ...cmds.map((c) => ({ section: c.section, title: c.title, keys: c.keys! }))];
  const sections = ["Navigation", "Conversation", "Accounts", "Compose", "App"];
  return (
    <Modal open onClose={() => set({ help: false })} className="w-[760px]">
      <ModalHeader title="Keyboard shortcuts" onClose={() => set({ help: false })} />
      <div className="grid max-h-[70vh] grid-cols-2 gap-x-8 gap-y-6 overflow-y-auto p-5">
        {sections.map((s) => (
          <div key={s}>
            <div className="mb-2 text-[11.5px] font-medium text-fg-3">{s}</div>
            <div className="flex flex-col">
              {rows
                .filter((r) => r.section === s)
                .map((r) => (
                  <div key={r.title} className="flex h-8 items-center border-b border-line text-[13px]">
                    <span className="flex-1 text-fg-2">{r.title}</span>
                    <Kbd keys={r.keys} />
                  </div>
                ))}
            </div>
          </div>
        ))}
      </div>
    </Modal>
  );
}
