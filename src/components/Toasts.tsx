import clsx from "clsx";
import { CheckCircle2, AlertCircle, X } from "lucide-react";
import { useStore } from "../store";
import { Kbd } from "./ui";

export function Toasts() {
  const toasts = useStore((s) => s.toasts);
  const dismiss = useStore((s) => s.dismissToast);
  return (
    <div className="pointer-events-none fixed bottom-5 left-1/2 z-[60] flex -translate-x-1/2 flex-col items-center gap-2">
      {toasts.map((t) => (
        <div
          key={t.id}
          className={clsx(
            "pointer-events-auto flex min-h-9 max-w-[560px] items-center gap-2.5 rounded-lg border bg-elev py-1.5 pl-3 pr-1.5 text-[13px] shadow-pop animate-slide-up",
            t.kind === "error" ? "border-danger/40" : "border-line-strong",
          )}
        >
          {t.kind === "success" && <CheckCircle2 size={14} className="shrink-0 text-success" />}
          {t.kind === "error" && <AlertCircle size={14} className="shrink-0 text-danger" />}
          <span className="min-w-0 flex-1 break-words selectable">{t.message}</span>
          {t.action && (
            <button
              onClick={() => {
                dismiss(t.id);
                t.action!.run();
              }}
              className="flex h-6 items-center gap-1.5 rounded-md px-2 text-xs font-medium text-fg hover:bg-active"
            >
              {t.action.label}
              {t.action.label === "Undo" && <Kbd keys={["Z"]} />}
            </button>
          )}
          <button onClick={() => dismiss(t.id)} className="rounded p-1 text-fg-3 hover:bg-active hover:text-fg">
            <X size={12} />
          </button>
        </div>
      ))}
    </div>
  );
}
