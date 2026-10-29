// Colors arrive from Rust as OKLCH (`oklch(l c h / a)`), hex, or token references (`var(--x)`).
// The inspector shows hex and alpha; Rust converts whatever we send back.

export type Rgba = { r: number; g: number; b: number; a: number };

const fromLinear = (c: number) => (c <= 0.0031308 ? c * 12.92 : 1.055 * c ** (1 / 2.4) - 0.055);

export function oklchToRgb(l: number, c: number, h: number, a = 1): Rgba {
  const hr = (h * Math.PI) / 180;
  const A = c * Math.cos(hr);
  const B = c * Math.sin(hr);
  const l_ = (l + 0.3963377774 * A + 0.2158037573 * B) ** 3;
  const m_ = (l - 0.1055613458 * A - 0.0638541728 * B) ** 3;
  const s_ = (l - 0.0894841775 * A - 1.291485548 * B) ** 3;
  return {
    r: fromLinear(4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_),
    g: fromLinear(-1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_),
    b: fromLinear(-0.0041960863 * l_ - 0.7034186147 * m_ + 1.707614701 * s_),
    a,
  };
}

/** Parses the color formats Spark emits. Token references resolve through `tokens`. */
export function parseColor(s: string, tokens?: Map<string, string>, depth = 0): Rgba | null {
  const t = s.trim();
  const v = t.match(/^var\((--[\w-]+)/);
  if (v) {
    const next = tokens?.get(v[1]);
    return next && depth < 16 ? parseColor(next, tokens, depth + 1) : null;
  }
  const ok = t.match(/^oklch\(\s*([\d.]+)(%?)\s+([\d.]+)\s+([\d.]+)(?:\s*\/\s*([\d.]+)(%?))?\s*\)$/i);
  if (ok) {
    const l = Number(ok[1]) / (ok[2] ? 100 : 1);
    const a = ok[5] === undefined ? 1 : Number(ok[5]) / (ok[6] ? 100 : 1);
    return oklchToRgb(l, Number(ok[3]), Number(ok[4]), a);
  }
  const hex = t.match(/^#([0-9a-f]{3,8})$/i);
  if (hex) {
    let h = hex[1];
    if (h.length === 3 || h.length === 4) h = [...h].map((c) => c + c).join("");
    const n = (i: number) => Number.parseInt(h.slice(i, i + 2), 16) / 255;
    return { r: n(0), g: n(2), b: n(4), a: h.length === 8 ? n(6) : 1 };
  }
  const rgb = t.match(/^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)(?:[\s,/]+([\d.]+)(%?))?\s*\)$/i);
  if (rgb) {
    return { r: Number(rgb[1]) / 255, g: Number(rgb[2]) / 255, b: Number(rgb[3]) / 255, a: rgb[4] === undefined ? 1 : Number(rgb[4]) / (rgb[5] ? 100 : 1) };
  }
  return null;
}

const byte = (v: number) => Math.round(Math.min(1, Math.max(0, v)) * 255);

/** `#rrggbb` (alpha dropped). */
export function toHex(c: Rgba): string {
  return `#${[c.r, c.g, c.b].map((v) => byte(v).toString(16).padStart(2, "0")).join("")}`;
}

/** A CSS color string for a hex and an alpha in 0..1. */
export function withAlpha(hex: string, a: number): string {
  if (a >= 1) return hex;
  return `${hex}${byte(a).toString(16).padStart(2, "0")}`;
}
