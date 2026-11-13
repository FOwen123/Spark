import {
  ArrowRight,
  Check,
  ChevronDown,
  ChevronRight,
  ChartColumn,
  ChartLine,
  ChartPie,
  CircleQuestionMark,
  Eye,
  EyeOff,
  File,
  Frame,
  Info,
  Lock,
  LockOpen,
  Minus,
  PanelLeft,
  PenTool,
  Plus,
  Presentation,
  Ruler,
  Square,
  SwatchBook,
  Table,
  Type,
  VectorSquare,
  X,
  Columns2,
  Rows2,
  Shapes,
  Image,
  Images,
} from "lucide-react";
import * as ContextMenu from "@radix-ui/react-context-menu";
import * as Menu from "@radix-ui/react-dropdown-menu";
import { useEffect, useRef, useState } from "react";
import { type DocStore, useDoc, useKids, useNode } from "../lib/doc";
import { comboFor } from "../lib/keymap";
import { setPrefs, usePrefs } from "../lib/prefs";
import type { Agent, NodeView } from "../lib/types";
import { CanvasMenu, Item, menuItem } from "./CanvasMenu";
import { PanelResizer } from "./PanelResizer";
import { Keycaps, Tip } from "./Tip";
import { TokensView } from "./inspector/Tokens";

export function KindIcon({ n }: { n: NodeView }) {
  const props = { size: 14, strokeWidth: 1.5, className: "shrink-0 opacity-60" };
  if (n.kind === "text") return <Type {...props} />;
  if (n.kind === "svg" || n.kind === "vector") return <Shapes {...props} />;
  if (n.style.fills?.some((f) => f.type === "image") || n.image) return <Image {...props} />;
  if (n.kind === "rect") return <Square {...props} />;
  if (n.style.display === "flex") return n.style.flexDirection?.startsWith("column") ? <Rows2 {...props} /> : <Columns2 {...props} />;
  return <Frame {...props} />;
}

/** The page icons a page can carry (the names the core accepts), as Lucide icons. */
const PAGE_ICONS: Record<string, [typeof File, string]> = {
  arrow: [ArrowRight, "Arrow"],
  padlock: [Lock, "Lock"],
  check: [Check, "Check"],
  cross: [X, "Cross"],
  info: [Info, "Info"],
  question: [CircleQuestionMark, "Question"],
  design: [PenTool, "Pen"],
  vector: [VectorSquare, "Vector"],
  images: [Images, "Image"],
  table: [Table, "Table"],
  presentation: [Presentation, "Presentation"],
  blueprint: [Ruler, "Blueprint"],
  swatches: [SwatchBook, "Swatches"],
  "bar-chart": [ChartColumn, "Bar chart"],
  "line-chart": [ChartLine, "Line chart"],
  "pie-chart": [ChartPie, "Pie chart"],
  page: [File, "Page"],
};

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
  /** While searching: the rows to show (matches and their ancestors). */
  filter: Set<string> | null;
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
  if (!n || (props.filter && !props.filter.has(id))) return null;
  const selected = store.selection.includes(id);
  const parentSelected = !selected && store.ancestors(id).some((a) => store.selection.includes(a));
  const open = props.filter ? kids.some((k) => props.filter?.has(k)) : props.expanded.has(id);
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
        onKeyDown={(e) => e.key === "Enter" && !e.metaKey && !e.ctrlKey && store.select([id])}
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
        style={{ paddingLeft: 6 + depth * 16 }}
      >
        {marker && marker !== "inside" && (
          <div className={`pointer-events-none absolute right-0 left-0 h-0.5 bg-fg ${marker === "before" ? "top-0" : "bottom-0"}`} style={{ left: 6 + depth * 16 }} />
        )}
        <button
          type="button"
          aria-label={open ? "Collapse" : "Expand"}
          tabIndex={-1}
          onClick={(e) => {
            e.stopPropagation();
            props.toggle(id);
          }}
          className={`-mr-1 grid size-3 shrink-0 place-items-center ${kids.length ? "" : "invisible"}`}
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

type PageDrag = { id: string; over: string | null; where: "before" | "after" } | null;

function Pages({ store, agents }: { store: DocStore; agents: Agent[] }) {
  useDoc(store);
  const p = usePrefs();
  const [renaming, setRenaming] = useState<string | null>(null);
  const [drag, setDrag] = useState<PageDrag>(null);
  // Pages with an agent working on one of their artboards.
  const busy = new Set<string>();
  for (const a of agents) for (const id of a.files[store.file.id] ?? []) busy.add(store.pageOf(id) ?? store.nodes.get(id)?.parent ?? "");
  const pageCount = store.pages.filter((x) => !x.divider).length;
  /** Row drag handlers: pages and dividers drag to reorder. */
  const dnd = (id: string) => ({
    draggable: renaming !== id,
    onDragStart: (e: React.DragEvent) => {
      e.dataTransfer.setData("text/plain", id);
      e.dataTransfer.effectAllowed = "move";
      setDrag({ id, over: null, where: "after" });
    },
    onDragOver: (e: React.DragEvent) => {
      if (!drag) return;
      e.preventDefault();
      const r = e.currentTarget.getBoundingClientRect();
      const where = e.clientY - r.top < r.height / 2 ? "before" : "after";
      if (drag.over !== id || drag.where !== where) setDrag({ ...drag, over: id, where });
    },
    onDrop: (e: React.DragEvent) => {
      e.preventDefault();
      if (drag && drag.id !== id) {
        const rest = store.pages.filter((x) => x.id !== drag.id);
        const at = rest.findIndex((x) => x.id === id);
        store.do({ type: "movePage", id: drag.id, index: drag.where === "before" ? at : at + 1 });
      }
      setDrag(null);
    },
    onDragEnd: () => setDrag(null),
  });
  const mark = (id: string) => (drag && drag.over === id && drag.id !== id ? (drag.where === "before" ? "shadow-[inset_0_2px_0_var(--spark-fg)]" : "shadow-[inset_0_-2px_0_var(--spark-fg)]") : "");
  return (
    <section aria-label="Pages" className="border-b border-line pb-1">
      <div className="flex h-[var(--row-h)] items-center justify-between pr-1 pl-1.5 text-base font-medium">
        {/* Chevron at x 6 like the layer rows' chevrons, label at x 20 like the page icons. */}
        <button type="button" aria-expanded={!p.pagesCollapsed} onClick={() => setPrefs({ pagesCollapsed: !p.pagesCollapsed })} className="flex h-6 items-center gap-0.5 rounded-control pr-2 hover:bg-hover">
          {p.pagesCollapsed ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
          Pages
        </button>
        <div className="flex">
          <button type="button" aria-label="Add divider" title="Add divider" onClick={() => store.do({ type: "addPage", divider: true })} className="grid size-6 place-items-center rounded-control text-fg-3 hover:bg-hover hover:text-fg">
            <Minus size={14} />
          </button>
          <button type="button" aria-label="Add page" title="Add page" onClick={() => store.do({ type: "addPage" })} className="grid size-6 place-items-center rounded-control text-fg-3 hover:bg-hover hover:text-fg">
            <Plus size={14} />
          </button>
        </div>
      </div>
      {!p.pagesCollapsed && (
        <ul className="max-h-[200px] overflow-auto spark-scroll">
          {store.pages.map((pg) => {
            if (pg.divider)
              return (
                <li key={pg.id} className={`group flex h-3 items-center pr-3 pl-5 ${mark(pg.id)}`} aria-label="Divider" {...dnd(pg.id)}>
                  <div className="h-px flex-1 bg-line" />
                  <button type="button" aria-label="Delete divider" onClick={() => store.do({ type: "deletePage", id: pg.id })} className="invisible ml-1 text-fg-3 group-hover:visible">
                    <Minus size={10} />
                  </button>
                </li>
              );
            const current = store.page === pg.id;
            const [Icon] = PAGE_ICONS[pg.icon ?? "page"] ?? PAGE_ICONS.page;
            return (
              <ContextMenu.Root key={pg.id}>
                <ContextMenu.Trigger asChild>
                  <li className={mark(pg.id)} {...dnd(pg.id)}>
                    {/* biome-ignore lint/a11y/useSemanticElements: the row holds its own rename field and delete button */}
                    <div
                      role="button"
                      tabIndex={0}
                      aria-current={current}
                      onClick={() => store.setPage(pg.id)}
                      onKeyDown={(e) => e.key === "Enter" && store.setPage(pg.id)}
                      onDoubleClick={() => setRenaming(pg.id)}
                      className={`group flex h-[var(--row-h)] items-center gap-1.5 pr-3 pl-5 text-base ${current ? "font-medium" : "text-fg-2 hover:bg-hover"}`}
                    >
                      <Icon size={14} strokeWidth={1.5} className="shrink-0 opacity-60" />
                      {renaming === pg.id ? (
                        <Rename
                          value={pg.name}
                          onDone={(v) => {
                            setRenaming(null);
                            if (v?.trim() && v !== pg.name) store.do({ type: "setPage", id: pg.id, prop: "name", value: v.trim() });
                          }}
                        />
                      ) : (
                        <span className="flex-1 truncate">{pg.name}</span>
                      )}
                      {busy.has(pg.id) && <span aria-hidden data-testid="page-busy" className="size-1.5 shrink-0 animate-pulse rounded-full bg-current" />}
                      {current && <Check size={12} data-testid="page-current" className="shrink-0" />}
                      {pageCount > 1 && (
                        <button
                          type="button"
                          aria-label={`Delete ${pg.name}`}
                          onClick={(e) => {
                            e.stopPropagation();
                            store.do({ type: "deletePage", id: pg.id });
                          }}
                          className="invisible text-fg-3 group-hover:visible hover:text-fg"
                        >
                          <Minus size={12} />
                        </button>
                      )}
                    </div>
                  </li>
                </ContextMenu.Trigger>
                <ContextMenu.Portal>
                  <ContextMenu.Content className="z-50 min-w-[180px] rounded-popover border border-line bg-panel p-1 shadow-popover" aria-label="Page menu" onCloseAutoFocus={(e) => e.preventDefault()}>
                    <Item label="Rename page" onSelect={() => setRenaming(pg.id)} />
                    <Item label="Duplicate page" onSelect={() => store.do({ type: "duplicatePage", id: pg.id })} />
                    <Item label="Add divider" onSelect={() => store.do({ type: "addPage", divider: true, index: store.pages.indexOf(pg) + 1 })} />
                    <ContextMenu.Sub>
                      <ContextMenu.SubTrigger className={menuItem}>
                        <span className="flex-1">Icon</span>
                        <ChevronRight size={12} />
                      </ContextMenu.SubTrigger>
                      <ContextMenu.Portal>
                        <ContextMenu.SubContent className="z-50 grid grid-cols-6 gap-0.5 rounded-popover border border-line bg-panel p-1 shadow-popover" aria-label="Page icons">
                          {Object.entries(PAGE_ICONS).map(([name, [I, label]]) => (
                            <ContextMenu.Item
                              key={name}
                              aria-label={label}
                              title={label}
                              data-active={(pg.icon ?? "page") === name}
                              onSelect={() => store.do({ type: "setPage", id: pg.id, prop: "icon", value: name === "page" ? null : name })}
                              className="grid size-7 cursor-default place-items-center rounded-control outline-none data-[active=true]:outline data-[active=true]:outline-1 data-[active=true]:outline-fg data-[highlighted]:bg-selected data-[highlighted]:text-selected-fg"
                            >
                              <I size={14} strokeWidth={1.5} />
                            </ContextMenu.Item>
                          ))}
                        </ContextMenu.SubContent>
                      </ContextMenu.Portal>
                    </ContextMenu.Sub>
                    <Item label="Delete page" onSelect={() => store.do({ type: "deletePage", id: pg.id })} disabled={pageCount < 2} />
                  </ContextMenu.Content>
                </ContextMenu.Portal>
              </ContextMenu.Root>
            );
          })}
        </ul>
      )}
    </section>
  );
}

/** Hides or shows the left panel. */
export function SidebarButton({ className = "" }: { className?: string }) {
  const p = usePrefs();
  return (
    <Tip label={p.leftHidden ? "Show sidebar" : "Hide sidebar"} side="bottom">
      <button type="button" aria-label={p.leftHidden ? "Show sidebar" : "Hide sidebar"} onClick={() => setPrefs({ leftHidden: !p.leftHidden })} className={`grid size-6 shrink-0 place-items-center rounded-control text-fg-3 hover:bg-hover hover:text-fg ${className}`}>
        <PanelLeft size={14} strokeWidth={1.5} />
      </button>
    </Tip>
  );
}

/** File name that opens the main menu; double-click renames. */
function FileHeader({ store, run }: { store: DocStore; run: (cmd: string) => void }) {
  useDoc(store);
  const p = usePrefs();
  const [renaming, setRenaming] = useState(false);
  const cmd = (label: string, name: string) => (
    <Menu.Item className={menuItem} onSelect={() => run(name)}>
      <span className="flex-1">{label}</span>
      {comboFor(name) && <Keycaps combo={comboFor(name) as string} />}
    </Menu.Item>
  );
  const check = (label: string, name: string, on: boolean) => (
    <Menu.CheckboxItem checked={on} onCheckedChange={() => run(name)} onSelect={(e) => e.preventDefault()} className={menuItem}>
      <span className="grid size-3 place-items-center">
        <Menu.ItemIndicator>
          <Check size={12} />
        </Menu.ItemIndicator>
      </span>
      {label}
    </Menu.CheckboxItem>
  );
  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: double-click renames; the file menu also offers rename
    <div className="flex h-[var(--inspector-header-h)] shrink-0 items-center gap-1 border-b border-line pr-2 pl-3 text-base font-medium" onDoubleClick={() => setRenaming(true)}>
      {renaming ? (
        <Rename
          value={store.file.name}
          onDone={(v) => {
            setRenaming(false);
            if (v?.trim() && v !== store.file.name) store.do({ type: "renameFile", name: v.trim() });
          }}
        />
      ) : (
        // Not modal, so a double-click on the name still reaches the header and renames.
        <Menu.Root modal={false}>
          <Menu.Trigger aria-label="File menu" title={store.file.path} className="flex h-6 min-w-0 items-center gap-1 rounded-control px-2 hover:bg-hover data-[state=open]:bg-hover">
            <span className="truncate">{store.file.name}</span>
            <ChevronDown size={12} className="shrink-0" />
          </Menu.Trigger>
          <Menu.Portal>
            <Menu.Content align="start" sideOffset={4} aria-label="File menu" onCloseAutoFocus={(e) => e.preventDefault()} className="z-50 min-w-[220px] rounded-popover border border-line bg-panel p-1 shadow-popover">
              <Menu.Item className={menuItem} onSelect={() => setRenaming(true)}>
                Rename file
              </Menu.Item>
              <Menu.Item className={menuItem} onSelect={() => store.conn.request<{ id: string }>({ t: "duplicate", id: store.file.id }).then((f) => store.conn.request({ t: "reveal", ref: f.id }))}>
                Duplicate file
              </Menu.Item>
              <Menu.Separator className="my-1 h-px bg-line" />
              {cmd("Keyboard shortcuts", "app.shortcuts")}
              {cmd("Settings", "showSettings")}
              <Menu.Separator className="my-1 h-px bg-line" />
              {check("Pixel grid", "togglePixelGrid", p.pixelGrid)}
              {check("Snap to pixel", "toggleSnapPixel", p.snapPixel)}
              {check("Layout guides", "toggleLayoutGuides", p.layoutGuides)}
            </Menu.Content>
          </Menu.Portal>
        </Menu.Root>
      )}
      <div className="flex-1" />
      <SidebarButton />
    </div>
  );
}

/** Rows to show while searching: matches and their ancestors, plus the first match in display order. */
function search(store: DocStore, roots: string[], q: string) {
  const show = new Set<string>();
  let first: string | undefined;
  const walk = (id: string): boolean => {
    const self = store.nodes.get(id)?.name.toLowerCase().includes(q) ?? false;
    if (self && !first) first = id;
    let any = self;
    for (const k of [...store.kids(id)].reverse()) if (walk(k)) any = true;
    if (any) show.add(id);
    return any;
  };
  for (const r of [...roots].reverse()) walk(r);
  return { show, first };
}

export function LeftPanel({ store, menu, agents, run }: { store: DocStore; menu: Omit<React.ComponentProps<typeof CanvasMenu>, "children" | "under" | "empty">; agents: Agent[]; run: (cmd: string) => void }) {
  useDoc(store);
  const top = useKids(store, store.page);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [drag, setDrag] = useState<LayerDrag>(null);
  const [tab, setTab] = useState<"Design" | "Theme">("Design");
  // null: search closed.
  const [query, setQuery] = useState<string | null>(null);
  useEffect(() => {
    const on = (e: Event) => {
      const cmd = (e as CustomEvent<string>).detail;
      if (cmd === "leftTab.design") setTab("Design");
      if (cmd === "leftTab.theme") setTab("Theme");
      if (cmd === "collapseLayers") setExpanded(new Set());
      if (cmd !== "findLayers") return;
      setTab("Design");
      setQuery((q) => q ?? "");
    };
    window.addEventListener("spark:command", on);
    return () => window.removeEventListener("spark:command", on);
  }, []);
  const found = query?.trim() ? search(store, top, query.trim().toLowerCase()) : null;
  // Expand ancestors of a new selection so selected layers are visible ("Collapse all layers" keeps them closed).
  const seen = useRef("");
  const sig = store.selection.join(",");
  const needed = seen.current === sig ? [] : store.selection.flatMap((s) => store.ancestors(s)).filter((a) => !expanded.has(a));
  seen.current = sig;
  if (needed.length) setExpanded(new Set([...expanded, ...needed]));
  useEffect(() => {
    const on = () => setExpanded(new Set());
    window.addEventListener("spark:collapse-layers", on);
    return () => window.removeEventListener("spark:collapse-layers", on);
  }, []);
  const toggle = (id: string) => {
    const next = new Set(expanded);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setExpanded(next);
  };
  return (
    <aside className="relative flex w-[var(--left-w)] shrink-0 flex-col bg-panel" aria-label="Pages and layers">
      <PanelResizer side="left" />
      <FileHeader store={store} run={run} />
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
          <Pages store={store} agents={agents} />
          <div className="flex h-[var(--row-h)] items-center px-5 text-base font-medium">Layers</div>
          {query !== null && (
            <div className="px-3 pb-1.5">
              <input
                // biome-ignore lint/a11y/noAutofocus: the search field takes focus when it opens
                autoFocus
                aria-label="Search layers"
                placeholder="Search layers"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                onFocus={(e) => e.currentTarget.select()}
                onKeyDown={(e) => {
                  e.stopPropagation();
                  if (e.key === "Escape") setQuery(null);
                  if (e.key === "Enter" && found?.first) store.select([found.first]);
                }}
                className="h-6 w-full rounded-control bg-control px-2 text-sm outline-none focus:outline focus:outline-1 focus:outline-fg"
              />
            </div>
          )}
          <CanvasMenu {...menu}>
            <div
              role="tree"
              aria-label="Layers"
              className="min-h-0 flex-1 overflow-auto pb-4 spark-scroll"
              onContextMenu={(e) => {
                // Right-clicking a row selects it unless it's already selected; elsewhere there is no menu.
                const id = (e.target as HTMLElement).closest("[data-layer]")?.getAttribute("data-layer");
                if (!id) return e.preventDefault();
                if (!store.selection.includes(id)) store.select([id]);
              }}
            >
              {[...top].reverse().map((id) => (
                <LayerRow key={id} store={store} id={id} depth={0} expanded={expanded} toggle={toggle} drag={drag} setDrag={setDrag} filter={found?.show ?? null} />
              ))}
            </div>
          </CanvasMenu>
        </>
      )}
    </aside>
  );
}
