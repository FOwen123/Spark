import { Camera } from "lucide-react";
import { useState } from "react";
import type { DocStore } from "../../lib/doc";
import { renderToPng } from "../../lib/render";
import { POP_ITEM, Pop } from "./fields";

/** Renders a layer to a PNG and stores it as an image asset; resolves to the asset hash. */
async function capture(store: DocStore, id: string): Promise<string> {
  const png = await renderToPng(store, id, 1);
  const res = await fetch(`/api/assets?file=${store.file.id}`, {
    method: "POST",
    headers: { Authorization: `Bearer ${sessionStorage.getItem("spark-token") ?? ""}`, "Content-Type": "image/png" },
    body: Uint8Array.from(atob(png), (c) => c.charCodeAt(0)),
  });
  if (!res.ok) throw new Error(await res.text());
  return ((await res.json()) as { hash: string }).hash;
}

/** Layers on the viewed page, depth first, for the picker. */
function pageLayers(store: DocStore): string[] {
  const out: string[] = [];
  const walk = (parent: string) => {
    for (const id of store.kids(parent)) {
      out.push(id);
      walk(id);
    }
  };
  walk(store.page);
  return out;
}

/** A button that lists the page's layers; picking one fills the selection with a rendered image of it. */
export function CaptureLayer(props: { store: DocStore; exclude: string[]; onCaptured: (hash: string) => void }) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);
  const q = query.trim().toLowerCase();
  const layers = pageLayers(props.store)
    .filter((id) => !props.exclude.includes(id) && (props.store.nodes.get(id)?.name ?? "").toLowerCase().includes(q))
    .slice(0, 60);
  return (
    <Pop label="Capture from layer" icon={<Camera size={14} />} width={220} open={open} onOpenChange={setOpen}>
      <input aria-label="Search layers" placeholder="Pick a layer to capture" value={query} onChange={(e) => setQuery(e.target.value)} className="mb-1 h-6 w-full rounded-control bg-control px-2 outline-none" />
      {error && <p className="px-2 py-1 text-fg-2">{error}</p>}
      <div className="max-h-56 overflow-auto spark-scroll">
        {layers.map((id) => (
          <button
            key={id}
            type="button"
            className={POP_ITEM}
            onClick={() => {
              setError(null);
              capture(props.store, id)
                .then((hash) => {
                  props.onCaptured(hash);
                  setOpen(false);
                })
                .catch((e: Error) => setError(e.message));
            }}
          >
            <span className="min-w-0 flex-1 truncate">{props.store.nodes.get(id)?.name}</span>
            <span className="text-xs text-fg-3">{props.store.nodes.get(id)?.kind}</span>
          </button>
        ))}
        {layers.length === 0 && <p className="px-2 py-1 text-fg-3">No other layers on this page.</p>}
      </div>
    </Pop>
  );
}
