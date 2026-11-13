import clsx from "clsx";
import { useState } from "react";
import { ChevronDown, Copy, PenLine, Search } from "lucide-react";
import { useStore } from "../store";
import { isMobile } from "../lib/platform";
import { displayName, formatAddress, longDate } from "../lib/format";
import { writeText } from "../lib/clipboard";
import type { Address, Message } from "../lib/types";
import { ActionSheet, type SheetAction } from "../mobile/Sheets";
import { Avatar } from "./ui";

function addressActions(a: Address): SheetAction[] {
  const { toast, compose, set, setQuery } = useStore.getState();
  const who = a.name?.trim() || a.email;
  const actions: SheetAction[] = [
    { label: `Copy ${a.email}`, icon: Copy, run: () => writeText(a.email).then(() => toast("Copied")) },
  ];
  if (a.name?.trim()) {
    actions.push({ label: "Copy name and address", icon: Copy, run: () => writeText(formatAddress(a)).then(() => toast("Copied")) });
  }
  actions.push(
    { label: `New message to ${who}`, icon: PenLine, run: () => compose({ to: [{ name: a.name, email: a.email }] }) },
    {
      label: `Find emails with ${who}`,
      icon: Search,
      run: () => {
        set({ searchOpen: true });
        setQuery(`from:${a.email} OR to:${a.email}`);
      },
    },
  );
  return actions;
}

/**
 * A person's name that opens their address and actions: a menu under the chip on
 * desktop, a bottom sheet on mobile. Clicks never reach the surrounding card, so a
 * name inside a collapsible header doesn't collapse it.
 */
export function AddressChip({ a, me, className, label, wrap }: { a: Address; me: string[]; className?: string; label?: string; wrap?: boolean }) {
  const openMenu = useStore((s) => s.openMenu);
  const [sheet, setSheet] = useState<SheetAction[] | null>(null);
  return (
    <>
      <button
        type="button"
        title={formatAddress(a)}
        onClick={(e) => {
          e.stopPropagation();
          const actions = addressActions(a);
          if (isMobile) {
            setSheet(actions);
            return;
          }
          const r = e.currentTarget.getBoundingClientRect();
          openMenu(r.left, r.bottom + 4, actions);
        }}
        onContextMenu={(e) => e.stopPropagation()}
        className={clsx("max-w-full rounded-[4px] text-left", wrap ? "break-all" : "truncate", "hover:text-fg hover:underline hover:decoration-fg-4 hover:underline-offset-2", className)}
      >
        {label ?? displayName(a, me)}
      </button>
      {isMobile && (
        <ActionSheet
          actions={sheet}
          onClose={() => setSheet(null)}
          title={
            <span className="flex items-center gap-3 pb-1 text-fg">
              <Avatar address={a} size={36} />
              <span className="min-w-0">
                {a.name?.trim() && <span className="block truncate text-[15px] font-semibold">{a.name.trim()}</span>}
                <span className="selectable block break-all text-[13.5px] text-fg-2">{a.email}</span>
              </span>
            </span>
          }
        />
      )}
    </>
  );
}

function ChipList({ list, me }: { list: Address[]; me: string[] }) {
  return (
    <>
      {list.map((a, i) => (
        <span key={`${a.email}-${i}`} className="min-w-0">
          <AddressChip a={a} me={me} />
          {i < list.length - 1 && ", "}
        </span>
      ))}
    </>
  );
}

/**
 * "to Alice, Bob ▾" under the sender: each name opens that person, the chevron
 * shows the full header details. The names truncate; the chevron always stays visible.
 */
export function RecipientsLine({
  m,
  me,
  open,
  onToggle,
  className,
}: {
  m: Message;
  me: string[];
  open: boolean;
  onToggle: () => void;
  className?: string;
}) {
  const list = [...m.to, ...m.cc, ...m.bcc];
  return (
    <div className={clsx("flex min-w-0 items-center gap-1 text-fg-3", className)}>
      {list.length > 0 && (
        <span className="flex min-w-0 items-center gap-1 overflow-hidden whitespace-nowrap">
          <span className="shrink-0">to</span>
          <ChipList list={list} me={me} />
        </span>
      )}
      <button
        type="button"
        aria-label={open ? "Hide details" : "Show details"}
        title={open ? "Hide details" : "Show details"}
        aria-expanded={open}
        onClick={(e) => {
          e.stopPropagation();
          onToggle();
        }}
        className={clsx("flex shrink-0 items-center rounded px-0.5 hover:bg-hover hover:text-fg-2", isMobile && "-my-2 px-2 py-2")}
      >
        {list.length === 0 && <span className="mr-0.5">details</span>}
        <ChevronDown size={isMobile ? 15 : 13} className={clsx("transition-transform", open && "rotate-180")} />
      </button>
    </div>
  );
}

/** Every header field with full, selectable addresses. */
export function MessageDetails({ m, me }: { m: Message; me: string[] }) {
  const rows: [string, Address[]][] = [
    ["From", [m.from]],
    ["Reply-To", m.replyTo.filter((r) => r.email.toLowerCase() !== m.from.email.toLowerCase())],
    ["To", m.to],
    ["Cc", m.cc],
    ["Bcc", m.bcc],
  ];
  return (
    <div
      onClick={(e) => e.stopPropagation()}
      // Long-press here selects text instead of opening the message menu.
      onContextMenu={(e) => e.stopPropagation()}
      className={clsx(
        "mt-2 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1.5 rounded-lg border border-line bg-elev-2 px-3 py-2.5",
        isMobile ? "text-[13px]" : "text-xs",
      )}
    >
      {rows
        .filter(([, list]) => list.length > 0)
        .map(([label, list]) => (
          <div key={label} className="contents">
            <span className="text-fg-3">{label}</span>
            <div className="min-w-0 space-y-0.5">
              {list.map((a, i) => (
                <div key={`${a.email}-${i}`} className="flex min-w-0 flex-wrap items-baseline gap-x-1.5">
                  {a.name?.trim() ? (
                    <>
                      <AddressChip a={a} me={me} className="text-fg" label={a.name.trim()} />
                      <span className="selectable min-w-0 break-all text-fg-2">{a.email}</span>
                    </>
                  ) : (
                    <AddressChip a={a} me={me} wrap className="text-fg-2" label={a.email} />
                  )}
                </div>
              ))}
            </div>
          </div>
        ))}
      <span className="text-fg-3">Date</span>
      <span className="selectable text-fg-2">{longDate(m.date)}</span>
    </div>
  );
}

/** True when the user just selected text, so the click shouldn't toggle anything. */
export function selectingText(): boolean {
  return !!window.getSelection()?.toString();
}
