import { createContext, memo, useCallback, useContext, useLayoutEffect, useRef } from "react";
import { type DocStore, useDetail, useKids, useNode } from "../lib/doc";
import { safeAttrs, safeTag } from "../lib/svg";

/** On the canvas, far-away or tiny artboards render as empty shells (level of detail). */
export const LevelOfDetail = createContext(false);

/** Rendered elements by node id, used for measuring and hit-testing. */
export const ElementRegistry = createContext<Map<string, HTMLElement | SVGElement>>(new Map());

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;");

/** SVG children are plain markup: they render as the stored tags and attributes. */
function vectorMarkup(store: DocStore, ids: string[]): string {
  return ids
    .map((id) => {
      const n = store.nodes.get(id);
      if (!n || n.hidden || !n.tag || !safeTag(n.tag)) return "";
      const attrs = safeAttrs(n.attrs ?? {})
        .map(([k, v]) => ` ${k}="${esc(v)}"`)
        .join("");
      return `<${n.tag} data-node="${id}"${attrs}>${vectorMarkup(store, store.kids(id))}</${n.tag}>`;
    })
    .join("");
}

function imageState(status: string | undefined) {
  return status === "error" ? "Image failed" : status === "pending" ? "Waiting for an image" : "Generating image…";
}

export const NodeEl = memo(function NodeEl({ store, id }: { store: DocStore; id: string }) {
  const n = useNode(store, id);
  const kids = useKids(store, id);
  const reg = useContext(ElementRegistry);
  const lod = useContext(LevelOfDetail);
  const board = !!n?.parent.startsWith("p-");
  const detail = useDetail(store, id);
  const shell = lod && board && !detail;
  const el = useRef<HTMLElement | SVGElement | null>(null);
  const ref = useCallback(
    (e: HTMLElement | SVGElement | null) => {
      el.current = e;
      if (e) reg.set(id, e);
      else reg.delete(id);
    },
    [id, reg],
  );
  // SVG attributes are set directly so hyphenated names (stroke-width) pass through untouched.
  useLayoutEffect(() => {
    if (n?.kind !== "svg" || !el.current) return;
    for (const [k, v] of safeAttrs(n.attrs ?? {})) el.current.setAttribute(k, v);
  }, [n]);
  if (!n) return null;
  let style = (n.hidden ? { ...n.css, display: "none" } : n.css) as React.CSSProperties;
  if (shell) {
    // Keep the artboard's size while its contents are unmounted.
    const r = store.rects.get(id);
    if (r) style = { ...style, width: r.w, height: r.h };
    return <div ref={ref} data-node={id} data-kind={n.kind} data-shell="" style={style} />;
  }
  if (n.kind === "text") {
    return (
      <div ref={ref} data-node={id} data-kind="text" style={style}>
        {n.text}
      </div>
    );
  }
  if (n.kind === "svg") {
    // biome-ignore lint/security/noDangerouslySetInnerHtml: vector markup is built above with escaped attribute values
    return <svg ref={ref} data-node={id} data-kind="svg" style={style} dangerouslySetInnerHTML={{ __html: vectorMarkup(store, kids) }} />;
  }
  const pending = n.image && n.image.status !== "ready";
  return (
    <div ref={ref} data-node={id} data-kind={n.kind} style={style}>
      {pending && (
        <div
          data-image-state={n.image?.status}
          style={{
            position: "absolute",
            inset: 0,
            display: "grid",
            placeItems: "center",
            background: "repeating-linear-gradient(45deg, #0000 0 6px, #0001 6px 12px), #eee",
            color: "#555",
            font: "12px Geist, sans-serif",
            pointerEvents: "none",
          }}
        >
          {imageState(n.image?.status)}
        </div>
      )}
      {kids.map((k) => (
        <NodeEl key={k} store={store} id={k} />
      ))}
    </div>
  );
});

export const PageContent = memo(function PageContent({ store, page }: { store: DocStore; page: string }) {
  const ids = useKids(store, page);
  return (
    <>
      {ids.map((id) => (
        <NodeEl key={id} store={store} id={id} />
      ))}
    </>
  );
});
