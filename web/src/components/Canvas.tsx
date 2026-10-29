import { useCallback, useEffect, useLayoutEffect, useMemo, useRef } from "react";
import { type DocStore, pendingReveal, tokenVars, useDoc } from "../lib/doc";
import { loadFonts } from "../lib/fonts";
import {
  type Camera,
  type Handle,
  HANDLES,
  fitRect,
  fromPoints,
  handlePoint,
  intersects,
  rectToScreen,
  resizeRect,
  toWorld,
  union,
  zoomAt,
} from "../lib/geometry";
import type { Rect } from "../lib/types";
import { ElementRegistry, LevelOfDetail, PageContent } from "./NodeTree";
import type { Tool } from "./Toolbar";

export type Guide = { x1: number; y1: number; x2: number; y2: number; dashed?: boolean };

type Drag =
  | { kind: "pan"; sx: number; sy: number; cam: Camera }
  | { kind: "marquee"; sx: number; sy: number; scope: string; base: string[] }
  | { kind: "move"; sx: number; sy: number; started: boolean; alt: boolean; click: string | null }
  | { kind: "resize"; handle: Handle; sx: number; sy: number; start: Rect }
  | { kind: "draw"; tool: Tool; wx: number; wy: number; parent: string };

export type CanvasApi = {
  zoomTo: (r: Rect | null) => void;
  zoomBy: (f: number) => void;
  zoom100: () => void;
  panBy: (dx: number, dy: number) => void;
  editText: (id: string) => void;
  center: () => { x: number; y: number };
  /** A canvas-relative screen point in world space, using the camera as displayed. */
  toWorld: (sx: number, sy: number) => { x: number; y: number };
  rectOf: (ids: string[]) => Rect | null;
};

const LABEL_H = 18;
/** Artboards smaller than this on screen render as shells (no contents). */
const PREVIEW_PX = 32;
/** Artboards whose contents mount per level-of-detail pass (mounting is the costly part of a frame). */
const MOUNTS_PER_PASS = 1;
/** How long after opening before previews start mounting. */
const WARMUP_MS = 800;
/** Most layers the canvas keeps mounted; artboards past it stay shells (nearest the centre win). */
const NODE_BUDGET = 6000;
/** Share of the remaining distance the camera covers per 1/120 s while easing. */
const EASE = 0.6;

function cssVar(name: string) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || "#000";
}

export function Canvas(props: {
  store: DocStore;
  tool: Tool;
  setTool: (t: Tool) => void;
  cam: Camera;
  setCam: (c: Camera | ((c: Camera) => Camera)) => void;
  spaceDown: boolean;
  apiRef: React.MutableRefObject<CanvasApi | null>;
  onImageTool: () => void;
  agentBoards?: Map<string, number>;
  onContextMenu?: (e: React.MouseEvent) => void;
}) {
  const { store, tool, cam, setCam } = props;
  useDoc(store);
  const vp = useRef<HTMLDivElement>(null);
  const camEl = useRef<HTMLDivElement>(null);
  const hud = useRef<HTMLCanvasElement>(null);
  const reg = useMemo(() => new Map<string, HTMLElement | SVGElement>(), []);
  // Wheel, pinch and hand drags move the camera here, outside React (one transform per frame), and
  // commit it to React once it settles. Camera changes from elsewhere (zoom to fit…) arrive as `cam`.
  const camRef = useRef(cam);
  const camTarget = useRef(cam);
  const committed = useRef(cam);
  if (cam !== committed.current) {
    committed.current = cam;
    camRef.current = cam;
    camTarget.current = cam;
  }
  const drag = useRef<Drag | null>(null);
  const ui = useRef({
    marquee: null as Rect | null,
    preview: null as Rect | null,
    guides: [] as Guide[],
    labels: [] as { id: string; r: Rect }[],
    size: { w: 0, h: 0 },
    fitted: false,
    editing: null as string | null,
  });
  const frame = useRef(0);

  // ---- drawing -------------------------------------------------------------
  const sweeps = useMemo(() => new Map<string, { at: number; write: boolean }>(), []);
  const redraw = useRef<() => void>(() => {});
  const requestDraw = useCallback(() => {
    if (frame.current) return;
    frame.current = requestAnimationFrame(() => {
      frame.current = 0;
      redraw.current();
    });
  }, []);

  redraw.current = () => {
    const c = hud.current;
    if (!c) return;
    const ctx = c.getContext("2d");
    if (!ctx) return;
    const dpr = Math.max(window.devicePixelRatio || 1, 2);
    const { w, h } = ui.current.size;
    if (c.width !== Math.round(w * dpr)) c.width = Math.round(w * dpr);
    if (c.height !== Math.round(h * dpr)) c.height = Math.round(h * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    const cam = camRef.current;
    const fg = cssVar("--hud-fg");
    const halo = cssVar("--hud-halo");
    const muted = cssVar("--spark-fg-3");
    const outline = (r: Rect, width = 1, dash: number[] = []) => {
      ctx.setLineDash(dash);
      ctx.strokeStyle = halo;
      ctx.lineWidth = width + 2;
      ctx.strokeRect(Math.round(r.x) + 0.5, Math.round(r.y) + 0.5, Math.round(r.w), Math.round(r.h));
      ctx.strokeStyle = fg;
      ctx.lineWidth = width;
      ctx.strokeRect(Math.round(r.x) + 0.5, Math.round(r.y) + 0.5, Math.round(r.w), Math.round(r.h));
      ctx.setLineDash([]);
    };
    // Artboard labels.
    ctx.font = "11px Geist, sans-serif";
    ctx.textBaseline = "bottom";
    const labels: { id: string; r: Rect }[] = [];
    for (const id of store.kids(store.page)) {
      const n = store.nodes.get(id);
      const r = store.rects.get(id);
      if (!n || !r || n.hidden) continue;
      const s = rectToScreen(cam, r);
      if (s.x > w || s.y > h + LABEL_H || s.x + s.w < 0 || s.y < 0) {
        if (s.x + s.w < 0 || s.x > w || s.y + s.h < 0 || s.y - LABEL_H > h) continue;
      }
      let text = n.name;
      const max = Math.max(s.w, 24);
      while (text.length > 1 && ctx.measureText(text).width > max) text = `${text.slice(0, -2)}…`;
      const selected = store.selection.includes(id) || store.hover === id;
      ctx.fillStyle = selected ? fg : muted;
      ctx.fillText(text, s.x, s.y - 4);
      labels.push({ id, r: { x: s.x, y: s.y - LABEL_H, w: Math.min(ctx.measureText(text).width, max), h: LABEL_H - 2 } });
      // Agent activity: a falling-dot band along the top edge of artboards an agent is working on.
      const since = props.agentBoards?.get(id);
      if (since !== undefined) {
        const t = performance.now() / 1000;
        const cols = 32;
        const pitch = 6;
        const right = s.x + s.w;
        const left = Math.max(s.x + ctx.measureText(text).width + 12, right - cols * pitch);
        for (let i = 0; right - (i + 1) * pitch >= left; i++) {
          for (let row = 0; row < 2; row++) {
            const phase = (t * (0.9 + ((i * 37) % 10) / 30) + i * 0.37 + row * 0.5) % 1;
            ctx.globalAlpha = 0.15 + 0.85 * phase ** 4;
            ctx.fillStyle = fg;
            ctx.beginPath();
            ctx.arc(right - (i + 0.5) * pitch, s.y - 13 + row * pitch, 1.4, 0, Math.PI * 2);
            ctx.fill();
          }
        }
        ctx.globalAlpha = 1;
      }
    }
    ui.current.labels = labels;
    // Agent read/write sweeps.
    const now = performance.now();
    for (const [id, sweep] of sweeps) {
      const r = store.rects.get(id);
      const age = now - sweep.at;
      if (!r || age > 1500) {
        if (age > 1500) sweeps.delete(id);
        continue;
      }
      const s = rectToScreen(cam, r);
      const p = age / 1500;
      const band = s.h * 0.35;
      const y = sweep.write ? s.y - band + p * (s.h + band) : s.y + s.h - p * (s.h + band);
      const g = ctx.createLinearGradient(0, y, 0, y + band);
      g.addColorStop(0, "rgba(128,128,128,0)");
      g.addColorStop(0.5, `rgba(128,128,128,${(sweep.write ? 0.35 : 0.22) * (1 - p)})`);
      g.addColorStop(1, "rgba(128,128,128,0)");
      ctx.save();
      ctx.beginPath();
      ctx.rect(s.x, s.y, s.w, s.h);
      ctx.clip();
      ctx.fillStyle = g;
      ctx.fillRect(s.x, y, s.w, band);
      ctx.restore();
    }
    // Hover.
    const hover = store.hover;
    if (hover && !store.selection.includes(hover)) {
      const r = store.rects.get(hover);
      if (r) outline(rectToScreen(cam, r));
    }
    // Selection and handles.
    const sel = store.selection.map((id) => store.rects.get(id)).filter((r): r is Rect => !!r);
    for (const r of sel) outline(rectToScreen(cam, r));
    const u = union(sel);
    if (u && !ui.current.editing) {
      const s = rectToScreen(cam, u);
      if (sel.length > 1) outline(s, 1, [4, 3]);
      const locked = store.selection.some((id) => store.nodes.get(id)?.locked);
      if (!locked) {
        for (const hd of HANDLES) {
          const p = handlePoint(s, hd);
          ctx.fillStyle = halo;
          ctx.fillRect(Math.round(p.x) - 4, Math.round(p.y) - 4, 8, 8);
          ctx.strokeStyle = fg;
          ctx.lineWidth = 1;
          ctx.strokeRect(Math.round(p.x) - 3.5, Math.round(p.y) - 3.5, 7, 7);
        }
      }
      // Size badge.
      const label = `${Math.round(u.w)} × ${Math.round(u.h)}`;
      ctx.font = "10px 'Geist Mono', monospace";
      const tw = ctx.measureText(label).width + 8;
      const bx = s.x + s.w / 2 - tw / 2;
      const by = s.y + s.h + 6;
      ctx.fillStyle = fg;
      ctx.beginPath();
      ctx.roundRect(bx, by, tw, 15, 3);
      ctx.fill();
      ctx.fillStyle = halo;
      ctx.textBaseline = "middle";
      ctx.fillText(label, bx + 4, by + 8);
    }
    // Marquee and draw preview.
    for (const r of [ui.current.marquee, ui.current.preview ? rectToScreen(cam, ui.current.preview) : null]) {
      if (!r) continue;
      ctx.fillStyle = `color-mix(in srgb, ${fg} 6%, transparent)`;
      ctx.fillRect(r.x, r.y, r.w, r.h);
      outline(r);
    }
    // Snap and spacing guides: dashed lines.
    for (const g of ui.current.guides) {
      const a = rectToScreen(cam, { x: g.x1, y: g.y1, w: 0, h: 0 });
      const b = rectToScreen(cam, { x: g.x2, y: g.y2, w: 0, h: 0 });
      ctx.setLineDash(g.dashed ? [2, 2] : [4, 3]);
      ctx.strokeStyle = fg;
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(Math.round(a.x) + 0.5, Math.round(a.y) + 0.5);
      ctx.lineTo(Math.round(b.x) + 0.5, Math.round(b.y) + 0.5);
      ctx.stroke();
      ctx.setLineDash([]);
    }
    if (sweeps.size || props.agentBoards?.size) requestDraw();
  };

  // biome-ignore lint/correctness/useExhaustiveDependencies: redraw when agent activity changes
  useEffect(() => requestDraw(), [props.agentBoards, requestDraw]);

  useEffect(() => {
    window.addEventListener("spark:theme", requestDraw);
    return () => window.removeEventListener("spark:theme", requestDraw);
  }, [requestDraw]);

  // Agent activity sweeps arrive as DOM events from the app.
  useEffect(() => {
    const on = (e: Event) => {
      const d = (e as CustomEvent<{ file: string; ids: string[]; write: boolean }>).detail;
      if (d.file !== store.file.id) return;
      for (const id of d.ids) sweeps.set(id, { at: performance.now(), write: d.write });
      requestDraw();
    };
    window.addEventListener("spark:sweep", on);
    return () => window.removeEventListener("spark:sweep", on);
  }, [store, sweeps, requestDraw]);

  const detailRef = useRef<() => void>(() => {});

  // ---- measuring -----------------------------------------------------------
  const pendingMeasure = useRef<Set<string> | "all">(new Set());
  const measureFrame = useRef(0);
  const measure = useCallback(() => {
    measureFrame.current = 0;
    const v = vp.current;
    if (!v) return;
    const cam = camRef.current;
    const vr = v.getBoundingClientRect();
    ui.current.size = { w: v.clientWidth, h: v.clientHeight };
    const boards = pendingMeasure.current === "all" ? new Set(store.kids(store.page)) : pendingMeasure.current;
    pendingMeasure.current = new Set();
    const out: Record<string, [number, number, number, number]> = {};
    const visit = (id: string) => {
      const el = reg.get(id);
      if (!el) return;
      const r = el.getBoundingClientRect();
      if (r.width === 0 && r.height === 0 && (el as HTMLElement).style?.display === "none") return;
      const round = (n: number) => Math.round(n * 100) / 100;
      const rect = {
        x: round((r.left - vr.left - cam.x) / cam.zoom),
        y: round((r.top - vr.top - cam.y) / cam.zoom),
        w: round(r.width / cam.zoom),
        h: round(r.height / cam.zoom),
      };
      const old = store.rects.get(id);
      if (!old || old.x !== rect.x || old.y !== rect.y || old.w !== rect.w || old.h !== rect.h) {
        store.rects.set(id, rect);
        out[id] = [rect.x, rect.y, rect.w, rect.h];
      }
      const n = store.nodes.get(id);
      if (n?.kind === "svg") return;
      for (const k of store.kids(id)) visit(k);
    };
    for (const b of boards) {
      if (store.nodes.get(b)?.parent !== store.page) continue;
      // Shells keep the rects they arrived with; their contents aren't on the page to measure.
      if (!store.detail.has(b)) continue;
      visit(b);
    }
    if (Object.keys(out).length) {
      store.conn.send({ t: "rects", file: store.file.id, rects: out });
      store.emitRects();
    }
    if (!ui.current.fitted && store.kids(store.page).some((id) => store.rects.has(id))) {
      ui.current.fitted = true;
      const saved = localStorage.getItem(`spark-cam:${store.file.id}:${store.page}`);
      const reveal = pendingReveal.get(store.file.id);
      const revealRect = reveal ? store.rects.get(reveal) : undefined;
      if (reveal) pendingReveal.delete(store.file.id);
      if (revealRect) props.apiRef.current?.zoomTo(revealRect);
      else if (saved) setCam(JSON.parse(saved));
      else props.apiRef.current?.zoomTo(union(store.kids(store.page).map((id) => store.rects.get(id)).filter((r): r is Rect => !!r)));
    }
    requestDraw();
  }, [store, reg, requestDraw, setCam, props.apiRef]);

  const scheduleMeasure = useCallback(
    (ids: Iterable<string> | "all") => {
      if (ids === "all") pendingMeasure.current = "all";
      else if (pendingMeasure.current !== "all") {
        for (const id of ids) {
          const b = store.nodes.has(id) ? store.artboardOf(id) : store.pages.some((p) => p.id === id) ? null : undefined;
          if (b) pendingMeasure.current.add(b);
          else if (b === null) pendingMeasure.current = "all";
          if (pendingMeasure.current === "all") break;
        }
      }
      if (!measureFrame.current) measureFrame.current = requestAnimationFrame(measure);
    },
    [store, measure],
  );

  useEffect(() => {
    scheduleMeasure("all");
    loadFonts(store);
    const off = store.subscribe(() => {
      // New or moved artboards (e.g. from an agent) may need their contents mounted.
      detailRef.current();
      scheduleMeasure(store.touched);
      loadFonts(store, store.touched);
      requestDraw();
    });
    // Text reflows when a downloaded font arrives.
    const remeasure = () => scheduleMeasure("all");
    document.fonts.ready.then(remeasure);
    document.fonts.addEventListener("loadingdone", remeasure);
    return () => {
      off();
      document.fonts.removeEventListener("loadingdone", remeasure);
    };
  }, [store, scheduleMeasure, requestDraw]);

  // Page switches: refit (or restore) the camera.
  // biome-ignore lint/correctness/useExhaustiveDependencies: runs when the viewed page changes
  useEffect(() => {
    ui.current.fitted = false;
    updateDetail();
    scheduleMeasure("all");
  }, [store.page]);

  // ---- camera, level of detail ---------------------------------------------------------
  // Artboards mount their contents when on (or near) screen and at least PREVIEW_PX across, so
  // zoomed-out pages still preview, nearest the view centre first and only while the mounted layers
  // stay within NODE_BUDGET; the rest render as sized shells. Artboards without a known rect mount
  // so they can be measured. A pass mounts at most MOUNTS_PER_PASS new artboards and schedules
  // another next frame, so zooming out over a big page doesn't mount them all in one frame.
  const camFrame = useRef(0);
  const sizes = useRef({ version: -1, count: new Map<string, number>() });
  const opened = useRef(performance.now());
  const layersIn = useCallback(
    (id: string): number => {
      if (sizes.current.version !== store.version) sizes.current = { version: store.version, count: new Map() };
      let n = sizes.current.count.get(id);
      if (n === undefined) {
        n = 1;
        for (const k of store.kids(id)) n += layersIn(k);
        sizes.current.count.set(id, n);
      }
      return n;
    },
    [store],
  );
  const updateDetail = useCallback(() => {
    const cam = camRef.current;
    const { w, h } = ui.current.size;
    if (!w || !h) return;
    const view = { x: -cam.x / cam.zoom, y: -cam.y / cam.zoom, w: w / cam.zoom, h: h / cam.zoom };
    const margin = Math.max(view.w, view.h) * 0.25;
    const grown = { x: view.x - margin, y: view.y - margin, w: view.w + 2 * margin, h: view.h + 2 * margin };
    const cx = view.x + view.w / 2;
    const cy = view.y + view.h / 2;
    const wanted: { id: string; d: number }[] = [];
    for (const id of store.kids(store.page)) {
      if (store.nodes.get(id)?.hidden) continue;
      const r = store.rects.get(id);
      if (!r) wanted.push({ id, d: -1 });
      else if (intersects(r, grown) && Math.max(r.w, r.h) * cam.zoom >= PREVIEW_PX) wanted.push({ id, d: Math.hypot(r.x + r.w / 2 - cx, r.y + r.h / 2 - cy) });
    }
    wanted.sort((a, b) => a.d - b.d);
    const next = new Set<string>();
    let layers = 0;
    // Previews wait until the file's first frames (canvas, layer list) are on screen.
    const warm = performance.now() - opened.current;
    const allowed = warm < WARMUP_MS ? 0 : MOUNTS_PER_PASS;
    let mounts = 0;
    let more = false;
    for (const { id, d } of wanted) {
      layers += layersIn(id);
      if (layers > NODE_BUDGET && next.size) break;
      if (d < 0 || store.detail.has(id)) next.add(id);
      else if (mounts < allowed) {
        next.add(id);
        mounts++;
      } else more = true;
    }
    const shown = store.setDetail(next);
    if (shown.length) scheduleMeasure(shown);
    // During a gesture the next pass runs next frame so previews keep up; otherwise (opening a file,
    // at rest) it waits for an idle moment so it never delays the UI's own first frames.
    if (more) {
      const wait = Math.max(WARMUP_MS - warm, 16);
      if (camFrame.current && warm >= WARMUP_MS) requestAnimationFrame(() => detailRef.current());
      else if ("requestIdleCallback" in window) requestIdleCallback(() => detailRef.current(), { timeout: wait + 200 });
      else setTimeout(() => detailRef.current(), wait);
    }
  }, [store, scheduleMeasure, layersIn]);
  detailRef.current = updateDetail;

  useLayoutEffect(() => {
    const el = camEl.current;
    if (!el) return;
    const c = camRef.current;
    el.style.transform = `matrix3d(${c.zoom},0,0,0,0,${c.zoom},0,0,0,0,1,0,${c.x},${c.y},0,1)`;
    if (ui.current.fitted) localStorage.setItem(`spark-cam:${store.file.id}:${store.page}`, JSON.stringify(cam));
    updateDetail();
    requestDraw();
  }, [cam, store, updateDetail, requestDraw]);

  // Gestures: ease toward the target (a trackpad's stream of small deltas reads smooth, frame-rate
  // independent), write the transform and HUD straight away, and commit to React after a pause.
  const camTick = useRef({ last: 0, detail: 0, settle: 0 });
  const stepCam = useRef<(t: number) => void>(() => {});
  stepCam.current = (t) => {
    camFrame.current = 0;
    const tick = camTick.current;
    const k = 1 - (1 - EASE) ** (Math.max(t - tick.last, 1) / (1000 / 120));
    tick.last = t;
    const c = camRef.current;
    const g = camTarget.current;
    let next = { x: c.x + (g.x - c.x) * k, y: c.y + (g.y - c.y) * k, zoom: c.zoom + (g.zoom - c.zoom) * k };
    const done = Math.abs(g.x - next.x) < 0.5 && Math.abs(g.y - next.y) < 0.5 && (Math.abs(g.zoom - next.zoom) / g.zoom) * ui.current.size.w < 0.5;
    if (done) next = g;
    camRef.current = next;
    if (camEl.current) camEl.current.style.transform = `matrix3d(${next.zoom},0,0,0,0,${next.zoom},0,0,0,0,1,0,${next.x},${next.y},0,1)`;
    redraw.current();
    if (done || t - tick.detail > 120) {
      tick.detail = t;
      updateDetail();
    }
    if (!done) camFrame.current = requestAnimationFrame((t2) => stepCam.current(t2));
    clearTimeout(tick.settle);
    tick.settle = window.setTimeout(() => {
      committed.current = camRef.current;
      setCam(camRef.current);
    }, 150);
  };
  const moveCam = useCallback((next: Camera, ease: boolean) => {
    camTarget.current = next;
    if (!ease) {
      // The camera and the displayed transform change together: a measure in between reads both.
      camRef.current = next;
      if (camEl.current) camEl.current.style.transform = `matrix3d(${next.zoom},0,0,0,0,${next.zoom},0,0,0,0,1,0,${next.x},${next.y},0,1)`;
    }
    if (!camFrame.current) {
      camTick.current.last = performance.now();
      camFrame.current = requestAnimationFrame((t) => stepCam.current(t));
    }
  }, []);
  useEffect(() => () => {
    cancelAnimationFrame(camFrame.current);
    clearTimeout(camTick.current.settle);
  }, []);

  useEffect(() => {
    const v = vp.current;
    if (!v) return;
    const ro = new ResizeObserver(() => {
      ui.current.size = { w: v.clientWidth, h: v.clientHeight };
      updateDetail();
      requestDraw();
    });
    ro.observe(v);
    return () => ro.disconnect();
  }, [updateDetail, requestDraw]);

  const api: CanvasApi = {
    zoomTo: (r) => {
      if (!r) return;
      const { w, h } = ui.current.size;
      setCam(fitRect(r, w, h, 64, 1));
    },
    zoomBy: (f) => setCam(zoomAt(camTarget.current, f, ui.current.size.w / 2, ui.current.size.h / 2)),
    zoom100: () => setCam(zoomAt(camTarget.current, 1 / camTarget.current.zoom, ui.current.size.w / 2, ui.current.size.h / 2)),
    panBy: (dx, dy) => setCam({ ...camTarget.current, x: camTarget.current.x + dx, y: camTarget.current.y + dy }),
    editText: (id) => startEditing(id),
    center: () => toWorld(camRef.current, ui.current.size.w / 2, ui.current.size.h / 2),
    toWorld: (sx, sy) => toWorld(camRef.current, sx, sy),
    rectOf: (ids) => union(ids.map((id) => store.rects.get(id)).filter((r): r is Rect => !!r)),
  };
  props.apiRef.current = api;

  // ---- wheel ---------------------------------------------------------------
  useEffect(() => {
    const v = vp.current;
    if (!v) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const r = v.getBoundingClientRect();
      if (e.ctrlKey || e.metaKey) {
        const f = Math.exp(-e.deltaY * (e.deltaMode === 1 ? 0.05 : 0.01));
        moveCam(zoomAt(camTarget.current, f, e.clientX - r.left, e.clientY - r.top), true);
      } else {
        const k = e.deltaMode === 1 ? 16 : 1;
        const dx = e.shiftKey && !e.deltaX ? e.deltaY : e.deltaX;
        const dy = e.shiftKey && !e.deltaX ? 0 : e.deltaY;
        moveCam({ ...camTarget.current, x: camTarget.current.x - dx * k, y: camTarget.current.y - dy * k }, true);
      }
    };
    v.addEventListener("wheel", onWheel, { passive: false });
    return () => v.removeEventListener("wheel", onWheel);
  }, [moveCam]);

  // ---- hit testing ---------------------------------------------------------
  const local = (e: { clientX: number; clientY: number }) => {
    const r = vp.current?.getBoundingClientRect();
    return { x: e.clientX - (r?.left ?? 0), y: e.clientY - (r?.top ?? 0) };
  };

  const hitAt = (clientX: number, clientY: number): string | null => {
    for (const el of document.elementsFromPoint(clientX, clientY)) {
      if (!camEl.current?.contains(el)) continue;
      const id = (el as HTMLElement).closest?.("[data-node]")?.getAttribute("data-node");
      if (id && store.nodes.has(id)) return id;
    }
    return null;
  };

  /** The node a click selects: same depth as the current selection, else the artboard's child. */
  const selectable = (id: string | null, deep: boolean): string | null => {
    if (!id) return null;
    let chain = [id, ...store.ancestors(id)];
    // SVG internals select as their svg unless deep-selecting.
    const svgAt = chain.findIndex((c) => store.nodes.get(c)?.kind === "svg");
    if (svgAt > 0 && !deep) chain = chain.slice(svgAt);
    if (chain.some((c) => store.nodes.get(c)?.locked)) return null;
    if (deep) return chain[0];
    for (const s of store.selection) if (chain.includes(s)) return s;
    for (const s of store.selection) {
      const p = store.nodes.get(s)?.parent;
      const c = chain.find((c) => store.nodes.get(c)?.parent === p);
      if (c) return c;
    }
    if (chain.length === 1) return store.kids(chain[0]).length ? null : chain[0];
    return chain[chain.length - 2];
  };

  /** The deepest frame under a point, for new nodes; the page if none. */
  const frameAt = (clientX: number, clientY: number): string => {
    for (const el of document.elementsFromPoint(clientX, clientY)) {
      if (!camEl.current?.contains(el)) continue;
      const f = (el as HTMLElement).closest?.('[data-kind="frame"]');
      const id = f?.getAttribute("data-node");
      if (id && !store.nodes.get(id)?.locked) return id;
    }
    return store.page;
  };

  /** The selection's bounds in world space, read live from the DOM (measured rects can lag a frame). */
  const liveSelection = (): Rect | null => {
    const v = vp.current?.getBoundingClientRect();
    const cam = camRef.current;
    const rects = store.selection.map((id) => {
      const el = reg.get(id);
      if (!el || !v) return store.rects.get(id);
      const r = el.getBoundingClientRect();
      return { x: (r.left - v.left - cam.x) / cam.zoom, y: (r.top - v.top - cam.y) / cam.zoom, w: r.width / cam.zoom, h: r.height / cam.zoom };
    });
    return union(rects.filter((r): r is Rect => !!r));
  };

  const handleAt = (p: { x: number; y: number }): Handle | null => {
    if (store.selection.some((id) => store.nodes.get(id)?.locked)) return null;
    const u = liveSelection();
    if (!u) return null;
    const s = rectToScreen(camRef.current, u);
    for (const hd of HANDLES) {
      const hp = handlePoint(s, hd);
      if (Math.abs(hp.x - p.x) <= 6 && Math.abs(hp.y - p.y) <= 6) return hd;
    }
    return null;
  };

  // ---- text editing --------------------------------------------------------
  function startEditing(id: string) {
    const el = reg.get(id) as HTMLElement | undefined;
    const n = store.nodes.get(id);
    if (!el || n?.kind !== "text") return;
    const original = n.text ?? "";
    ui.current.editing = id;
    store.select([id]);
    el.contentEditable = "plaintext-only";
    el.spellcheck = false;
    el.style.outline = "none";
    el.style.cursor = "text";
    el.focus();
    const range = document.createRange();
    range.selectNodeContents(el);
    getSelection()?.removeAllRanges();
    getSelection()?.addRange(range);
    const finish = (commit: boolean) => {
      el.removeEventListener("blur", onBlur);
      el.removeEventListener("keydown", onKey);
      const text = el.innerText.replace(/\n$/, "");
      el.contentEditable = "false";
      el.removeAttribute("contenteditable");
      el.style.cursor = "";
      // Hand the DOM back to React exactly as it rendered it; the patch brings the new text.
      el.textContent = original;
      ui.current.editing = null;
      if (commit && text !== original) {
        if (text.trim() === "" && original === "") store.do({ type: "delete", ids: [id] });
        else store.do({ type: "set", ids: [id], prop: "text", value: text });
      } else if (original === "" && text.trim() === "") store.do({ type: "delete", ids: [id] });
      requestDraw();
    };
    const onBlur = () => finish(true);
    const onKey = (e: KeyboardEvent) => {
      e.stopPropagation();
      if (e.key === "Escape" || (e.key === "Enter" && (e.metaKey || e.ctrlKey))) {
        e.preventDefault();
        el.blur();
      }
    };
    el.addEventListener("blur", onBlur);
    el.addEventListener("keydown", onKey);
    requestDraw();
  }

  // ---- pointer -------------------------------------------------------------
  const pendingDrag = useRef<{ dx: number; dy: number; x: number; y: number; snap: boolean } | null>(null);
  const pendingResize = useRef<Rect | null>(null);
  const sending = useRef(false);
  const snapOff = useRef(false);
  const flush = useCallback(async () => {
    if (sending.current) return;
    sending.current = true;
    try {
      while (pendingDrag.current || pendingResize.current) {
        if (pendingDrag.current) {
          const d = pendingDrag.current;
          pendingDrag.current = null;
          const r = await store.do<{ guides: Guide[] }>({ type: "drag", ...d, zoom: camRef.current.zoom }).catch(() => null);
          ui.current.guides = r?.guides ?? [];
        }
        if (pendingResize.current) {
          const rect = pendingResize.current;
          pendingResize.current = null;
          const r = await store.do<{ guides: Guide[] }>({ type: "resize", rect, snap: !snapOff.current, zoom: camRef.current.zoom }).catch(() => null);
          ui.current.guides = r?.guides ?? [];
        }
        requestDraw();
      }
    } finally {
      sending.current = false;
    }
  }, [store, requestDraw]);

  const onPointerDown = (e: React.PointerEvent) => {
    if (ui.current.editing) return;
    const p = local(e);
    const v = vp.current;
    if (e.button === 1 || props.spaceDown || tool === "hand") {
      e.preventDefault();
      drag.current = { kind: "pan", sx: e.clientX, sy: e.clientY, cam: camRef.current };
      v?.setPointerCapture(e.pointerId);
      return;
    }
    if (e.button === 2) {
      const hit = selectable(hitAt(e.clientX, e.clientY), e.metaKey || e.ctrlKey);
      if (hit && !store.selection.includes(hit)) store.select([hit]);
      return;
    }
    if (e.button !== 0) return;
    v?.setPointerCapture(e.pointerId);
    const w = toWorld(camRef.current, p.x, p.y);
    if (tool === "frame" || tool === "rect" || tool === "text") {
      drag.current = { kind: "draw", tool, wx: w.x, wy: w.y, parent: frameAt(e.clientX, e.clientY) };
      return;
    }
    if (tool !== "move") return;
    const handle = handleAt(p);
    if (handle) {
      const start = liveSelection();
      if (start) {
        drag.current = { kind: "resize", handle, sx: p.x, sy: p.y, start };
        store.do({ type: "resizeStart", ids: store.selection });
        return;
      }
    }
    const label = ui.current.labels.find((l) => p.x >= l.r.x && p.x <= l.r.x + l.r.w && p.y >= l.r.y && p.y <= l.r.y + l.r.h);
    const hit = label && !store.nodes.get(label.id)?.locked ? label.id : selectable(hitAt(e.clientX, e.clientY), e.metaKey || e.ctrlKey);
    if (e.shiftKey && hit) {
      store.select(store.selection.includes(hit) ? store.selection.filter((s) => s !== hit) : [...store.selection, hit]);
      return;
    }
    const inSelection = (() => {
      const u = union(store.selection.map((id) => store.rects.get(id)).filter((r): r is Rect => !!r));
      return !!u && w.x >= u.x && w.x <= u.x + u.w && w.y >= u.y && w.y <= u.y + u.h;
    })();
    if (hit || (inSelection && store.selection.length)) {
      let click: string | null = null;
      if (hit && !store.selection.includes(hit)) store.select([hit]);
      else if (hit) click = hit;
      drag.current = { kind: "move", sx: p.x, sy: p.y, started: false, alt: e.altKey, click };
      return;
    }
    // Marquee: inside an artboard it picks the artboard's children, on the canvas it picks artboards.
    const under = hitAt(e.clientX, e.clientY);
    const scope = under ? (store.artboardOf(under) ?? store.page) : store.page;
    const base = e.shiftKey ? store.selection : [];
    if (!e.shiftKey) store.select([]);
    drag.current = { kind: "marquee", sx: p.x, sy: p.y, scope, base };
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const p = local(e);
    const d = drag.current;
    if (!d) {
      if (ui.current.editing || tool !== "move") return;
      const label = ui.current.labels.find((l) => p.x >= l.r.x && p.x <= l.r.x + l.r.w && p.y >= l.r.y && p.y <= l.r.y + l.r.h);
      const hover = label ? label.id : selectable(hitAt(e.clientX, e.clientY), e.metaKey || e.ctrlKey);
      if (hover !== store.hover) {
        // Only the HUD shows canvas hover; a full emit would re-render every document subscriber.
        store.hover = hover;
        requestDraw();
      }
      const v = vp.current;
      const hd = handleAt(p);
      if (v) v.style.cursor = hd ? (hd === "n" || hd === "s" ? "ns-resize" : hd === "e" || hd === "w" ? "ew-resize" : hd === "nw" || hd === "se" ? "nwse-resize" : "nesw-resize") : "";
      return;
    }
    const cam = camRef.current;
    if (d.kind === "pan") {
      moveCam({ ...d.cam, x: d.cam.x + e.clientX - d.sx, y: d.cam.y + e.clientY - d.sy }, false);
    } else if (d.kind === "marquee") {
      const m = fromPoints(d.sx, d.sy, p.x, p.y);
      ui.current.marquee = m;
      const wr = { x: (m.x - cam.x) / cam.zoom, y: (m.y - cam.y) / cam.zoom, w: m.w / cam.zoom, h: m.h / cam.zoom };
      const hits = store.kids(d.scope).filter((id) => {
        const n = store.nodes.get(id);
        const r = store.rects.get(id);
        return n && !n.hidden && !n.locked && r && intersects(r, wr);
      });
      const next = [...new Set([...d.base, ...hits])];
      if (next.join() !== store.selection.join()) store.select(next);
      requestDraw();
    } else if (d.kind === "move") {
      const dx = (p.x - d.sx) / cam.zoom;
      const dy = (p.y - d.sy) / cam.zoom;
      if (!d.started) {
        if (Math.hypot(p.x - d.sx, p.y - d.sy) < 3) return;
        d.started = true;
        store.do({ type: "dragStart", ids: store.selection, duplicate: d.alt });
      }
      const wp = toWorld(cam, p.x, p.y);
      pendingDrag.current = { dx, dy, x: wp.x, y: wp.y, snap: !(e.metaKey || e.ctrlKey) };
      flush();
    } else if (d.kind === "resize") {
      const r = resizeRect(d.start, d.handle, (p.x - d.sx) / cam.zoom, (p.y - d.sy) / cam.zoom, e.shiftKey, e.altKey);
      snapOff.current = e.metaKey || e.ctrlKey || e.shiftKey || e.altKey;
      pendingResize.current = r;
      flush();
    } else if (d.kind === "draw") {
      const w = toWorld(cam, p.x, p.y);
      let r = fromPoints(d.wx, d.wy, w.x, w.y);
      if (e.shiftKey) {
        const s = Math.max(r.w, r.h);
        r = { x: w.x < d.wx ? d.wx - s : d.wx, y: w.y < d.wy ? d.wy - s : d.wy, w: s, h: s };
      }
      ui.current.preview = r;
      requestDraw();
    }
  };

  const onPointerUp = async (e: React.PointerEvent) => {
    const d = drag.current;
    drag.current = null;
    vp.current?.releasePointerCapture?.(e.pointerId);
    if (!d) return;
    if (d.kind === "marquee") {
      ui.current.marquee = null;
      requestDraw();
    } else if (d.kind === "move") {
      if (d.started) {
        while (sending.current || pendingDrag.current) await new Promise((r) => setTimeout(r, 4));
        const p = local(e);
        const w = toWorld(camRef.current, p.x, p.y);
        await store.do({ type: "dragEnd", x: w.x, y: w.y, reparent: !e.metaKey && !e.ctrlKey });
        ui.current.guides = [];
        requestDraw();
      } else if (d.click && !e.shiftKey && store.selection.length > 1) store.select([d.click]);
    } else if (d.kind === "resize") {
      while (sending.current || pendingResize.current) await new Promise((r) => setTimeout(r, 4));
      await store.do({ type: "resizeEnd" });
      ui.current.guides = [];
      requestDraw();
    } else if (d.kind === "draw") {
      const r = ui.current.preview;
      ui.current.preview = null;
      requestDraw();
      const kind = d.tool === "frame" ? "frame" : d.tool === "rect" ? "rect" : "text";
      const clicked = !r || (r.w < 2 && r.h < 2);
      const size = kind === "text" ? { w: clicked ? 0 : (r?.w ?? 0), h: 0 } : clicked ? { w: 100, h: 100 } : { w: r.w, h: r.h };
      const at = clicked ? { x: d.wx, y: d.wy } : { x: r.x, y: r.y };
      const res = await store.do<{ id: string }>({
        type: "create",
        kind,
        parent: d.parent,
        x: at.x,
        y: at.y,
        w: size.w,
        h: size.h,
        text: kind === "text" ? "" : undefined,
      });
      props.setTool("move");
      if (kind === "text") requestAnimationFrame(() => requestAnimationFrame(() => startEditing(res.id)));
    }
  };

  const onDoubleClick = (e: React.MouseEvent) => {
    if (tool !== "move" || ui.current.editing) return;
    const raw = hitAt(e.clientX, e.clientY);
    if (!raw) return;
    const chain = [raw, ...store.ancestors(raw)];
    const sel = store.selection.length === 1 ? store.selection[0] : null;
    const n = sel ? store.nodes.get(sel) : undefined;
    if (sel && n?.kind === "text" && chain.includes(sel)) return startEditing(sel);
    const i = sel ? chain.indexOf(sel) : -1;
    const next = i > 0 ? chain[i - 1] : selectable(raw, false);
    if (!next) return;
    if (store.nodes.get(next)?.kind === "text" && next === raw && sel === next) return startEditing(next);
    store.select([next]);
  };

  const page = store.pages.find((p) => p.id === store.page);
  return (
    <ElementRegistry.Provider value={reg}>
      {/* biome-ignore lint/a11y/noStaticElementInteractions: the canvas is a pointer surface */}
      <div
        ref={vp}
        data-testid="canvas"
        className="relative min-w-0 flex-1 overflow-hidden"
        style={{
          ...tokenVars(store.tokens),
          background: page?.canvas ?? "var(--spark-canvas)",
          cursor: props.spaceDown || tool === "hand" ? "grab" : tool === "move" ? "default" : "crosshair",
        }}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerLeave={() => {
          if (store.hover && !drag.current) {
            store.hover = null;
            requestDraw();
          }
        }}
        onDoubleClick={onDoubleClick}
        onContextMenu={props.onContextMenu}
      >
        <div ref={camEl} className="spark-camera" data-testid="camera">
          <LevelOfDetail.Provider value={true}>
            <PageContent store={store} page={store.page} />
          </LevelOfDetail.Provider>
        </div>
        <canvas ref={hud} className="pointer-events-none absolute inset-0 size-full" data-testid="hud" />
      </div>
    </ElementRegistry.Provider>
  );
}
