import * as ContextMenu from "@radix-ui/react-context-menu";
import { Archive, Copy, FilePlus, FileText, Folder, FolderOpen, FolderPlus, LayoutGrid, List, Plug, Search, Trash2, X } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import type { Conn } from "../lib/conn";
import { showToast } from "../lib/toast";
import type { FileInfo } from "../lib/types";
import { Item } from "./CanvasMenu";
import { ConnectDialog } from "./ConnectDialog";
import { Rename } from "./LeftPanel";
import { MoveDialog } from "./MoveDialog";
import { SparkLogo } from "./SparkLogo";

/** A file in the list; `folder` is its folder of the files folder (`""` for the top). */
type Recent = { id: string; name: string; path: string; openedAt: number; folder: string };

const VIEW_KEY = "spark-home-view";
const CALLOUT_KEY = "spark-home-callout-dismissed";
const FIRST_PROMPT = "Design a landing page for a small coffee shop, with a hero, a menu and a footer.";

function ago(ms: number) {
  const s = Math.max(0, (Date.now() - ms) / 1000);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  return new Date(ms).toLocaleDateString();
}

/** Right-click menu for a file row or card. */
function FileMenu(props: {
  children: ReactNode;
  onOpen: (activate: boolean) => void;
  onLink: () => void;
  onRename: () => void;
  onDuplicate: () => void;
  onMove: () => void;
  onArchive: () => void;
  onTrash: () => void;
}) {
  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild>{props.children}</ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className="z-50 min-w-[200px] rounded-popover border border-line bg-panel p-1 shadow-popover" aria-label="File menu" onCloseAutoFocus={(e) => e.preventDefault()}>
          <Item label="Open" onSelect={() => props.onOpen(true)} />
          <Item label="Open in new tab" onSelect={() => props.onOpen(false)} />
          <Item label="Copy link" onSelect={props.onLink} />
          <ContextMenu.Separator className="my-1 h-px bg-line" />
          <Item label="Rename" onSelect={props.onRename} />
          <Item label="Duplicate" onSelect={props.onDuplicate} />
          <Item label="Move to…" onSelect={props.onMove} />
          <ContextMenu.Separator className="my-1 h-px bg-line" />
          <Item label="Archive" onSelect={props.onArchive} />
          <Item label="Move to Trash" onSelect={props.onTrash} />
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}

export function Home(props: { conn: Conn; token: string; onOpen: (ref: string, activate?: boolean) => void; onNew: () => void; error: string | null }) {
  const [files, setFiles] = useState<Recent[]>([]);
  const [folders, setFolders] = useState<string[]>([]);
  const [path, setPath] = useState("");
  const [search, setSearch] = useState("");
  const [view, setView] = useState<"list" | "grid">(() => (localStorage.getItem(VIEW_KEY) === "grid" ? "grid" : "list"));
  const [callout, setCallout] = useState(() => localStorage.getItem(CALLOUT_KEY) !== "1");
  const [renaming, setRenaming] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [moving, setMoving] = useState<Recent | null>(null);
  const [dragging, setDragging] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [moved, setMoved] = useState<string | null>(null);
  const [connect, setConnect] = useState(false);
  const [native, setNative] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const folderDone = useRef(false);

  const refresh = useCallback(() => {
    props.conn.request<{ folders: string[]; files: Recent[] }>({ t: "browse", dir: path }).then(
      (r) => {
        setFolders(r.folders);
        setFiles(r.files);
      },
      () => setPath(""),
    );
  }, [props.conn, path]);
  useEffect(refresh, [refresh]);
  useEffect(() => {
    props.conn.request<{ native: boolean }>({ t: "hello" }).then((r) => setNative(r.native));
  }, [props.conn]);
  useEffect(() => localStorage.setItem(VIEW_KEY, view), [view]);

  const q = search.trim().toLowerCase();
  const shownFiles = files.filter((f) => (q ? f.name.toLowerCase().includes(q) : f.folder === path));
  const shownFolders = folders.filter((n) => !q || n.toLowerCase().includes(q));
  const enter = (n: string) => {
    setSearch("");
    setPath(path ? `${path}/${n}` : n);
  };

  /** Runs a file operation on the server; shows its failure inline. */
  async function act(msg: Record<string, unknown>, done: string | null) {
    setError(null);
    try {
      await props.conn.request(msg);
      if (done) showToast(done);
      refresh();
    } catch (e) {
      setError((e as Error).message);
    }
  }

  /** Archives a recent file, or moves it to the Trash, and drops it from the list. */
  async function remove(f: Recent, to: "archive" | "trash") {
    setError(null);
    try {
      const r = await props.conn.request<{ path: string } | null>({ t: to, id: f.id });
      setFiles((list) => list.filter((x) => x.id !== f.id));
      setMoved(to === "trash" ? `Moved ${f.name} to the Trash` : `Moved ${f.name} to ${r?.path}`);
    } catch (e) {
      setMoved(null);
      setError((e as Error).message);
    }
  }

  async function upload(file: File) {
    setError(null);
    const res = await fetch(`/api/import?name=${encodeURIComponent(file.name)}`, {
      method: "POST",
      headers: { Authorization: `Bearer ${props.token}` },
      body: file,
    });
    if (!res.ok) return setError(await res.text());
    const info = (await res.json()) as FileInfo;
    props.onOpen(info.id);
  }

  return (
    <main
      className="relative flex flex-1 justify-center overflow-auto bg-bg"
      onDragOver={(e) => {
        e.preventDefault();
        setDragging(true);
      }}
      onDragLeave={() => setDragging(false)}
      onDrop={(e) => {
        e.preventDefault();
        setDragging(false);
        const file = [...e.dataTransfer.files].find((f) => f.name.endsWith(".spark"));
        if (file) upload(file);
      }}
    >
      <div className="w-full max-w-[720px] px-8 py-16">
        <div className="flex items-center gap-3">
          <SparkLogo size={48} />
          <h1 className="text-[22px] font-semibold tracking-tight">Spark</h1>
        </div>
        <p className="mt-1 text-base text-fg-2">Design with real HTML and CSS, together with your agents.</p>
        <div className="mt-8 flex gap-2">
          <button
            type="button"
            onClick={props.onNew}
            className="flex h-8 items-center gap-2 rounded-control bg-selected px-3 text-base text-selected-fg"
          >
            <FilePlus size={15} strokeWidth={1.5} /> New file
          </button>
          <button
            type="button"
            onClick={() => (native ? props.conn.send({ t: "native", cmd: "openFile" }) : input.current?.click())}
            className="flex h-8 items-center gap-2 rounded-control border border-line px-3 text-base hover:bg-hover"
          >
            <FolderOpen size={15} strokeWidth={1.5} /> Open file…
          </button>
          <button
            type="button"
            onClick={() => setConnect(true)}
            className="flex h-8 items-center gap-2 rounded-control border border-line px-3 text-base hover:bg-hover"
          >
            <Plug size={15} strokeWidth={1.5} /> Connect your agent
          </button>
          <button
            type="button"
            onClick={() => {
              folderDone.current = false;
              setCreating(true);
            }}
            className="flex h-8 items-center gap-2 rounded-control border border-line px-3 text-base hover:bg-hover"
          >
            <FolderPlus size={15} strokeWidth={1.5} /> New folder
          </button>
          <input
            ref={input}
            type="file"
            accept=".spark"
            hidden
            onChange={(e) => {
              const f = e.target.files?.[0];
              if (f) upload(f);
              e.target.value = "";
            }}
          />
        </div>
        {(error ?? props.error) && <p className="mt-4 text-base text-fg-2">⚠ {error ?? props.error}</p>}
        {moved && !error && <p className="mt-4 text-base text-fg-2">{moved}</p>}
        {callout && (
          <section aria-label="Get started" className="mt-8 rounded-popover border border-dashed border-line p-4">
            <div className="flex items-start gap-3">
              <Plug size={16} strokeWidth={1.5} className="mt-0.5 shrink-0" />
              <div className="min-w-0 flex-1">
                <h2 className="text-base font-medium">Bring in an AI agent</h2>
                <p className="mt-1 text-sm text-fg-2">Spark is built to be drawn on together with an agent. Two steps to get going:</p>
                <p className="mt-3 flex items-center gap-2 text-sm">
                  <span className="font-medium">1.</span> Connect the agent you already use.
                  <button type="button" onClick={() => setConnect(true)} className="h-6 rounded-control bg-selected px-2 text-sm text-selected-fg">
                    Set up an agent
                  </button>
                </p>
                <p className="mt-2 flex items-center gap-2 text-sm">
                  <span className="font-medium">2.</span> Try a first prompt:
                  <q className="min-w-0 flex-1 truncate text-fg-2">{FIRST_PROMPT}</q>
                  <button
                    type="button"
                    aria-label="Copy the first prompt"
                    onClick={() => navigator.clipboard.writeText(FIRST_PROMPT).then(() => showToast("Prompt copied"), () => showToast("Couldn't copy the prompt"))}
                    className="grid size-6 shrink-0 place-items-center rounded-control hover:bg-hover"
                  >
                    <Copy size={13} strokeWidth={1.5} />
                  </button>
                </p>
              </div>
              <button
                type="button"
                aria-label="Dismiss"
                onClick={() => {
                  localStorage.setItem(CALLOUT_KEY, "1");
                  setCallout(false);
                }}
                className="grid size-6 shrink-0 place-items-center rounded-control text-fg-3 hover:bg-hover hover:text-fg"
              >
                <X size={14} />
              </button>
            </div>
          </section>
        )}
        <div className="mt-10 mb-2 flex items-center gap-2">
          <h2 className="flex min-w-0 flex-1 items-center gap-1 text-sm font-medium text-fg-3 uppercase">
            {q ? (
              "Search results"
            ) : (
              ["Recent", ...(path ? path.split("/") : [])].map((crumb, i, all) => (
                <span key={all.slice(0, i + 1).join("/")} className="flex min-w-0 items-center gap-1">
                  {i > 0 && <span aria-hidden="true">/</span>}
                  {i < all.length - 1 ? (
                    <button type="button" onClick={() => setPath(path.split("/").slice(0, i).join("/"))} className="uppercase hover:text-fg">
                      {crumb}
                    </button>
                  ) : (
                    <span className="truncate text-fg">{crumb}</span>
                  )}
                </span>
              ))
            )}
          </h2>
          <label className="flex h-7 w-44 items-center gap-1.5 rounded-control border border-line px-2 text-fg-3 focus-within:border-fg">
            <Search size={13} strokeWidth={1.5} />
            <input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search files"
              aria-label="Search files"
              className="min-w-0 flex-1 bg-transparent text-sm text-fg outline-none placeholder:text-fg-3"
            />
          </label>
          <div className="flex rounded-control border border-line">
            {(["list", "grid"] as const).map((v) => (
              <button
                key={v}
                type="button"
                aria-label={v === "list" ? "List view" : "Grid view"}
                aria-pressed={view === v}
                onClick={() => setView(v)}
                className={`grid size-7 place-items-center first:rounded-l-control last:rounded-r-control ${view === v ? "bg-selected text-selected-fg" : "text-fg-3 hover:bg-hover hover:text-fg"}`}
              >
                {v === "list" ? <List size={14} strokeWidth={1.5} /> : <LayoutGrid size={14} strokeWidth={1.5} />}
              </button>
            ))}
          </div>
        </div>
        {shownFiles.length === 0 && shownFolders.length === 0 && !creating ? (
          <p className="text-base text-fg-3">
            {q ? "No files match your search." : path ? "This folder is empty." : "No files yet. Create one, or drop a .spark file here."}
          </p>
        ) : (
          <ul
            className={view === "grid" ? "grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-3" : "divide-y divide-line border-y border-line"}
            aria-label="Recent files"
          >
            {creating && (
              <li className={view === "grid" ? "flex h-[120px] flex-col justify-center gap-2 rounded-popover border border-line p-3" : "flex items-center gap-3 py-2.5 pl-2"}>
                <Folder size={view === "grid" ? 28 : 15} strokeWidth={1.5} className="shrink-0 text-fg-3" />
                <Rename
                  value="Untitled folder"
                  onDone={(v) => {
                    if (folderDone.current) return;
                    folderDone.current = true;
                    setCreating(false);
                    if (v?.trim()) act({ t: "mkdir", dir: path, name: v.trim() }, null);
                  }}
                />
              </li>
            )}
            {shownFolders.map((n) => (
              <li key={`dir:${n}`} className={view === "grid" ? "rounded-popover border border-line hover:bg-hover" : "hover:bg-hover"}>
                <button
                  type="button"
                  aria-label={`Open folder ${n}`}
                  onClick={() => enter(n)}
                  className={view === "grid" ? "flex h-[120px] w-full flex-col justify-center gap-2 p-3 text-left" : "flex w-full items-center gap-3 py-2.5 pl-2 text-left"}
                >
                  <Folder size={view === "grid" ? 28 : 15} strokeWidth={1.5} className="shrink-0 text-fg-3" />
                  <span className="truncate text-base">{n}</span>
                </button>
              </li>
            ))}
            {shownFiles.map((f) => {
              const open = (activate = true) => props.onOpen(f.id, activate);
              const grid = view === "grid";
              return (
                <FileMenu
                  key={f.id}
                  onOpen={open}
                  onLink={() => navigator.clipboard.writeText(`spark://file/${f.id}`).then(() => showToast("Link copied"), () => showToast("Couldn't copy the link"))}
                  onRename={() => setRenaming(f.id)}
                  onDuplicate={() => act({ t: "duplicate", id: f.id }, `Duplicated ${f.name}`)}
                  onMove={() => setMoving(f)}
                  onArchive={() => remove(f, "archive")}
                  onTrash={() => remove(f, "trash")}
                >
                  <li className={`group relative flex hover:bg-hover ${grid ? "h-[120px] rounded-popover border border-line" : "items-center"}`}>
                    {renaming === f.id ? (
                      <div className={`flex min-w-0 flex-1 items-center gap-3 ${grid ? "flex-col justify-center p-3" : "py-2.5 pl-2"}`}>
                        <FileText size={grid ? 28 : 15} strokeWidth={1.5} className="shrink-0 text-fg-3" />
                        <Rename
                          value={f.name}
                          onDone={(v) => {
                            setRenaming(null);
                            if (v?.trim() && v.trim() !== f.name) act({ t: "rename", id: f.id, name: v.trim() }, null);
                          }}
                        />
                      </div>
                    ) : (
                      <button
                        type="button"
                        onClick={() => open()}
                        className={`flex min-w-0 flex-1 text-left ${grid ? "flex-col justify-center gap-1 p-3" : "items-center gap-3 py-2.5 pl-2"}`}
                      >
                        {grid && <FileText size={28} strokeWidth={1.5} className="mb-1 text-fg-3" />}
                        <span className={`truncate text-base ${grid ? "" : "flex-1"}`}>{f.name}</span>
                        {!grid && <span className="max-w-[320px] truncate font-mono text-xs text-fg-3">{f.path}</span>}
                        <span className={`shrink-0 text-sm text-fg-3 ${grid ? "" : "w-20 text-right"}`}>{ago(f.openedAt)}</span>
                      </button>
                    )}
                    <span className={grid ? "absolute top-1 right-1 flex" : "flex"}>
                      <button
                        type="button"
                        aria-label={`Archive ${f.name}`}
                        title="Archive: move the file to the Archive folder and off this list"
                        onClick={() => remove(f, "archive")}
                        className="ml-1 grid size-7 shrink-0 place-items-center rounded-control text-fg-3 opacity-0 group-hover:opacity-100 hover:bg-line hover:text-fg focus-visible:opacity-100"
                      >
                        <Archive size={14} strokeWidth={1.5} />
                      </button>
                      <button
                        type="button"
                        aria-label={`Move ${f.name} to the Trash`}
                        title="Delete: move the file to the Trash and off this list"
                        onClick={() => remove(f, "trash")}
                        className="mr-1 grid size-7 shrink-0 place-items-center rounded-control text-fg-3 opacity-0 group-hover:opacity-100 hover:bg-line hover:text-fg focus-visible:opacity-100"
                      >
                        <Trash2 size={14} strokeWidth={1.5} />
                      </button>
                    </span>
                  </li>
                </FileMenu>
              );
            })}
          </ul>
        )}
      </div>
      <MoveDialog
        conn={props.conn}
        file={moving}
        onClose={() => setMoving(null)}
        onMoved={() => {
          showToast(`Moved ${moving?.name}`);
          setMoving(null);
          refresh();
        }}
      />
      <ConnectDialog conn={props.conn} open={connect} onOpenChange={setConnect} />
      {dragging && (
        <div className="pointer-events-none absolute inset-4 grid place-items-center rounded-popover border-2 border-dashed border-fg text-base">
          Drop a .spark file to open it
        </div>
      )}
    </main>
  );
}
