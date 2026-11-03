import clsx from "clsx";
import { forwardRef, useEffect, useRef, useState, type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown, X } from "lucide-react";
import { colorFor, initials, mod } from "../lib/format";
import type { Address } from "../lib/types";

export function focusables(root: HTMLElement): HTMLElement[] {
  return [
    ...root.querySelectorAll<HTMLElement>(
      "button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), summary, [tabindex]:not([tabindex='-1'])",
    ),
  ].filter((el) => el.offsetParent !== null);
}

export function Kbd({ keys, className }: { keys: string[]; className?: string }) {
  return (
    <span className={clsx("inline-flex items-center gap-0.5", className)}>
      {keys.map((k, i) => (
        <span key={i} className="kbd">
          {k === "Mod" ? mod : k}
        </span>
      ))}
    </span>
  );
}

type Variant = "primary" | "secondary" | "ghost" | "danger";

export const Button = forwardRef<
  HTMLButtonElement,
  ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: "sm" | "md"; icon?: ReactNode }
>(function Button({ variant = "secondary", size = "md", icon, className, children, ...rest }, ref) {
  return (
    <button
      ref={ref}
      className={clsx(
        "inline-flex items-center justify-center gap-1.5 rounded-md font-medium whitespace-nowrap transition-colors select-none",
        "disabled:opacity-50 disabled:pointer-events-none",
        size === "sm" ? "h-6 px-2 text-xs" : "h-7 px-2.5 text-[13px]",
        variant === "primary" && "bg-accent-strong text-white hover:bg-accent shadow-[inset_0_1px_0_rgb(255_255_255/0.12)]",
        variant === "secondary" && "bg-elev-2 text-fg border border-line-strong hover:bg-active",
        variant === "ghost" && "text-fg-2 hover:text-fg hover:bg-hover",
        variant === "danger" && "bg-danger/90 text-white hover:bg-danger",
        className,
      )}
      {...rest}
    >
      {icon}
      {children}
    </button>
  );
});

export function IconButton({
  label,
  shortcut,
  className,
  children,
  active,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { label: string; shortcut?: string[]; active?: boolean }) {
  return (
    <button
      aria-label={label}
      title={shortcut ? `${label}  ·  ${shortcut.map((k) => (k === "Mod" ? mod : k)).join(" ")}` : label}
      className={clsx(
        "inline-flex h-7 w-7 items-center justify-center rounded-md text-fg-3 hover:text-fg hover:bg-hover transition-colors",
        active && "text-fg bg-active",
        className,
      )}
      {...rest}
    >
      {children}
    </button>
  );
}

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(function Input(
  { className, ...rest },
  ref,
) {
  return (
    <input
      ref={ref}
      className={clsx(
        "h-8 w-full rounded-md border border-line-strong bg-input px-2.5 text-[13px] text-fg placeholder:text-fg-4",
        "focus:border-fg-4 transition-colors",
        className,
      )}
      {...rest}
    />
  );
});

export function Field({ label, hint, children }: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-xs font-medium text-fg-2">{label}</span>
      {children}
      {hint && <span className="text-xs text-fg-3">{hint}</span>}
    </label>
  );
}

/** True while the media query matches; re-renders when it flips. */
export function useMediaQuery(query: string) {
  const [matches, setMatches] = useState(() => window.matchMedia(query).matches);
  useEffect(() => {
    const mq = window.matchMedia(query);
    const h = () => setMatches(mq.matches);
    h();
    mq.addEventListener("change", h);
    return () => mq.removeEventListener("change", h);
  }, [query]);
  return matches;
}

/**
 * A themed replacement for <select>: the native popup ignores the app's colors
 * on most platforms. The list is portaled to <body> so dialogs don't clip it.
 */
export function Select({
  value,
  onChange,
  options,
  className,
  bare,
}: {
  value: string;
  onChange: (v: string) => void;
  options: { value: string; label: ReactNode }[];
  className?: string;
  /** No border or fill, for selects that sit inline in a form row. */
  bare?: boolean;
}) {
  const trigger = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [pos, setPos] = useState<{ left: number; width: number; top?: number; bottom?: number; maxHeight: number }>();
  const current = options.find((o) => o.value === value) ?? options[0];

  const show = () => {
    const r = trigger.current!.getBoundingClientRect();
    const below = window.innerHeight - r.bottom - 8;
    const above = r.top - 8;
    const want = Math.min(options.length * 28 + 8, 320);
    const up = below < want && above > below;
    setPos({
      left: r.left,
      width: r.width,
      ...(up ? { bottom: window.innerHeight - r.top + 4 } : { top: r.bottom + 4 }),
      maxHeight: Math.min(want, (up ? above : below) - 4),
    });
    setActive(Math.max(0, options.findIndex((o) => o.value === value)));
    setOpen(true);
  };
  const close = (refocus = true) => {
    setOpen(false);
    if (refocus) trigger.current?.focus();
  };
  const pick = (i: number) => {
    const o = options[i];
    if (o && o.value !== value) onChange(o.value);
    close();
  };

  useEffect(() => {
    if (!open) return;
    list.current?.focus();
    const outside = (e: MouseEvent) => {
      const t = e.target as Node;
      if (!list.current?.contains(t) && !trigger.current?.contains(t)) close(false);
    };
    const dismiss = (e: Event) => {
      if (!list.current?.contains(e.target as Node)) close(false);
    };
    window.addEventListener("mousedown", outside, true);
    window.addEventListener("scroll", dismiss, true);
    window.addEventListener("resize", dismiss);
    return () => {
      window.removeEventListener("mousedown", outside, true);
      window.removeEventListener("scroll", dismiss, true);
      window.removeEventListener("resize", dismiss);
    };
  }, [open]);

  useEffect(() => {
    if (open) list.current?.querySelector<HTMLElement>(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [open, active]);

  return (
    <>
      <button
        ref={trigger}
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => (open ? close() : show())}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === " " || e.key === "Enter") {
            e.preventDefault();
            e.stopPropagation();
            show();
          }
        }}
        className={clsx(
          "inline-flex h-8 min-w-0 items-center gap-2 rounded-md px-2.5 text-left text-[13px] text-fg transition-colors",
          bare ? "hover:bg-hover" : "border border-line-strong bg-input hover:border-fg-4",
          open && !bare && "border-fg-4",
          className,
        )}
      >
        <span className="min-w-0 flex-1 truncate">{current?.label}</span>
        <ChevronDown size={14} className="shrink-0 text-fg-3" />
      </button>
      {open &&
        pos &&
        createPortal(
          <div
            ref={list}
            role="listbox"
            tabIndex={-1}
            data-own-escape
            aria-activedescendant={`opt-${active}`}
            onBlur={(e) => {
              if (!e.currentTarget.contains(e.relatedTarget as Node)) close(false);
            }}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "ArrowDown") setActive((i) => Math.min(options.length - 1, i + 1));
              else if (e.key === "ArrowUp") setActive((i) => Math.max(0, i - 1));
              else if (e.key === "Home") setActive(0);
              else if (e.key === "End") setActive(options.length - 1);
              else if (e.key === "Enter" || e.key === " ") pick(active);
              else if (e.key === "Escape" || e.key === "Tab") close();
              else if (e.key.length === 1) {
                // Type-ahead: jump to the next option starting with that letter.
                const k = e.key.toLowerCase();
                const n = options.length;
                for (let step = 1; step <= n; step++) {
                  const i = (active + step) % n;
                  if (String(options[i].label).toLowerCase().startsWith(k)) {
                    setActive(i);
                    break;
                  }
                }
              } else return;
              e.preventDefault();
            }}
            style={{ left: pos.left, top: pos.top, bottom: pos.bottom, minWidth: pos.width, maxHeight: pos.maxHeight }}
            className="fixed z-[60] max-w-[min(420px,calc(100vw-16px))] overflow-y-auto rounded-lg bg-elev p-1 shadow-pop animate-in"
          >
            {options.map((o, i) => (
              <div
                key={o.value}
                id={`opt-${i}`}
                data-index={i}
                role="option"
                aria-selected={o.value === value}
                onMouseMove={() => setActive(i)}
                onClick={() => pick(i)}
                className={clsx(
                  "flex h-7 items-center gap-2 rounded-md px-2 text-[13px]",
                  i === active ? "bg-active text-fg" : "text-fg-2",
                )}
              >
                <span className="flex-1 truncate">{o.label}</span>
                <Check size={13} className={clsx("shrink-0 text-accent", o.value !== value && "invisible")} />
              </div>
            ))}
          </div>,
          document.body,
        )}
    </>
  );
}

export function Switch({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className={clsx(
        "relative h-[18px] w-8 shrink-0 rounded-full transition-colors",
        checked ? "bg-accent-strong" : "bg-active border border-line-strong",
      )}
    >
      <span
        className={clsx(
          "absolute top-1/2 h-3.5 w-3.5 -translate-y-1/2 rounded-full bg-white shadow transition-all",
          checked ? "left-[15px]" : "left-[2px]",
        )}
      />
    </button>
  );
}

export function Avatar({ address, size = 28, className }: { address?: Address; size?: number; className?: string }) {
  const color = colorFor(address?.email ?? "?");
  return (
    <div
      className={clsx("flex shrink-0 items-center justify-center rounded-full font-semibold text-white", className)}
      style={{
        width: size,
        height: size,
        fontSize: size * 0.4,
        background: `linear-gradient(135deg, ${color}, ${color}cc)`,
      }}
    >
      {initials(address)}
    </div>
  );
}

export function Modal({
  open,
  onClose,
  children,
  className,
  align = "center",
  onEscapeField,
}: {
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  className?: string;
  align?: "center" | "top";
  /** Esc inside a text field calls this instead of closing (e.g. to move focus back out). */
  onEscapeField?: () => void;
}) {
  const panel = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const h = (e: KeyboardEvent) => {
      // Popovers inside the dialog (e.g. an open Select) handle their own Esc.
      if ((document.activeElement as HTMLElement | null)?.closest("[data-own-escape]")) return;
      if (e.key === "Escape") {
        e.stopPropagation();
        const field = (document.activeElement as HTMLElement | null)?.closest("input, textarea, select");
        if (onEscapeField && field && panel.current?.contains(field)) {
          e.preventDefault();
          onEscapeField();
        } else {
          onClose();
        }
      } else if (e.key === "Tab" && panel.current) {
        // Keep Tab cycling inside the dialog instead of the page behind it.
        const items = focusables(panel.current);
        if (!items.length) return;
        const i = items.indexOf(document.activeElement as HTMLElement);
        const next = e.shiftKey ? (i <= 0 ? items.length - 1 : i - 1) : i === -1 || i === items.length - 1 ? 0 : i + 1;
        e.preventDefault();
        items[next].focus();
      }
    };
    window.addEventListener("keydown", h, true);
    return () => window.removeEventListener("keydown", h, true);
  }, [open, onClose, onEscapeField]);
  useEffect(() => {
    if (!open || !panel.current) return;
    if (panel.current.contains(document.activeElement)) return;
    const preferred = panel.current.querySelector<HTMLElement>("[data-autofocus]");
    (preferred ?? focusables(panel.current)[0])?.focus();
  }, [open]);
  if (!open) return null;
  return (
    <div
      className={clsx(
        "fixed inset-0 z-50 flex justify-center bg-black/40 backdrop-blur-[2px] animate-fade",
        align === "top" ? "items-start pt-[14vh]" : "items-center",
      )}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div ref={panel} role="dialog" aria-modal className={clsx("rounded-xl bg-elev shadow-pop animate-in overflow-hidden", className)}>
        {children}
      </div>
    </div>
  );
}

export function ModalHeader({ title, onClose, children }: { title: ReactNode; onClose: () => void; children?: ReactNode }) {
  return (
    <div className="flex h-12 items-center gap-2 border-b border-line px-4">
      <div className="flex-1 text-[14px] font-medium">{title}</div>
      {children}
      <IconButton label="Close" shortcut={["Esc"]} onClick={onClose}>
        <X size={15} />
      </IconButton>
    </div>
  );
}

export function Spinner({ size = 14, className }: { size?: number; className?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      className={clsx("animate-spin text-fg-3", className)}
      fill="none"
    >
      <circle cx="12" cy="12" r="9" stroke="currentColor" strokeOpacity="0.25" strokeWidth="3" />
      <path d="M21 12a9 9 0 0 0-9-9" stroke="currentColor" strokeWidth="3" strokeLinecap="round" />
    </svg>
  );
}

export function AccountDot({ color, size = 8 }: { color: string; size?: number }) {
  return <span className="inline-block shrink-0 rounded-full" style={{ width: size, height: size, background: color }} />;
}

export function ProviderGlyph({ kind, size = 14 }: { kind: string; size?: number }) {
  if (kind === "gmail")
    return (
      <svg width={size} height={size} viewBox="0 0 24 24">
        <path fill="#4285F4" d="M2 6.5V18a2 2 0 0 0 2 2h3V10.6L2 6.5z" />
        <path fill="#34A853" d="M17 20h3a2 2 0 0 0 2-2V6.5l-5 4.1V20z" />
        <path fill="#FBBC04" d="M17 4.6v6l5-4.1V5.4c0-1.8-2-2.8-3.4-1.7L17 4.6z" />
        <path fill="#EA4335" d="M7 10.6V4.6l5 3.8 5-3.8v6l-5 3.8-5-3.8z" />
        <path fill="#C5221F" d="M2 5.4v1.1l5 4.1v-6L5.4 3.7C4 2.6 2 3.6 2 5.4z" />
      </svg>
    );
  if (kind === "outlook")
    return (
      <svg width={size} height={size} viewBox="0 0 24 24">
        <rect x="9" y="4" width="13" height="16" rx="2" fill="#0F78D4" />
        <path d="M9 9l6.5 4L22 9" stroke="#fff" strokeOpacity=".7" strokeWidth="1.3" fill="none" />
        <rect x="2" y="6" width="11" height="12" rx="2" fill="#0A5FAB" />
        <ellipse cx="7.5" cy="12" rx="2.6" ry="3" stroke="#fff" strokeWidth="1.6" fill="none" />
      </svg>
    );
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
      <rect x="3" y="5" width="18" height="14" rx="2" />
      <path d="M3 7l9 6 9-6" />
    </svg>
  );
}
