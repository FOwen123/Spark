import { expect, test } from "vitest";
import { fitRect, fromPoints, intersects, resizeRect, toScreen, toWorld, union, zoomAt } from "./geometry";

test("world and screen conversions invert each other", () => {
  const cam = { x: 120, y: -40, zoom: 2.5 };
  const w = toWorld(cam, 300, 200);
  expect(toScreen(cam, w.x, w.y)).toEqual({ x: 300, y: 200 });
});

test("zooming keeps the point under the cursor fixed", () => {
  const cam = { x: 10, y: 20, zoom: 1 };
  const before = toWorld(cam, 400, 300);
  const next = zoomAt(cam, 2, 400, 300);
  const after = toWorld(next, 400, 300);
  expect(next.zoom).toBe(2);
  expect(after.x).toBeCloseTo(before.x);
  expect(after.y).toBeCloseTo(before.y);
  expect(zoomAt(cam, 1e9, 0, 0).zoom).toBe(256);
  expect(zoomAt(cam, 1e-9, 0, 0).zoom).toBe(0.02);
});

test("fit centers the rect and caps zoom at 100%", () => {
  const cam = fitRect({ x: 0, y: 0, w: 100, h: 100 }, 1000, 800);
  expect(cam.zoom).toBe(1);
  expect(cam.x).toBe(450);
  const big = fitRect({ x: 0, y: 0, w: 4000, h: 1000 }, 1000, 800, 0);
  expect(big.zoom).toBe(0.25);
});

test("rect helpers", () => {
  expect(fromPoints(10, 10, 0, 5)).toEqual({ x: 0, y: 5, w: 10, h: 5 });
  expect(union([{ x: 0, y: 0, w: 1, h: 1 }, { x: 5, y: -2, w: 1, h: 1 }])).toEqual({ x: 0, y: -2, w: 6, h: 3 });
  expect(intersects({ x: 0, y: 0, w: 10, h: 10 }, { x: 9, y: 9, w: 5, h: 5 })).toBe(true);
  expect(intersects({ x: 0, y: 0, w: 10, h: 10 }, { x: 10, y: 0, w: 5, h: 5 })).toBe(false);
});

test("resize with modifiers", () => {
  const r = { x: 0, y: 0, w: 100, h: 50 };
  expect(resizeRect(r, "se", 20, 10, false, false)).toEqual({ x: 0, y: 0, w: 120, h: 60 });
  expect(resizeRect(r, "nw", 20, 10, false, false)).toEqual({ x: 20, y: 10, w: 80, h: 40 });
  expect(resizeRect(r, "e", 50, 0, true, false)).toEqual({ x: 0, y: -12.5, w: 150, h: 75 });
  expect(resizeRect(r, "e", 10, 0, false, true)).toEqual({ x: -10, y: 0, w: 120, h: 50 });
  expect(resizeRect(r, "w", 500, 0, false, false).w).toBeGreaterThanOrEqual(1);
});
