import clsx from "clsx";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useStore } from "../store";
import { Kbd } from "./ui";

/** App-wide right-click menu, opened via `useStore.openMenu`. Arrow keys + Enter work too. */
export function ContextMenu() {
  const menu = useStore((s) => s.menu);
  const set = useStore((s) => s.set);
  const ref = useRef<HTMLDivElement>(null);
  const [active, setActive] = useState(0);
  const [pos, setPos] = useState({ x: 0, y: 0 });

  useLayoutEffect(() => {
    if (!menu || !ref.current) return;
    const r = ref.current.getBoundingClientRect();
    setPos({
      x: Math.min(menu.x, window.innerWidth - r.width - 8),
      y: Math.min(menu.y, window.innerHeight - r.height - 8),
    });
    setActive(0);
    ref.current.focus();
  }, [menu]);

  useEffect(() => {
    if (!menu) return;
    const close = () => set({ menu: null });
    window.addEventListener("blur", close);
    window.addEventListener("resize", close);
    window.addEventListener("wheel", close, { passive: true });
    return () => {
      window.removeEventListener("blur", close);
      window.removeEventListener("resize", close);
      window.removeEventListener("wheel", close);
    };
  }, [menu, set]);

  if (!menu) return null;
  const close = () => set({ menu: null });
  const run = (i: number) => {
    close();
    menu.items[i]?.run();
  };

  return (
    <div
      className="fixed inset-0 z-[70]"
      onMouseDown={(e) => e.target === e.currentTarget && close()}
      onContextMenu={(e) => {
        e.preventDefault();
        close();
      }}
    >
      <div
        ref={ref}
        tabIndex={-1}
        role="menu"
        style={{ left: pos.x, top: pos.y }}
        className="absolute min-w-[220px] rounded-lg border border-line-strong bg-elev p-1 shadow-pop animate-in outline-none"
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Escape") close();
          else if (e.key === "ArrowDown" || e.key === "j") setActive((a) => (a + 1) % menu.items.length);
          else if (e.key === "ArrowUp" || e.key === "k") setActive((a) => (a - 1 + menu.items.length) % menu.items.length);
          else if (e.key === "Enter") run(active);
          else return;
          e.preventDefault();
        }}
      >
        {menu.items.map((item, i) => (
          <div key={item.label}>
            {item.separator && <div className="my-1 h-px bg-line" />}
            <button
              role="menuitem"
              onMouseEnter={() => setActive(i)}
              onClick={() => run(i)}
              className={clsx(
                "flex h-8 w-full items-center gap-2.5 rounded-md px-2 text-left text-[13px]",
                i === active ? "bg-active text-fg" : "text-fg-2",
                item.danger && "text-danger",
              )}
            >
              <span className="flex w-4 justify-center text-fg-3">{item.icon && <item.icon size={14} />}</span>
              <span className="flex-1">{item.label}</span>
              {item.keys && <Kbd keys={item.keys} />}
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
