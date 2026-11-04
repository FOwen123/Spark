import { useRef } from "react";
import type { DocStore } from "../../lib/doc";
import { ColorField, Section } from "./fields";

/** Every color used by `ids` and their descendants (fills, gradient stops, border, shadows, text), with use counts. */
function usedColors(store: DocStore, ids: string[]): Map<string, number> {
  const counts = new Map<string, number>();
  const add = (c: string | undefined) => c && counts.set(c, (counts.get(c) ?? 0) + 1);
  const walk = (id: string) => {
    const s = store.nodes.get(id)?.style;
    if (!s) return;
    for (const f of s.fills ?? []) {
      if (f.type === "solid") add(f.color);
      else if ("stops" in f) for (const stop of f.stops) add(stop.color);
    }
    add(s.border?.color);
    for (const sh of s.shadows ?? []) add(sh.color);
    add(s.color);
    for (const kid of store.kids(id)) walk(kid);
  };
  ids.forEach(walk);
  return counts;
}

function Row(props: { store: DocStore; ids: string[]; index: number; value: string; count: number; tokens: Map<string, string> }) {
  // While a color is dragged the node values trail the picker, so remember what we last wrote.
  const current = useRef(props.value);
  current.current = props.value;
  return (
    <div className="flex items-center gap-1" data-testid="selection-color">
      <ColorField
        label={`Selection color ${props.index + 1}`}
        value={props.value}
        tokens={props.tokens}
        onChange={(to, live) => {
          const from = current.current;
          current.current = to;
          props.store.do({ type: "replaceColor", ids: props.ids, from, to, merge: live ? `replaceColor:${props.ids.join()}` : undefined });
        }}
      />
      <span className="w-6 shrink-0 text-right font-mono text-sm text-fg-3" title="Uses in the selection">
        {props.count}
      </span>
    </div>
  );
}

/** Lists the colors in the selection; changing one swaps it everywhere in the selection as one undo step. */
export function SelectionColorsSection(props: { store: DocStore; ids: string[]; tokens: Map<string, string> }) {
  const colors = [...usedColors(props.store, props.ids)];
  if (!colors.length) return null;
  return (
    <Section title="Selection colors">
      {colors.map(([value, count], i) => (
        // Rows keep their place while a color changes, so a color picker being dragged isn't remounted.
        // biome-ignore lint/suspicious/noArrayIndexKey: see above
        <Row key={i} store={props.store} ids={props.ids} index={i} value={value} count={count} tokens={props.tokens} />
      ))}
    </Section>
  );
}
