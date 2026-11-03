import { useEffect, useRef } from "react";

/**
 * Android's back button (and the back gesture) navigates the webview history.
 * Every open layer (drawer, thread, composer, sheet…) pushes one history entry,
 * so back closes the topmost layer instead of leaving the app.
 */

interface Layer {
  onBack: () => void;
}

const stack: Layer[] = [];
/** popstate events we caused ourselves by unwinding a layer closed in-app. */
let selfPops = 0;
/**
 * Entries to push once those pops land: history.back() is asynchronous, and an
 * entry pushed before it completes would be the one it removes.
 */
let queued = 0;

function push() {
  if (selfPops > 0) queued++;
  else history.pushState({ layer: stack.length }, "");
}

window.addEventListener("popstate", () => {
  if (selfPops > 0) {
    selfPops--;
    if (selfPops === 0) {
      for (; queued > 0; queued--) history.pushState({ layer: stack.length }, "");
    }
    return;
  }
  stack.pop()?.onBack();
});

export function useBackLayer(open: boolean, onBack: () => void) {
  const handler = useRef(onBack);
  handler.current = onBack;
  useEffect(() => {
    if (!open) return;
    const layer: Layer = { onBack: () => handler.current() };
    stack.push(layer);
    push();
    return () => {
      const i = stack.indexOf(layer);
      if (i < 0) return; // Already popped by the back button.
      // Closed from the UI: drop its history entry too.
      stack.splice(i, 1);
      if (queued > 0) {
        queued--; // Its entry was never pushed.
        return;
      }
      selfPops++;
      history.back();
    };
  }, [open]);
}
