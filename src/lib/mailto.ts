import type { Address, Draft } from "./types";
import { parseAddresses, textToHtml } from "./format";

const decode = (s: string) => {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
};

/** A `mailto:` link (RFC 6068) as the start of a new message. */
export function draftFromMailto(url: string): Partial<Draft> {
  const rest = url.replace(/^mailto:/i, "");
  const q = rest.indexOf("?");
  const path = q < 0 ? rest : rest.slice(0, q);
  const to = parseAddresses(decode(path));
  const cc: Address[] = [];
  const bcc: Address[] = [];
  let subject = "";
  let body = "";
  for (const pair of q < 0 ? [] : rest.slice(q + 1).split("&")) {
    const eq = pair.indexOf("=");
    const key = decode(eq < 0 ? pair : pair.slice(0, eq)).toLowerCase();
    const value = eq < 0 ? "" : decode(pair.slice(eq + 1));
    if (key === "to") to.push(...parseAddresses(value));
    else if (key === "cc") cc.push(...parseAddresses(value));
    else if (key === "bcc") bcc.push(...parseAddresses(value));
    else if (key === "subject") subject = value;
    else if (key === "body") body = value.replace(/\r\n/g, "\n");
  }
  return { to, cc, bcc, subject, text: body, html: body ? textToHtml(body) : "" };
}
