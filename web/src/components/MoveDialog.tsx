import * as Dialog from "@radix-ui/react-dialog";
import { ChevronLeft, Folder } from "lucide-react";
import { useEffect, useState } from "react";
import type { Conn } from "../lib/conn";

/** Picks a folder of the files folder for a file and moves it there on the server. */
export function MoveDialog(props: { conn: Conn; file: { id: string; name: string; folder: string } | null; onClose: () => void; onMoved: (path: string) => void }) {
  const { conn, file } = props;
  const [dir, setDir] = useState("");
  const [folders, setFolders] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (file) setDir(file.folder);
  }, [file]);
  useEffect(() => {
    if (!file) return;
    conn.request<{ folders: string[] }>({ t: "browse", dir }).then(
      (r) => {
        setFolders(r.folders);
        setError(null);
      },
      (e: Error) => setError(e.message),
    );
  }, [conn, file, dir]);

  const up = dir.includes("/") ? dir.slice(0, dir.lastIndexOf("/")) : "";
  return (
    <Dialog.Root open={file !== null} onOpenChange={(o) => !o && props.onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-bg/60" />
        <Dialog.Content
          aria-describedby={undefined}
          className="fixed top-1/2 left-1/2 z-50 flex max-h-[80vh] w-[min(420px,92vw)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-popover border border-line bg-panel shadow-popover"
        >
          <Dialog.Title className="truncate px-3 pt-3 text-base font-medium">Move {file?.name} to…</Dialog.Title>
          <div className="mt-2 flex h-8 items-center gap-1 border-y border-line px-2 text-sm text-fg-2">
            <button
              type="button"
              aria-label="Up one folder"
              disabled={!dir}
              onClick={() => setDir(up)}
              className="grid size-6 place-items-center rounded-control hover:bg-hover disabled:opacity-40"
            >
              <ChevronLeft size={14} />
            </button>
            <span data-testid="move-path" className="truncate font-mono text-xs">{dir ? `Files / ${dir.replaceAll("/", " / ")}` : "Files"}</span>
          </div>
          <div role="listbox" aria-label="Folders" className="min-h-[120px] flex-1 overflow-auto p-1 spark-scroll">
            {folders.length === 0 && <p className="px-2 py-3 text-sm text-fg-3">No folders here.</p>}
            {folders.map((n) => (
              <button
                key={n}
                type="button"
                role="option"
                aria-selected={false}
                onClick={() => setDir(dir ? `${dir}/${n}` : n)}
                className="flex h-7 w-full items-center gap-2 rounded-control px-2 text-left text-base hover:bg-hover"
              >
                <Folder size={14} strokeWidth={1.5} className="shrink-0 text-fg-3" />
                <span className="truncate">{n}</span>
              </button>
            ))}
          </div>
          {error && <p className="px-3 pb-1 text-sm text-fg-2">⚠ {error}</p>}
          <div className="flex justify-end gap-2 border-t border-line p-3">
            <button type="button" onClick={props.onClose} className="h-8 rounded-control border border-line px-3 text-base hover:bg-hover">
              Cancel
            </button>
            <button
              type="button"
              onClick={() =>
                file &&
                conn.request<{ path: string }>({ t: "move", id: file.id, dir }).then(
                  (r) => props.onMoved(r.path),
                  (e: Error) => setError(e.message),
                )
              }
              className="h-8 rounded-control bg-selected px-3 text-base text-selected-fg"
            >
              Move here
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
