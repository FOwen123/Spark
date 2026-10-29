import type { DocStore } from "./doc";

// CSS generic families and Spark's bundled fonts never need downloading.
const BUILT_IN = new Set([
  "serif",
  "sans-serif",
  "monospace",
  "cursive",
  "fantasy",
  "math",
  "emoji",
  "fangsong",
  "system-ui",
  "ui-serif",
  "ui-sans-serif",
  "ui-monospace",
  "ui-rounded",
  "-apple-system",
  "blinkmacsystemfont",
  "inherit",
  "initial",
  "unset",
  "geist",
  "geist mono",
]);
const requested = new Map<string, Promise<void>>();

/** Asks Spark's server for every font family named in `values` (a CSS `font-family` list each).
 * The server answers with Google Fonts `@font-face` rules, downloading them once, or nothing
 * when the family is installed or unknown. Resolves when those rules are in place. */
function request(values: Iterable<string | undefined>): Promise<unknown> {
  const pending: Promise<void>[] = [];
  for (const value of values) {
    for (const part of value?.split(",") ?? []) {
      const name = part.trim().replace(/^["']|["']$/g, "");
      const key = name.toLowerCase();
      if (!name || name.includes("(") || BUILT_IN.has(key)) continue;
      let loaded = requested.get(key);
      if (!loaded) {
        const link = document.createElement("link");
        link.rel = "stylesheet";
        link.href = `/fonts/css?family=${encodeURIComponent(name)}`;
        loaded = new Promise<void>((r) => {
          link.onload = link.onerror = () => r();
        });
        requested.set(key, loaded);
        document.head.append(link);
      }
      pending.push(loaded);
    }
  }
  return Promise.all(pending);
}

/** Loads the fonts used by `ids` (every node by default) and by the file's font tokens. */
export function loadFonts(store: DocStore, ids: Iterable<string> = store.nodes.keys()) {
  const families: (string | undefined)[] = store.tokens.filter((t) => t.name.startsWith("--font-")).map((t) => t.value);
  for (const id of ids) families.push(store.nodes.get(id)?.css.fontFamily);
  return request(families);
}
