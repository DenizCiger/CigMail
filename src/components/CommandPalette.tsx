import { useEffect, useMemo, useState } from "react";
import { Command } from "cmdk";
import { FileText, Mail, Search } from "lucide-react";
import { useStore } from "../store";
import { allCommands } from "../lib/commands";
import { api } from "../lib/api";
import { displayName, shortDate } from "../lib/format";
import type { Draft, ThreadSummary } from "../lib/types";
import { Kbd } from "./ui";

const SECTIONS = ["Conversation", "Navigation", "Compose", "Accounts", "App"] as const;

export function CommandPalette() {
  const open = useStore((s) => s.palette);
  const set = useStore((s) => s.set);
  const openThread = useStore((s) => s.openThread);
  const setQuery = useStore((s) => s.setQuery);
  const compose = useStore((s) => s.compose);
  const accounts = useStore((s) => s.accounts);
  const [search, setSearch] = useState("");
  const [hits, setHits] = useState<ThreadSummary[]>([]);
  const [drafts, setDrafts] = useState<Draft[]>([]);

  const commands = useMemo(() => (open ? allCommands().filter((c) => !c.hidden && (!c.enabled || c.enabled())) : []), [open]);

  useEffect(() => {
    if (!open) {
      setSearch("");
      setHits([]);
      return;
    }
    api.listDrafts().then(setDrafts).catch(() => setDrafts([]));
  }, [open]);

  useEffect(() => {
    if (search.trim().length < 2) {
      setHits([]);
      return;
    }
    const t = window.setTimeout(async () => {
      const res = await api.listThreads({ view: "search", query: search, limit: 6 }).catch(() => []);
      setHits(res);
    }, 100);
    return () => window.clearTimeout(t);
  }, [search]);

  if (!open) return null;
  const close = () => set({ palette: false });
  const run = (fn: () => void) => {
    close();
    window.setTimeout(fn, 0);
  };
  const me = accounts.map((a) => a.email);

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/40 pt-[13vh] animate-fade"
      onMouseDown={(e) => e.target === e.currentTarget && close()}
    >
      <Command
        label="Command palette"
        loop
        className="w-[640px] overflow-hidden rounded-xl border border-line-strong bg-elev shadow-pop animate-in"
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            close();
          }
        }}
      >
        <div className="flex items-center gap-2.5 border-b border-line px-4">
          <Search size={15} className="text-fg-3" />
          <Command.Input
            autoFocus
            value={search}
            onValueChange={setSearch}
            placeholder="Type a command or search mail…"
            className="h-12 flex-1 bg-transparent text-[14px] placeholder:text-fg-4"
          />
          <Kbd keys={["Esc"]} />
        </div>
        <Command.List className="max-h-[420px] overflow-y-auto p-1.5 [&_[cmdk-group-heading]]:px-2.5 [&_[cmdk-group-heading]]:pb-1 [&_[cmdk-group-heading]]:pt-2.5 [&_[cmdk-group-heading]]:text-[11px] [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:text-fg-3">
          <Command.Empty className="px-3 py-8 text-center text-[13px] text-fg-3">No results.</Command.Empty>

          {search.trim().length >= 2 && (
            <Command.Group heading="Mail">
              <Item value={`search-all ${search}`} onSelect={() => run(() => setQuery(search))} icon={<Search size={15} />}>
                Search all mail for “{search}”
              </Item>
              {hits.map((t) => (
                <Item
                  key={t.id}
                  value={`thread ${t.id} ${t.subject} ${search}`}
                  onSelect={() => run(() => {
                    useStore.setState({ threads: hits, cursor: hits.indexOf(t), view: "search", query: search });
                    openThread(t.id);
                  })}
                  icon={<Mail size={15} />}
                  right={<span className="text-xs text-fg-3">{shortDate(t.lastDate)}</span>}
                >
                  <span className="truncate">
                    <span className="text-fg-2">{displayName(t.participants[t.participants.length - 1], me)}</span>
                    <span className="mx-1.5 text-fg-4">·</span>
                    {t.subject || "(no subject)"}
                  </span>
                </Item>
              ))}
            </Command.Group>
          )}

          {SECTIONS.map((section) => {
            const items = commands.filter((c) => c.section === section);
            if (!items.length) return null;
            return (
              <Command.Group key={section} heading={section}>
                {items.map((c) => {
                  const Icon = c.icon;
                  return (
                    <Item
                      key={c.id}
                      value={`${c.title} ${c.id}`}
                      onSelect={() => run(c.run)}
                      icon={Icon ? <Icon size={15} /> : null}
                      right={c.keys && <Kbd keys={c.keys} />}
                    >
                      {c.title}
                    </Item>
                  );
                })}
              </Command.Group>
            );
          })}

          {drafts.length > 0 && (
            <Command.Group heading="Drafts">
              {drafts.slice(0, 8).map((d) => (
                <Item key={d.id} value={`draft ${d.subject} ${d.id}`} onSelect={() => run(() => compose(d))} icon={<FileText size={15} />}>
                  {d.subject || "(no subject)"}
                  {d.to.length > 0 && <span className="ml-2 text-fg-3">to {d.to.map((a) => a.name || a.email).join(", ")}</span>}
                </Item>
              ))}
            </Command.Group>
          )}
        </Command.List>
      </Command>
    </div>
  );
}

function Item({
  value,
  onSelect,
  icon,
  right,
  children,
}: {
  value: string;
  onSelect: () => void;
  icon?: React.ReactNode;
  right?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <Command.Item
      value={value}
      onSelect={onSelect}
      className="flex h-9 items-center gap-2.5 rounded-md px-2.5 text-[13px] text-fg-2 data-[selected=true]:bg-active data-[selected=true]:text-fg"
    >
      <span className="flex w-4 justify-center text-fg-3">{icon}</span>
      <span className="flex min-w-0 flex-1 items-center">{children}</span>
      {right}
    </Command.Item>
  );
}
