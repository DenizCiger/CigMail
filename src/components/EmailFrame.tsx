import { useEffect, useMemo, useRef, useState } from "react";
import DOMPurify from "dompurify";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ImageOff } from "lucide-react";
import { useStore } from "../store";
import { parseAddresses } from "../lib/format";

/** Emails that bring their own colors/layout look broken on a dark canvas, so they get a light "paper". */
function isDesigned(html: string): boolean {
  return /bgcolor=|background(-color)?\s*:|<table[\s\S]*<table/i.test(html);
}

function sanitize(html: string, allowRemote: boolean): { html: string; blocked: number } {
  let blocked = 0;
  DOMPurify.removeAllHooks();
  DOMPurify.addHook("afterSanitizeAttributes", (node) => {
    const el = node as Element;
    if (el.tagName === "A") {
      el.setAttribute("target", "_blank");
      el.setAttribute("rel", "noopener noreferrer");
    }
    if (!allowRemote) {
      for (const attr of ["src", "background", "poster", "srcset"]) {
        const v = el.getAttribute(attr);
        if (v && /^(https?:)?\/\//i.test(v.trim())) {
          el.setAttribute(`data-blocked-${attr}`, v);
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
    FORBID_TAGS: ["script", "iframe", "object", "embed", "form", "input", "button", "textarea", "select", "meta", "link", "base"],
    FORBID_ATTR: ["onerror", "onload", "onclick"],
    ADD_ATTR: ["target"],
  });
  DOMPurify.removeAllHooks();
  let out = clean;
  if (!allowRemote) {
    // Remote stylesheets / fonts in <style> blocks can also track opens.
    out = out.replace(/@import[^;]+;/gi, "").replace(/url\(\s*['"]?(https?:)?\/\/[^)]*\)/gi, "none");
  }
  return { html: out, blocked };
}

function frameCss(paper: boolean, dark: boolean): string {
  const fg = paper ? "#1b1c1f" : dark ? "#e4e4e7" : "#1b1c1f";
  const link = paper || !dark ? "#4f5bc4" : "#8e98ee";
  const quote = paper || !dark ? "#d4d4d8" : "#3a3b40";
  return `
    html, body { margin: 0; padding: 0; background: transparent; }
    body { color: ${fg}; font: 14px/1.6 "Inter Variable", Inter, system-ui, -apple-system, "Segoe UI", sans-serif;
           word-wrap: break-word; overflow-wrap: anywhere; ${paper ? "padding: 16px;" : ""} }
    a { color: ${link}; }
    img { max-width: 100%; height: auto; }
    table { max-width: 100% !important; }
    pre { white-space: pre-wrap; }
    blockquote { margin: 0 0 0 .8ex; border-left: 2px solid ${quote}; padding-left: 1ex; color: inherit; opacity: .85; }
    .gmail_quote, .cm-quote { opacity: .85; }
  `;
}

export function EmailFrame({ html, sender }: { html: string; sender: string }) {
  const settings = useStore((s) => s.settings);
  const setSetting = useStore((s) => s.setSetting);
  const compose = useStore((s) => s.compose);
  const [allowOnce, setAllowOnce] = useState(false);
  const ref = useRef<HTMLIFrameElement>(null);
  const [height, setHeight] = useState(80);

  const trusted: string[] = useMemo(() => {
    try {
      return JSON.parse(settings.image_senders ?? "[]");
    } catch {
      return [];
    }
  }, [settings.image_senders]);
  const allowRemote = settings.remote_images === "always" || allowOnce || trusted.includes(sender.toLowerCase());
  const dark = document.documentElement.dataset.theme !== "light";
  const paper = dark && isDesigned(html);

  const { html: clean, blocked } = useMemo(() => sanitize(html, allowRemote), [html, allowRemote]);

  const srcDoc = useMemo(() => {
    const css = `<style>${frameCss(paper, dark)}</style>`;
    if (/<head[^>]*>/i.test(clean)) return clean.replace(/<head([^>]*)>/i, `<head$1>${css}`);
    return `<!doctype html><html><head>${css}</head><body>${clean}</body></html>`;
  }, [clean, paper, dark]);

  useEffect(() => {
    const iframe = ref.current;
    if (!iframe) return;
    let ro: ResizeObserver | undefined;
    const onLoad = () => {
      const doc = iframe.contentDocument;
      if (!doc) return;
      const measure = () => setHeight(Math.max(40, doc.documentElement.scrollHeight));
      measure();
      ro = new ResizeObserver(measure);
      ro.observe(doc.documentElement);
      doc.querySelectorAll("img").forEach((img) => img.addEventListener("load", measure));
      doc.addEventListener("click", (e) => {
        const a = (e.target as Element)?.closest?.("a[href]") as HTMLAnchorElement | null;
        if (!a) return;
        e.preventDefault();
        const href = a.getAttribute("href") ?? "";
        if (href.startsWith("mailto:")) {
          const url = new URL(href);
          compose({ to: parseAddresses(decodeURIComponent(url.pathname)), subject: url.searchParams.get("subject") ?? "" });
        } else if (/^https?:/i.test(href)) {
          openUrl(href).catch(() => {});
        }
      });
      // Let global shortcuts work while the email has focus.
      doc.addEventListener("keydown", (e) => {
        window.dispatchEvent(new KeyboardEvent("keydown", e));
      });
    };
    iframe.addEventListener("load", onLoad);
    return () => {
      iframe.removeEventListener("load", onLoad);
      ro?.disconnect();
    };
  }, [srcDoc, compose]);

  return (
    <div>
      {blocked > 0 && !allowRemote && (
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
      <div className={paper ? "overflow-hidden rounded-lg bg-[#fbfbfc]" : ""}>
        <iframe
          ref={ref}
          title="Email content"
          sandbox="allow-same-origin allow-popups allow-popups-to-escape-sandbox"
          srcDoc={srcDoc}
          className="block w-full border-0"
          style={{ height }}
        />
      </div>
    </div>
  );
}
