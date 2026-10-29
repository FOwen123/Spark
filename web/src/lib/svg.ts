// SVG from a design is inserted into Spark's own page, so anything that could run script or load
// another document is dropped before it reaches the DOM.
const blockedTags = new Set(["script", "foreignobject", "iframe", "object", "embed"]);

export function safeTag(tag: string): boolean {
  return /^[a-z][\w.-]*(:[\w.-]+)?$/i.test(tag) && !blockedTags.has(tag.toLowerCase());
}

export function safeAttrs(attrs: Record<string, string>): [string, string][] {
  return Object.entries(attrs).filter(([k, v]) => {
    const name = k.toLowerCase();
    if (!/^[a-z_][\w:.-]*$/i.test(k) || name.startsWith("on")) return false;
    // Browsers ignore spaces and control characters inside a URL scheme.
    const value = [...v]
      .filter((c) => c > " ")
      .join("")
      .toLowerCase();
    if (value.startsWith("javascript:") || value.startsWith("data:text/html")) return false;
    // <set>/<animate> could otherwise write a handler or a link at runtime.
    return !(name === "attributename" && (value.startsWith("on") || value.endsWith("href")));
  });
}
