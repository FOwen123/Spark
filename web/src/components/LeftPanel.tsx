import { ChevronRight, Eye, EyeOff, File, Frame, Lock, LockOpen, Minus, Plus, Square, Type, Columns2, Rows2, Shapes, Image } from "lucide-react";
import { useEffect, useState } from "react";
import { type DocStore, useDoc, useKids, useNode } from "../lib/doc";
import type { NodeView } from "../lib/types";
import { TokensView } from "./inspector/Tokens";

function KindIcon({ n }: { n: NodeView }) {
  const props = { size: 14, strokeWidth: 1.5, className: "shrink-0 opacity-60" };
  if (n.kind === "text") return <Type {...props} />;
  if (n.kind === "svg" || n.kind === "vector") return <Shapes {...props} />;
  if (n.style.fills?.some((f) => f.type === "image") || n.image) return <Image {...props} />;
  if (n.kind === "rect") return <Square {...props} />;
  if (n.style.display === "flex") return n.style.flexDirection?.startsWith("column") ? <Rows2 {...props} /> : <Columns2 {...props} />;
  return <Frame {...props} />;
}

export function Rename({ value, onDone }: { value: string; onDone: (v: string | null) => void }) {
  return (
    <input
      // biome-ignore lint/a11y/noAutofocus: rename fields take focus when they open
      autoFocus
      defaultValue={value}
      aria-label="Name"
      className="h-5 min-w-0 flex-1 rounded border border-fg bg-bg px-1 text-base outline-none"
      onFocus={(e) => e.currentTarget.select()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") onDone(e.currentTarget.value);
        if (e.key === "Escape") onDone(null);
      }}
      onBlur={(e) => onDone(e.currentTarget.value)}
    />
  );
}

export type LayerDrag = { id: string; over: string | null; where: "before" | "after" | "inside" } | null;

function LayerRow(props: {
  store: DocStore;
  id: string;
  depth: number;
  expanded: Set<string>;
  toggle: (id: string) => void;
  drag: LayerDrag;
  setDrag: (d: LayerDrag) => void;
}) {
  const { store, id, depth } = props;
  const n = useNode(store, id);
  const kids = useKids(store, id);
  useDoc(store);
  const [renaming, setRenaming] = useState(false);
  useEffect(() => {
    const on = (e: Event) => (e as CustomEvent<string>).detail === id && setRenaming(true);
    window.addEventListener("spark:rename", on);
    return () => window.removeEventListener("spark:rename", on);
  }, [id]);
  if (!n) return null;
  const selected = store.selection.includes(id);
  const parentSelected = !selected && store.ancestors(id).some((a) => store.selection.includes(a));
  const open = props.expanded.has(id);
  const canNest = n.kind === "frame" || n.kind === "svg";
  const d = props.drag;
  const marker = d && d.over === id && d.id !== id ? d.where : null;
  return (
    <>
      <div
        role="treeitem"
        aria-selected={selected}
        aria-expanded={kids.length ? open : undefined}
        aria-level={depth + 1}
        data-layer={id}
        tabIndex={-1}
        draggable={!renaming}
        onKeyDown={(e) => e.key === "Enter" && store.select([id])}
        onDragStart={(e) => {
          e.dataTransfer.setData("text/plain", id);
          e.dataTransfer.effectAllowed = "move";
          props.setDrag({ id, over: null, where: "after" });
        }}
        onDragOver={(e) => {
          if (!d) return;
          e.preventDefault();
          const r = e.currentTarget.getBoundingClientRect();
          const t = (e.clientY - r.top) / r.height;
          const where = canNest && t > 0.3 && t < 0.7 ? "inside" : t < 0.5 ? "before" : "after";
          if (d.over !== id || d.where !== where) props.setDrag({ ...d, over: id, where });
        }}
        onDrop={(e) => {
          e.preventDefault();
          if (!d || d.id === id) return props.setDrag(null);
          const ids = store.selection.includes(d.id) ? store.selection : [d.id];
          if (d.where === "inside") store.do({ type: "move", ids, parent: id, index: 0 });
          else {
            const siblings = store.kids(n.parent).filter((s) => !ids.includes(s));
            const at = siblings.indexOf(id);
            // The layer list shows the top-most child first, so "before" in the list is later in order.
            store.do({ type: "move", ids, parent: n.parent, index: d.where === "before" ? at + 1 : at });
          }
          props.setDrag(null);
        }}
        onDragEnd={() => props.setDrag(null)}
        onMouseEnter={() => {
          store.hover = id;
          store.emit();
        }}
        onMouseLeave={() => {
          if (store.hover === id) {
            store.hover = null;
            store.emit();
          }
        }}
        onClick={(e) => {
          if (e.shiftKey || e.metaKey || e.ctrlKey) {
            store.select(selected ? store.selection.filter((s) => s !== id) : [...store.selection, id]);
          } else store.select([id]);
        }}
        onDoubleClick={() => setRenaming(true)}
        className={`group relative flex h-[var(--row-h)] cursor-default items-center gap-1.5 pr-2 text-base ${
          selected ? "bg-selected text-selected-fg" : parentSelected ? "bg-hover" : "hover:bg-hover"
        } ${n.hidden ? "opacity-50" : ""} ${marker === "inside" ? "outline outline-1 -outline-offset-1 outline-fg" : ""}`}
        style={{ paddingLeft: 2 + depth * 16 }}
      >
        {marker && marker !== "inside" && (
          <div className={`pointer-events-none absolute right-0 left-0 h-0.5 bg-fg ${marker === "before" ? "top-0" : "bottom-0"}`} style={{ left: 2 + depth * 16 }} />
        )}
        <button
          type="button"
          aria-label={open ? "Collapse" : "Expand"}
          tabIndex={-1}
          onClick={(e) => {
            e.stopPropagation();
            props.toggle(id);
          }}
          className={`grid size-3 shrink-0 place-items-center ${kids.length ? "" : "invisible"}`}
        >
          <ChevronRight size={12} className={open ? "rotate-90" : ""} />
        </button>
        <KindIcon n={n} />
        {renaming ? (
          <Rename
            value={n.name}
            onDone={(v) => {
              setRenaming(false);
              if (v?.trim() && v !== n.name) store.do({ type: "set", ids: [id], prop: "name", value: v.trim() });
            }}
          />
        ) : (
          <span className="min-w-0 flex-1 truncate">{n.name}</span>
        )}
        <span className={`flex gap-0.5 ${n.hidden || n.locked ? "" : "invisible group-hover:visible"}`}>
          <button
            type="button"
            aria-label={n.locked ? "Unlock" : "Lock"}
            onClick={(e) => {
              e.stopPropagation();
              store.do({ type: "set", ids: [id], prop: "locked", value: !n.locked });
            }}
            className={`grid size-5 place-items-center rounded ${n.locked ? "" : "invisible group-hover:visible"}`}
          >
            {n.locked ? <Lock size={12} /> : <LockOpen size={12} />}
          </button>
          <button
            type="button"
            aria-label={n.hidden ? "Show" : "Hide"}
            onClick={(e) => {
              e.stopPropagation();
              store.do({ type: "set", ids: [id], prop: "hidden", value: !n.hidden });
            }}
            className={`grid size-5 place-items-center rounded ${n.hidden ? "" : "invisible group-hover:visible"}`}
          >
            {n.hidden ? <EyeOff size={12} /> : <Eye size={12} />}
          </button>
        </span>
      </div>
      {open &&
        [...kids].reverse().map((k) => (
          <LayerRow key={k} {...props} id={k} depth={depth + 1} />
        ))}
    </>
  );
}

function Pages({ store }: { store: DocStore }) {
  useDoc(store);
  const [renaming, setRenaming] = useState<string | null>(null);
  return (
    <section aria-label="Pages" className="border-b border-line pb-1">
      <div className="flex h-[var(--row-h)] items-center justify-between pr-1 pl-5 text-base font-medium">
        Pages
        <div className="flex">
          <button type="button" aria-label="Add divider" title="Add divider" onClick={() => store.do({ type: "addPage", divider: true })} className="grid size-6 place-items-center rounded-control text-fg-3 hover:bg-hover hover:text-fg">
            <Minus size={14} />
          </button>
          <button type="button" aria-label="Add page" title="Add page" onClick={() => store.do({ type: "addPage" })} className="grid size-6 place-items-center rounded-control text-fg-3 hover:bg-hover hover:text-fg">
            <Plus size={14} />
          </button>
        </div>
      </div>
      <ul className="max-h-[200px] overflow-auto spark-scroll">
        {store.pages.map((p) =>
          p.divider ? (
            <li key={p.id} className="group flex h-3 items-center pr-3 pl-5" aria-label="Divider">
              <div className="h-px flex-1 bg-line" />
              <button type="button" aria-label="Delete divider" onClick={() => store.do({ type: "deletePage", id: p.id })} className="invisible ml-1 text-fg-3 group-hover:visible">
                <Minus size={10} />
              </button>
            </li>
          ) : (
            <li key={p.id}>
              {/* biome-ignore lint/a11y/useSemanticElements: the row holds its own rename field and delete button */}
              <div
                role="button"
                tabIndex={0}
                aria-current={store.page === p.id}
                onClick={() => store.setPage(p.id)}
                onKeyDown={(e) => e.key === "Enter" && store.setPage(p.id)}
                onDoubleClick={() => setRenaming(p.id)}
                className={`group flex h-[var(--row-h)] items-center gap-1.5 pr-3 pl-5 text-base ${store.page === p.id ? "font-medium" : "text-fg-2 hover:bg-hover"}`}
              >
                <File size={14} strokeWidth={1.5} className="shrink-0 opacity-60" />
                {renaming === p.id ? (
                  <Rename
                    value={p.name}
                    onDone={(v) => {
                      setRenaming(null);
                      if (v?.trim() && v !== p.name) store.do({ type: "setPage", id: p.id, prop: "name", value: v.trim() });
                    }}
                  />
                ) : (
                  <span className="flex-1 truncate">{p.name}</span>
                )}
                {store.pages.filter((x) => !x.divider).length > 1 && (
                  <button
                    type="button"
                    aria-label={`Delete ${p.name}`}
                    onClick={(e) => {
                      e.stopPropagation();
                      store.do({ type: "deletePage", id: p.id });
                    }}
                    className="invisible text-fg-3 group-hover:visible hover:text-fg"
                  >
                    <Minus size={12} />
                  </button>
                )}
              </div>
            </li>
          ),
        )}
      </ul>
    </section>
  );
}

function FileHeader({ store }: { store: DocStore }) {
  useDoc(store);
  const [renaming, setRenaming] = useState(false);
  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: double-click renames; the file menu also offers rename
    <div className="flex h-[var(--inspector-header-h)] shrink-0 items-center gap-2 border-b border-line pr-3 pl-5 text-base font-medium" onDoubleClick={() => setRenaming(true)}>
      {renaming ? (
        <Rename
          value={store.file.name}
          onDone={(v) => {
            setRenaming(false);
            if (v?.trim() && v !== store.file.name) store.do({ type: "renameFile", name: v.trim() });
          }}
        />
      ) : (
        <span className="truncate" title={store.file.path}>
          {store.file.name}
        </span>
      )}
    </div>
  );
}

export function LeftPanel({ store }: { store: DocStore }) {
  useDoc(store);
  const top = useKids(store, store.page);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [drag, setDrag] = useState<LayerDrag>(null);
  const [tab, setTab] = useState<"Design" | "Theme">("Design");
  // Expand ancestors of the selection so selected layers are visible.
  const needed = store.selection.flatMap((s) => store.ancestors(s)).filter((a) => !expanded.has(a));
  if (needed.length) setExpanded(new Set([...expanded, ...needed]));
  const toggle = (id: string) => {
    const next = new Set(expanded);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setExpanded(next);
  };
  return (
    <aside className="flex w-[var(--left-w)] shrink-0 flex-col bg-panel" aria-label="Pages and layers">
      <FileHeader store={store} />
      <div className="flex gap-1 border-b border-line px-3 py-2">
        {(["Design", "Theme"] as const).map((t) => (
          <button
            key={t}
            type="button"
            aria-pressed={tab === t}
            onClick={() => setTab(t)}
            className={`h-6 flex-1 rounded-control text-sm ${tab === t ? "bg-selected text-selected-fg" : "bg-control hover:bg-hover"}`}
          >
            {t}
          </button>
        ))}
      </div>
      {tab === "Theme" ? (
        <div className="min-h-0 flex-1 overflow-auto spark-scroll">
          <TokensView store={store} />
        </div>
      ) : (
        <>
          <Pages store={store} />
          <div className="flex h-[var(--row-h)] items-center px-5 text-base font-medium">Layers</div>
          <div role="tree" aria-label="Layers" className="min-h-0 flex-1 overflow-auto pb-4 spark-scroll">
            {[...top].reverse().map((id) => (
              <LayerRow key={id} store={store} id={id} depth={0} expanded={expanded} toggle={toggle} drag={drag} setDrag={setDrag} />
            ))}
          </div>
        </>
      )}
    </aside>
  );
}
