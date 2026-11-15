import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { type DocStore, pendingReveal, themeCss, tokenVars, useDoc } from "../lib/doc";
import { loadFonts } from "../lib/fonts";
import {
  type Camera,
  type Handle,
  HANDLES,
  MAX_ZOOM,
  angleDeg,
  fitRect,
  fromPoints,
  handlePoint,
  intersects,
  normalizeAngle,
  rectToScreen,
  resizeRect,
  toLocal,
  toScreen,
  toWorld,
  union,
  zoomAt,
} from "../lib/geometry";
import { prefs } from "../lib/prefs";
import type { Rect } from "../lib/types";
import { getView, onViewChange } from "../lib/view";
import { FLEX_PROP, type FlexKey, type Paint, badge, drawFlex, drawLayoutGuides, drawMeasure, drawPixelGrid, flexBox, flexHandles, isFlex } from "./canvasOverlays";
import { Rename } from "./LeftPanel";
import { ElementRegistry, LevelOfDetail, PageContent } from "./NodeTree";
import type { Tool } from "./Toolbar";

export type Guide = { x1: number; y1: number; x2: number; y2: number; dashed?: boolean };

/** A drag the server follows: `frame` is its newest command still waiting to be sent. */
type Live = { frame?: Record<string, unknown> };
type Drag =
  | { kind: "pan"; sx: number; sy: number; cam: Camera; right?: boolean }
  | { kind: "marquee"; sx: number; sy: number; scope: string; base: string[] }
  | ({ kind: "move"; sx: number; sy: number; started: boolean; alt: boolean; click: string | null } & Live)
  | ({ kind: "resize"; handle: Handle; sx: number; sy: number; start: Rect } & Live)
  | { kind: "draw"; tool: Tool; wx: number; wy: number; parent: string }
  | ({ kind: "flex"; id: string; key: FlexKey; sx: number; sy: number; start: number } & Live)
  | ({ kind: "rotate"; id: string; c: { x: number; y: number }; a0: number; start: number } & Live)
  | { kind: "zoom"; sx: number; sy: number; alt: boolean };

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
/** How far outside a selected node's corner (screen px) a drag rotates it. */
const ROTATE_ZONE = 18;
const ROTATE_ARROW = '<path d="M19 12a7 7 0 1 1-2.1-5"/><path d="M19.5 3v4.5H15"/>';
const ROTATE_CURSOR = `url("data:image/svg+xml,${encodeURIComponent(
  `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke-linecap="round" stroke-linejoin="round"><g stroke="#fff" stroke-width="4.5">${ROTATE_ARROW}</g><g stroke="#000" stroke-width="2">${ROTATE_ARROW}</g></svg>`,
)}") 12 12, crosshair`;
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
    /** Alt-measure target, held keys and the last pointer position (client coordinates). */
    measure: null as string | null,
    alt: false,
    z: false,
    pointer: null as { x: number; y: number } | null,
  });
  const [renaming, setRenaming] = useState<{ id: string; r: Rect } | null>(null);
  const frame = useRef(0);
  /** Lets a right-click through to the context menu when right-drag panning holds back the native event. */
  const menuOk = useRef(false);

  /** The flex frame whose padding and gap handles are live: the selected one, else the hovered one. */
  const flexTarget = (): string | null => {
    const one = store.selection.length === 1 ? store.selection[0] : null;
    const id = isFlex(store, one) ? one : isFlex(store, store.hover) ? store.hover : null;
    return id && !store.nodes.get(id)?.locked ? id : null;
  };

  /** A turned node's own box (unturned size, world centre); measured rects only hold its bounding box. */
  const rotBox = (id: string) => {
    const deg = store.nodes.get(id)?.style.rotate;
    const el = reg.get(id);
    const r = store.rects.get(id);
    if (!deg || !r || !(el instanceof HTMLElement)) return null;
    const cs = getComputedStyle(el);
    const [w, h] = [Number.parseFloat(cs.width), Number.parseFloat(cs.height)];
    return Number.isFinite(w) && Number.isFinite(h) ? { c: { x: r.x + r.w / 2, y: r.y + r.h / 2 }, w, h, deg } : null;
  };

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
    const paint: Paint = { ctx, cam, w, h, fg, halo };
    const view = getView();
    if (view.pixelGrid) drawPixelGrid(paint);
    if (view.layoutGuides) drawLayoutGuides(paint, store);
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
        const t = prefs().reduceMotion ? 0 : performance.now() / 1000;
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
    // Flex frames: padding and gap areas, dashed children and (for the one in reach) drag handles.
    const flexOn = new Set([...store.selection.slice(0, 20), store.hover].filter((id): id is string => isFlex(store, id)));
    const handled = flexTarget();
    for (const id of flexOn) {
      const b = flexBox(store, reg, id);
      if (b) drawFlex(paint, b, id === handled && !ui.current.editing);
    }
    // Selection and handles.
    const sel = store.selection.map((id) => store.rects.get(id)).filter((r): r is Rect => !!r);
    const rot = store.selection.length === 1 ? rotBox(store.selection[0]) : null;
    if (!rot) for (const r of sel) outline(rectToScreen(cam, r));
    const u = union(sel);
    if (rot && !ui.current.editing) {
      // A turned node: outline and size badge turn with it.
      const c = toScreen(cam, rot.c.x, rot.c.y);
      const hw = (rot.w * cam.zoom) / 2;
      const hh = (rot.h * cam.zoom) / 2;
      ctx.save();
      ctx.translate(c.x, c.y);
      ctx.rotate((rot.deg * Math.PI) / 180);
      outline({ x: -hw, y: -hh, w: 2 * hw, h: 2 * hh });
      badge(paint, `${Math.round(rot.w)} × ${Math.round(rot.h)}`, 0, hh + 13.5);
      ctx.restore();
    } else if (u && !ui.current.editing) {
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
      badge(paint, `${Math.round(u.w)} × ${Math.round(u.h)}`, s.x + s.w / 2, s.y + s.h + 13.5);
    }
    // Alt held: distances from the selection to what the pointer is over.
    const target = ui.current.alt && ui.current.measure ? store.rects.get(ui.current.measure) : undefined;
    if (target && u) {
      outline(rectToScreen(cam, target), 1, [4, 3]);
      drawMeasure(paint, u, target);
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
    if ((sweeps.size || props.agentBoards?.size) && !prefs().reduceMotion) requestDraw();
  };

  // biome-ignore lint/correctness/useExhaustiveDependencies: redraw when agent activity changes
  useEffect(() => requestDraw(), [props.agentBoards, requestDraw]);

  useEffect(() => {
    window.addEventListener("spark:theme", requestDraw);
    const off = onViewChange(requestDraw);
    return () => {
      window.removeEventListener("spark:theme", requestDraw);
      off();
    };
  }, [requestDraw]);

  // Held keys: Alt measures, Z is the zoom brush.
  const syncCursor = () => {
    if (vp.current && !drag.current) vp.current.style.cursor = ui.current.z ? (ui.current.alt ? "zoom-out" : "zoom-in") : "";
  };
  const updateMeasure = () => {
    const u = ui.current;
    let next: string | null = null;
    const raw = u.alt && u.pointer && store.selection.length && !drag.current && tool === "move" ? hitAt(u.pointer.x, u.pointer.y) : null;
    if (raw) {
      // The node under the pointer; over the selection itself (or inside it), its parent.
      const chain = [raw, ...store.ancestors(raw)];
      const i = chain.findIndex((c) => store.selection.includes(c));
      next = i < 0 ? raw : (chain[i + 1] ?? null);
    }
    if (next !== u.measure) {
      u.measure = next;
      requestDraw();
    }
  };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const down = e.type === "keydown";
      const el = e.target as HTMLElement | null;
      const typing = !!el && (el.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName));
      if (e.key === "Alt") {
        ui.current.alt = down;
        updateMeasure();
      }
      if (e.code === "KeyZ" && !(down && (typing || e.metaKey || e.ctrlKey))) ui.current.z = down;
      syncCursor();
    };
    const reset = () => {
      ui.current.alt = ui.current.z = false;
      updateMeasure();
      syncCursor();
    };
    // Escape abandons a drag. Capture phase: it must not also reach the editor's Escape (select parent).
    const onEscape = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || !drag.current) return;
      e.stopPropagation();
      e.preventDefault();
      cancelDrag();
    };
    window.addEventListener("keydown", onEscape, true);
    window.addEventListener("keydown", onKey);
    window.addEventListener("keyup", onKey);
    window.addEventListener("blur", reset);
    return () => {
      window.removeEventListener("keydown", onEscape, true);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("keyup", onKey);
      window.removeEventListener("blur", reset);
    };
  });

  // Agent activity sweeps arrive as DOM events from the app.
  useEffect(() => {
    const on = (e: Event) => {
      const d = (e as CustomEvent<{ file: string; ids: string[]; write: boolean }>).detail;
      if (d.file !== store.file.id) return;
      if (!prefs().reduceMotion) for (const id of d.ids) sweeps.set(id, { at: performance.now(), write: d.write });
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

  // A measurement sent a frame later would reach the server after the next command and roll its
  // layout back to before that command, so commands send it first (see DocStore.do).
  useEffect(() => {
    store.flushMeasure = () => {
      if (!measureFrame.current) return;
      cancelAnimationFrame(measureFrame.current);
      measure();
    };
    return () => {
      store.flushMeasure = undefined;
    };
  }, [store, measure]);

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

  /** Screen point zooming holds still: the selection's center when "zoom centers on selection" is on. */
  const zoomFocus = useCallback(() => {
    const u = prefs().zoomToSelection ? union(store.selection.map((id) => store.rects.get(id)).filter((r): r is Rect => !!r)) : null;
    const cam = camTarget.current;
    return u ? { x: (u.x + u.w / 2) * cam.zoom + cam.x, y: (u.y + u.h / 2) * cam.zoom + cam.y } : null;
  }, [store]);

  const api: CanvasApi = {
    zoomTo: (r) => {
      if (!r) return;
      const { w, h } = ui.current.size;
      setCam(fitRect(r, w, h, 64, 1));
    },
    zoomBy: (f) => {
      const at = zoomFocus() ?? { x: ui.current.size.w / 2, y: ui.current.size.h / 2 };
      setCam(zoomAt(camTarget.current, f, at.x, at.y));
    },
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
      const p = prefs();
      if (e.ctrlKey || e.metaKey || p.wheelZooms) {
        // Pinch gestures arrive as ctrl+wheel; a plain wheel zooms more gently.
        const rate = e.ctrlKey || e.metaKey ? 0.01 : 0.003;
        const f = Math.exp(-e.deltaY * (e.deltaMode === 1 ? rate * 5 : rate) * (p.invertZoom ? -1 : 1));
        const at = zoomFocus() ?? { x: e.clientX - r.left, y: e.clientY - r.top };
        moveCam(zoomAt(camTarget.current, f, at.x, at.y), true);
      } else {
        const k = e.deltaMode === 1 ? 16 : 1;
        const dx = e.shiftKey && !e.deltaX ? e.deltaY : e.deltaX;
        const dy = e.shiftKey && !e.deltaX ? 0 : e.deltaY;
        moveCam({ ...camTarget.current, x: camTarget.current.x - dx * k, y: camTarget.current.y - dy * k }, true);
      }
    };
    v.addEventListener("wheel", onWheel, { passive: false });
    return () => v.removeEventListener("wheel", onWheel);
  }, [moveCam, zoomFocus]);

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

  /** Deep select: Mod flips whatever the "select deepest layer by default" preference says. */
  const deepOf = (e: { metaKey: boolean; ctrlKey: boolean }) => (e.metaKey || e.ctrlKey) !== prefs().deepSelect;

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
    // A turned node resizes from the inspector; its outline has no handles.
    if (store.selection.length === 1 && rotBox(store.selection[0])) return null;
    const u = liveSelection();
    if (!u) return null;
    const s = rectToScreen(camRef.current, u);
    for (const hd of HANDLES) {
      const hp = handlePoint(s, hd);
      if (Math.abs(hp.x - p.x) <= 6 && Math.abs(hp.y - p.y) <= 6) return hd;
    }
    return null;
  };

  /** The selected node when the pointer is just outside one of its corners (where a drag rotates it). */
  const rotateAt = (p: { x: number; y: number }): { id: string; c: { x: number; y: number }; deg: number } | null => {
    if (store.selection.length !== 1 || ui.current.editing) return null;
    const id = store.selection[0];
    const n = store.nodes.get(id);
    const r = store.rects.get(id);
    if (!n || !r || n.locked || store.ancestors(id).some((a) => store.nodes.get(a)?.locked)) return null;
    const cam = camRef.current;
    const box = rotBox(id);
    const c = toScreen(cam, r.x + r.w / 2, r.y + r.h / 2);
    const q = toLocal(c, box?.deg ?? 0, p);
    const dx = Math.abs(q.x) - ((box?.w ?? r.w) * cam.zoom) / 2;
    const dy = Math.abs(q.y) - ((box?.h ?? r.h) * cam.zoom) / 2;
    return dx > 0 && dy > 0 && dx < ROTATE_ZONE && dy < ROTATE_ZONE ? { id, c, deg: box?.deg ?? 0 } : null;
  };

  /** A flex handle under the pointer: a padding side or a gap. */
  const flexAt = (p: { x: number; y: number }): { id: string; key: FlexKey; start: number } | null => {
    const id = flexTarget();
    const b = id && !ui.current.editing ? flexBox(store, reg, id) : null;
    if (!id || !b) return null;
    const hit = flexHandles(b, camRef.current).find((h) => Math.abs(p.x - h.x) <= h.w / 2 + 3 && Math.abs(p.y - h.y) <= h.h / 2 + 3);
    return hit ? { id, key: hit.key, start: hit.key === "gap" ? b.gap : b.pad[["pt", "pr", "pb", "pl"].indexOf(hit.key)] } : null;
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
  /** Gesture commands reach the server one at a time, in the order the pointer made them. */
  const lane = useRef<Promise<unknown>>(Promise.resolve());
  const toServer = (cmd: () => Record<string, unknown> | undefined) => {
    const sent = lane.current.then(() => {
      const c = cmd();
      return c ? store.do<{ guides?: Guide[] }>(c).catch(() => null) : null;
    });
    lane.current = sent;
    return sent;
  };
  /** Sends a drag's newest frame. Frames that pile up behind a slow reply collapse into the latest. */
  const sendFrame = (d: Live, cmd: Record<string, unknown>) => {
    const waiting = !!d.frame;
    d.frame = cmd;
    if (waiting) return;
    toServer(() => {
      const frame = d.frame;
      d.frame = undefined;
      return frame;
    }).then((r) => {
      ui.current.guides = r?.guides ?? [];
      requestDraw();
    });
  };
  /** Abandons the drag in progress: Escape, or the browser took the pointer away. */
  function cancelDrag() {
    const d = drag.current;
    drag.current = null;
    if (!d) return;
    if (d.kind === "resize" || d.kind === "flex" || d.kind === "rotate" || (d.kind === "move" && d.started)) toServer(() => ({ type: "cancel" }));
    ui.current.marquee = ui.current.preview = null;
    ui.current.guides = [];
    syncCursor();
    requestDraw();
  }

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
    if (e.button === 2 && prefs().rightDragPans) {
      drag.current = { kind: "pan", sx: e.clientX, sy: e.clientY, cam: camRef.current, right: true };
      v?.setPointerCapture(e.pointerId);
      return;
    }
    if (ui.current.z && e.button === 0) {
      drag.current = { kind: "zoom", sx: p.x, sy: p.y, alt: e.altKey };
      v?.setPointerCapture(e.pointerId);
      return;
    }
    if (e.button === 2) {
      const hit = selectable(hitAt(e.clientX, e.clientY), deepOf(e));
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
        const ids = store.selection;
        toServer(() => ({ type: "resizeStart", ids }));
        return;
      }
    }
    const fx = flexAt(p);
    if (fx) {
      drag.current = { kind: "flex", ...fx, sx: p.x, sy: p.y };
      toServer(() => ({ type: "begin" }));
      return;
    }
    const rt = rotateAt(p);
    if (rt) {
      drag.current = { kind: "rotate", id: rt.id, c: rt.c, a0: angleDeg(rt.c, p), start: rt.deg };
      toServer(() => ({ type: "begin" }));
      if (v) v.style.cursor = ROTATE_CURSOR;
      return;
    }
    const label = ui.current.labels.find((l) => p.x >= l.r.x && p.x <= l.r.x + l.r.w && p.y >= l.r.y && p.y <= l.r.y + l.r.h);
    const hit = label && !store.nodes.get(label.id)?.locked ? label.id : selectable(hitAt(e.clientX, e.clientY), deepOf(e));
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
    ui.current.pointer = { x: e.clientX, y: e.clientY };
    if (ui.current.alt !== e.altKey) ui.current.alt = e.altKey;
    if (!d) {
      if (ui.current.editing || tool !== "move") return;
      updateMeasure();
      const label = ui.current.labels.find((l) => p.x >= l.r.x && p.x <= l.r.x + l.r.w && p.y >= l.r.y && p.y <= l.r.y + l.r.h);
      const hover = label ? label.id : selectable(hitAt(e.clientX, e.clientY), deepOf(e));
      if (hover !== store.hover) {
        // Only the HUD shows canvas hover; a full emit would re-render every document subscriber.
        store.hover = hover;
        requestDraw();
      }
      const v = vp.current;
      const hd = handleAt(p);
      const fx = flexAt(p);
      if (v && !ui.current.z) {
        v.style.cursor = hd
          ? hd === "n" || hd === "s"
            ? "ns-resize"
            : hd === "e" || hd === "w"
              ? "ew-resize"
              : hd === "nw" || hd === "se"
                ? "nwse-resize"
                : "nesw-resize"
          : fx
            ? fx.key === "pt" || fx.key === "pb" || (fx.key === "gap" && !flexBox(store, reg, fx.id)?.row)
              ? "ns-resize"
              : "ew-resize"
            : rotateAt(p)
              ? ROTATE_CURSOR
              : "";
      }
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
        const ids = store.selection;
        toServer(() => ({ type: "dragStart", ids, duplicate: d.alt }));
      }
      const wp = toWorld(cam, p.x, p.y);
      sendFrame(d, { type: "drag", dx, dy, x: wp.x, y: wp.y, snap: !(e.metaKey || e.ctrlKey), pixel: getView().snapPixel, zoom: cam.zoom });
    } else if (d.kind === "resize") {
      const rect = resizeRect(d.start, d.handle, (p.x - d.sx) / cam.zoom, (p.y - d.sy) / cam.zoom, e.shiftKey, e.altKey);
      sendFrame(d, { type: "resize", rect, snap: !(e.metaKey || e.ctrlKey || e.shiftKey || e.altKey), pixel: getView().snapPixel, zoom: cam.zoom });
    } else if (d.kind === "flex") {
      const b = flexBox(store, reg, d.id);
      const horizontal = d.key === "pl" || d.key === "pr" || (d.key === "gap" && b?.row);
      const delta = (horizontal ? p.x - d.sx : p.y - d.sy) / cam.zoom;
      const value = Math.max(0, d.start + (d.key === "pb" || d.key === "pr" ? -delta : delta));
      sendFrame(d, { type: "setStyle", gesture: true, ids: [d.id], styles: { [FLEX_PROP[d.key]]: getView().snapPixel ? Math.round(value) : Math.round(value * 100) / 100 } });
    } else if (d.kind === "rotate") {
      const deg = Math.round(normalizeAngle(d.start + angleDeg(d.c, p) - d.a0, e.shiftKey ? 15 : 0) * 100) / 100;
      sendFrame(d, { type: "setStyle", gesture: true, ids: [d.id], styles: { rotate: deg ? `${deg}deg` : null } });
    } else if (d.kind === "zoom") {
      ui.current.marquee = fromPoints(d.sx, d.sy, p.x, p.y);
      requestDraw();
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
    if (d.kind === "pan" && d.right && Math.hypot(e.clientX - d.sx, e.clientY - d.sy) < 4) {
      // A right-click that didn't drag opens the menu; the browser's own event was held back.
      const hit = selectable(hitAt(e.clientX, e.clientY), deepOf(e));
      if (hit && !store.selection.includes(hit)) store.select([hit]);
      menuOk.current = true;
      vp.current?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: e.clientX, clientY: e.clientY }));
      menuOk.current = false;
    }
    if (d.kind === "marquee") {
      ui.current.marquee = null;
      requestDraw();
    } else if (d.kind === "move") {
      if (d.started) {
        const p = local(e);
        const w = toWorld(camRef.current, p.x, p.y);
        const end = { type: "dragEnd", x: w.x, y: w.y, reparent: !e.metaKey && !e.ctrlKey };
        await toServer(() => end);
        ui.current.guides = [];
        requestDraw();
      } else if (d.click && !e.shiftKey && store.selection.length > 1) store.select([d.click]);
    } else if (d.kind === "resize") {
      await toServer(() => ({ type: "resizeEnd" }));
      ui.current.guides = [];
      requestDraw();
    } else if (d.kind === "flex" || d.kind === "rotate") {
      await toServer(() => ({ type: "commit" }));
      syncCursor();
    } else if (d.kind === "zoom") {
      const m = ui.current.marquee;
      ui.current.marquee = null;
      const cam = camRef.current;
      const { w, h } = ui.current.size;
      if (m && m.w + m.h > 8) moveCam(fitRect({ x: (m.x - cam.x) / cam.zoom, y: (m.y - cam.y) / cam.zoom, w: m.w / cam.zoom, h: m.h / cam.zoom }, w, h, 0, MAX_ZOOM), true);
      else moveCam(zoomAt(camTarget.current, d.alt ? 0.5 : 2, d.sx, d.sy), true);
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
    const p = local(e);
    const label = ui.current.labels.find((l) => p.x >= l.r.x && p.x <= l.r.x + l.r.w && p.y >= l.r.y && p.y <= l.r.y + l.r.h);
    if (label && !store.nodes.get(label.id)?.locked) return setRenaming(label);
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
        data-tokens={store.file.id}
        className="relative min-w-0 flex-1 overflow-hidden"
        style={{
          ...tokenVars(store.tokens),
          background: page?.canvas ?? "var(--spark-canvas)",
          cursor: props.spaceDown || tool === "hand" ? "grab" : tool === "move" ? "default" : "crosshair",
        }}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={cancelDrag}
        onPointerLeave={() => {
          if (store.hover && !drag.current) {
            store.hover = null;
            requestDraw();
          }
        }}
        onDoubleClick={onDoubleClick}
        onContextMenu={(e) => {
          if (prefs().rightDragPans && !menuOk.current) e.preventDefault();
          else props.onContextMenu?.(e);
        }}
      >
        <style>{themeCss(store.file.id, store.tokens)}</style>
        <div ref={camEl} className="spark-camera" data-testid="camera">
          <LevelOfDetail.Provider value={true}>
            <PageContent store={store} page={store.page} />
          </LevelOfDetail.Provider>
        </div>
        <canvas ref={hud} className="pointer-events-none absolute inset-0 size-full" data-testid="hud" />
        {renaming && (
          <div className="absolute z-10 flex" style={{ left: renaming.r.x - 4, top: renaming.r.y - 1, width: Math.max(renaming.r.w + 24, 140) }}>
            <Rename
              value={store.nodes.get(renaming.id)?.name ?? ""}
              onDone={(v) => {
                setRenaming(null);
                if (v?.trim() && v !== store.nodes.get(renaming.id)?.name) store.do({ type: "set", ids: [renaming.id], prop: "name", value: v.trim() });
              }}
            />
          </div>
        )}
      </div>
    </ElementRegistry.Provider>
  );
}
