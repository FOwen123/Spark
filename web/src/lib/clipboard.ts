// Spark's clipboard: node trees (lossless between Spark files) wrapped in an HTML comment, next to
// plain HTML with inline styles so other apps get something useful too.
import type { DocStore } from "./doc";
import type { NodeView } from "./types";

const MARK = "spark-clipboard:";

type Tree = { node: Omit<NodeView, "parent" | "key" | "css">; rect?: { x: number; y: number; w: number; h: number }; children: Tree[] };
export type SparkClip = { spark: 1; from: string; roots: Tree[] };

/** Top-level ids only (drops ids whose ancestor is also selected). */
export function topLevel(store: DocStore, ids: string[]): string[] {
  return ids.filter((id) => !store.ancestors(id).some((a) => ids.includes(a)));
}

/** Text layers among `ids` and their descendants. */
export function textIds(store: DocStore, ids: string[]): string[] {
  return ids.flatMap((id) => [...(store.nodes.get(id)?.kind === "text" ? [id] : []), ...textIds(store, store.kids(id))]);
}

function tree(store: DocStore, id: string): Tree | null {
  const n = store.nodes.get(id);
  if (!n) return null;
  const { parent: _p, key: _k, css: _c, ...node } = n;
  return { node, rect: store.rects.get(id), children: store.kids(id).map((k) => tree(store, k)).filter((t): t is Tree => !!t) };
}

const kebab = (k: string) => (k.startsWith("--") ? k : k.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`).replace(/^(webkit|moz|ms)-/, "-$1-"));
const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

function html(store: DocStore, id: string, root: boolean): string {
  const n = store.nodes.get(id);
  if (!n || n.hidden) return "";
  const css = Object.entries(n.css)
    .filter(([k]) => !(root && ["position", "left", "top", "right", "bottom"].includes(k)))
    .map(([k, v]) => `${kebab(k)}: ${v}`)
    .join("; ")
    .replace(/url\("\/assets\//g, `url("${location.origin}/assets/`);
  const style = css ? ` style="${esc(css)}"` : "";
  if (n.kind === "text") return `<div${style}>${esc(n.text ?? "")}</div>`;
  if (n.kind === "svg" || n.kind === "vector") {
    const tag = n.kind === "svg" ? "svg" : (n.tag ?? "g");
    const attrs = Object.entries(n.attrs ?? {})
      .filter(([k]) => k !== "data-text")
      .map(([k, v]) => ` ${k}="${esc(v)}"`)
      .join("");
    return `<${tag}${attrs}${n.kind === "svg" ? style : ""}>${esc(n.attrs?.["data-text"] ?? "")}${store.kids(id).map((k) => html(store, k, false)).join("")}</${tag}>`;
  }
  return `<div${style}>${store.kids(id).map((k) => html(store, k, false)).join("")}</div>`;
}

function encode(s: string) {
  return btoa(String.fromCharCode(...new TextEncoder().encode(s)));
}

function decode(s: string) {
  return new TextDecoder().decode(Uint8Array.from(atob(s), (c) => c.charCodeAt(0)));
}

export function copyPayload(store: DocStore, ids: string[]) {
  const roots = topLevel(store, ids);
  const clip: SparkClip = { spark: 1, from: store.nodes.get(roots[0])?.parent ?? "", roots: roots.map((id) => tree(store, id)).filter((t): t is Tree => !!t) };
  const body = roots.map((id) => html(store, id, true)).join("\n");
  const plain = roots.every((id) => store.nodes.get(id)?.kind === "text") ? roots.map((id) => store.nodes.get(id)?.text ?? "").join("\n") : body;
  return { html: `<meta charset="utf-8"><!--${MARK}${encode(JSON.stringify(clip))}-->${body}`, plain, clip };
}

export function readSparkClip(htmlText: string): SparkClip | null {
  const m = htmlText.match(new RegExp(`<!--${MARK}([A-Za-z0-9+/=]+)-->`));
  if (!m) return null;
  try {
    const v = JSON.parse(decode(m[1]));
    return v?.spark === 1 ? (v as SparkClip) : null;
  } catch {
    return null;
  }
}
