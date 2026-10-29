import {
  AlignCenterHorizontal,
  AlignLeft,
  AlignCenter,
  AlignRight,
  AlignJustify,
  ArrowDown,
  ArrowRight,
  Square,
  SquareDashed,
  WrapText,
} from "lucide-react";
import type { DocStore } from "../../lib/doc";
import type { Fill, NodeView, Shadow } from "../../lib/types";
import { ColorField, IconButton, NumField, RowTools, Section, Select } from "./fields";

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

export function PositionSection({ store, n, ids, setStyle }: Ctx) {
  const r = store.rects.get(n.id);
  const pr = store.rects.get(n.parent);
  const free = n.parent.startsWith("p-") || n.style.position === "absolute";
  const x = px(n.style.left) ?? (r ? r.x - (pr?.x ?? 0) : null);
  const y = px(n.style.top) ?? (r ? r.y - (pr?.y ?? 0) : null);
  const mode = (axis: "width" | "height") => {
    const v = n.style[axis];
    return v === "fill" ? "fill" : v === "hug" ? "hug" : v ? "fixed" : n.kind === "text" && axis === "width" ? "auto" : "auto";
  };
  const sizeOptions: [string, string][] = [
    ["fixed", "Fixed"],
    ["fill", "Fill"],
    ["hug", "Hug"],
    ["auto", "Auto"],
  ];
  const setMode = (axis: "width" | "height", m: string) => {
    const cur = axis === "width" ? r?.w : r?.h;
    setStyle({ [axis]: m === "fixed" ? `${Math.round(cur ?? 100)}px` : m === "auto" ? null : m });
  };
  const multi = ids.length > 1;
  return (
    <Section title="Position and size">
      <div className="grid grid-cols-[1fr_1fr_72px] gap-2">
        <NumField label="X" testid="field-x" value={multi ? null : x} disabled={!free} onCommit={(v, s) => setStyle({ left: `${v}px`, position: "absolute" }, s ? "x" : undefined)} />
        <NumField label="Y" testid="field-y" value={multi ? null : y} disabled={!free} onCommit={(v, s) => setStyle({ top: `${v}px`, position: "absolute" }, s ? "y" : undefined)} />
        <NumField label="∠" value={n.style.rotate ?? 0} suffix="°" onCommit={(v, s) => setStyle({ rotate: v ? `${v}deg` : null }, s ? "rotate" : undefined)} title="Rotation" />
      </div>
      <div className="grid grid-cols-2 gap-2">
        {(["width", "height"] as const).map((axis) => (
          <NumField
            key={axis}
            label={axis === "width" ? "W" : "H"}
            testid={axis === "width" ? "field-w" : "field-h"}
            value={r && !multi ? Math.round((axis === "width" ? r.w : r.h) * 100) / 100 : null}
            placeholder={multi ? "Mixed" : "–"}
            min={1}
            onCommit={(v, s) => setStyle({ [axis]: `${v}px` }, s ? axis : undefined)}
          />
        ))}
        {(["width", "height"] as const).map((axis) => (
          <Select key={axis} label={`${axis} sizing`} value={mode(axis)} options={sizeOptions} onChange={(m) => setMode(axis, m)} />
        ))}
      </div>
    </Section>
  );
}

const JUSTIFY = ["flex-start", "center", "flex-end"];

export function LayoutSection({ store, n, ids, setStyle }: Ctx) {
  const flex = n.style.display === "flex" || n.style.display === "inline-flex";
  const row = !n.style.flexDirection?.startsWith("column");
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
            <IconButton label="Row" active={row} onClick={() => setStyle({ flexDirection: "row" })}>
              <ArrowRight size={14} />
            </IconButton>
            <IconButton label="Column" active={!row} onClick={() => setStyle({ flexDirection: "column" })}>
              <ArrowDown size={14} />
            </IconButton>
            <IconButton label="Wrap" active={n.style.flexWrap === "wrap"} onClick={() => setStyle({ flexWrap: n.style.flexWrap === "wrap" ? null : "wrap" })}>
              <WrapText size={14} />
            </IconButton>
            <IconButton label="Space between" active={between} onClick={() => setStyle({ justifyContent: between ? "flex-start" : "space-between" })}>
              <AlignCenterHorizontal size={14} />
            </IconButton>
            <div className="ml-1 min-w-0 flex-1">
              <NumField label="G" title="Gap" value={px(n.style.gap) ?? 0} min={0} onCommit={(v, s) => setStyle({ gap: `${v}px` }, s ? "gap" : undefined)} />
            </div>
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
        <NumField label="R" title="Corner radius" testid="field-radius" value={same ? (px(radius?.[0]) ?? 0) : null} min={0} onCommit={(v, s) => setStyle({ borderRadius: `${v}px` }, s ? "radius" : undefined)} />
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

export function FillSection({ store, n, setProp, tokens }: Ctx) {
  const fills = n.style.fills ?? [];
  const set = (next: Fill[], live = false) => setProp("style.fills", next.length ? next : null, live ? "fills" : undefined);
  const update = (i: number, f: Fill, live = false) => set(fills.map((x, j) => (j === i ? f : x)), live);
  const stopsOf = (f: Fill) => ("stops" in f ? f.stops : "color" in f ? [{ color: f.color, pos: 0 }, { color: "#000000", pos: 1 }] : [{ color: "#ffffff", pos: 0 }, { color: "#000000", pos: 1 }]);
  const firstColor = (f: Fill) => ("color" in f ? f.color : "stops" in f ? f.stops[0]?.color : "#d9d9d9") ?? "#d9d9d9";
  const changeType = (i: number, t: Fill["type"]) => {
    const f = fills[i];
    if (t === f.type) return;
    if (t === "solid") update(i, solid(firstColor(f)));
    else if (t === "linear") update(i, { type: "linear", angle: 180, stops: stopsOf(f) });
    else if (t === "radial") update(i, { type: "radial", stops: stopsOf(f) });
    else window.dispatchEvent(new CustomEvent("spark:pick-image", { detail: (hash: string) => update(i, { type: "image", asset: hash, fit: "cover" }) }));
  };
  return (
    <Section title="Fill" onAdd={() => set([solid(fills.length ? "#000000" : "#d9d9d9"), ...fills])}>
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
                ["image", "Image"],
              ]}
              onChange={(t) => changeType(i, t)}
              className="w-[68px] shrink-0"
            />
            {f.type === "solid" && <ColorField label="Fill" value={f.color} tokens={tokens} onChange={(c, live) => update(i, { ...f, color: c }, live)} />}
            {(f.type === "linear" || f.type === "radial") && <div className="h-6 flex-1 rounded-control border border-line" style={{ background: `${f.type}-gradient(${f.type === "linear" ? `${f.angle}deg, ` : "circle, "}${f.stops.map((s) => `${s.color} ${s.pos * 100}%`).join(", ")})` }} />}
            {f.type === "image" && (
              <div className="flex h-6 flex-1 items-center gap-1">
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
            <RowTools label="fill" hidden={f.hidden} onToggle={() => update(i, { ...f, hidden: !f.hidden })} onRemove={() => set(fills.filter((_, j) => j !== i))} />
          </div>
          {(f.type === "linear" || f.type === "radial") && (
            <div className="flex flex-col gap-1 pl-[72px]">
              {f.type === "linear" && <NumField label="∠" title="Angle" value={f.angle} suffix="°" onCommit={(v, s) => update(i, { ...f, angle: v }, s)} />}
              {f.stops.map((s, k) => (
                <div
                  // biome-ignore lint/suspicious/noArrayIndexKey: stops are positional
                  key={k}
                  className="flex gap-1"
                >
                  <ColorField label={`Stop ${k + 1}`} value={s.color} tokens={tokens} onChange={(c, live) => update(i, { ...f, stops: f.stops.map((x, j) => (j === k ? { ...x, color: c } : x)) }, live)} />
                  <div className="w-14 shrink-0">
                    <NumField label="%" value={Math.round(s.pos * 100)} min={0} max={100} onCommit={(v, live) => update(i, { ...f, stops: f.stops.map((x, j) => (j === k ? { ...x, pos: v / 100 } : x)) }, live)} />
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
  return (
    <Section title="Border" onAdd={b ? undefined : () => setProp("style.border", { width: [1, 1, 1, 1], style: "solid", color: "#000000" })}>
      {b && (
        <>
          <div className="flex items-center gap-1">
            <ColorField label="Border" value={b.color} tokens={tokens} onChange={(color, live) => setProp("style.border", { ...b, color }, live ? "border" : undefined)} />
            <RowTools label="border" onRemove={() => setProp("style.border", null)} />
          </div>
          <div className="grid grid-cols-2 gap-2">
            <NumField label="W" title="Border width" value={b.width.every((w) => w === b.width[0]) ? b.width[0] : null} min={0} onCommit={(v, s) => setProp("style.border", { ...b, width: [v, v, v, v] }, s ? "border" : undefined)} />
            <Select
              label="Border style"
              value={b.style}
              options={[
                ["solid", "Solid"],
                ["dashed", "Dashed"],
                ["dotted", "Dotted"],
              ]}
              onChange={(style) => setProp("style.border", { ...b, style })}
            />
          </div>
        </>
      )}
    </Section>
  );
}

export function ShadowSection({ n, setProp, tokens }: Ctx) {
  const shadows = n.style.shadows ?? [];
  const set = (next: Shadow[], live = false) => setProp("style.shadows", next.length ? next : null, live ? "shadows" : undefined);
  const update = (i: number, s: Shadow, live = false) => set(shadows.map((x, j) => (j === i ? s : x)), live);
  return (
    <Section title="Shadow" onAdd={() => set([...shadows, { x: 0, y: 4, blur: 16, spread: 0, color: "#0000001f" }])}>
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
              className="w-[68px] shrink-0"
            />
            <ColorField label="Shadow" value={s.color} tokens={tokens} onChange={(color, live) => update(i, { ...s, color }, live)} />
            <RowTools label="shadow" hidden={s.hidden} onToggle={() => update(i, { ...s, hidden: !s.hidden })} onRemove={() => set(shadows.filter((_, j) => j !== i))} />
          </div>
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
const FAMILIES = ["Geist", "Geist Mono", "Inter", "system-ui", "Helvetica", "Arial", "Georgia", "Times New Roman", "Menlo"];

export function TypographySection({ n, setStyle, tokens }: Ctx) {
  const s = n.style;
  const family = (s.fontFamily ?? "Geist").split(",")[0].replace(/["']/g, "").trim();
  const align = s.textAlign ?? "left";
  return (
    <Section title="Typography">
      <input
        aria-label="Font family"
        list="spark-fonts"
        className="h-6 rounded-control bg-control px-1.5 text-sm outline-none"
        defaultValue={family}
        key={family}
        onBlur={(e) => e.target.value.trim() && e.target.value !== family && setStyle({ fontFamily: `${e.target.value.trim()}, sans-serif` })}
        onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
      />
      <datalist id="spark-fonts">
        {FAMILIES.map((f) => (
          <option key={f} value={f} />
        ))}
      </datalist>
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
        <ColorField label="Text color" value={s.color ?? "#000000"} tokens={tokens} onChange={(c, live) => setStyle({ color: c }, live ? "color" : undefined)} />
      </div>
      <div className="grid grid-cols-2 gap-2">
        <Select
          label="Decoration"
          value={s.textDecoration ?? "none"}
          options={[
            ["none", "No decoration"],
            ["underline", "Underline"],
            ["line-through", "Strikethrough"],
          ]}
          onChange={(v) => setStyle({ textDecoration: v === "none" ? null : v })}
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
    </Section>
  );
}

export function EffectsSection({ n, setStyle }: Ctx) {
  return (
    <Section title="Effects">
      <div className="grid grid-cols-[64px_1fr] items-center gap-x-2 gap-y-1 text-sm text-fg-2">
        <span>Layer blur</span>
        <NumField label={<Square size={10} />} title="Layer blur" value={n.style.blur ?? 0} min={0} onCommit={(v, s) => setStyle({ filter: v ? `blur(${v}px)` : null }, s ? "blur" : undefined)} />
        <span>Backdrop</span>
        <NumField label={<SquareDashed size={10} />} title="Background blur" value={n.style.backdropBlur ?? 0} min={0} onCommit={(v, s) => setStyle({ backdropFilter: v ? `blur(${v}px)` : null }, s ? "backdrop" : undefined)} />
      </div>
    </Section>
  );
}
