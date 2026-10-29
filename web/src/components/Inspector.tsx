import { ChevronDown, Download } from "lucide-react";
import { useState } from "react";
import { type DocStore, useDoc, useRects } from "../lib/doc";
import type { Camera } from "../lib/geometry";
import type { Agent, ExportPreset, Rect } from "../lib/types";
import { BusyDots } from "./AppBar";
import { ColorField, IconButton, RowTools, Section, Select } from "./inspector/fields";
import {
  AppearanceSection,
  BorderSection,
  type Ctx,
  EffectsSection,
  FillSection,
  LayoutSection,
  PositionSection,
  ShadowSection,
  TypographySection,
} from "./inspector/sections";
import { ConnectDialog } from "./ConnectDialog";

const SCALES: [string, string][] = ["0.5x", "1x", "2x", "3x", "4x", "512w", "1024w", "512h"].map((s) => [s, s]);
const FORMATS: [string, string][] = ["png", "jpg", "webp", "svg", "pdf"].map((f) => [f, f.toUpperCase()]);

function ExportSection({ store, n, ids, setProp }: Ctx) {
  const presets: ExportPreset[] = n.exports ?? [];
  const set = (next: ExportPreset[]) => setProp("exports", next.length ? next : null);
  return (
    <Section title="Export" onAdd={() => set([...presets, { format: "png", scale: presets.length ? "2x" : "1x" }])}>
      {presets.map((p, i) => (
        <div
          // biome-ignore lint/suspicious/noArrayIndexKey: presets are positional
          key={i}
          className="flex items-center gap-1"
          data-testid="export-preset"
        >
          <Select label="Export scale" value={p.scale} options={SCALES} onChange={(scale) => set(presets.map((x, j) => (j === i ? { ...x, scale } : x)))} className="flex-1" />
          <Select label="Export format" value={p.format} options={FORMATS} onChange={(format) => set(presets.map((x, j) => (j === i ? { ...x, format } : x)))} className="flex-1" />
          <RowTools label="export setting" onRemove={() => set(presets.filter((_, j) => j !== i))} />
        </div>
      ))}
      <button
        type="button"
        onClick={() => window.dispatchEvent(new CustomEvent("spark:export", { detail: { ids, presets: presets.length ? presets : [{ format: "png", scale: "1x" }] } }))}
        className="flex h-7 items-center justify-center gap-1.5 rounded-control border border-line text-sm hover:bg-hover"
      >
        <Download size={13} /> Export {ids.length > 1 ? `${ids.length} layers` : (store.nodes.get(ids[0])?.name ?? "")}
      </button>
      {ids.length > 1 && (
        <button
          type="button"
          onClick={() => window.dispatchEvent(new CustomEvent("spark:export", { detail: { ids, presets: [], combined: true } }))}
          className="flex h-7 items-center justify-center gap-1.5 rounded-control border border-line text-sm hover:bg-hover"
        >
          <Download size={13} /> Export combined PDF
        </button>
      )}
    </Section>
  );
}

function PageSection({ store }: { store: DocStore }) {
  const page = store.pages.find((p) => p.id === store.page);
  if (!page) return null;
  const tokens = new Map(store.tokens.map((t) => [t.name, t.value]));
  return (
    <Section title="Page">
      <div className="flex items-center gap-1">
        <span className="w-14 shrink-0 text-sm text-fg-2">Canvas</span>
        <ColorField label="Canvas color" value={page.canvas ?? "#f2f2f2"} tokens={tokens} onChange={(c) => store.do({ type: "setPage", id: page.id, prop: "canvas", value: c })} />
        {page.canvas && <RowTools label="canvas color" onRemove={() => store.do({ type: "setPage", id: page.id, prop: "canvas", value: null })} />}
      </div>
    </Section>
  );
}

/** Agents working in this file, grouped by page; idle sessions collapse into one count. */
function AgentsSection({ store, agents, reveal }: { store: DocStore; agents: Agent[]; reveal: (page: string, ids: string[]) => void }) {
  const [open, setOpen] = useState(false);
  // page -> agent name -> frames it is working on (sessions with the same name merge).
  const pages = new Map<string, Map<string, Set<string>>>();
  let idle = 0;
  for (const a of agents) {
    const ids = (a.files[store.file.id] ?? []).filter((id) => store.nodes.has(id));
    if (!ids.length) idle++;
    for (const id of ids) {
      const page = store.pageOf(id) ?? store.nodes.get(id)?.parent;
      if (!page) continue;
      const byName = pages.get(page) ?? new Map<string, Set<string>>();
      byName.set(a.name, (byName.get(a.name) ?? new Set()).add(id));
      pages.set(page, byName);
    }
  }
  const working = store.pages.filter((p) => pages.has(p.id));
  return (
    <Section title="Agents">
      {working.length === 0 && <p className="text-sm text-fg-3">No agent is working in this file.</p>}
      {working.map((p) => (
        <div key={p.id} data-testid="agents-page">
          <div className="mb-0.5 text-xs text-fg-3 uppercase">{p.name}</div>
          {[...(pages.get(p.id) ?? [])].map(([name, ids]) => (
            <button
              key={name}
              type="button"
              onClick={() => reveal(p.id, [...ids])}
              className="-mx-1.5 flex h-7 w-[calc(100%+12px)] items-center gap-2 rounded-control px-1.5 text-left text-base hover:bg-hover"
            >
              <BusyDots />
              <span className="flex-1 truncate">{name}</span>
              <span className="text-sm text-fg-3">
                {ids.size} {ids.size === 1 ? "frame" : "frames"}
              </span>
            </button>
          ))}
        </div>
      ))}
      {idle > 0 && <p className="text-sm text-fg-3">{idle} more connected, not working here</p>}
      <button type="button" onClick={() => setOpen(true)} className="h-7 rounded-control bg-control text-sm hover:bg-hover">
        Connect more agents
      </button>
      <ConnectDialog conn={store.conn} open={open} onOpenChange={setOpen} />
    </Section>
  );
}

export function Inspector(props: {
  store: DocStore;
  cam: Camera;
  run: (cmd: string) => void;
  zoomTo: (r: Rect | null) => void;
  agents: Agent[];
  reveal: (page: string, ids: string[]) => void;
}) {
  const { store } = props;
  useDoc(store);
  useRects(store);
  const [zoomMenu, setZoomMenu] = useState(false);
  const ids = store.selection.filter((id) => store.nodes.has(id));
  const n = ids.length ? store.nodes.get(ids[0]) : undefined;
  const tokens = new Map(store.tokens.map((t) => [t.name, t.value]));
  const ctx: Ctx | null = n
    ? {
        store,
        n,
        ids,
        tokens,
        setStyle: (styles, merge) => store.do({ type: "setStyle", ids, styles, merge: merge && `${merge}:${ids.join()}` }),
        setProp: (prop, value, merge) => store.do({ type: "set", ids, prop, value, merge: merge && `${merge}:${ids.join()}` }),
      }
    : null;
  const zoomItems: [string, string][] = [
    ["Zoom in", "zoomIn"],
    ["Zoom out", "zoomOut"],
    ["Zoom to fit", "zoomFit"],
    ["Zoom to selection", "zoomSelection"],
    ["Zoom to 100%", "zoom100"],
  ];
  return (
    <aside aria-label="Inspector" className="flex w-[var(--right-w)] shrink-0 flex-col border-l border-line bg-panel">
      <div className="relative flex h-[var(--inspector-header-h)] shrink-0 items-center gap-1 border-b border-line px-3">
        <div className="min-w-0 flex-1 truncate text-base font-medium">{n ? (ids.length > 1 ? `${ids.length} layers` : n.name) : ""}</div>
        <button type="button" data-testid="zoom" onClick={() => setZoomMenu(!zoomMenu)} className="flex h-6 items-center gap-1 rounded-control px-2 font-mono text-sm hover:bg-hover">
          {Math.round(props.cam.zoom * 100)}%
          <ChevronDown size={12} />
        </button>
        {n && (
          <IconButton label="Export" onClick={() => props.run("export")}>
            <Download size={14} />
          </IconButton>
        )}
        {zoomMenu && (
          <div role="menu" className="absolute top-10 right-3 z-20 w-44 rounded-popover border border-line bg-panel p-1 shadow-popover">
            {zoomItems.map(([label, cmd]) => (
              <button
                key={cmd}
                type="button"
                role="menuitem"
                onClick={() => {
                  setZoomMenu(false);
                  props.run(cmd);
                }}
                className="flex h-6 w-full items-center rounded-control px-2 text-left text-sm hover:bg-hover"
              >
                {label}
              </button>
            ))}
          </div>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-auto spark-scroll" key={ids.join()}>
        {ctx ? (
          <>
            <PositionSection {...ctx} />
            {ctx.n.kind === "frame" && <LayoutSection {...ctx} />}
            <AppearanceSection {...ctx} />
            {ctx.n.kind !== "vector" && <FillSection {...ctx} />}
            <BorderSection {...ctx} />
            <ShadowSection {...ctx} />
            {ctx.n.kind === "text" && <TypographySection {...ctx} />}
            <EffectsSection {...ctx} />
            <ExportSection {...ctx} />
          </>
        ) : (
          <>
            <PageSection store={store} />
            <AgentsSection store={store} agents={props.agents} reveal={props.reveal} />
          </>
        )}
      </div>
    </aside>
  );
}
