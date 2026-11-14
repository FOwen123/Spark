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

/** Renders `nodeId` to a base64 PNG at `scale` (capped at 4096 px per side). */
export async function renderToPng(store: DocStore, nodeId: string, scale: number): Promise<string> {
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
  } finally {
    root.unmount();
    host.remove();
    themes.remove();
  }
}
