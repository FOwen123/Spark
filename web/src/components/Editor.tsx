import { useCallback, useEffect, useRef, useState } from "react";
import { copyPayload, readSparkClip, textIds, topLevel } from "../lib/clipboard";
import { type DocStore, useDoc } from "../lib/doc";
import type { Camera } from "../lib/geometry";
import { commandFor } from "../lib/keymap";
import { prefs, setPrefs, usePrefs } from "../lib/prefs";
import { renderToPng } from "../lib/render";
import { selectionFor } from "../lib/selection";
import { COMMAND_TOASTS, showToast } from "../lib/toast";
import type { Agent, Rect, Style } from "../lib/types";
import { Toast } from "./Toast";
import { Canvas, type CanvasApi } from "./Canvas";
import { AgentPanel, type ToolCall } from "./AgentPanel";
import { CanvasMenu } from "./CanvasMenu";
import { CodeDialog } from "./CodeDialog";
import { Inspector } from "./Inspector";
import { LeftPanel, SidebarButton } from "./LeftPanel";
import { type Tool, Toolbar } from "./Toolbar";

function typing(t: EventTarget | null) {
  const el = t as HTMLElement | null;
  return !!el && (el.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName));
}

type ClipData = { html: string; text: string; files: File[] };

export function Editor({ store, agents, calls }: { store: DocStore; agents: Agent[]; calls: ToolCall[] }) {
  useDoc(store);
  const [agentPanel, setAgentPanel] = useState(false);
  // Hides every panel and the toolbar (not remembered across reloads); `floating` and `leftHidden` are in prefs.
  const [uiHidden, setUiHidden] = useState(false);
  const p = usePrefs();
  // Artboards agents are working on in this file, for the falling-dot indicator.
  const agentBoards = new Map<string, number>();
  for (const a of agents) for (const b of a.files[store.file.id] ?? []) agentBoards.set(b, 0);
  const [tool, setTool] = useState<Tool>("move");
  const [code, setCode] = useState<{ title: string; text: string } | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const lastClip = useRef<{ html: string; plain: string } | null>(null);
  const menuPoint = useRef<{ x: number; y: number } | null>(null);
  const [under, setUnder] = useState<string[]>([]);
  const styleClip = useRef<{ style: Style; text: boolean } | null>(null);
  const pickImage = useRef<((hash: string) => void) | null>(null);
  const [cam, setCam] = useState<Camera>({ x: 0, y: 0, zoom: 1 });
  const [spaceDown, setSpaceDown] = useState(false);
  const api = useRef<CanvasApi | null>(null);
  const imageInput = useRef<HTMLInputElement>(null);

  const chooseTool = useCallback((t: Tool) => {
    if (t === "image") {
      imageInput.current?.click();
      return;
    }
    setTool(t);
  }, []);

  /** Where pasted content goes: into a selected frame, next to the selection, or onto the page. */
  const pasteTarget = useCallback(() => {
    const sel = store.selection;
    const one = sel.length === 1 ? store.nodes.get(sel[0]) : undefined;
    if (one?.kind === "frame") return one.id;
    if (sel.length) return store.nodes.get(sel[0])?.parent ?? store.page;
    return store.page;
  }, [store]);

  const uploadImage = useCallback(
    async (file: Blob) => {
      const res = await fetch(`/api/assets?file=${store.file.id}`, {
        method: "POST",
        headers: { Authorization: `Bearer ${sessionStorage.getItem("spark-token") ?? ""}`, "Content-Type": file.type || "application/octet-stream" },
        body: file,
      });
      if (!res.ok) throw new Error(await res.text());
      return (await res.json()) as { hash: string; width: number; height: number };
    },
    [store],
  );

  const placeImage = useCallback(
    async (file: File, at?: { x: number; y: number }) => {
      const a = await uploadImage(file);
      const scale = Math.min(1, 800 / Math.max(a.width, a.height, 1));
      const c = at ?? api.current?.center() ?? { x: 0, y: 0 };
      const w = Math.round(a.width * scale);
      const h = Math.round(a.height * scale);
      const target = pasteTarget();
      const r = await store.do<{ id: string }>({ type: "create", kind: "rect", parent: target, x: c.x - (at ? 0 : w / 2), y: c.y - (at ? 0 : h / 2), w, h });
      await store.do({ type: "set", ids: [r.id], prop: "style.fills", value: [{ type: "image", asset: a.hash, fit: "cover" }] });
      await store.do({ type: "set", ids: [r.id], prop: "name", value: file.name.replace(/\.[^.]+$/, "") || "Image" });
    },
    [store, uploadImage, pasteTarget],
  );

  const paste = useCallback(
    async (data: ClipData, at?: { x: number; y: number }, mode?: "top" | "replace") => {
      const sel = store.selection;
      if (mode === "replace" && !readSparkClip(data.html)) return setToast("Copy layers first, then paste to replace");
      // Paste on top: same place the layers were copied from, next to the selection.
      const parent = mode === "top" && sel.length ? (store.nodes.get(sel[0])?.parent ?? store.page) : pasteTarget();
      const pos = at ?? (parent === store.page ? api.current?.center() : (() => {
        const r = store.rects.get(parent);
        return r ? { x: r.x + 16, y: r.y + 16 } : undefined;
      })());
      const spark = readSparkClip(data.html);
      if (spark) {
        if (mode === "replace") {
          const r = await store.do<{ ids: string[] }>({ type: "pasteSpark", roots: spark.roots, replace: topLevel(store, sel) });
          return store.select(r.ids);
        }
        const rects = spark.roots.flatMap((t) => (t.rect ? [t.rect] : []));
        const origin = mode === "top" && rects.length ? { x: Math.min(...rects.map((r) => r.x)), y: Math.min(...rects.map((r) => r.y)) } : undefined;
        const r = await store.do<{ ids: string[] }>({ type: "pasteSpark", parent, roots: spark.roots, from: spark.from, ...(origin ?? at ?? {}) });
        return store.select(r.ids);
      }
      const image = data.files.find((f) => f.type.startsWith("image/"));
      if (image) return placeImage(image, at);
      const html = data.html.trim() || (/^\s*<svg[\s>]/i.test(data.text) ? data.text : "");
      if (html) {
        const r = await store.do<{ ids: string[] }>({ type: "pasteHtml", parent, html, ...(pos ?? {}) });
        return store.select(r.ids);
      }
      if (data.text.trim()) {
        const r = await store.do<{ ids: string[] }>({ type: "pasteText", parent, text: data.text, ...(pos ?? { x: 0, y: 0 }) });
        return store.select(r.ids);
      }
    },
    [store, pasteTarget, placeImage],
  );

  const copy = useCallback(
    (cut: boolean, e?: ClipboardEvent) => {
      const sel = topLevel(store, store.selection);
      if (!sel.length) return;
      const p = copyPayload(store, sel);
      lastClip.current = p;
      if (e?.clipboardData) {
        e.clipboardData.setData("text/html", p.html);
        e.clipboardData.setData("text/plain", p.plain);
        e.preventDefault();
      } else {
        // Menu copies: write both formats when the async clipboard allows it.
        const item = typeof ClipboardItem !== "undefined" ? new ClipboardItem({ "text/html": new Blob([p.html], { type: "text/html" }), "text/plain": new Blob([p.plain], { type: "text/plain" }) }) : null;
        if (item) navigator.clipboard.write([item]).catch(() => {});
      }
      if (cut) store.do({ type: "delete", ids: sel });
    },
    [store],
  );

  const readClipboard = useCallback(async (): Promise<ClipData | null> => {
    try {
      const items = await navigator.clipboard.read();
      const data: ClipData = { html: "", text: "", files: [] };
      for (const it of items) {
        for (const type of it.types) {
          const blob = await it.getType(type);
          if (type === "text/html") data.html = await blob.text();
          else if (type === "text/plain") data.text = await blob.text();
          else if (type.startsWith("image/")) data.files.push(new File([blob], "Pasted image", { type }));
        }
      }
      return data;
    } catch {
      return lastClip.current ? { html: lastClip.current.html, text: lastClip.current.plain, files: [] } : null;
    }
  }, []);

  const pasteFromMenu = useCallback(
    async (here: boolean) => {
      const data = await readClipboard();
      if (data) return paste(data, here && menuPoint.current ? menuPoint.current : undefined);
    },
    [paste, readClipboard],
  );

  const pasteWith = useCallback(
    async (mode: "top" | "replace") => {
      const data = await readClipboard();
      if (data) return paste(data, undefined, mode);
    },
    [paste, readClipboard],
  );

  /** Puts a blob (possibly still being made) on the system clipboard, then says so. */
  const writeClip = useCallback(async (type: string, blob: Promise<Blob>, label: string) => {
    try {
      await navigator.clipboard.write([new ClipboardItem({ [type]: blob })]);
      setToast(`Copied ${label}`);
    } catch (e) {
      setToast(`Couldn't copy ${label}: ${(e as Error).message}`);
    }
  }, []);

  const showCode = useCallback(
    async (format: "html" | "jsx" | "tailwind") => {
      const ids = topLevel(store, store.selection);
      if (!ids.length) return;
      const text = await store.do<string>({ type: "code", ids, format });
      setCode({ title: format === "html" ? "HTML" : format === "jsx" ? "JSX" : "JSX with Tailwind", text });
    },
    [store],
  );

  useEffect(() => {
    const onCopy = (e: ClipboardEvent) => {
      if (typing(e.target) || !store.selection.length) return;
      copy(e.type === "cut", e);
    };
    const onPaste = (e: ClipboardEvent) => {
      if (typing(e.target)) return;
      e.preventDefault();
      const dt = e.clipboardData;
      paste({ html: dt?.getData("text/html") ?? "", text: dt?.getData("text/plain") ?? "", files: [...(dt?.files ?? [])] });
    };
    const onPick = (e: Event) => {
      pickImage.current = (e as CustomEvent<(hash: string) => void>).detail;
      fillInput.current?.click();
    };
    document.addEventListener("copy", onCopy);
    document.addEventListener("cut", onCopy);
    document.addEventListener("paste", onPaste);
    window.addEventListener("spark:pick-image", onPick);
    return () => {
      document.removeEventListener("copy", onCopy);
      document.removeEventListener("cut", onCopy);
      document.removeEventListener("paste", onPaste);
      window.removeEventListener("spark:pick-image", onPick);
    };
  }, [store, copy, paste]);

  const fillInput = useRef<HTMLInputElement>(null);

  const exportNodes = useCallback(
    async (ids: string[], presets: unknown[], combined = false) => {
      if (!ids.length) return;
      setToast(combined ? "Exporting PDF…" : "Exporting…");
      try {
        const r = await store.conn.request<{ paths: string[] }>({ t: "export", file: store.file.id, ids, presets, combined });
        setToast(r.paths.length === 1 ? `Exported to ${r.paths[0]}` : `Exported ${r.paths.length} files to ${r.paths[0].replace(/[\\/][^\\/]*$/, "")}`);
      } catch (e) {
        setToast(`Export failed: ${(e as Error).message}`);
      }
    },
    [store],
  );

  useEffect(() => {
    const on = (e: Event) => {
      const d = (e as CustomEvent<{ ids: string[]; presets: unknown[]; combined?: boolean }>).detail;
      exportNodes(d.ids, d.presets, d.combined);
    };
    window.addEventListener("spark:export", on);
    return () => window.removeEventListener("spark:export", on);
  }, [exportNodes]);

  useEffect(() => {
    if (!toast || toast.endsWith("…")) return;
    const t = setTimeout(() => setToast(null), 5000);
    return () => clearTimeout(t);
  }, [toast]);

  const run = useCallback(
    (cmd: string) => {
      if (COMMAND_TOASTS[cmd]) showToast(COMMAND_TOASTS[cmd]);
      const sel = store.selection;
      const rectsOf = (ids: string[]) => api.current?.rectOf(ids) ?? null;
      const nudge = (dx: number, dy: number) => sel.length && store.do({ type: "nudge", ids: sel, dx, dy, merge: "nudge" });
      const [name, arg] = cmd.split(".");
      switch (name) {
        case "tool":
          return chooseTool(arg as Tool);
        case "undo":
        case "redo":
          return store.do({ type: name });
        case "delete":
          return sel.length && store.do({ type: "delete", ids: sel });
        case "duplicate":
          return sel.length && store.do<{ ids: string[] }>({ type: "duplicate", ids: sel }).then((r) => store.select(r.ids));
        case "selectAll": {
          const parent = sel.length ? store.nodes.get(sel[0])?.parent : store.page;
          return parent && store.select(store.kids(parent).filter((id) => !store.nodes.get(id)?.locked));
        }
        case "selectChildren":
          return store.select(sel.flatMap((id) => store.kids(id)).length ? sel.flatMap((id) => store.kids(id)) : sel);
        case "selectParent":
        case "escape": {
          const parents = [...new Set(sel.map((id) => store.nodes.get(id)?.parent).filter((p): p is string => !!p && !p.startsWith("p-")))];
          return store.select(parents.length ? parents : name === "escape" ? [] : sel);
        }
        case "selectNext":
        case "selectPrev":
        case "selectOthers":
        case "boardNext":
        case "boardPrev": {
          const ids = selectionFor(store, name);
          if (!ids) return;
          store.select(ids);
          return name.startsWith("board") ? api.current?.zoomTo(rectsOf(ids)) : undefined;
        }
        case "align":
        case "distribute":
        case "transform":
          return sel.length && store.do({ type: name, ids: sel, dir: arg });
        case "swapColors":
        case "ungroup":
          return sel.length && store.do({ type: name, ids: sel });
        case "toggleClip": {
          const frames = sel.filter((id) => store.nodes.get(id)?.kind === "frame");
          const on = frames.every((id) => ["clip", "hidden"].includes(store.nodes.get(id)?.style.overflow ?? ""));
          return frames.length && store.do({ type: "setStyle", ids: frames, styles: { overflow: on ? null : "clip" } });
        }
        case "page": {
          const pages = store.pages.filter((p) => !p.divider);
          const next = pages[pages.findIndex((p) => p.id === store.page) + (arg === "next" ? 1 : -1)];
          return next && store.setPage(next.id);
        }
        case "zoomFit":
          return api.current?.zoomTo(rectsOf(store.kids(store.page)));
        case "zoomSelection":
          return api.current?.zoomTo(rectsOf(sel.length ? sel : store.kids(store.page)));
        case "zoom100":
          return api.current?.zoom100();
        case "zoomIn":
          return api.current?.zoomBy(2);
        case "zoomOut":
          return api.current?.zoomBy(0.5);
        case "nudge":
        case "nudgeBig": {
          const k = name === "nudgeBig" ? prefs().nudgeLarge : prefs().nudgeSmall;
          const dirs: Record<string, [number, number]> = { up: [0, -k], down: [0, k], left: [-k, 0], right: [k, 0] };
          if (!sel.length) {
            const [dx, dy] = dirs[arg];
            return api.current?.panBy(-dx * 8, -dy * 8);
          }
          return nudge(...dirs[arg]);
        }
        case "groupInFrame":
          return sel.length && store.do<{ id: string }>({ type: "wrap", ids: sel }).then((r) => store.select([r.id]));
        case "addFlex":
          return sel.length && store.do({ type: "flex", ids: sel });
        case "bringForward":
        case "sendBackward":
        case "bringToFront":
        case "sendToBack":
          return sel.length && store.do({ type: "zorder", ids: sel, dir: name });
        case "toggleHidden":
        case "toggleLocked": {
          const prop = name === "toggleHidden" ? "hidden" : "locked";
          const value = !sel.every((id) => store.nodes.get(id)?.[prop]);
          return sel.length && store.do({ type: "set", ids: sel, prop, value });
        }
        case "rename":
          return sel.length === 1 && window.dispatchEvent(new CustomEvent("spark:rename", { detail: sel[0] }));
        case "export":
          return sel.length && exportNodes(sel, sel.length === 1 ? (store.nodes.get(sel[0])?.exports ?? []) : []);
        case "copyStyle": {
          const n = store.nodes.get(sel[0]);
          if (!n || sel.length !== 1) return;
          styleClip.current = { style: n.style, text: n.kind === "text" };
          return setToast("Copied styles");
        }
        case "pasteStyle":
          if (!styleClip.current) return setToast("Copy styles first");
          return sel.length && store.do({ type: "pasteStyle", ids: sel, ...styleClip.current });
        case "pasteOnTop":
          return pasteWith("top");
        case "pasteReplace":
          return sel.length && pasteWith("replace");
        case "copyPng":
          return sel.length && writeClip("image/png", renderToPng(store, sel[0], 2).then((b) => fetch(`data:image/png;base64,${b}`).then((r) => r.blob())), "PNG");
        case "copySvg":
          return sel.length && writeClip("text/plain", store.do<string>({ type: "code", ids: topLevel(store, sel), format: "svg" }).then((t) => new Blob([t], { type: "text/plain" })), "SVG");
        case "copyLink":
          return sel.length && navigator.clipboard.writeText(`spark://file/${store.file.id}/${store.page}?node=${sel[0]}`).then(() => setToast("Copied link"));
        case "adjustText": {
          const [prop, dir] = arg.split(":");
          return store.do({ type: "adjustText", ids: textIds(store, sel), prop, dir: Number(dir) });
        }
        case "panel":
          return setAgentPanel((v) => !v);
        case "toggleUI":
          return setUiHidden((v) => !v);
        case "toggleFloating":
          return setPrefs({ floating: !prefs().floating });
        case "togglePixelGrid":
        case "toggleSnapPixel":
        case "toggleLayoutGuides": {
          // The menu shows these from prefs; features read them there. The event lets them react to a flip.
          const key = { togglePixelGrid: "pixelGrid", toggleSnapPixel: "snapPixel", toggleLayoutGuides: "layoutGuides" }[name] as "pixelGrid" | "snapPixel" | "layoutGuides";
          setPrefs({ [key]: !prefs()[key] });
          return window.dispatchEvent(new CustomEvent("spark:command", { detail: cmd }));
        }
        case "copyJsx":
          return showCode("jsx");
        case "copyTailwind":
          return showCode("tailwind");
        default:
          window.dispatchEvent(new CustomEvent("spark:command", { detail: cmd }));
      }
    },
    [store, chooseTool, showCode, exportNodes, pasteWith, writeClip],
  );

  // Native menu commands (desktop app).
  useEffect(() => {
    const onMenu = (e: Event) => {
      const cmd = (e as CustomEvent<string>).detail;
      // In a text field, Undo and Redo belong to the field.
      if ((cmd === "undo" || cmd === "redo") && typing(document.activeElement)) return document.execCommand(cmd);
      run(cmd);
    };
    window.addEventListener("spark:menu", onMenu);
    return () => window.removeEventListener("spark:menu", onMenu);
  }, [run]);


  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (typing(e.target)) return;
      if (e.code === "Space" && !e.repeat) {
        setSpaceDown(true);
        e.preventDefault();
        return;
      }
      const cmd = commandFor(e);
      if (!cmd) return;
      // Tab keeps moving focus inside a focused control or menu; it selects only from the canvas and layers.
      if (e.key === "Tab" && (e.target as HTMLElement).closest?.("button, a, input, [role^=menu]")) return;
      e.preventDefault();
      run(cmd);
    };
    const up = (e: KeyboardEvent) => {
      if (e.code === "Space") setSpaceDown(false);
    };
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
    };
  }, [run]);

  // In floating mode the toolbar and panels sit over the canvas as rounded cards instead of docking.
  const card = "absolute z-30 flex overflow-hidden rounded-popover border border-line shadow-popover [&>aside]:border-0 [&>nav]:border-0";
  const menu = { store, run, onPaste: pasteFromMenu, onCopy: (cut: boolean) => copy(cut), onCode: showCode };
  const left = p.leftHidden ? null : <LeftPanel store={store} menu={menu} agents={agents} run={run} />;
  const inspector = (
    <Inspector
        store={store}
        cam={cam}
        run={run}
        zoomTo={(r: Rect | null) => api.current?.zoomTo(r)}
        agents={agents}
        reveal={(page, ids) => {
          store.setPage(page);
          // The page's layer rects exist once it has rendered and laid out, two frames later.
          requestAnimationFrame(() => requestAnimationFrame(() => api.current?.zoomTo(api.current.rectOf(ids))));
        }}
      />
  );
  return (
    <div className="relative flex min-h-0 flex-1">
      {!uiHidden && (p.floating ? left && <div className={`${card} top-2 bottom-2 left-2`}>{left}</div> : left)}
      {!uiHidden && (p.floating ? <div className={`${card} top-2 ${p.leftHidden ? "left-2" : "left-[calc(var(--left-w)+1rem)]"}`}><Toolbar tool={tool} onTool={chooseTool} /></div> : <Toolbar tool={tool} onTool={chooseTool} />)}
      {!uiHidden && p.leftHidden && <SidebarButton className="absolute top-2 left-2 z-30 bg-panel" />}
      <CanvasMenu {...menu} under={under} empty={!under.length}>
        {/* biome-ignore lint/a11y/noStaticElementInteractions: records where the context menu opened */}
        <div
          className="flex min-w-0 flex-1"
          onContextMenu={(e) => {
            const r = e.currentTarget.getBoundingClientRect();
            menuPoint.current = api.current?.toWorld(e.clientX - r.left, e.clientY - r.top) ?? null;
            // Layers under the pointer, topmost first (for "Select layer"); none means empty canvas.
            const ids = document.elementsFromPoint(e.clientX, e.clientY).map((el) => el.closest("[data-node]")?.getAttribute("data-node"));
            setUnder([...new Set(ids)].filter((id): id is string => !!id && !!store.nodes.get(id) && store.nodes.get(id)?.kind !== "vector"));
          }}
        >
          <Canvas store={store} tool={tool} setTool={setTool} cam={cam} setCam={setCam} spaceDown={spaceDown} apiRef={api} onImageTool={() => imageInput.current?.click()} agentBoards={agentBoards} />
        </div>
      </CanvasMenu>
      {agentPanel && !uiHidden && <AgentPanel store={store} agents={agents} calls={calls} onClose={() => setAgentPanel(false)} />}
      {!uiHidden && (p.floating ? <div className={`${card} top-2 right-2 bottom-2`}>{inspector}</div> : inspector)}
      <input
        ref={imageInput}
        type="file"
        accept="image/png,image/jpeg,image/webp,image/gif,image/svg+xml"
        hidden
        data-testid="image-input"
        onChange={(e) => {
          const f = e.target.files?.[0];
          if (f) placeImage(f);
          e.target.value = "";
        }}
      />
      <input
        ref={fillInput}
        type="file"
        accept="image/png,image/jpeg,image/webp,image/gif,image/svg+xml"
        hidden
        data-testid="fill-image-input"
        onChange={async (e) => {
          const f = e.target.files?.[0];
          e.target.value = "";
          if (f && pickImage.current) pickImage.current((await uploadImage(f)).hash);
        }}
      />
      <CodeDialog title={code?.title ?? ""} code={code?.text ?? null} onClose={() => setCode(null)} />
      <Toast>{toast}</Toast>
    </div>
  );
}
