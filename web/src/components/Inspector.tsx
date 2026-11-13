import { Copy, Download, Settings2 } from "lucide-react";
import { useState } from "react";
import { type DocStore, useDoc, useRects } from "../lib/doc";
import type { Camera } from "../lib/geometry";
import { renderToPng } from "../lib/render";
import type { Agent, ExportPreset, Rect } from "../lib/types";
import { BusyDots } from "./AppBar";
import { ColorField, IconButton, NumField, Pop, RowTools, Section, Select, StoreContext } from "./inspector/fields";
import { SelectionColorsSection } from "./inspector/SelectionColors";
import {
  AppearanceSection,
  BorderSection,
  type Ctx,
  EffectsSection,
  FillSection,
  LayoutSection,
  OutlineSection,
  PositionSection,
  ShadowSection,
  TypographySection,
} from "./inspector/sections";
import { GuidesSection } from "./inspector/GuidesSection";
import { ConnectDialog } from "./ConnectDialog";
import { PanelResizer } from "./PanelResizer";
import { ZoomMenu } from "./ZoomMenu";

const SCALES: [string, string][] = ["0.5x", "1x", "2x", "3x", "4x", "512w", "1024w", "512h"].map((s) => [s, s]);
const FORMATS: [string, string][] = ["png", "jpg", "webp", "svg", "pdf"].map((f) => [f, f.toUpperCase()]);

/** Puts a PNG of the first selected layer on the clipboard, at the scale of the first PNG export setting. */
async function copyPng(store: DocStore, id: string, presets: ExportPreset[]) {
  const factor = Number.parseFloat(presets.find((p) => p.format === "png")?.scale ?? "1");
  const png = renderToPng(store, id, Number.isFinite(factor) && factor > 0 && presets.some((p) => p.format === "png" && p.scale.endsWith("x")) ? factor : 1).then(
    async (b64) => (await fetch(`data:image/png;base64,${b64}`)).blob(),
  );
  // The clipboard item takes the promise itself, so Safari keeps the click's permission while it renders.
  await navigator.clipboard.write([new ClipboardItem({ "image/png": png })]);
}

function ExportSection({ store, n, ids, setProp }: Ctx) {
  const presets: ExportPreset[] = n.exports ?? [];
  const set = (next: ExportPreset[]) => setProp("exports", next.length ? next : null);
  const [copied, setCopied] = useState<string | null>(null);
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
          <Pop label="Export settings" icon={<Settings2 size={13} />} width={200}>
            <div className="flex flex-col gap-2 p-1">
              {p.format === "jpg" ? (
                <NumField label="Q" title="JPEG quality" value={p.quality ?? 90} min={1} max={100} onCommit={(quality) => set(presets.map((x, j) => (j === i ? { ...x, quality } : x)))} />
              ) : (
                <p className="text-fg-3">{p.format === "webp" ? "WebP is exported lossless, so it has no quality setting." : "Quality applies to JPEG exports."}</p>
              )}
              <label className="flex h-6 items-center gap-1.5 rounded-control bg-control px-1.5">
                <span className="shrink-0 text-fg-3">Suffix</span>
                <input
                  aria-label="File name suffix"
                  size={1}
                  placeholder={p.scale === "1x" ? "none" : `@${p.scale}`}
                  defaultValue={p.suffix ?? ""}
                  onBlur={(e) => e.target.value !== (p.suffix ?? "") && set(presets.map((x, j) => (j === i ? { ...x, suffix: e.target.value || undefined } : x)))}
                  onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
                  className="min-w-0 flex-1 bg-transparent font-mono outline-none"
                />
              </label>
            </div>
          </Pop>
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
      <button
        type="button"
        onClick={() =>
          copyPng(store, ids[0], presets)
            .then(() => setCopied("Copied"))
            .catch((e: Error) => setCopied(`Copy failed: ${e.message}`))
        }
        className="flex h-7 items-center justify-center gap-1.5 rounded-control border border-line text-sm hover:bg-hover"
      >
        <Copy size={13} /> {copied ?? "Copy as PNG"}
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
  return (
    <StoreContext.Provider value={store}>
    <aside aria-label="Inspector" className="relative flex w-[var(--right-w)] shrink-0 flex-col border-l border-line bg-panel">
      <PanelResizer side="right" />
      <div className="flex h-[var(--inspector-header-h)] shrink-0 items-center gap-1 border-b border-line px-3">
        <div className="min-w-0 flex-1 truncate text-base font-medium">{n ? (ids.length > 1 ? `${ids.length} layers` : n.name) : ""}</div>
        <ZoomMenu zoom={props.cam.zoom} run={props.run} />
        {n && (
          <IconButton label="Export" onClick={() => props.run("export")}>
            <Download size={14} />
          </IconButton>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-auto spark-scroll" key={ids.join()}>
        {ctx ? (
          <>
            <PositionSection {...ctx} />
            {ctx.n.kind === "frame" && <LayoutSection {...ctx} />}
            {ctx.n.kind === "frame" && <GuidesSection {...ctx} />}
            <AppearanceSection {...ctx} />
            {ctx.n.kind !== "vector" && <FillSection {...ctx} />}
            <BorderSection {...ctx} />
            <OutlineSection {...ctx} />
            <ShadowSection {...ctx} />
            {ctx.n.kind === "text" && <TypographySection {...ctx} />}
            <EffectsSection {...ctx} />
            <SelectionColorsSection store={store} ids={ids} tokens={tokens} />
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
    </StoreContext.Provider>
  );
}
