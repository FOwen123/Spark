// Per-device UI preferences, kept in localStorage like the theme and open tabs.

import { useSyncExternalStore } from "react";

export const DEFAULTS = {
  invertZoom: false,
  wheelZooms: false,
  rightDragPans: false,
  zoomToSelection: false,
  deepSelect: false,
  reduceMotion: false,
  nudgeSmall: 1,
  nudgeLarge: 8,
  leftHidden: false,
  floating: false,
  pagesCollapsed: false,
  // Side panel widths in px, dragged from their inner edges.
  leftW: 206,
  rightW: 280,
  // View toggles; the canvas reads these and the menu and shortcuts flip them (see Editor `toggle*` commands).
  pixelGrid: true,
  snapPixel: true,
  layoutGuides: true,
};
export type Prefs = typeof DEFAULTS;

/** Canvas behavior toggles shown in the zoom menu and in Settings. */
export const CANVAS_PREFS: [keyof Prefs, string][] = [
  ["invertZoom", "Invert zoom direction"],
  ["wheelZooms", "Scroll wheel zooms"],
  ["rightDragPans", "Right-click drag pans"],
  ["zoomToSelection", "Zoom centers on selection"],
  ["deepSelect", "Select deepest layer by default"],
  ["reduceMotion", "Reduce agent animations"],
];

const KEY = "spark-prefs";
const subs = new Set<() => void>();
let cur: Prefs = { ...DEFAULTS };
try {
  cur = { ...DEFAULTS, ...JSON.parse(localStorage.getItem(KEY) ?? "{}") };
} catch {}

const apply = () => {
  const root = document.documentElement;
  root.toggleAttribute("data-reduce-motion", cur.reduceMotion);
  root.style.setProperty("--left-w", `${cur.leftW}px`);
  root.style.setProperty("--right-w", `${cur.rightW}px`);
};
apply();

export const prefs = () => cur;

export function setPrefs(p: Partial<Prefs>) {
  cur = { ...cur, ...p };
  localStorage.setItem(KEY, JSON.stringify(cur));
  apply();
  for (const f of subs) f();
}

/** Calls `fn` after every change; returns the unsubscribe. */
export function onPrefs(fn: () => void) {
  subs.add(fn);
  return () => subs.delete(fn);
}

export function usePrefs(): Prefs {
  return useSyncExternalStore(
    (fn) => {
      subs.add(fn);
      return () => subs.delete(fn);
    },
    () => cur,
  );
}
