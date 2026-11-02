import clsx from "clsx";
import { forwardRef, useEffect, type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode } from "react";
import { X } from "lucide-react";
import { colorFor, initials, mod } from "../lib/format";
import type { Address } from "../lib/types";

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

export function Select({
  value,
  onChange,
  options,
  className,
}: {
  value: string;
  onChange: (v: string) => void;
  options: { value: string; label: string }[];
  className?: string;
}) {
  return (
    <select
      value={value}
      onChange={(e) => onChange(e.target.value)}
      className={clsx(
        "h-8 rounded-md border border-line-strong bg-input px-2 text-[13px] text-fg focus:border-fg-4",
        className,
      )}
    >
      {options.map((o) => (
        <option key={o.value} value={o.value} className="bg-elev">
          {o.label}
        </option>
      ))}
    </select>
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
}: {
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  className?: string;
  align?: "center" | "top";
}) {
  useEffect(() => {
    if (!open) return;
    const h = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", h, true);
    return () => window.removeEventListener("keydown", h, true);
  }, [open, onClose]);
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
      <div className={clsx("rounded-xl bg-elev shadow-pop animate-in overflow-hidden", className)}>{children}</div>
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
