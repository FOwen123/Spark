import { expect, test } from "vitest";
import { parseColor, type Rgba, toHex, withAlpha } from "./color";

const must = (c: Rgba | null): Rgba => {
  if (!c) throw new Error("expected a color");
  return c;
};

test("parses Spark color formats to hex", () => {
  expect(toHex(must(parseColor("#ff0000")))).toBe("#ff0000");
  expect(toHex(must(parseColor("#f00")))).toBe("#ff0000");
  expect(toHex(must(parseColor("oklch(0.628 0.2577 29.23)")))).toBe("#ff0000");
  expect(toHex(must(parseColor("oklch(1 0 0)")))).toBe("#ffffff");
  expect(must(parseColor("#00000080")).a).toBeCloseTo(0.5, 2);
  expect(must(parseColor("rgb(0 0 0 / 0.12)")).a).toBeCloseTo(0.12);
  expect(parseColor("nonsense")).toBeNull();
});

test("token references resolve through aliases", () => {
  const tokens = new Map([
    ["--color-brand", "var(--color-base)"],
    ["--color-base", "#123456"],
    ["--color-loop", "var(--color-loop)"],
  ]);
  expect(toHex(must(parseColor("var(--color-brand)", tokens)))).toBe("#123456");
  expect(parseColor("var(--color-loop)", tokens)).toBeNull();
});

test("alpha suffix", () => {
  expect(withAlpha("#000000", 1)).toBe("#000000");
  expect(withAlpha("#000000", 0.5)).toBe("#00000080");
});
