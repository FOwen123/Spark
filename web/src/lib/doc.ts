// A read-only mirror of one open file. It changes only when the server sends a snapshot or patch;
// components subscribe per node so a patch re-renders just what changed.

import { useSyncExternalStore } from "react";
import type { Conn } from "./conn";
import type { FileInfo, NodeView, Page, Patch, Rect, Snapshot, Token } from "./types";

type Listener = () => void;

export class DocStore {
  file: FileInfo;
  pages: Page[] = [];
  tokens: Token[] = [];
  guide = "";
  page = "";
  selection: string[] = [];
  canUndo = false;
  canRedo = false;
  nodes = new Map<string, NodeView>();
  children = new Map<string, string[]>();
  /** World rects measured from the DOM. */
  rects = new Map<string, Rect>();
  version = 0;
  /** Node under the pointer (canvas or layers), local UI state. */
  hover: string | null = null;

  private nodeSubs = new Map<string, Set<Listener>>();
  private subs = new Set<Listener>();
  /** Artboards whose contents are mounted on the canvas (on screen and big enough to see). */
  detail = new Set<string>();
  private detailSubs = new Map<string, Set<Listener>>();
  private rectSubs = new Set<Listener>();
  rectsVersion = 0;
  /** Ids touched by the last update, for incremental measuring. */
  touched = new Set<string>();

  constructor(
    public conn: Conn,
    snap: Snapshot,
  ) {
    this.file = snap.file;
    this.applySnapshot(snap);
  }

  applySnapshot(s: Snapshot) {
    this.file = s.file;
    this.pages = s.pages;
    this.tokens = s.tokens;
    this.guide = s.guide;
    this.page = s.page ?? s.pages.find((p) => !p.divider)?.id ?? "";
    this.selection = s.selection;
    this.canUndo = s.canUndo;
    this.canRedo = s.canRedo;
    this.nodes = new Map(Object.entries(s.nodes));
    this.rects = new Map(Object.entries(s.rects ?? {}).map(([id, [x, y, w, h]]) => [id, { x, y, w, h }]));
    this.children = new Map();
    for (const n of this.nodes.values()) this.link(n);
    for (const list of this.children.values()) list.sort(this.byKey);
    this.touched = new Set(this.nodes.keys());
    this.emitAll();
  }

  private byKey = (a: string, b: string) => {
    const ka = this.nodes.get(a)?.key ?? "";
    const kb = this.nodes.get(b)?.key ?? "";
    return ka < kb ? -1 : ka > kb ? 1 : a < b ? -1 : a > b ? 1 : 0;
  };

  private link(n: NodeView) {
    const list = this.children.get(n.parent);
    if (list) list.push(n.id);
    else this.children.set(n.parent, [n.id]);
  }

  private unlink(n: NodeView) {
    const list = this.children.get(n.parent);
    if (!list) return;
    const i = list.indexOf(n.id);
    if (i >= 0) list.splice(i, 1);
  }

  applyPatch(p: Patch) {
    const changed = new Set<string>();
    const moved: [string, number, number][] = [];
    for (const [id, view] of Object.entries(p.nodes)) {
      const old = this.nodes.get(id);
      if (old && view?.parent.startsWith("p-") && old.parent === view.parent) {
        const dx = pxDelta(old.css.left, view.css.left);
        const dy = pxDelta(old.css.top, view.css.top);
        if (dx || dy) moved.push([id, dx, dy]);
      } else if (old && view?.parent.startsWith("p-") && !view.css.rotate) {
        // Landed on a page from another parent (undoing a drop into a frame): its left/top now say
        // where it is. Rotated artboards measure their rotated box, so they're left alone.
        const at = this.rects.get(id);
        const dx = at ? pxDelta(`${at.x}px`, view.css.left ?? "0px") : 0;
        const dy = at ? pxDelta(`${at.y}px`, view.css.top ?? "0px") : 0;
        if (dx || dy) moved.push([id, dx, dy]);
      }
      if (old) {
        this.unlink(old);
        changed.add(old.parent);
      }
      if (view) {
        this.nodes.set(id, view);
        this.link(view);
        this.children.get(view.parent)?.sort(this.byKey);
        changed.add(view.parent);
      } else {
        this.nodes.delete(id);
      }
      changed.add(id);
    }
    // Replace child arrays so subscribers see a new reference.
    for (const parent of changed) {
      const list = this.children.get(parent);
      if (list) this.children.set(parent, [...list]);
    }
    // An artboard's left/top is its place on the canvas: carry the measured rects inside it along
    // (as the server does, see Editor::apply_all), since level-of-detail shells aren't re-measured.
    for (const [id, dx, dy] of moved) {
      const shift = (n: string) => {
        const r = this.rects.get(n);
        if (r) this.rects.set(n, { ...r, x: r.x + dx, y: r.y + dy });
        for (const k of this.kids(n)) shift(k);
      };
      shift(id);
    }
    if (moved.length) this.emitRects();
    if (p.pages) this.pages = p.pages;
    if (p.tokens) this.tokens = p.tokens;
    if (p.guide !== undefined) this.guide = p.guide;
    if (p.name !== undefined) this.file = { ...this.file, name: p.name };
    if (p.page) this.page = p.page;
    if (p.selection) this.selection = p.selection;
    if (p.canUndo !== undefined) this.canUndo = p.canUndo;
    if (p.canRedo !== undefined) this.canRedo = p.canRedo;
    this.touched = changed;
    for (const id of changed) this.emitNode(id);
    this.emit();
  }

  setSelectionLocal(ids: string[]) {
    this.selection = ids;
    this.emit();
  }

  /** Children of a page or node, in order. */
  kids(parent: string): string[] {
    return this.children.get(parent) ?? EMPTY;
  }

  /** Top-level ancestor (artboard) of a node. */
  artboardOf(id: string): string | undefined {
    let cur = this.nodes.get(id);
    while (cur && !cur.parent.startsWith("p-")) cur = this.nodes.get(cur.parent);
    return cur?.id;
  }

  /** Ancestors from the parent up to the artboard. */
  ancestors(id: string): string[] {
    const out: string[] = [];
    let cur = this.nodes.get(id);
    while (cur && !cur.parent.startsWith("p-")) {
      out.push(cur.parent);
      cur = this.nodes.get(cur.parent);
    }
    return out;
  }

  /** The page a node lives on. */
  pageOf(id: string): string | undefined {
    const ab = this.artboardOf(id);
    return ab ? this.nodes.get(ab)?.parent : undefined;
  }

  subscribeNode(id: string, fn: Listener) {
    const set = this.nodeSubs.get(id) ?? new Set<Listener>();
    this.nodeSubs.set(id, set);
    set.add(fn);
    return () => set.delete(fn);
  }

  subscribe(fn: Listener) {
    this.subs.add(fn);
    return () => this.subs.delete(fn);
  }

  subscribeDetail(id: string, fn: Listener) {
    const set = this.detailSubs.get(id) ?? new Set<Listener>();
    this.detailSubs.set(id, set);
    set.add(fn);
    return () => set.delete(fn);
  }

  /** Updates which artboards show their contents; returns the ones that just appeared. */
  setDetail(next: Set<string>): string[] {
    const changed = [...next].filter((id) => !this.detail.has(id));
    const gone = [...this.detail].filter((id) => !next.has(id));
    this.detail = next;
    for (const id of [...changed, ...gone]) for (const fn of this.detailSubs.get(id) ?? []) fn();
    return changed;
  }

  subscribeRects(fn: Listener) {
    this.rectSubs.add(fn);
    return () => this.rectSubs.delete(fn);
  }

  /** Measured rects changed (they are bookkeeping, so this skips the full emit). */
  emitRects() {
    this.rectsVersion++;
    for (const fn of this.rectSubs) fn();
  }

  private emitNode(id: string) {
    for (const fn of this.nodeSubs.get(id) ?? []) fn();
  }

  emit() {
    this.version++;
    for (const fn of this.subs) fn();
  }

  private emitAll() {
    for (const set of this.nodeSubs.values()) for (const fn of set) fn();
    this.emit();
  }

  /** Sends the canvas's measurement still waiting for its frame; set by the canvas. */
  flushMeasure?: () => void;

  /** Sends an editing intent to the server. */
  do<T = unknown>(cmd: Record<string, unknown>): Promise<T> {
    // Commands like align read the server's layout, so a report of the current one goes first.
    this.flushMeasure?.();
    return this.conn.request<T>({ t: "do", file: this.file.id, cmd });
  }

  select(ids: string[]) {
    this.setSelectionLocal(ids);
    this.conn.send({ t: "select", file: this.file.id, ids });
  }

  setPage(page: string) {
    this.page = page;
    this.selection = [];
    this.emit();
    this.conn.send({ t: "page", file: this.file.id, page });
  }
}

const EMPTY: string[] = [];

/** A file's design tokens as CSS custom properties, so `var(--token)` in layer styles resolves. */
export function tokenVars(tokens: Token[]): Record<string, string> {
  return Object.fromEntries(tokens.map((t) => [t.name, t.value]));
}

export function useNode(store: DocStore, id: string): NodeView | undefined {
  return useSyncExternalStore(
    (fn) => store.subscribeNode(id, fn),
    () => store.nodes.get(id),
  );
}

export function useKids(store: DocStore, id: string): string[] {
  return useSyncExternalStore(
    (fn) => store.subscribeNode(id, fn),
    () => store.kids(id),
  );
}

/** Re-renders on any change to the store; read fields off the store directly. */
export function useDoc(store: DocStore): number {
  return useSyncExternalStore(
    (fn) => store.subscribe(fn),
    () => store.version,
  );
}

/** How far a px `left`/`top` moved (a missing value is 0); 0 when either isn't in px. */
function pxDelta(from: string | undefined, to: string | undefined): number {
  const px = (v: string | undefined) => (v === undefined ? 0 : v.endsWith("px") ? Number.parseFloat(v) : Number.NaN);
  const d = px(to) - px(from);
  return Number.isFinite(d) ? d : 0;
}

/** Re-renders when measured rects change. */
export function useRects(store: DocStore): number {
  return useSyncExternalStore(
    (fn) => store.subscribeRects(fn),
    () => store.rectsVersion,
  );
}

/** Whether an artboard's contents are mounted. */
export function useDetail(store: DocStore, id: string): boolean {
  return useSyncExternalStore(
    (fn) => store.subscribeDetail(id, fn),
    () => store.detail.has(id),
  );
}

/** Nodes to zoom to when their file's canvas first lays out (set by deep links). */
export const pendingReveal = new Map<string, string>();
