import * as Popover from "@radix-ui/react-popover";
import { Fragment, useState } from "react";
import {
  AlignCenterHorizontal,
  AlignHorizontalSpaceBetween,
  AlignVerticalJustifyCenter,
  AlignVerticalJustifyEnd,
  AlignVerticalJustifyStart,
  Layers,
  AlignCenterVertical,
  AlignEndHorizontal,
  AlignEndVertical,
  AlignHorizontalDistributeCenter,
  AlignStartHorizontal,
  AlignStartVertical,
  AlignVerticalDistributeCenter,
  FlipHorizontal,
  FlipVertical,
  RotateCw,
  AlignLeft,
  AlignCenter,
  AlignRight,
  AlignJustify,
  ArrowDown,
  ArrowLeftRight,
  ArrowRight,
  Baseline,
  Square,
  SquareDashed,
  WrapText,
} from "lucide-react";
import type { DocStore } from "../../lib/doc";
import type { Fill, NodeView, Shadow, Style } from "../../lib/types";
import { CaptureLayer } from "./CaptureLayer";
import { ColorField, IconButton, NumField, POP_ITEM, Pop, RowTools, Section, Select } from "./fields";
import { FontPicker } from "./FontPicker";

export type Ctx = {
  store: DocStore;
  n: NodeView;
  ids: string[];
  tokens: Map<string, string>;
  setStyle: (styles: Record<string, string | number | null>, merge?: string) => void;
  setProp: (prop: string, value: unknown, merge?: string) => void;
};

const px = (s: string | undefined) => (s && /^-?[\d.]+px$/.test(s) ? Number.parseFloat(s) : null);
const num = (s: string | undefined) => (s === undefined ? null : Number.parseFloat(s));

const round2 = (v: number) => Math.round(v * 100) / 100;
const pct = (s: string | undefined) => (s?.endsWith("%") ? Number.parseFloat(s) : null);

type Anchor = "start" | "end" | "stretch" | "center" | "scale";

/** The constraint a child's CSS describes on one axis ("h" = left/right, "v" = top/bottom). */
function anchorOf(st: Style, axis: "h" | "v"): Anchor {
  const [a, b, ma, mb] = axis === "h" ? (["left", "right", "margin-left", "margin-right"] as const) : (["top", "bottom", "margin-top", "margin-bottom"] as const);
  if (st.raw?.[ma] === "auto" && st.raw?.[mb] === "auto") return "center";
  if (pct(st[a])) return "scale";
  if (st[a] && st[b]) return "stretch";
  return st[b] ? "end" : "start";
}

const ANCHORS: Record<"h" | "v", [Anchor, string][]> = {
  h: [["start", "Left"], ["end", "Right"], ["stretch", "Left and right"], ["center", "Center"], ["scale", "Scale"]],
  v: [["start", "Top"], ["end", "Bottom"], ["stretch", "Top and bottom"], ["center", "Center"], ["scale", "Scale"]],
};

/** CSS that anchors a box at `pos`/`size` inside a parent of `parent` px along one axis. */
function anchorStyles(axis: "h" | "v", mode: Anchor, pos: number, size: number, parent: number) {
  const [a, b, dim, ma, mb] = axis === "h" ? (["left", "right", "width", "marginLeft", "marginRight"] as const) : (["top", "bottom", "height", "marginTop", "marginBottom"] as const);
  const none = { [a]: null, [b]: null, [dim]: `${round2(size)}px`, [ma]: null, [mb]: null };
  const end = parent - pos - size;
  switch (mode) {
    case "start":
      return { ...none, [a]: `${round2(pos)}px` };
    case "end":
      return { ...none, [b]: `${round2(end)}px` };
    case "stretch":
      return { ...none, [a]: `${round2(pos)}px`, [b]: `${round2(end)}px`, [dim]: null };
    case "center": {
      // Auto margins between left/right insets centre the box; unequal insets keep its offset.
      const off = pos + size / 2 - parent / 2;
      return { ...none, [a]: `${round2(Math.max(2 * off, 0))}px`, [b]: `${round2(Math.max(-2 * off, 0))}px`, [ma]: "auto", [mb]: "auto" };
    }
    case "scale":
      return { ...none, [a]: `${round2((pos / parent) * 100)}%`, [dim]: `${round2((size / parent) * 100)}%` };
  }
}
const ARRANGE: [string, string, string, typeof AlignLeft][] = [
  ["align", "left", "Align left", AlignStartVertical],
  ["align", "hcenter", "Align horizontal center", AlignCenterVertical],
  ["align", "right", "Align right", AlignEndVertical],
  ["align", "top", "Align top", AlignStartHorizontal],
  ["align", "vmiddle", "Align vertical middle", AlignCenterHorizontal],
  ["align", "bottom", "Align bottom", AlignEndHorizontal],
  ["distribute", "x", "Distribute horizontally", AlignHorizontalDistributeCenter],
  ["distribute", "y", "Distribute vertically", AlignVerticalDistributeCenter],
];
const TRANSFORMS: [string, string, typeof AlignLeft][] = [
  ["rotate90", "Rotate 90° clockwise", RotateCw],
  ["flipH", "Flip horizontal", FlipHorizontal],
  ["flipV", "Flip vertical", FlipVertical],
];

export function PositionSection({ store, n, ids, setStyle }: Ctx) {
  const r = store.rects.get(n.id);
  const pr = store.rects.get(n.parent);
  const free = n.parent.startsWith("p-") || n.style.position === "absolute";
  const x = px(n.style.left) ?? (r ? r.x - (pr?.x ?? 0) : null);
  const y = px(n.style.top) ?? (r ? r.y - (pr?.y ?? 0) : null);
  const mode = (axis: "width" | "height") => {
    const v = n.style[axis];
    return v === "fill" ? "fill" : v === "hug" ? "hug" : pct(v) !== null ? "percent" : v ? "fixed" : "auto";
  };
  const sizeOptions: [string, string][] = [
    ["fixed", "Fixed"],
    ["percent", "Relative (%)"],
    ["fill", "Fill"],
    ["hug", "Hug"],
    ["auto", "Auto"],
  ];
  const setMode = (axis: "width" | "height", m: string) => {
    const cur = axis === "width" ? r?.w : r?.h;
    const whole = axis === "width" ? pr?.w : pr?.h;
    const value = m === "fixed" ? `${Math.round(cur ?? 100)}px` : m === "percent" ? `${round2(((cur ?? 100) / (whole || cur || 100)) * 100)}%` : m === "auto" ? null : m;
    setStyle({ [axis]: value });
  };
  const multi = ids.length > 1;
  // Constraints apply to a child that is positioned by left/top inside a frame that isn't a flex container.
  const parent = store.nodes.get(n.parent);
  const constrained = !multi && !!parent && !parent.style.display?.includes("flex") && n.style.position === "absolute" && !!r && !!pr;
  const anchors = { h: anchorOf(n.style, "h"), v: anchorOf(n.style, "v") };
  const setAnchor = (axis: "h" | "v", m: Anchor) => {
    if (!r || !pr) return;
    const [pos, size, whole] = axis === "h" ? [r.x - pr.x, r.w, pr.w] : [r.y - pr.y, r.h, pr.h];
    setStyle(anchorStyles(axis, m, pos, size, whole));
  };
  const place = (axis: "h" | "v", v: number, scrub: boolean) => {
    const [a, b, whole, size] = axis === "h" ? (["left", "right", pr?.w, r?.w] as const) : (["top", "bottom", pr?.h, r?.h] as const);
    const m = anchors[axis];
    // A right/bottom-anchored box keeps its anchor: moving it edits the far inset instead.
    const css = m === "end" && whole !== undefined && size !== undefined ? { [b]: `${round2(whole - v - size)}px` } : m === "scale" && whole ? { [a]: `${round2((v / whole) * 100)}%` } : { [a]: `${v}px` };
    setStyle({ ...css, position: "absolute" }, scrub ? axis : undefined);
  };
  // Layers inside a flex parent are placed by layout, so only freeform ones can be arranged.
  const loose = ids.filter((id) => {
    const l = store.nodes.get(id);
    return l && (l.parent.startsWith("p-") || l.style.position === "absolute");
  });
  const canAlign = loose.length > 1 || (loose.length === 1 && !store.nodes.get(loose[0])?.parent.startsWith("p-"));
  return (
    <Section title="Position and size">
      <div className="flex justify-between">
        {ARRANGE.map(([type, dir, label, Icon]) => (
          <IconButton key={dir} label={label} disabled={type === "align" ? !canAlign : loose.length < 3} onClick={() => store.do({ type, ids, dir })}>
            <Icon size={14} />
          </IconButton>
        ))}
      </div>
      <div className="flex gap-1">
        {TRANSFORMS.map(([dir, label, Icon]) => (
          <IconButton key={dir} label={label} onClick={() => store.do({ type: "transform", ids, dir })}>
            <Icon size={14} />
          </IconButton>
        ))}
      </div>
      <div className="grid grid-cols-[1fr_1fr_72px] gap-2">
        <NumField label="X" testid="field-x" value={multi ? null : x} disabled={!free} onCommit={(v, s) => place("h", v, s)} />
        <NumField label="Y" testid="field-y" value={multi ? null : y} disabled={!free} onCommit={(v, s) => place("v", v, s)} />
        <NumField label="∠" value={n.style.rotate ?? 0} suffix="°" onCommit={(v, s) => setStyle({ rotate: v ? `${v}deg` : null }, s ? "rotate" : undefined)} title="Rotation" />
      </div>
      <div className="grid grid-cols-2 gap-2">
        {(["width", "height"] as const).map((axis) => {
          const rel = mode(axis) === "percent";
          return (
            <NumField
              key={axis}
              label={axis === "width" ? "W" : "H"}
              testid={axis === "width" ? "field-w" : "field-h"}
              value={rel ? pct(n.style[axis]) : r && !multi ? round2(axis === "width" ? r.w : r.h) : null}
              suffix={rel ? "%" : undefined}
              placeholder={multi ? "Mixed" : "–"}
              min={1}
              onCommit={(v, s) => setStyle({ [axis]: `${v}${rel ? "%" : "px"}` }, s ? axis : undefined)}
            />
          );
        })}
        {(["width", "height"] as const).map((axis) => (
          <Select key={axis} label={`${axis} sizing`} value={mode(axis)} options={sizeOptions} onChange={(m) => setMode(axis, m)} />
        ))}
      </div>
      {constrained && (
        <div className="grid grid-cols-2 gap-2">
          {(["h", "v"] as const).map((axis) => (
            <Select key={axis} label={axis === "h" ? "Horizontal constraint" : "Vertical constraint"} value={anchors[axis]} options={ANCHORS[axis]} onChange={(m) => setAnchor(axis, m)} />
          ))}
        </div>
      )}
    </Section>
  );
}

const JUSTIFY = ["flex-start", "center", "flex-end"];

const ALIGN_CONTENT: [string, string][] = ["normal", "flex-start", "center", "flex-end", "stretch", "space-between", "space-around"].map((v) => [v, v === "normal" ? "Content: default" : `Content: ${v.replace("flex-", "").replace("-", " ")}`]);

export function LayoutSection({ store, n, ids, setStyle }: Ctx) {
  const flex = n.style.display === "flex" || n.style.display === "inline-flex";
  const row = !n.style.flexDirection?.startsWith("column");
  const reversed = !!n.style.flexDirection?.endsWith("-reverse");
  const dir = (column: boolean, reverse: boolean) => `${column ? "column" : "row"}${reverse ? "-reverse" : ""}`;
  const wraps = n.style.flexWrap === "wrap" || n.style.flexWrap === "wrap-reverse";
  const pad = n.style.padding ?? ["0px", "0px", "0px", "0px"];
  const clip = n.style.overflow === "clip" || n.style.overflow === "hidden";
  const setPad = (i: number, v: number) => {
    const next = [...pad];
    next[i] = `${v}px`;
    setStyle({ padding: next.join(" ") });
  };
  const ai = n.style.alignItems ?? "stretch";
  const jc = n.style.justifyContent ?? "flex-start";
  const between = jc === "space-between";
  const cell = (cx: number, cy: number) => {
    const main = row ? cx : cy;
    const cross = row ? cy : cx;
    const active = (between || JUSTIFY.indexOf(jc) === main) && JUSTIFY.indexOf(ai === "start" ? "flex-start" : ai === "end" ? "flex-end" : ai) === cross;
    return { active, set: () => setStyle({ justifyContent: between ? "space-between" : JUSTIFY[main], alignItems: JUSTIFY[cross] }) };
  };
  return (
    <Section title="Layout">
      <div className="flex gap-1">
        {(["Freeform", "Flex"] as const).map((m) => {
          const on = (m === "Flex") === flex;
          return (
            <button
              key={m}
              type="button"
              aria-pressed={on}
              onClick={() => !on && store.do({ type: m === "Flex" ? "flex" : "freeform", ids })}
              className={`h-6 flex-1 rounded-control text-sm ${on ? "bg-selected text-selected-fg" : "bg-control hover:bg-hover"}`}
            >
              {m}
            </button>
          );
        })}
      </div>
      {flex && (
        <>
          <div className="flex items-center gap-1">
            <IconButton label="Row" active={row} onClick={() => setStyle({ flexDirection: dir(false, reversed) })}>
              <ArrowRight size={14} />
            </IconButton>
            <IconButton label="Column" active={!row} onClick={() => setStyle({ flexDirection: dir(true, reversed) })}>
              <ArrowDown size={14} />
            </IconButton>
            <IconButton label="Wrap" active={wraps} onClick={() => setStyle({ flexWrap: wraps ? null : "wrap" })}>
              <WrapText size={14} />
            </IconButton>
            <IconButton label="Space between" active={between} onClick={() => setStyle({ justifyContent: between ? "flex-start" : "space-between" })}>
              <AlignCenterHorizontal size={14} />
            </IconButton>
            <div className="ml-1 min-w-0 flex-1">
              <NumField label="G" title="Gap" value={px(n.style.gap) ?? 0} min={0} onCommit={(v, s) => setStyle({ gap: `${v}px` }, s ? "gap" : undefined)} />
            </div>
          </div>
          <div className="flex items-center gap-1">
            <IconButton label="Reverse direction" active={reversed} onClick={() => setStyle({ flexDirection: dir(!row, !reversed) })}>
              <ArrowLeftRight size={14} />
            </IconButton>
            <IconButton label="Align baseline" active={ai === "baseline"} onClick={() => setStyle({ alignItems: ai === "baseline" ? "flex-start" : "baseline" })}>
              <Baseline size={14} />
            </IconButton>
            {wraps && <Select label="Align content" value={n.style.raw?.["align-content"] ?? "normal"} options={ALIGN_CONTENT} onChange={(v) => setStyle({ alignContent: v === "normal" ? null : v })} className="min-w-0 flex-1" />}
          </div>
          <div className="flex gap-2">
            <fieldset className="m-0 grid min-w-0 grid-cols-3 gap-0.5 rounded-control border-0 bg-control p-1" aria-label="Alignment">
              {[0, 1, 2].flatMap((cy) =>
                [0, 1, 2].map((cx) => {
                  const c = cell(cx, cy);
                  return (
                    <button
                      key={`${cx}${cy}`}
                      type="button"
                      aria-label={`Align ${["top", "middle", "bottom"][cy]} ${["left", "center", "right"][cx]}`}
                      aria-pressed={c.active}
                      onClick={c.set}
                      className="grid size-4 place-items-center"
                    >
                      <span className={`rounded-full ${c.active ? "size-2 bg-fg" : "size-1 bg-fg-3"}`} />
                    </button>
                  );
                }),
              )}
            </fieldset>
            <div className="grid flex-1 grid-cols-2 gap-1">
              {["T", "R", "B", "L"].map((l, i) => (
                <NumField key={l} label={l} title={`Padding ${["top", "right", "bottom", "left"][i]}`} value={px(pad[i]) ?? 0} min={0} onCommit={(v) => setPad(i, v)} />
              ))}
            </div>
          </div>
        </>
      )}
      {n.kind === "frame" && (
        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={clip} onChange={() => setStyle({ overflow: clip ? null : "clip" })} />
          Clip content
        </label>
      )}
    </Section>
  );
}

const BLENDS = ["normal", "multiply", "screen", "overlay", "darken", "lighten", "color-dodge", "color-burn", "hard-light", "soft-light", "difference", "exclusion", "hue", "saturation", "color", "luminosity"];

export function AppearanceSection({ n, setStyle }: Ctx) {
  const radius = n.style.radius;
  const same = !radius || radius.every((r) => r === radius[0]);
  return (
    <Section title="Appearance">
      <div className="grid grid-cols-2 gap-2">
        <NumField label="◐" title="Opacity" testid="field-opacity" value={Math.round((n.style.opacity ?? 1) * 100)} min={0} max={100} suffix="%" onCommit={(v, s) => setStyle({ opacity: v / 100 }, s ? "opacity" : undefined)} />
        <div className="flex min-w-0 items-center gap-1">
          <NumField label="R" title="Corner radius" testid="field-radius" value={same ? (px(radius?.[0]) ?? 0) : null} min={0} onCommit={(v, s) => setStyle({ borderRadius: `${v}px` }, s ? "radius" : undefined)} />
          <input
            type="range"
            aria-label="Corner radius slider"
            min={0}
            max={Math.max(100, px(radius?.[0]) ?? 0)}
            value={same ? (px(radius?.[0]) ?? 0) : 0}
            disabled={!same}
            onChange={(e) => setStyle({ borderRadius: `${e.target.value}px` }, "radius")}
            className="h-6 w-12 shrink-0 accent-fg"
          />
        </div>
      </div>
      {!same && radius && (
        <div className="grid grid-cols-4 gap-1">
          {radius.map((r, i) => (
            <NumField
              // biome-ignore lint/suspicious/noArrayIndexKey: corners are positional
              key={i}
              label={["↖", "↗", "↘", "↙"][i]}
              value={px(r) ?? 0}
              min={0}
              onCommit={(v) => setStyle({ [["borderTopLeftRadius", "borderTopRightRadius", "borderBottomRightRadius", "borderBottomLeftRadius"][i]]: `${v}px` })}
            />
          ))}
        </div>
      )}
      <Select label="Blend mode" value={n.style.blend ?? "normal"} options={BLENDS.map((b) => [b, b[0].toUpperCase() + b.slice(1).replace("-", " ")])} onChange={(b) => setStyle({ mixBlendMode: b === "normal" ? null : b })} />
    </Section>
  );
}

const solid = (color: string): Fill => ({ type: "solid", color });

type Gradient = Extract<Fill, { stops: unknown }>;
const gradientCss = (f: Gradient) => {
  const stops = f.stops.map((s) => `${s.color} ${s.pos * 100}%`).join(", ");
  const space = f.space ? ` in ${f.space}` : "";
  return f.type === "linear" ? `linear-gradient(${f.angle}deg${space}, ${stops})` : f.type === "radial" ? `radial-gradient(circle${space}, ${stops})` : `conic-gradient(from ${f.angle}deg${space}, ${stops})`;
};

const SPACES: [string, string][] = [
  ["", "Default space"],
  ["srgb", "sRGB"],
  ["oklab", "OKLab"],
  ["oklch", "OKLCH"],
];

export function FillSection({ store, n, ids, setProp, tokens }: Ctx) {
  const fills = n.style.fills ?? [];
  const set = (next: Fill[], live = false) => setProp("style.fills", next.length ? next : null, live ? "fills" : undefined);
  const update = (i: number, f: Fill, live = false) => set(fills.map((x, j) => (j === i ? f : x)), live);
  const stopsOf = (f: Fill) => ("stops" in f ? f.stops : "color" in f ? [{ color: f.color, pos: 0 }, { color: "#000000", pos: 1 }] : [{ color: "#ffffff", pos: 0 }, { color: "#000000", pos: 1 }]);
  const firstColor = (f: Fill) => ("color" in f ? f.color : "stops" in f ? f.stops[0]?.color : "#d9d9d9") ?? "#d9d9d9";
  const changeType = (i: number, t: Fill["type"]) => {
    const f = fills[i];
    if (t === f.type) return;
    const space = "space" in f ? f.space : undefined;
    if (t === "solid") update(i, solid(firstColor(f)));
    else if (t === "linear") update(i, { type: "linear", angle: "angle" in f ? f.angle : 180, stops: stopsOf(f), space });
    else if (t === "radial") update(i, { type: "radial", stops: stopsOf(f), space });
    else if (t === "conic") update(i, { type: "conic", angle: "angle" in f ? f.angle : 0, stops: stopsOf(f), space });
    else window.dispatchEvent(new CustomEvent("spark:pick-image", { detail: (hash: string) => update(i, { type: "image", asset: hash, fit: "cover" }) }));
  };
  const withStops = (i: number, f: Gradient, stops: Gradient["stops"], live = false) => update(i, { ...f, stops }, live);
  return (
    <Section title="Fill" onAdd={() => set([solid(fills.length ? "#000000" : "#d9d9d9"), ...fills])} right={<CaptureLayer store={store} exclude={ids} onCaptured={(asset) => set([{ type: "image", asset, fit: "cover" }, ...fills])} />}>
      {fills.map((f, i) => (
        <div
          // biome-ignore lint/suspicious/noArrayIndexKey: fills are positional layers
          key={i}
          className={`flex flex-col gap-1 ${f.hidden ? "opacity-50" : ""}`}
          data-testid="fill-row"
        >
          <div className="flex items-center gap-1">
            <Select
              label="Fill type"
              value={f.type}
              options={[
                ["solid", "Solid"],
                ["linear", "Linear"],
                ["radial", "Radial"],
                ["conic", "Conic"],
                ["image", "Image"],
              ]}
              onChange={(t) => changeType(i, t)}
              className="min-w-0 flex-1"
            />
            <RowTools label="fill" hidden={f.hidden} onToggle={() => update(i, { ...f, hidden: !f.hidden })} onRemove={() => set(fills.filter((_, j) => j !== i))} />
          </div>
          {f.type === "solid" && <ColorField label="Fill" value={f.color} tokens={tokens} onChange={(c, live) => update(i, { ...f, color: c }, live)} />}
          {"stops" in f && <div className="h-6 rounded-control border border-line" style={{ background: gradientCss(f) }} />}
          {f.type === "image" && (
            <div className="flex h-6 items-center gap-1">
              <div className="size-6 shrink-0 rounded-control border border-line bg-cover bg-center" style={{ backgroundImage: `url(/assets/${f.asset})` }} />
              <Select
                label="Image fit"
                value={f.fit ?? "cover"}
                options={[
                  ["cover", "Fill"],
                  ["contain", "Fit"],
                  ["fill", "Stretch"],
                  ["tile", "Tile"],
                ]}
                onChange={(fit) => update(i, { ...f, fit })}
                className="min-w-0 flex-1"
              />
            </div>
          )}
          {"stops" in f && (
            <div className="flex flex-col gap-1">
              {"angle" in f && <NumField label="∠" title="Angle" value={f.angle} suffix="°" onCommit={(v, s) => update(i, { ...f, angle: v }, s)} />}
              <div className="flex items-center gap-1">
                <IconButton label="Reverse stops" onClick={() => withStops(i, f, f.stops.map((s) => ({ ...s, pos: 1 - s.pos })).reverse())}>
                  <ArrowLeftRight size={13} />
                </IconButton>
                <IconButton label="Rotate 90°" className={"angle" in f ? "" : "opacity-40"} onClick={() => "angle" in f && update(i, { ...f, angle: (f.angle + 90) % 360 })}>
                  <RotateCw size={13} />
                </IconButton>
                <IconButton label="Distribute stops evenly" onClick={() => withStops(i, f, f.stops.map((s, k) => ({ ...s, pos: f.stops.length > 1 ? k / (f.stops.length - 1) : 0 })))}>
                  <AlignHorizontalSpaceBetween size={13} />
                </IconButton>
                <Select label="Interpolation color space" value={f.space ?? ""} options={SPACES} onChange={(space) => update(i, { ...f, space: space || undefined })} className="min-w-0 flex-1" />
              </div>
              {f.stops.map((s, k) => (
                <div
                  // biome-ignore lint/suspicious/noArrayIndexKey: stops are positional
                  key={k}
                  className="flex gap-1"
                >
                  <ColorField label={`Stop ${k + 1}`} value={s.color} tokens={tokens} onChange={(c, live) => withStops(i, f, f.stops.map((x, j) => (j === k ? { ...x, color: c } : x)), live)} />
                  <div className="w-14 shrink-0">
                    <NumField label="%" title="Stop position" value={Math.round(s.pos * 100)} min={0} max={100} onCommit={(v, live) => withStops(i, f, f.stops.map((x, j) => (j === k ? { ...x, pos: v / 100 } : x)), live)} />
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      ))}
      {store.nodes.get(n.id)?.image && <p className="text-sm text-fg-3">Image: {String(n.image?.status ?? "")}</p>}
    </Section>
  );
}

export function BorderSection({ n, setProp, tokens }: Ctx) {
  const b = n.style.border;
  const [sides, setSides] = useState(false);
  const uniform = !b || b.width.every((w) => w === b.width[0]);
  return (
    <Section title="Border" onAdd={b ? undefined : () => setProp("style.border", { width: [1, 1, 1, 1], style: "solid", color: "#000000" })}>
      {b && (
        <>
          <div className="flex items-center gap-1">
            <ColorField label="Border" value={b.color} tokens={tokens} onChange={(color, live) => setProp("style.border", { ...b, color }, live ? "border" : undefined)} />
            <RowTools label="border" onRemove={() => setProp("style.border", null)} />
          </div>
          <div className="flex items-center gap-1">
            <NumField label="W" title="Border width" value={uniform ? b.width[0] : null} min={0} onCommit={(v, s) => setProp("style.border", { ...b, width: [v, v, v, v] }, s ? "border" : undefined)} />
            <Select
              label="Border style"
              value={b.style}
              options={[
                ["solid", "Solid"],
                ["dashed", "Dashed"],
                ["dotted", "Dotted"],
              ]}
              onChange={(style) => setProp("style.border", { ...b, style })}
              className="flex-1"
            />
            <IconButton label="Per-side widths" active={sides || !uniform} onClick={() => setSides(!sides)}>
              <SquareDashed size={13} />
            </IconButton>
          </div>
          {(sides || !uniform) && (
            <div className="grid grid-cols-4 gap-1">
              {["T", "R", "B", "L"].map((l, i) => (
                <NumField key={l} label={l} title={`Border ${["top", "right", "bottom", "left"][i]} width`} value={b.width[i]} min={0} onCommit={(v, s) => setProp("style.border", { ...b, width: b.width.map((w, j) => (j === i ? v : w)) }, s ? `border${i}` : undefined)} />
              ))}
            </div>
          )}
        </>
      )}
    </Section>
  );
}

const OUTLINE_PROPS = ["outlineWidth", "outlineStyle", "outlineColor", "outlineOffset", "outline"];

export function OutlineSection({ n, setStyle, tokens }: Ctx) {
  const raw = n.style.raw ?? {};
  const on = !!raw["outline-style"] && raw["outline-style"] !== "none";
  return (
    <Section title="Outline" onAdd={on ? undefined : () => setStyle({ outline: null, outlineWidth: "1px", outlineStyle: "solid", outlineColor: "#000000", outlineOffset: "0px" })}>
      {on && (
        <>
          <div className="flex items-center gap-1">
            <ColorField label="Outline" value={raw["outline-color"] ?? "#000000"} tokens={tokens} onChange={(c, live) => setStyle({ outlineColor: c }, live ? "outline" : undefined)} />
            <RowTools label="outline" onRemove={() => setStyle(Object.fromEntries(OUTLINE_PROPS.map((k) => [k, null])))} />
          </div>
          <div className="grid grid-cols-3 gap-1">
            <NumField label="W" title="Outline width" value={Number.parseFloat(raw["outline-width"] ?? "1")} min={0} onCommit={(v, s) => setStyle({ outlineWidth: `${v}px` }, s ? "outlineW" : undefined)} />
            <NumField label="O" title="Outline offset" value={Number.parseFloat(raw["outline-offset"] ?? "0")} onCommit={(v, s) => setStyle({ outlineOffset: `${v}px` }, s ? "outlineO" : undefined)} />
            <Select
              label="Outline style"
              value={raw["outline-style"] ?? "solid"}
              options={[
                ["solid", "Solid"],
                ["dashed", "Dashed"],
                ["dotted", "Dotted"],
              ]}
              onChange={(v) => setStyle({ outlineStyle: v })}
            />
          </div>
        </>
      )}
    </Section>
  );
}

const SHADOW_PRESETS: [string, Shadow][] = [
  ["Subtle", { x: 0, y: 1, blur: 2, spread: 0, color: "#0000001a" }],
  ["Medium", { x: 0, y: 4, blur: 12, spread: 0, color: "#0000001f" }],
  ["Large", { x: 0, y: 16, blur: 40, spread: -8, color: "#00000040" }],
  ["Inner", { inset: true, x: 0, y: 2, blur: 4, spread: 0, color: "#00000026" }],
];

export function ShadowSection({ n, setProp, tokens }: Ctx) {
  const shadows = n.style.shadows ?? [];
  const set = (next: Shadow[], live = false) => setProp("style.shadows", next.length ? next : null, live ? "shadows" : undefined);
  const update = (i: number, s: Shadow, live = false) => set(shadows.map((x, j) => (j === i ? s : x)), live);
  return (
    <Section
      title="Shadow"
      onAdd={() => set([...shadows, { x: 0, y: 4, blur: 16, spread: 0, color: "#0000001f" }])}
      right={
        <Pop label="Shadow presets" icon={<Layers size={14} />} width={140}>
          {SHADOW_PRESETS.map(([name, preset]) => (
            <Popover.Close key={name} asChild>
              <button type="button" className={POP_ITEM} onClick={() => set([...shadows, preset])}>
                {name}
              </button>
            </Popover.Close>
          ))}
        </Pop>
      }
    >
      {shadows.map((s, i) => (
        <div
          // biome-ignore lint/suspicious/noArrayIndexKey: shadows are positional
          key={i}
          className={`flex flex-col gap-1 ${s.hidden ? "opacity-50" : ""}`}
        >
          <div className="flex items-center gap-1">
            <Select
              label="Shadow type"
              value={s.inset ? "inner" : "drop"}
              options={[
                ["drop", "Drop"],
                ["inner", "Inner"],
              ]}
              onChange={(v) => update(i, { ...s, inset: v === "inner" })}
              className="min-w-0 flex-1"
            />
            <RowTools label="shadow" hidden={s.hidden} onToggle={() => update(i, { ...s, hidden: !s.hidden })} onRemove={() => set(shadows.filter((_, j) => j !== i))} />
          </div>
          <ColorField label="Shadow" value={s.color} tokens={tokens} onChange={(color, live) => update(i, { ...s, color }, live)} />
          <div className="grid grid-cols-4 gap-1">
            {(["x", "y", "blur", "spread"] as const).map((k) => (
              <NumField key={k} label={k === "blur" ? "B" : k === "spread" ? "S" : k.toUpperCase()} title={k} value={s[k]} min={k === "blur" ? 0 : undefined} onCommit={(v, live) => update(i, { ...s, [k]: v }, live)} />
            ))}
          </div>
        </div>
      ))}
    </Section>
  );
}

const WEIGHTS: [string, string][] = [
  ["100", "Thin"],
  ["200", "Extra light"],
  ["300", "Light"],
  ["400", "Regular"],
  ["500", "Medium"],
  ["600", "Semibold"],
  ["700", "Bold"],
  ["800", "Extra bold"],
  ["900", "Black"],
];

/** Toggles a word in a space-separated CSS value; an empty result clears the property. */
const toggleWord = (value: string | undefined, word: string) => {
  const words = (value ?? "").split(/\s+/).filter(Boolean);
  const next = words.includes(word) ? words.filter((w) => w !== word) : [...words, word];
  return next.length ? next.join(" ") : null;
};

export function TypographySection({ n, setStyle, tokens }: Ctx) {
  const s = n.style;
  const raw = s.raw ?? {};
  const family = (s.fontFamily ?? "Geist").split(",")[0].replace(/["']/g, "").trim();
  const align = s.textAlign ?? "left";
  const valign = raw["align-content"] ?? "start";
  const clamp = raw["-webkit-line-clamp"];
  const decorated = (s.textDecoration ?? "none") !== "none";
  const numeric = raw["font-variant-numeric"];
  const features: [string, string, boolean, () => void][] = [
    ["Ligatures", "fi", raw["font-variant-ligatures"] !== "none", () => setStyle({ fontVariantLigatures: raw["font-variant-ligatures"] === "none" ? null : "none" })],
    ["Tabular numbers", "1 2", !!numeric?.includes("tabular-nums"), () => setStyle({ fontVariantNumeric: toggleWord(numeric, "tabular-nums") })],
    ["Slashed zero", "0", !!numeric?.includes("slashed-zero"), () => setStyle({ fontVariantNumeric: toggleWord(numeric, "slashed-zero") })],
    ["Fractions", "½", !!numeric?.includes("diagonal-fractions"), () => setStyle({ fontVariantNumeric: toggleWord(numeric, "diagonal-fractions") })],
  ];
  return (
    <Section title="Typography">
      <FontPicker value={s.fontFamily?.startsWith("var(") ? s.fontFamily : family} onPick={(f) => setStyle({ fontFamily: f.startsWith("var(") ? f : `${f}, sans-serif` })} />
      <div className="grid grid-cols-2 gap-2">
        <Select label="Font weight" value={String(s.fontWeight ?? 400)} options={WEIGHTS} onChange={(w) => setStyle({ fontWeight: Number(w) })} />
        <NumField label="S" title="Font size" testid="field-font-size" value={num(s.fontSize) ?? 16} min={1} onCommit={(v, l) => setStyle({ fontSize: `${v}px` }, l ? "fontSize" : undefined)} />
        <NumField label="L" title="Line height" value={s.lineHeight ? num(s.lineHeight) : 1.2} step={0.05} min={0} onCommit={(v, l) => setStyle({ lineHeight: String(v) }, l ? "lineHeight" : undefined)} />
        <NumField label="T" title="Letter spacing (em)" value={s.letterSpacing ? num(s.letterSpacing) : 0} step={0.01} suffix="em" onCommit={(v, l) => setStyle({ letterSpacing: v ? `${v}em` : null }, l ? "tracking" : undefined)} />
      </div>
      <div className="flex items-center gap-1">
        {(
          [
            ["left", AlignLeft],
            ["center", AlignCenter],
            ["right", AlignRight],
            ["justify", AlignJustify],
          ] as const
        ).map(([a, Icon]) => (
          <IconButton key={a} label={`Align ${a}`} active={align === a} onClick={() => setStyle({ textAlign: a === "left" ? null : a })}>
            <Icon size={14} />
          </IconButton>
        ))}
      </div>
      <ColorField label="Text color" value={s.color ?? "#000000"} tokens={tokens} onChange={(c, live) => setStyle({ color: c }, live ? "color" : undefined)} />
      <div className="flex items-center gap-1">
        {(
          [
            ["start", "top", AlignVerticalJustifyStart],
            ["center", "middle", AlignVerticalJustifyCenter],
            ["end", "bottom", AlignVerticalJustifyEnd],
          ] as const
        ).map(([v, name, Icon]) => (
          <IconButton key={v} label={`Align text ${name}`} active={valign === v} onClick={() => setStyle({ alignContent: v === "start" ? null : v })}>
            <Icon size={14} />
          </IconButton>
        ))}
        <Select
          label="Text wrap"
          value={raw["text-wrap"] ?? "wrap"}
          options={[
            ["wrap", "Wrap: normal"],
            ["balance", "Wrap: balance"],
            ["pretty", "Wrap: pretty"],
          ]}
          onChange={(v) => setStyle({ textWrap: v === "wrap" ? null : v })}
          className="min-w-0 flex-1"
        />
      </div>
      <div className="flex items-center gap-2">
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            aria-label="Truncate text"
            checked={!!clamp}
            onChange={() => setStyle(clamp ? { display: null, WebkitBoxOrient: null, WebkitLineClamp: null, overflow: null, textOverflow: null } : { display: "-webkit-box", WebkitBoxOrient: "vertical", WebkitLineClamp: "1", overflow: "hidden", textOverflow: "ellipsis" })}
          />
          Truncate
        </label>
        <div className="w-24">
          <NumField label="#" title="Max lines" value={clamp ? Number.parseInt(clamp, 10) : null} placeholder="Lines" min={1} disabled={!clamp} onCommit={(v) => setStyle({ WebkitLineClamp: String(Math.round(v)) })} />
        </div>
      </div>
      <div className="grid grid-cols-2 gap-2">
        <Select
          label="Decoration"
          value={s.textDecoration ?? "none"}
          options={[
            ["none", "No decoration"],
            ["underline", "Underline"],
            ["overline", "Overline"],
            ["line-through", "Strikethrough"],
          ]}
          onChange={(v) => setStyle({ textDecoration: v === "none" ? null : v, ...(v === "none" ? { textDecorationStyle: null } : {}) })}
        />
        <Select
          label="Decoration style"
          value={raw["text-decoration-style"] ?? "solid"}
          options={[
            ["solid", "Solid"],
            ["dashed", "Dashed"],
            ["dotted", "Dotted"],
            ["wavy", "Wavy"],
          ]}
          onChange={(v) => setStyle({ textDecorationStyle: v === "solid" ? null : v })}
          className={decorated ? "" : "opacity-50"}
        />
        <Select
          label="Case"
          value={s.textTransform ?? "none"}
          options={[
            ["none", "As typed"],
            ["uppercase", "Uppercase"],
            ["lowercase", "Lowercase"],
            ["capitalize", "Title case"],
          ]}
          onChange={(v) => setStyle({ textTransform: v === "none" ? null : v })}
        />
      </div>
      <fieldset className="m-0 grid grid-cols-4 gap-1 border-0 p-0" aria-label="Font features">
        {features.map(([label, glyph, on, toggle]) => (
          <button key={label} type="button" title={label} aria-label={label} aria-pressed={on} onClick={toggle} className={`h-6 rounded-control text-sm ${on ? "bg-selected text-selected-fg" : "bg-control hover:bg-hover"}`}>
            {glyph}
          </button>
        ))}
      </fieldset>
    </Section>
  );
}

const FILTERS: [name: string, label: string, unit: string, rest: number][] = [
  ["brightness", "Brightness", "%", 1],
  ["contrast", "Contrast", "%", 1],
  ["grayscale", "Grayscale", "%", 0],
  ["hue-rotate", "Hue rotate", "°", 0],
  ["invert", "Invert", "%", 0],
  ["saturate", "Saturate", "%", 1],
  ["sepia", "Sepia", "%", 0],
];

/** The CSS `filter` value for a blur plus the other filter functions (resting values left out). */
const filterCss = (blur: number, filters: Record<string, number>) => {
  const parts = blur ? [`blur(${blur}px)`] : [];
  for (const [name, , unit, rest] of FILTERS) {
    const v = filters[name] ?? rest;
    if (v !== rest) parts.push(`${name}(${unit === "°" ? `${v}deg` : v})`);
  }
  return parts.length ? parts.join(" ") : null;
};

export function EffectsSection({ n, setStyle }: Ctx) {
  const blur = n.style.blur ?? 0;
  const filters = n.style.filters ?? {};
  return (
    <Section title="Effects">
      <div className="grid grid-cols-[64px_1fr] items-center gap-x-2 gap-y-1 text-sm text-fg-2">
        <span>Layer blur</span>
        <NumField label={<Square size={10} />} title="Layer blur" value={blur} min={0} onCommit={(v, s) => setStyle({ filter: filterCss(v, filters) }, s ? "blur" : undefined)} />
        <span>Backdrop</span>
        <NumField label={<SquareDashed size={10} />} title="Background blur" value={n.style.backdropBlur ?? 0} min={0} onCommit={(v, s) => setStyle({ backdropFilter: v ? `blur(${v}px)` : null }, s ? "backdrop" : undefined)} />
        {FILTERS.map(([name, label, unit, rest]) => {
          const shown = unit === "%" ? Math.round((filters[name] ?? rest) * 100) : (filters[name] ?? rest);
          return (
            <Fragment key={name}>
              <span>{label}</span>
              <NumField
                label={unit}
                title={label}
                value={shown}
                min={0}
                max={unit === "°" ? 360 : undefined}
                onCommit={(v, s) => setStyle({ filter: filterCss(blur, { ...filters, [name]: unit === "%" ? v / 100 : v }) }, s ? name : undefined)}
              />
            </Fragment>
          );
        })}
      </div>
    </Section>
  );
}
