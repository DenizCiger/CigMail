import clsx from "clsx";

/** The app's icon (the same picture as the favicon and the installed app). */
export function AppIcon({ className }: { className?: string }) {
  return <img src="/favicon.svg" alt="" aria-hidden draggable={false} className={clsx("shrink-0 select-none", className)} />;
}
