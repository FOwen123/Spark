// Picks a color from the screen. Chromium has `EyeDropper`; the macOS webview doesn't, so the
// fallback renders the clicked artboard with `renderToPng` and reads the pixel under the pointer.
import type { DocStore } from "./doc";
import { renderToPng } from "./render";

type NativeDropper = new () => { open(): Promise<{ sRGBHex: string }> };

/** Whether the browser has its own eyedropper (otherwise the canvas-sampling fallback is used). */
export const hasNativeEyedropper = () => "EyeDropper" in window;

/** Waits for one click anywhere (Escape cancels) under a crosshair overlay. */
function clickPoint(): Promise<{ x: number; y: number } | null> {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.dataset.testid = "eyedropper-overlay";
    overlay.style.cssText = "position:fixed;inset:0;z-index:1000;cursor:crosshair";
    const done = (p: { x: number; y: number } | null) => {
      overlay.remove();
      window.removeEventListener("keydown", onKey, true);
      resolve(p);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation(); // cancels the pick only, not the editor's selection
      done(null);
    };
    overlay.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      done({ x: e.clientX, y: e.clientY });
    });
    window.addEventListener("keydown", onKey, true);
    document.body.append(overlay);
  });
}

const hex = (r: number, g: number, b: number, a: number) => {
  const h = (v: number) => Math.round(v).toString(16).padStart(2, "0");
  return `#${h(r)}${h(g)}${h(b)}${a < 255 ? h(a) : ""}`;
};

/** The CSS background of the nearest ancestor that has one (for clicks outside any design node). */
function chromeColor(el: Element | null): string | null {
  for (let e = el; e; e = e.parentElement) {
    const c = getComputedStyle(e).backgroundColor;
    const m = c.match(/rgba?\((\d+)[, ]+(\d+)[, ]+(\d+)(?:[,/ ]+([\d.]+))?/);
    if (m && (m[4] === undefined || Number(m[4]) > 0)) return hex(+m[1], +m[2], +m[3], m[4] === undefined ? 255 : Math.round(Number(m[4]) * 255));
  }
  return null;
}

/** Resolves to a hex color, or null when cancelled or nothing could be read. */
export async function pickColor(store: DocStore | null): Promise<string | null> {
  const Native = (window as unknown as { EyeDropper?: NativeDropper }).EyeDropper;
  if (Native) {
    try {
      return (await new Native().open()).sRGBHex;
    } catch {
      return null; // the user pressed Escape
    }
  }
  const at = await clickPoint();
  if (!at) return null;
  const el = document.elementFromPoint(at.x, at.y);
  const id = el?.closest("[data-node]")?.getAttribute("data-node");
  const board = id && store ? store.artboardOf(id) : null;
  const boardEl = board ? document.querySelector<HTMLElement>(`[data-node="${board}"]`) : null;
  if (!store || !board || !boardEl) return chromeColor(el);
  const box = boardEl.getBoundingClientRect();
  const img = new Image();
  img.src = `data:image/png;base64,${await renderToPng(store, board, 1)}`;
  await img.decode();
  const canvas = document.createElement("canvas");
  canvas.width = img.naturalWidth;
  canvas.height = img.naturalHeight;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) return null;
  ctx.drawImage(img, 0, 0);
  const x = Math.min(canvas.width - 1, Math.max(0, Math.floor(((at.x - box.left) / box.width) * canvas.width)));
  const y = Math.min(canvas.height - 1, Math.max(0, Math.floor(((at.y - box.top) / box.height) * canvas.height)));
  const [r, g, b, a] = ctx.getImageData(x, y, 1, 1).data;
  return hex(r, g, b, a);
}
