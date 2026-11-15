import { expect, test } from "vitest";
import { themeCss } from "./doc";

const tokens = [
  { name: "--color-bg", value: "#fff", themes: { dark: "#000" } },
  { name: "--color-ink", value: "#111" },
  { name: "--color-bad", value: "red}body{display:none", themes: { dark: "x" } },
];

test("each theme declares every token, falling back to base values", () => {
  expect(themeCss("f1", tokens)).toBe('[data-tokens="f1"] [data-theme="dark"]{--color-bg:#000;--color-ink:#111;--color-bad:x}');
});

test("file content can't break out of the rules", () => {
  expect(themeCss('x"]{} body{display:none} /*', tokens)).toBe("");
  expect(themeCss("f1", [{ name: "--color-a", value: "#fff", themes: { 'x"]{}': "#000", dark: "#000;}body{x:y" } }])).toBe('[data-tokens="f1"] [data-theme="dark"]{}');
});
