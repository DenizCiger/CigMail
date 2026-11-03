/**
 * Phones get the touch UI in src/mobile; everything else the keyboard-first
 * desktop layout. `?mobile` forces the phone UI in a desktop browser, which is
 * handy with `pnpm dev` and the browser's device toolbar.
 */
export const isMobile: boolean = (() => {
  const params = new URLSearchParams(window.location.search);
  if (params.has("mobile")) return params.get("mobile") !== "0";
  return /Android|iPhone|iPad|iPod/i.test(navigator.userAgent);
})();
