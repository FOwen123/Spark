import { FilePlus, FolderOpen, Plug } from "lucide-react";
import { ConnectDialog } from "./ConnectDialog";
import { useEffect, useRef, useState } from "react";
import type { Conn } from "../lib/conn";
import type { FileInfo } from "../lib/types";
import { SparkLogo } from "./SparkLogo";

type Recent = { id: string; name: string; path: string; openedAt: number };

function ago(ms: number) {
  const s = Math.max(0, (Date.now() - ms) / 1000);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  return new Date(ms).toLocaleDateString();
}

export function Home(props: { conn: Conn; token: string; onOpen: (ref: string) => void; onNew: () => void; error: string | null }) {
  const [recent, setRecent] = useState<Recent[]>([]);
  const [dragging, setDragging] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [connect, setConnect] = useState(false);
  const [native, setNative] = useState(false);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    props.conn.request<{ recent: Recent[]; native: boolean }>({ t: "hello" }).then((r) => {
      setRecent(r.recent);
      setNative(r.native);
    });
  }, [props.conn]);

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
        <h2 className="mt-12 mb-2 text-sm font-medium text-fg-3 uppercase">Recent</h2>
        {recent.length === 0 ? (
          <p className="text-base text-fg-3">No files yet. Create one, or drop a .spark file here.</p>
        ) : (
          <ul className="divide-y divide-line border-y border-line" aria-label="Recent files">
            {recent.map((f) => (
              <li key={f.id}>
                <button type="button" onClick={() => props.onOpen(f.id)} className="flex w-full items-center gap-3 px-2 py-2.5 text-left hover:bg-hover">
                  <span className="flex-1 truncate text-base">{f.name}</span>
                  <span className="max-w-[320px] truncate font-mono text-xs text-fg-3">{f.path}</span>
                  <span className="w-20 shrink-0 text-right text-sm text-fg-3">{ago(f.openedAt)}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
      <ConnectDialog conn={props.conn} open={connect} onOpenChange={setConnect} />
      {dragging && (
        <div className="pointer-events-none absolute inset-4 grid place-items-center rounded-popover border-2 border-dashed border-fg text-base">
          Drop a .spark file to open it
        </div>
      )}
    </main>
  );
}
