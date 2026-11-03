import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource-variable/inter";
import "./index.css";
import { isMobile } from "./lib/platform";

// Outside Tauri (plain `pnpm dev` in a browser) fall back to sample data.
if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window && (window as any).__TAURI_INTERNALS__?.invoke)) {
  const { installDevMock } = await import("./lib/devMock");
  installDevMock();
  document.body.insertAdjacentHTML(
    "beforeend",
    `<div style="position:fixed;${isMobile ? "bottom:4px" : "top:8px"};left:50%;transform:translateX(-50%);z-index:100;padding:4px 10px;border-radius:999px;background:#f2c94c;color:#1b1c1f;font:500 12px Inter,system-ui,sans-serif;pointer-events:none">Demo data · run <code>pnpm tauri dev</code> for the real app</div>`,
  );
}

if (isMobile) document.documentElement.classList.add("mobile");
// Each shell only loads its own components.
const { default: App } = isMobile ? await import("./mobile/MobileApp") : await import("./App");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
