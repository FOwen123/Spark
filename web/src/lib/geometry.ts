import type { Rect } from "./types";

/** Camera: world → screen is `screen = world * zoom + pan`. */
export type Camera = { x: number; y: number; zoom: number };

export const MIN_ZOOM = 0.02;
export const MAX_ZOOM = 256;

export const clampZoom = (z: number) => Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, z));

export function toWorld(cam: Camera, sx: number, sy: number) {
  return { x: (sx - cam.x) / cam.zoom, y: (sy - cam.y) / cam.zoom };
}

export function toScreen(cam: Camera, wx: number, wy: number) {
  return { x: wx * cam.zoom + cam.x, y: wy * cam.zoom + cam.y };
}

export function rectToScreen(cam: Camera, r: Rect): Rect {
  return { x: r.x * cam.zoom + cam.x, y: r.y * cam.zoom + cam.y, w: r.w * cam.zoom, h: r.h * cam.zoom };
}

/** Zooms by `factor` keeping the screen point (sx, sy) fixed. */
export function zoomAt(cam: Camera, factor: number, sx: number, sy: number): Camera {
  const zoom = clampZoom(cam.zoom * factor);
  const w = toWorld(cam, sx, sy);
  return { zoom, x: sx - w.x * zoom, y: sy - w.y * zoom };
}

/** A camera that fits `r` into a viewport of size (vw, vh) with padding, capped at 100%. */
export function fitRect(r: Rect, vw: number, vh: number, pad = 64, maxZoom = 1): Camera {
  if (r.w <= 0 || r.h <= 0) return { x: vw / 2 - r.x, y: vh / 2 - r.y, zoom: 1 };
  const zoom = clampZoom(Math.min((vw - pad * 2) / r.w, (vh - pad * 2) / r.h, maxZoom));
  return { zoom, x: (vw - r.w * zoom) / 2 - r.x * zoom, y: (vh - r.h * zoom) / 2 - r.y * zoom };
}

export function union(rects: Rect[]): Rect | null {
  if (!rects.length) return null;
  let x1 = Number.POSITIVE_INFINITY;
  let y1 = Number.POSITIVE_INFINITY;
  let x2 = Number.NEGATIVE_INFINITY;
  let y2 = Number.NEGATIVE_INFINITY;
  for (const r of rects) {
    x1 = Math.min(x1, r.x);
    y1 = Math.min(y1, r.y);
    x2 = Math.max(x2, r.x + r.w);
    y2 = Math.max(y2, r.y + r.h);
  }
  return { x: x1, y: y1, w: x2 - x1, h: y2 - y1 };
}

export function intersects(a: Rect, b: Rect) {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}

/** Normalized rect from two corner points. */
export function fromPoints(ax: number, ay: number, bx: number, by: number): Rect {
  return { x: Math.min(ax, bx), y: Math.min(ay, by), w: Math.abs(bx - ax), h: Math.abs(by - ay) };
}

export type Handle = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";
export const HANDLES: Handle[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];

/** Screen position of a resize handle on rect `r`. */
export function handlePoint(r: Rect, h: Handle) {
  const x = h.includes("w") ? r.x : h.includes("e") ? r.x + r.w : r.x + r.w / 2;
  const y = h.includes("n") ? r.y : h.includes("s") ? r.y + r.h : r.y + r.h / 2;
  return { x, y };
}

/**
 * Resizes `start` by dragging handle `h` by (dx, dy). Shift keeps the aspect ratio, Alt resizes
 * from the center. Sizes never go below 1.
 */
export function resizeRect(start: Rect, h: Handle, dx: number, dy: number, keepRatio: boolean, fromCenter: boolean): Rect {
  const sx = h.includes("w") ? -1 : h.includes("e") ? 1 : 0;
  const sy = h.includes("n") ? -1 : h.includes("s") ? 1 : 0;
  const k = fromCenter ? 2 : 1;
  let w = start.w + sx * dx * k;
  let hgt = start.h + sy * dy * k;
  if (keepRatio && start.w > 0 && start.h > 0) {
    const ratio = start.w / start.h;
    if (sx === 0) w = hgt * ratio;
    else if (sy === 0) hgt = w / ratio;
    else if (Math.abs(w / start.w) > Math.abs(hgt / start.h)) hgt = w / ratio;
    else w = hgt * ratio;
  }
  w = Math.max(1, Math.abs(w));
  hgt = Math.max(1, Math.abs(hgt));
  let x = start.x;
  let y = start.y;
  if (fromCenter) {
    x = start.x + (start.w - w) / 2;
    y = start.y + (start.h - hgt) / 2;
  } else {
    if (sx < 0) x = start.x + start.w - w;
    else if (sx === 0) x = start.x + (start.w - w) / 2;
    if (sy < 0) y = start.y + start.h - hgt;
    else if (sy === 0) y = start.y + (start.h - hgt) / 2;
  }
  return { x, y, w, h: hgt };
}

export type Measure = { x1: number; y1: number; x2: number; y2: number; value: number; dashed?: boolean };

/** Stretches along one axis between the edges of [a1, a2] and [b1, b2]: the gap, or the insets when one contains the other. */
function gaps(a1: number, a2: number, b1: number, b2: number): [number, number][] {
  if (a2 <= b1) return [[a2, b1]];
  if (b2 <= a1) return [[b2, a1]];
  if (b1 <= a1 && a2 <= b2) return [[b1, a1], [a2, b2]];
  if (a1 <= b1 && b2 <= a2) return [[a1, b1], [b2, a2]];
  return [];
}

const overlapMid = (a1: number, a2: number, b1: number, b2: number) => {
  const lo = Math.max(a1, b1);
  const hi = Math.min(a2, b2);
  return hi > lo ? (lo + hi) / 2 : null;
};

/**
 * Distance lines between rects `a` and `b` (world space), the way design tools show them with Alt held:
 * a line across each gap, at the middle of the overlap on the other axis. When the rects don't overlap
 * on that axis a dashed line reaches over to `b`.
 */
export function measureLines(a: Rect, b: Rect): Measure[] {
  const out: Measure[] = [];
  const ymid = overlapMid(a.y, a.y + a.h, b.y, b.y + b.h);
  const xmid = overlapMid(a.x, a.x + a.w, b.x, b.x + b.w);
  for (const [s, e] of gaps(a.x, a.x + a.w, b.x, b.x + b.w)) {
    if (e - s < 0.01) continue;
    const y = ymid ?? a.y + a.h / 2;
    out.push({ x1: s, y1: y, x2: e, y2: y, value: e - s });
    if (ymid === null) {
      const x = a.x + a.w <= b.x ? e : s;
      out.push({ x1: x, y1: y, x2: x, y2: y < b.y ? b.y : b.y + b.h, value: 0, dashed: true });
    }
  }
  for (const [s, e] of gaps(a.y, a.y + a.h, b.y, b.y + b.h)) {
    if (e - s < 0.01) continue;
    const x = xmid ?? a.x + a.w / 2;
    out.push({ x1: x, y1: s, x2: x, y2: e, value: e - s });
    if (xmid === null) {
      const y = a.y + a.h <= b.y ? e : s;
      out.push({ x1: x, y1: y, x2: x < b.x ? b.x : b.x + b.w, y2: y, value: 0, dashed: true });
    }
  }
  return out;
}

/** The angle in degrees of the vector from `c` to `p`, 0 pointing right and growing clockwise (screen space). */
export const angleDeg = (c: { x: number; y: number }, p: { x: number; y: number }) => (Math.atan2(p.y - c.y, p.x - c.x) * 180) / Math.PI;

/** An angle folded into (-180, 180], optionally rounded to a multiple of `step`. */
export function normalizeAngle(a: number, step = 0): number {
  const v = step ? Math.round(a / step) * step : a;
  const m = ((((v + 180) % 360) + 360) % 360) - 180;
  return m === -180 ? 180 : m;
}

/** Where `p` sits in the frame of a rect centred on `c` and turned by `deg`: (0, 0) is the centre, x runs along its width. */
export function toLocal(c: { x: number; y: number }, deg: number, p: { x: number; y: number }) {
  const r = (-deg * Math.PI) / 180;
  const dx = p.x - c.x;
  const dy = p.y - c.y;
  return { x: dx * Math.cos(r) - dy * Math.sin(r), y: dx * Math.sin(r) + dy * Math.cos(r) };
}
