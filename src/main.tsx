import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource-variable/inter";
import "./index.css";
import App from "./App";

// Outside Tauri (plain `pnpm dev` in a browser) fall back to sample data.
if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window && (window as any).__TAURI_INTERNALS__?.invoke)) {
  const { installDevMock } = await import("./lib/devMock");
  installDevMock();
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
