import { FileText } from "lucide-react";
import { useState } from "react";
import type { DocStore } from "../../lib/doc";
import { CodeDialog } from "../CodeDialog";
import { IconButton } from "./fields";

/** The file's style guide: markdown rules agents read with get_basic_info and follow. */
export function GuideView({ store }: { store: DocStore }) {
  const [md, setMd] = useState<string | null>(null);
  return (
    <section aria-label="Style guide" className="flex min-h-0 flex-1 flex-col pt-2 pr-3 pb-3 pl-5">
      <div className="flex h-7 items-center justify-between">
        <h3 className="text-base font-medium">Style guide</h3>
        <IconButton label="Export as DESIGN.md" onClick={async () => setMd(await store.do<string>({ type: "designMd" }))}>
          <FileText size={14} />
        </IconButton>
      </div>
      <p className="mb-2 text-sm text-fg-3">Rules and decisions agents follow in this file. Markdown.</p>
      <textarea
        aria-label="Style guide"
        // Remount when the guide changes elsewhere (an agent, undo) so the field shows it.
        key={store.guide}
        defaultValue={store.guide}
        placeholder={"# Rules\n- Headings: Geist 600\n- Flat surfaces, 1px borders, no shadows\n\n## Decisions\n- Grayscale only, no accent color"}
        spellCheck={false}
        className="min-h-40 flex-1 resize-none rounded-control bg-control p-2 font-mono text-sm outline-none placeholder:text-fg-3"
        onBlur={(e) => e.target.value !== store.guide && store.do({ type: "setGuide", text: e.target.value })}
      />
      <CodeDialog title="DESIGN.md" code={md} onClose={() => setMd(null)} />
    </section>
  );
}
