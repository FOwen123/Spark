// Overlays the canvas HUD draws on top of the page: pixel grid, layout guides, flex padding and gap
// areas with their drag handles, and Alt-measurements. Everything here is gray and screen-space.

import type { DocStore } from "../lib/doc";
import { type Camera, measureLines, rectToScreen } from "../lib/geometry";
import type { Rect } from "../lib/types";

export type Paint = { ctx: CanvasRenderingContext2D; cam: Camera; w: number; h: number; fg: string; halo: string };

// Mid-gray at a given opacity (percent): reads over a white artboard and a black one, in either theme.
const tint = (_p: Paint, pct: number) => `rgba(128, 128, 128, ${pct / 100})`;
const crisp = (v: number) => Math.round(v) + 0.5;

export function badge(p: Paint, text: string, cx: number, cy: number) {
  const { ctx } = p;
  ctx.font = "10px 'Geist Mono', monospace";
  const tw = ctx.measureText(text).width + 8;
  ctx.fillStyle = p.fg;
  ctx.beginPath();
  ctx.roundRect(cx - tw / 2, cy - 7.5, tw, 15, 3);
  ctx.fill();
  ctx.fillStyle = p.halo;
  ctx.textBaseline = "middle";
  ctx.fillText(text, cx - tw / 2 + 4, cy + 0.5);
}

// ---- pixel grid ---------------------------------------------------------------

/** One-pixel grid lines, from 800% zoom up. */
export function drawPixelGrid(p: Paint) {
  const { ctx, cam } = p;
  if (cam.zoom < 8) return;
  ctx.strokeStyle = tint(p, 55);
  ctx.lineWidth = 1;
  ctx.setLineDash([]);
  ctx.beginPath();
  for (let wx = Math.ceil(-cam.x / cam.zoom); wx * cam.zoom + cam.x <= p.w; wx++) {
    const x = crisp(wx * cam.zoom + cam.x);
    ctx.moveTo(x, 0);
    ctx.lineTo(x, p.h);
  }
  for (let wy = Math.ceil(-cam.y / cam.zoom); wy * cam.zoom + cam.y <= p.h; wy++) {
    const y = crisp(wy * cam.zoom + cam.y);
    ctx.moveTo(0, y);
    ctx.lineTo(p.w, y);
  }
  ctx.stroke();
}

// ---- layout guides ------------------------------------------------------------

const guided = new WeakMap<DocStore, { version: number; ids: string[] }>();

/** Frames with layout guides, rescanned only when the document changed. */
function guidedFrames(store: DocStore): string[] {
  const hit = guided.get(store);
  if (hit && hit.version === store.version) return hit.ids;
  const ids: string[] = [];
  for (const n of store.nodes.values()) if (n.layoutGuides?.length && !n.hidden) ids.push(n.id);
  guided.set(store, { version: store.version, ids });
  return ids;
}

export function drawLayoutGuides(p: Paint, store: DocStore) {
  const { ctx, cam } = p;
  for (const id of guidedFrames(store)) {
    const r = store.rects.get(id);
    if (!r) continue;
    const s = rectToScreen(cam, r);
    if (s.x > p.w || s.y > p.h || s.x + s.w < 0 || s.y + s.h < 0) continue;
    ctx.save();
    ctx.beginPath();
    ctx.rect(Math.max(s.x, 0), Math.max(s.y, 0), Math.min(s.x + s.w, p.w) - Math.max(s.x, 0), Math.min(s.y + s.h, p.h) - Math.max(s.y, 0));
    ctx.clip();
    for (const g of store.nodes.get(id)?.layoutGuides ?? []) {
      if (g.kind === "grid") {
        const step = g.size * cam.zoom;
        if (step < 3) continue;
        ctx.strokeStyle = tint(p, 32);
        ctx.lineWidth = 1;
        ctx.beginPath();
        for (let x = s.x; x <= s.x + s.w; x += step) {
          if (x < 0 || x > p.w) continue;
          ctx.moveTo(crisp(x), s.y);
          ctx.lineTo(crisp(x), s.y + s.h);
        }
        for (let y = s.y; y <= s.y + s.h; y += step) {
          if (y < 0 || y > p.h) continue;
          ctx.moveTo(s.x, crisp(y));
          ctx.lineTo(s.x + s.w, crisp(y));
        }
        ctx.stroke();
        continue;
      }
      const cols = g.kind === "columns";
      const total = (cols ? s.w : s.h) - 2 * g.margin * cam.zoom;
      const size = (total - g.gutter * cam.zoom * (g.count - 1)) / g.count;
      if (size <= 0) continue;
      ctx.fillStyle = tint(p, 16);
      for (let i = 0; i < g.count; i++) {
        const at = g.margin * cam.zoom + i * (size + g.gutter * cam.zoom);
        if (cols) ctx.fillRect(s.x + at, s.y, size, s.h);
        else ctx.fillRect(s.x, s.y + at, s.w, size);
      }
    }
    ctx.restore();
  }
}

// ---- flex frames --------------------------------------------------------------

export type FlexBox = {
  id: string;
  r: Rect;
  /** Border widths and padding (top, right, bottom, left), in world pixels. */
  border: number[];
  pad: number[];
  gap: number;
  row: boolean;
  wrap: boolean;
  /** Rects of the children laid out in the flow. */
  kids: Rect[];
};

export const isFlex = (store: DocStore, id: string | null) => {
  const n = id ? store.nodes.get(id) : undefined;
  return !!n && n.kind === "frame" && (n.style.display === "flex" || n.style.display === "inline-flex");
};

/** A flex frame's padding, gap and in-flow children, read from the browser's own layout. */
export function flexBox(store: DocStore, reg: Map<string, HTMLElement | SVGElement>, id: string): FlexBox | null {
  const n = store.nodes.get(id);
  const el = reg.get(id);
  const r = store.rects.get(id);
  if (!n || !el || !r || !isFlex(store, id)) return null;
  const cs = getComputedStyle(el);
  const px = (s: string) => Number.parseFloat(s) || 0;
  const row = !n.style.flexDirection?.startsWith("column");
  const kids = store
    .kids(id)
    .filter((k) => {
      const kn = store.nodes.get(k);
      return kn && !kn.hidden && kn.style.position !== "absolute" && kn.style.position !== "fixed";
    })
    .map((k) => store.rects.get(k))
    .filter((k): k is Rect => !!k);
  return {
    id,
    r,
    border: [cs.borderTopWidth, cs.borderRightWidth, cs.borderBottomWidth, cs.borderLeftWidth].map(px),
    pad: [cs.paddingTop, cs.paddingRight, cs.paddingBottom, cs.paddingLeft].map(px),
    gap: px(row ? cs.columnGap : cs.rowGap),
    row,
    wrap: !!n.style.flexWrap && n.style.flexWrap !== "nowrap",
    kids,
  };
}

export type FlexKey = "pt" | "pr" | "pb" | "pl" | "gap";
/** The style property each handle changes. */
export const FLEX_PROP: Record<FlexKey, string> = { pt: "padding-top", pr: "padding-right", pb: "padding-bottom", pl: "padding-left", gap: "gap" };
export type FlexHandle = { key: FlexKey; x: number; y: number; w: number; h: number };

/** The padding and gap areas in world space: [rect, is a gap]. */
function areas(b: FlexBox): [Rect, boolean][] {
  const { r, border: bd, pad } = b;
  const ix = r.x + bd[3];
  const iy = r.y + bd[0];
  const iw = r.w - bd[1] - bd[3];
  const ih = r.h - bd[0] - bd[2];
  const out: [Rect, boolean][] = [
    [{ x: ix, y: iy, w: iw, h: pad[0] }, false],
    [{ x: ix, y: iy + ih - pad[2], w: iw, h: pad[2] }, false],
    [{ x: ix, y: iy + pad[0], w: pad[3], h: ih - pad[0] - pad[2] }, false],
    [{ x: ix + iw - pad[1], y: iy + pad[0], w: pad[1], h: ih - pad[0] - pad[2] }, false],
  ];
  if (b.wrap) return out;
  const cy = iy + pad[0];
  const ch = ih - pad[0] - pad[2];
  const cx = ix + pad[3];
  const cw = iw - pad[1] - pad[3];
  for (let i = 1; i < b.kids.length; i++) {
    const a = b.kids[i - 1];
    const k = b.kids[i];
    out.push([b.row ? { x: a.x + a.w, y: cy, w: k.x - a.x - a.w, h: ch } : { x: cx, y: a.y + a.h, w: cw, h: k.y - a.y - a.h }, true]);
  }
  return out;
}

/** Drag handles at the middle of each padding band and of each gap, in screen pixels. */
export function flexHandles(b: FlexBox, cam: Camera): FlexHandle[] {
  const z = cam.zoom;
  const s = rectToScreen(cam, b.r);
  // Keep handles clear of the frame's own resize handles when a side has little or no padding.
  const off = (i: number) => Math.max(((b.border[i] + b.pad[i] / 2) * z), 12);
  const out: FlexHandle[] = [
    { key: "pt", x: s.x + s.w / 2, y: s.y + off(0), w: 16, h: 4 },
    { key: "pb", x: s.x + s.w / 2, y: s.y + s.h - off(2), w: 16, h: 4 },
    { key: "pl", x: s.x + off(3), y: s.y + s.h / 2, w: 4, h: 16 },
    { key: "pr", x: s.x + s.w - off(1), y: s.y + s.h / 2, w: 4, h: 16 },
  ];
  if (!b.wrap) {
    for (const [a, isGap] of areas(b).slice(4)) {
      if (!isGap) continue;
      const g = rectToScreen(cam, a);
      out.push(b.row ? { key: "gap", x: g.x + g.w / 2, y: s.y + s.h / 2, w: 4, h: 16 } : { key: "gap", x: s.x + s.w / 2, y: g.y + g.h / 2, w: 16, h: 4 });
    }
  }
  return out;
}

/** Hatches `r` (screen space) with 45° lines over a faint fill. */
function hatch(p: Paint, r: Rect) {
  const { ctx } = p;
  const x1 = Math.max(r.x, 0);
  const y1 = Math.max(r.y, 0);
  const x2 = Math.min(r.x + r.w, p.w);
  const y2 = Math.min(r.y + r.h, p.h);
  if (x2 <= x1 || y2 <= y1) return;
  ctx.save();
  ctx.beginPath();
  ctx.rect(x1, y1, x2 - x1, y2 - y1);
  ctx.clip();
  ctx.fillStyle = tint(p, 14);
  ctx.fillRect(x1, y1, x2 - x1, y2 - y1);
  ctx.strokeStyle = tint(p, 32);
  ctx.lineWidth = 1;
  ctx.setLineDash([]);
  ctx.beginPath();
  const step = 6;
  for (let c = Math.floor((x1 - y2) / step) * step; c < x2 - y1; c += step) {
    ctx.moveTo(c + y1, y1);
    ctx.lineTo(c + y2, y2);
  }
  ctx.stroke();
  ctx.restore();
}

/** Dashed outlines on a flex frame's children. */
function dashKids(p: Paint, b: FlexBox) {
  const { ctx, cam } = p;
  ctx.save();
  for (const [dash, color, width] of [[[], p.halo, 3], [[3, 3], p.fg, 1]] as const) {
    ctx.setLineDash([...dash]);
    ctx.strokeStyle = color;
    ctx.globalAlpha = width === 3 ? 0.5 : 0.85;
    ctx.lineWidth = width;
    for (const k of b.kids.slice(0, 400)) {
      const s = rectToScreen(cam, k);
      if (s.x > p.w || s.y > p.h || s.x + s.w < 0 || s.y + s.h < 0) continue;
      ctx.strokeRect(crisp(s.x), crisp(s.y), Math.round(s.w), Math.round(s.h));
    }
  }
  ctx.restore();
}

export function drawFlex(p: Paint, b: FlexBox, handles: boolean) {
  for (const [a] of areas(b)) if (a.w > 0 && a.h > 0) hatch(p, rectToScreen(p.cam, a));
  dashKids(p, b);
  if (!handles) return;
  const { ctx } = p;
  for (const h of flexHandles(b, p.cam)) {
    ctx.fillStyle = p.halo;
    ctx.strokeStyle = p.fg;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.roundRect(h.x - h.w / 2, h.y - h.h / 2, h.w, h.h, 2);
    ctx.fill();
    ctx.stroke();
  }
}

// ---- measuring ----------------------------------------------------------------

/** Distance lines and pixel badges between two world rects. */
export function drawMeasure(p: Paint, a: Rect, b: Rect) {
  const { ctx, cam } = p;
  const pt = (x: number, y: number) => rectToScreen(cam, { x, y, w: 0, h: 0 });
  ctx.lineWidth = 1;
  for (const m of measureLines(a, b)) {
    const s = pt(m.x1, m.y1);
    const e = pt(m.x2, m.y2);
    ctx.setLineDash(m.dashed ? [3, 3] : []);
    ctx.beginPath();
    ctx.moveTo(crisp(s.x), crisp(s.y));
    ctx.lineTo(crisp(e.x), crisp(e.y));
    if (!m.dashed) {
      // End caps across the line.
      const h = m.y1 === m.y2;
      for (const q of [s, e]) {
        ctx.moveTo(crisp(q.x) - (h ? 0 : 3), crisp(q.y) - (h ? 3 : 0));
        ctx.lineTo(crisp(q.x) + (h ? 0 : 3), crisp(q.y) + (h ? 3 : 0));
      }
    }
    // A halo under the line keeps it readable over any fill.
    ctx.strokeStyle = p.halo;
    ctx.lineWidth = 3;
    ctx.stroke();
    ctx.strokeStyle = p.fg;
    ctx.lineWidth = 1;
    ctx.stroke();
    ctx.setLineDash([]);
    if (!m.dashed) badge(p, `${Math.round(m.value * 10) / 10}`, (s.x + e.x) / 2, (s.y + e.y) / 2);
  }
}
