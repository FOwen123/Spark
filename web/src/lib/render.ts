// Rasterizes a node for agents (get_screenshot) and exports: the node's artboard renders off
// screen, independent of the camera and culling, and the node is drawn to a PNG.
import { createElement } from "react";
import { createRoot } from "react-dom/client";
import { domToPng } from "modern-screenshot";
import { ElementRegistry, NodeEl } from "../components/NodeTree";
import { type DocStore, themeCss, tokenVars } from "./doc";
import { loadFonts } from "./fonts";

const MAX_SIDE = 4096;

const frames = (n: number) => new Promise<void>((r) => {
  const step = (k: number) => (k <= 0 ? r() : requestAnimationFrame(() => step(k - 1)));
  step(n);
});

async function imagesReady(root: HTMLElement) {
  const urls = new Set<string>();
  for (const el of root.querySelectorAll<HTMLElement>("*")) {
    const bg = getComputedStyle(el).backgroundImage;
    for (const m of bg.matchAll(/url\("?([^")]+)"?\)/g)) urls.add(m[1]);
  }
  await Promise.all([...urls].map((u) => new Promise<void>((r) => {
    const img = new Image();
    img.onload = img.onerror = () => r();
    img.src = u;
  })));
}

/** Renders `nodeId`'s artboard off screen with its fonts loaded, runs `use` on the registered elements, then cleans up. */
async function offscreen<T>(store: DocStore, nodeId: string, use: (reg: Map<string, HTMLElement | SVGElement>) => Promise<T>): Promise<T> {
  const board = store.artboardOf(nodeId);
  if (!board) throw new Error(`node ${nodeId} is not on a page`);
  const host = document.createElement("div");
  host.style.cssText = "position:fixed;left:-200000px;top:0;pointer-events:none;contain:layout style;";
  for (const [name, value] of Object.entries(tokenVars(store.tokens))) host.style.setProperty(name, value);
  host.dataset.tokens = store.file.id;
  // React owns the host's children, so the theme rules go in the head.
  const themes = document.createElement("style");
  themes.textContent = themeCss(store.file.id, store.tokens);
  document.head.appendChild(themes);
  document.body.appendChild(host);
  const reg = new Map<string, HTMLElement | SVGElement>();
  const root = createRoot(host);
  try {
    await loadFonts(store);
    root.render(createElement(ElementRegistry.Provider, { value: reg }, createElement(NodeEl, { store, id: board })));
    await frames(2);
    await document.fonts.ready;
    return await use(reg);
  } finally {
    root.unmount();
    host.remove();
    themes.remove();
  }
}

/** Renders `nodeId` to a base64 PNG at `scale` (capped at 4096 px per side). */
export function renderToPng(store: DocStore, nodeId: string, scale: number): Promise<string> {
  return offscreen(store, nodeId, async (reg) => {
    const el = reg.get(nodeId) as HTMLElement | undefined;
    if (!el) throw new Error(`node ${nodeId} did not render`);
    await imagesReady(el);
    const r = el.getBoundingClientRect();
    if (r.width < 1 || r.height < 1) throw new Error("the node has no visible size");
    const s = Math.min(scale, MAX_SIDE / Math.max(r.width, r.height));
    const url = await domToPng(el, {
      scale: s,
      width: r.width,
      height: r.height,
      style: { left: "0px", top: "0px", right: "auto", bottom: "auto", margin: "0", position: "relative", transform: "none", rotate: "none" },
    });
    return url.slice(url.indexOf(",") + 1);
  });
}

/** Lays out an artboard off screen and returns each layer's box relative to it, as the canvas measures them. */
export function measureBoard(store: DocStore, boardId: string): Promise<Record<string, [number, number, number, number]>> {
  return offscreen(store, boardId, async (reg) => {
    // Export draws each box unturned and applies the rotation itself; rotate doesn't change layout.
    for (const el of reg.values()) el.style.rotate = "none";
    const origin = reg.get(boardId)?.getBoundingClientRect();
    if (!origin) throw new Error(`node ${boardId} did not render`);
    const round = (n: number) => Math.round(n * 100) / 100;
    const out: Record<string, [number, number, number, number]> = {};
    const visit = (id: string) => {
      const el = reg.get(id);
      if (!el) return;
      const r = el.getBoundingClientRect();
      if (r.width === 0 && r.height === 0 && (el as HTMLElement).style?.display === "none") return;
      out[id] = [round(r.left - origin.left), round(r.top - origin.top), round(r.width), round(r.height)];
      if (store.nodes.get(id)?.kind === "svg") return;
      for (const k of store.kids(id)) visit(k);
    };
    visit(boardId);
    return out;
  });
}
