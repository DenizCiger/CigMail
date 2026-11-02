import { useEffect, useMemo, useRef, useState } from "react";
import DOMPurify from "dompurify";
import { openUrl } from "@tauri-apps/plugin-opener";
import { writeText } from "../lib/clipboard";
import { ExternalLink, Copy, ImageOff, Mail } from "lucide-react";
import { useStore } from "../store";
import { parseAddresses } from "../lib/format";

/**
 * Renders an email inside a shadow root. The HTML is sanitized (no scripts,
 * forms or remote content unless allowed), and the shadow root keeps the
 * email's CSS from leaking into the app. Unlike a sandboxed iframe, clicks and
 * keyboard input work normally — WebKitGTK refuses to run any handler inside
 * a script-less sandboxed frame, which made links dead.
 */

function sanitize(html: string, allowRemote: boolean): { styles: string; body: string; bodyStyle: string; blocked: number } {
  let blocked = 0;
  DOMPurify.removeAllHooks();
  DOMPurify.addHook("afterSanitizeAttributes", (node) => {
    const el = node as Element;
    if (!allowRemote) {
      for (const attr of ["src", "background", "poster", "srcset"]) {
        const v = el.getAttribute(attr);
        if (v && /^(https?:)?\/\//i.test(v.trim())) {
          el.removeAttribute(attr);
          blocked++;
        }
      }
      const style = el.getAttribute("style");
      if (style && /url\(\s*['"]?(https?:)?\/\//i.test(style)) {
        el.setAttribute("style", style.replace(/url\(\s*['"]?(https?:)?\/\/[^)]*\)/gi, "none"));
        blocked++;
      }
    }
  });
  const clean = DOMPurify.sanitize(html, {
    WHOLE_DOCUMENT: true,
    FORBID_TAGS: ["script", "iframe", "object", "embed", "form", "input", "button", "textarea", "select", "meta", "link", "base", "title"],
    FORBID_ATTR: ["onerror", "onload", "onclick"],
  });
  DOMPurify.removeAllHooks();

  const doc = new DOMParser().parseFromString(clean, "text/html");
  let styles = [...doc.querySelectorAll("style")]
    .map((s) => s.textContent ?? "")
    .join("\n")
    // Email CSS targets html/body, which don't exist inside the shadow root.
    .replace(/(^|[\s,}>~+])html(?=[\s,{.:#[>~+])/g, "$1:host")
    .replace(/(^|[\s,}>~+])body(?=[\s,{.:#[>~+])/g, "$1.cm-body");
  doc.querySelectorAll("style").forEach((s) => s.remove());
  if (!allowRemote) {
    styles = styles.replace(/@import[^;]+;/gi, "").replace(/url\(\s*['"]?(https?:)?\/\/[^)]*\)/gi, "none");
  }
  const b = doc.body;
  const bodyStyle = [b.getAttribute("style") ?? "", b.getAttribute("bgcolor") ? `background-color:${b.getAttribute("bgcolor")}` : ""]
    .filter(Boolean)
    .join(";");
  return { styles, body: b.innerHTML, bodyStyle, blocked };
}

function escapeHtml(t: string) {
  return t.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function linkify(t: string) {
  return escapeHtml(t).replace(/\b(https?:\/\/[^\s<>"')\]]+)/g, '<a href="$1">$1</a>');
}

const BASE_CSS = `
  :host { display: block; }
  .cm-paper { border-radius: 10px; padding: 16px 18px; background: #ffffff; color: #1b1c1f; overflow-x: auto;
              font: 14px/1.6 "Inter Variable", Inter, system-ui, -apple-system, "Segoe UI", sans-serif;
              word-wrap: break-word; overflow-wrap: anywhere; transform: translateZ(0); }
  /* Dark mode: invert the whole email, then flip media back so photos look right. */
  /* contrast() keeps white from becoming pure black: it lands near the app's panel color. */
  .cm-paper.dark { background: #ffffff; filter: invert(1) hue-rotate(180deg) contrast(0.88); }
  .cm-paper.dark :is(img, video, picture, svg, [style*="background-image"], [background]) { filter: invert(1) hue-rotate(180deg); }
  .cm-paper.blend { border-radius: 0; padding: 0; }
  .cm-body a { color: #3b48c4; }
  .cm-body img { max-width: 100%; height: auto; }
  .cm-body table { max-width: 100% !important; }
  .cm-body pre, .cm-plain { white-space: pre-wrap; font-family: inherit; margin: 0; }
  .cm-body blockquote { margin: 0 0 0 .8ex; border-left: 2px solid #d4d4d8; padding-left: 1ex; opacity: .85; }
  .cm-body a, .cm-body a * { cursor: pointer; }
`;

export function EmailFrame({ html, text, sender }: { html: string | null; text: string | null; sender: string }) {
  const settings = useStore((s) => s.settings);
  const setSetting = useStore((s) => s.setSetting);
  const compose = useStore((s) => s.compose);
  const openMenu = useStore((s) => s.openMenu);
  const toast = useStore((s) => s.toast);
  const [allowOnce, setAllowOnce] = useState(false);
  const host = useRef<HTMLDivElement>(null);

  const trusted: string[] = useMemo(() => {
    try {
      return JSON.parse(settings.image_senders ?? "[]");
    } catch {
      return [];
    }
  }, [settings.image_senders]);
  const allowRemote = settings.remote_images === "always" || allowOnce || trusted.includes(sender.toLowerCase());
  const appDark = document.documentElement.dataset.theme !== "light";
  const dark = settings.email_theme ? settings.email_theme === "dark" : appDark;
  // When the email matches the app theme it blends into the page; otherwise it sits on a card.
  const blend = dark === appDark;

  const content = useMemo(() => {
    if (html) return sanitize(html, allowRemote);
    return { styles: "", body: `<div class="cm-plain">${linkify(text ?? "")}</div>`, bodyStyle: "", blocked: 0 };
  }, [html, text, allowRemote]);

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const root = el.shadowRoot ?? el.attachShadow({ mode: "open" });
    root.innerHTML =
      `<style>${BASE_CSS}</style><style>${content.styles}</style>` +
      `<div class="cm-paper${dark ? " dark" : ""}${blend ? " blend" : ""}"><div class="cm-body" style="${escapeHtml(content.bodyStyle).replace(/"/g, "&quot;")}">${content.body}</div></div>`;
  }, [content, dark, blend]);

  const openLink = (href: string) => {
    if (href.startsWith("mailto:")) {
      const url = new URL(href);
      compose({ to: parseAddresses(decodeURIComponent(url.pathname)), subject: url.searchParams.get("subject") ?? "" });
    } else if (/^https?:/i.test(href)) {
      openUrl(href).catch((e) => toast(String(e), { kind: "error" }));
    }
  };

  const linkAt = (e: React.MouseEvent) => {
    const path = e.nativeEvent.composedPath() as Element[];
    return path.find((n) => n instanceof HTMLAnchorElement && n.getAttribute("href")) as HTMLAnchorElement | undefined;
  };

  return (
    <div>
      {content.blocked > 0 && !allowRemote && (
        <div className="mb-3 flex items-center gap-2 rounded-md border border-line bg-hover px-3 py-1.5 text-xs text-fg-3">
          <ImageOff size={13} />
          <span className="flex-1">Remote images are blocked to protect your privacy.</span>
          <button className="text-fg-2 hover:text-fg" onClick={() => setAllowOnce(true)}>
            Load once
          </button>
          <span className="text-fg-4">·</span>
          <button
            className="text-fg-2 hover:text-fg"
            onClick={() => setSetting("image_senders", JSON.stringify([...trusted, sender.toLowerCase()]))}
          >
            Always from this sender
          </button>
        </div>
      )}
      <div
        ref={host}
        className="selectable"
        onClick={(e) => {
          const a = linkAt(e);
          if (!a) return;
          e.preventDefault();
          openLink(a.getAttribute("href")!);
        }}
        onContextMenu={(e) => {
          const a = linkAt(e);
          // Keep the native menu for copying selected text.
          if (!a && window.getSelection()?.toString()) return;
          e.preventDefault();
          if (!a) return;
          const href = a.getAttribute("href")!;
          const mail = href.startsWith("mailto:");
          openMenu(e.clientX, e.clientY, [
            { label: mail ? "Write email" : "Open link", icon: mail ? Mail : ExternalLink, run: () => openLink(href) },
            {
              label: mail ? "Copy address" : "Copy link",
              icon: Copy,
              run: () => writeText(mail ? decodeURIComponent(href.slice(7).split("?")[0]) : href).then(() => toast("Copied")),
            },
          ]);
        }}
      />
    </div>
  );
}
