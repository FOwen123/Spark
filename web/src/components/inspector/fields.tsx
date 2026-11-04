import * as Popover from "@radix-ui/react-popover";
import { Eye, EyeOff, Minus, Pipette, Plus, Unlink, Variable } from "lucide-react";
import { Children, createContext, useContext, useRef, useState } from "react";
import { parseColor, toHex, withAlpha } from "../../lib/color";
import type { DocStore } from "../../lib/doc";
import { pickColor } from "../../lib/eyedropper";

/** The open file, for fields that read tokens or sample the canvas. */
export const StoreContext = createContext<DocStore | null>(null);

const ICON_BTN = "grid size-6 shrink-0 place-items-center rounded-control text-fg-2 hover:bg-hover hover:text-fg data-[state=open]:bg-selected data-[state=open]:text-selected-fg";

/** An icon button that opens a small popover. Children render inside it; wrap items in `Popover.Close` to close on pick. */
export function Pop(props: { label: string; icon: React.ReactNode; children: React.ReactNode; className?: string; width?: number; align?: "start" | "center" | "end"; open?: boolean; onOpenChange?: (open: boolean) => void }) {
  return (
    <Popover.Root open={props.open} onOpenChange={props.onOpenChange}>
      <Popover.Trigger type="button" title={props.label} aria-label={props.label} className={`${ICON_BTN} ${props.className ?? ""}`}>
        {props.icon}
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          role="dialog"
          aria-label={props.label}
          align={props.align ?? "end"}
          sideOffset={4}
          collisionPadding={8}
          // Escape closes the popover only; the editor's own Escape (deselect) must not see it.
          onEscapeKeyDown={(e) => e.stopPropagation()}
          style={{ width: props.width ?? 200 }}
          className="z-50 rounded-popover border border-line bg-panel p-1 text-sm shadow-popover outline-none"
        >
          {props.children}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}

/** A full-width menu row for use inside a popover. */
export const POP_ITEM = "flex h-6 w-full items-center gap-2 rounded-control px-2 text-left hover:bg-hover";

export const fmt = (v: number) => String(Math.round(v * 100) / 100);

/** A number, or simple arithmetic like "120+16" or "300/2". */
export function evalNumber(raw: string): number | null {
  const s = raw.trim();
  const m = s.match(/^(-?\d*\.?\d+)\s*([-+*/])\s*(-?\d*\.?\d+)$/);
  if (m) {
    const [a, b] = [Number(m[1]), Number(m[3])];
    const v = m[2] === "+" ? a + b : m[2] === "-" ? a - b : m[2] === "*" ? a * b : b === 0 ? Number.NaN : a / b;
    return Number.isFinite(v) ? v : null;
  }
  const v = Number(s);
  return s !== "" && Number.isFinite(v) ? v : null;
}

/** A numeric field with a scrubbable label. `onCommit` gets the new number and whether the edit is a scrub. */
export function NumField(props: {
  label: React.ReactNode;
  value: number | null;
  onCommit: (v: number, scrub: boolean) => void;
  step?: number;
  min?: number;
  max?: number;
  suffix?: string;
  disabled?: boolean;
  title?: string;
  testid?: string;
  placeholder?: string;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const scrub = useRef<{ x: number; v: number } | null>(null);
  const clamp = (v: number) => Math.min(props.max ?? Number.POSITIVE_INFINITY, Math.max(props.min ?? Number.NEGATIVE_INFINITY, v));
  const commit = (raw: string) => {
    setDraft(null);
    const v = evalNumber(raw.replace(props.suffix ?? "", ""));
    if (v !== null && v !== props.value) props.onCommit(clamp(v), false);
  };
  return (
    <label
      title={props.title}
      className={`flex h-6 min-w-0 flex-1 items-center gap-1 rounded-control bg-control px-1.5 text-sm ${props.disabled ? "opacity-50" : ""}`}
    >
      <span
        className="w-3 shrink-0 cursor-ew-resize text-center text-fg-3 select-none"
        onPointerDown={(e) => {
          if (props.disabled || props.value === null) return;
          e.preventDefault();
          (e.target as HTMLElement).setPointerCapture(e.pointerId);
          scrub.current = { x: e.clientX, v: props.value };
        }}
        onPointerMove={(e) => {
          if (!scrub.current) return;
          const d = Math.round((e.clientX - scrub.current.x) / 2) * (props.step ?? 1);
          props.onCommit(clamp(scrub.current.v + d), true);
        }}
        onPointerUp={() => {
          scrub.current = null;
        }}
      >
        {props.label}
      </span>
      <input
        data-testid={props.testid}
        disabled={props.disabled}
        size={1}
        className="min-w-0 flex-1 bg-transparent font-mono text-sm outline-none"
        value={draft ?? (props.value === null ? "" : fmt(props.value) + (props.suffix ?? ""))}
        placeholder={props.value === null ? (props.placeholder ?? "Mixed") : undefined}
        onChange={(e) => setDraft(e.target.value)}
        onFocus={(e) => e.target.select()}
        onBlur={(e) => draft !== null && commit(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") (e.target as HTMLInputElement).blur();
          if (e.key === "Escape") {
            setDraft(null);
            (e.target as HTMLInputElement).blur();
          }
          if ((e.key === "ArrowUp" || e.key === "ArrowDown") && props.value !== null) {
            e.preventDefault();
            const d = (e.key === "ArrowUp" ? 1 : -1) * (props.step ?? 1) * (e.shiftKey ? 10 : 1);
            setDraft(null);
            props.onCommit(clamp(props.value + d), true);
          }
        }}
      />
    </label>
  );
}

export function Select<T extends string>(props: { value: T; options: [T, string][]; onChange: (v: T) => void; label?: string; className?: string }) {
  return (
    <select
      aria-label={props.label}
      value={props.value}
      onChange={(e) => props.onChange(e.target.value as T)}
      className={`h-6 min-w-0 rounded-control bg-control px-1 text-sm outline-none ${props.className ?? ""}`}
    >
      {props.options.map(([v, l]) => (
        <option key={v} value={v}>
          {l}
        </option>
      ))}
    </select>
  );
}

/** Swatch + hex + alpha for a color string. Token references show as their name. */
export function ColorField(props: { value: string; onChange: (css: string, live: boolean) => void; tokens: Map<string, string>; label?: string }) {
  const rgba = parseColor(props.value, props.tokens);
  const hex = rgba ? toHex(rgba) : "#000000";
  const alpha = rgba ? Math.round(rgba.a * 100) : 100;
  const token = props.value.match(/^var\((--[\w-]+)/)?.[1];
  const [draft, setDraft] = useState<string | null>(null);
  const store = useContext(StoreContext);
  const colorTokens = store?.tokens.filter((t) => t.name.startsWith("--color-")) ?? [];
  const label = props.label ?? "Color";
  return (
    <div className="flex h-6 min-w-0 flex-1 items-center gap-1.5 rounded-control bg-control px-1.5 text-sm">
      <span className="relative size-4 shrink-0 overflow-hidden rounded-[3px] border border-line" style={{ background: props.value }}>
        <input
          type="color"
          aria-label={props.label ?? "Color"}
          value={hex}
          onChange={(e) => props.onChange(withAlpha(e.target.value, alpha / 100), true)}
          className="absolute inset-0 size-full cursor-pointer opacity-0"
        />
      </span>
      {token ? (
        <span className="min-w-0 flex-1 truncate font-mono" title={token}>
          {token.replace(/^--color-/, "")}
        </span>
      ) : (
        <input
          aria-label={`${props.label ?? "Color"} hex`}
          size={1}
          className="min-w-0 flex-1 bg-transparent font-mono uppercase outline-none"
          value={draft ?? hex.slice(1)}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={() => {
            if (draft !== null) {
              const v = draft.trim().replace(/^#/, "");
              if (/^[0-9a-f]{3}([0-9a-f]{3})?$/i.test(v)) props.onChange(withAlpha(`#${v}`, alpha / 100), false);
              else if (parseColor(draft)) props.onChange(draft, false);
            }
            setDraft(null);
          }}
          onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
        />
      )}
      <button
        type="button"
        title="Pick a color from the canvas"
        aria-label={`${label} eyedropper`}
        className="grid size-5 shrink-0 place-items-center rounded-control text-fg-3 hover:text-fg"
        onClick={async () => {
          const c = await pickColor(store);
          if (c) props.onChange(c, false);
        }}
      >
        <Pipette size={12} />
      </button>
      {store && (
        <Pop label={`${label} tokens`} icon={<Variable size={12} />} className="!size-5 !rounded-control !text-fg-3 hover:!text-fg" width={200}>
          {colorTokens.length === 0 && <p className="px-2 py-1 text-fg-3">No color tokens in this file.</p>}
          <div className="max-h-56 overflow-auto spark-scroll">
            {colorTokens.map((t) => (
              <Popover.Close key={t.name} asChild>
                <button type="button" aria-label={`Bind ${t.name}`} aria-pressed={t.name === token} onClick={() => props.onChange(`var(${t.name})`, false)} className={`${POP_ITEM} ${t.name === token ? "font-medium" : ""}`}>
                  <span className="size-3.5 shrink-0 rounded-[3px] border border-line" style={{ background: `var(${t.name})` }} />
                  <span className="min-w-0 flex-1 truncate font-mono">{t.name.replace(/^--color-/, "")}</span>
                </button>
              </Popover.Close>
            ))}
          </div>
        </Pop>
      )}
      {token && (
        <button
          type="button"
          title="Detach from token"
          aria-label={`Detach ${label} token`}
          className="grid size-5 shrink-0 place-items-center rounded-control text-fg-3 hover:text-fg"
          onClick={() => props.onChange(rgba ? withAlpha(hex, rgba.a) : "#000000", false)}
        >
          <Unlink size={12} />
        </button>
      )}
      <input
        aria-label={`${props.label ?? "Color"} opacity`}
        hidden={!!token}
        className="w-8 shrink-0 bg-transparent text-right font-mono outline-none"
        defaultValue={`${alpha}%`}
        key={`${props.value}`}
        onBlur={(e) => {
          const a = Number.parseFloat(e.target.value);
          if (Number.isFinite(a) && a !== alpha) props.onChange(withAlpha(hex, Math.min(100, Math.max(0, a)) / 100), false);
        }}
        onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
      />
    </div>
  );
}

export function IconButton(props: { label: string; onClick: () => void; children: React.ReactNode; active?: boolean; disabled?: boolean; className?: string }) {
  return (
    <button
      type="button"
      disabled={props.disabled}
      title={props.label}
      aria-label={props.label}
      aria-pressed={props.active}
      onClick={props.onClick}
      className={`grid size-6 shrink-0 place-items-center rounded-control ${props.active ? "bg-selected text-selected-fg" : "text-fg-2 hover:bg-hover hover:text-fg"} disabled:pointer-events-none disabled:opacity-40 ${props.className ?? ""}`}
    >
      {props.children}
    </button>
  );
}

export function Section(props: { title: string; onAdd?: () => void; children?: React.ReactNode; right?: React.ReactNode }) {
  return (
    <section aria-label={props.title} className="border-b border-line px-3 pt-2 pb-3">
      <div className="flex h-7 items-center justify-between">
        <h3 className="text-base font-medium">{props.title}</h3>
        <div className="flex items-center">
          {props.right}
          {props.onAdd && (
            <IconButton label={`Add ${props.title.toLowerCase()}`} onClick={props.onAdd}>
              <Plus size={14} />
            </IconButton>
          )}
        </div>
      </div>
      {Children.toArray(props.children).length > 0 && <div className="mt-1 flex flex-col gap-2">{props.children}</div>}
    </section>
  );
}

export function RowTools(props: { hidden?: boolean; onToggle?: () => void; onRemove: () => void; label: string }) {
  return (
    <>
      {props.onToggle && (
        <IconButton label={props.hidden ? `Show ${props.label}` : `Hide ${props.label}`} onClick={props.onToggle}>
          {props.hidden ? <EyeOff size={13} /> : <Eye size={13} />}
        </IconButton>
      )}
      <IconButton label={`Remove ${props.label}`} onClick={props.onRemove}>
        <Minus size={13} />
      </IconButton>
    </>
  );
}
