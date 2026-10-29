import { expect, test } from "vitest";
import { safeAttrs, safeTag } from "./svg";

test("drops SVG that could run script", () => {
  expect(safeTag("path")).toBe(true);
  expect(safeTag("svg:linearGradient")).toBe(true);
  expect(safeTag("script")).toBe(false);
  expect(safeTag("foreignObject")).toBe(false);
  expect(safeTag('img src=x onerror="x"')).toBe(false);
  const kept = safeAttrs({
    d: "M0 0L1 1",
    fill: "#000",
    onerror: "x()",
    ONLOAD: "x()",
    href: " java\nscript:x()",
    "xlink:href": "#grad",
    attributeName: "onclick",
    'x"y': "1",
  });
  expect(Object.fromEntries(kept)).toEqual({ d: "M0 0L1 1", fill: "#000", "xlink:href": "#grad" });
});
