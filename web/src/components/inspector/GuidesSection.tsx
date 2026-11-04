import type { LayoutGuide } from "../../lib/types";
import { NumField, RowTools, Section, Select } from "./fields";
import type { Ctx } from "./sections";

const KINDS: [LayoutGuide["kind"], string][] = [
  ["columns", "Columns"],
  ["rows", "Rows"],
  ["grid", "Grid"],
];

/** Column, row and grid guides for a frame. They draw on the canvas (Shift+G hides them) and aren't exported. */
export function GuidesSection({ n, setProp }: Ctx) {
  const guides = n.layoutGuides ?? [];
  const set = (next: LayoutGuide[], merge?: string) => setProp("layoutGuides", next.length ? next : null, merge);
  const patch = (i: number, change: Partial<LayoutGuide>, scrub?: boolean) => set(guides.map((g, j) => (j === i ? { ...g, ...change } : g)), scrub ? "guides" : undefined);
  return (
    <Section title="Layout guides" onAdd={() => set([...guides, { kind: "columns", count: 12, gutter: 16, margin: 0, size: 8 }])}>
      {guides.map((g, i) => (
        <div
          // biome-ignore lint/suspicious/noArrayIndexKey: guides are positional
          key={i}
          className="flex flex-col gap-1"
          data-testid="layout-guide"
        >
          <div className="flex items-center gap-1">
            <Select label="Guide type" value={g.kind} options={KINDS} onChange={(kind) => patch(i, { kind })} className="flex-1" />
            <RowTools label="layout guide" onRemove={() => set(guides.filter((_, j) => j !== i))} />
          </div>
          <div className="grid grid-cols-3 gap-1">
            {g.kind === "grid" ? (
              <NumField label="S" title="Cell size" value={g.size} min={1} onCommit={(size, s) => patch(i, { size }, s)} />
            ) : (
              <>
                <NumField label="#" title="Count" value={g.count} min={1} max={500} onCommit={(count, s) => patch(i, { count: Math.round(count) }, s)} />
                <NumField label="G" title="Gutter" value={g.gutter} min={0} onCommit={(gutter, s) => patch(i, { gutter }, s)} />
                <NumField label="M" title="Margin" value={g.margin} min={0} onCommit={(margin, s) => patch(i, { margin }, s)} />
              </>
            )}
          </div>
        </div>
      ))}
    </Section>
  );
}
