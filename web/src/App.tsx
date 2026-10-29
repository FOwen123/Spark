import { useCallback, useEffect, useState } from "react";
import type { ToolCall } from "./components/AgentPanel";
import { AppBar } from "./components/AppBar";
import { Editor } from "./components/Editor";
import { Home } from "./components/Home";
import { sessionToken } from "./lib/auth";
import { Conn } from "./lib/conn";
import { DocStore } from "./lib/doc";
import { pendingReveal } from "./lib/doc";
import { renderToPng } from "./lib/render";
import type { Agent, FileInfo, Patch, Snapshot } from "./lib/types";

const token = sessionToken();
const conn = new Conn(`${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/ui?token=${encodeURIComponent(token)}`);
const TABS_KEY = "spark-tabs";

function initialTheme(): "light" | "dark" {
  const saved = localStorage.getItem("spark-theme");
  if (saved === "light" || saved === "dark") return saved;
  return matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function App() {
  const [stores, setStores] = useState<Map<string, DocStore>>(new Map());
  const [tabs, setTabs] = useState<string[]>([]);
  const [active, setActive] = useState<string>("home");
  const [error, setError] = useState<string | null>(null);
  const [saveErrors, setSaveErrors] = useState<Record<string, string>>({});
  const [theme, setTheme] = useState(initialTheme);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [calls, setCalls] = useState<ToolCall[]>([]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem("spark-theme", theme);
    window.dispatchEvent(new Event("spark:theme"));
  }, [theme]);

  const openFile = useCallback(async (ref: string, activate = true, tab = true) => {
    try {
      const snap = await conn.request<Snapshot>({ t: "open", ref });
      setStores((prev) => {
        const next = new Map(prev);
        const existing = next.get(snap.file.id);
        if (existing) existing.applySnapshot(snap);
        else next.set(snap.file.id, new DocStore(conn, snap));
        return next;
      });
      if (tab) setTabs((t) => (t.includes(snap.file.id) ? t : [...t, snap.file.id]));
      if (activate) setActive(snap.file.id);
      setError(null);
      return snap;
    } catch (e) {
      setError((e as Error).message);
      return null;
    }
  }, []);

  const closeTab = useCallback(
    (id: string) => {
      conn.send({ t: "close", file: id });
      setTabs((t) => t.filter((x) => x !== id));
      setStores((prev) => {
        const next = new Map(prev);
        next.delete(id);
        return next;
      });
      setActive((a) => (a === id ? "home" : a));
    },
    [],
  );

  const newFile = useCallback(async () => {
    const info = await conn.request<FileInfo>({ t: "create", name: "Untitled" });
    await openFile(info.id);
  }, [openFile]);

  // Restore tabs from the last session, then open `?open=<id, route, link or path>` if given.
  useEffect(() => {
    const saved = JSON.parse(localStorage.getItem(TABS_KEY) ?? "{}") as { tabs?: string[]; active?: string };
    const requested = new URLSearchParams(location.search).get("open");
    (async () => {
      for (const id of saved.tabs ?? []) await openFile(id, false);
      if (requested) {
        history.replaceState(null, "", location.pathname);
        // The server opens it and tells every window to show it (with page and node for links).
        await conn.request({ t: "reveal", ref: requested }).catch((e: Error) => setError(e.message));
      } else if (saved.active) setActive(saved.active);
    })();
  }, [openFile]);

  useEffect(() => {
    if (tabs.length || active !== "home") localStorage.setItem(TABS_KEY, JSON.stringify({ tabs, active }));
    else localStorage.removeItem(TABS_KEY);
  }, [tabs, active]);

  // Tell the server which file the user is looking at (agents default to it).
  useEffect(() => {
    conn.send({ t: "view", file: active === "home" ? null : active });
  }, [active]);

  useEffect(() => {
    conn.request<{ agents: Agent[] }>({ t: "hello" }).then((r) => setAgents(r.agents ?? []));
    const off = conn.on((m) => {
      const store = stores.get(m.file as string);
      if (m.t === "patch") store?.applyPatch(m as unknown as Patch);
      if (m.t === "selection") store?.setSelectionLocal(m.selection as string[]);
      if (m.t === "agents") setAgents(m.agents as Agent[]);
      if (m.t === "toolCall") setCalls((c) => [...c.slice(-199), { ...(m as unknown as ToolCall), at: Date.now() }]);
      if (m.t === "activity") window.dispatchEvent(new CustomEvent("spark:sweep", { detail: m }));
      if (m.t === "saveError") {
        setSaveErrors(({ [m.file as string]: _, ...rest }) =>
          typeof m.message === "string" ? { ...rest, [m.file as string]: m.message } : rest,
        );
      }
      if (m.t === "openTab") {
        // Deep links name a node to zoom to once the canvas has laid out.
        if (typeof m.node === "string" && m.node) pendingReveal.set(m.file as string, m.node);
        openFile(m.file as string, !!m.activate);
      }
      if (m.t === "menu") {
        if (m.cmd === "closeTab") {
          if (active !== "home") closeTab(active);
        } else window.dispatchEvent(new CustomEvent("spark:menu", { detail: m.cmd }));
      }
      if (m.t === "render") {
        const call = m.callId;
        (async () => {
          const file = m.file as string;
          let target = stores.get(file);
          if (!target) {
            const snap = await openFile(file, false, false);
            target = snap ? new DocStore(conn, snap) : undefined;
          }
          if (!target) throw new Error("could not open the file to render it");
          return renderToPng(target, m.node as string, Number(m.scale) || 1);
        })()
          .then((data) => conn.send({ t: "rendered", callId: call, ok: true, data }))
          .catch((e: Error) => conn.send({ t: "rendered", callId: call, ok: false, error: e.message }));
      }
    });
    const offStatus = conn.onStatus((up) => {
      if (!up) return;
      for (const id of stores.keys()) openFile(id, false);
      conn.send({ t: "view", file: active === "home" ? null : active });
    });
    return () => {
      off();
      offStatus();
    };
  }, [stores, openFile, active, closeTab]);

  const store = active === "home" ? undefined : stores.get(active);
  return (
    <div className="flex h-full flex-col bg-bg text-fg">
      <AppBar
        tabs={tabs.map((id) => stores.get(id)).filter((s): s is DocStore => !!s)}
        active={active}
        onSelect={setActive}
        onClose={closeTab}
        onNew={newFile}
        theme={theme}
        onTheme={() => setTheme(theme === "dark" ? "light" : "dark")}
        agents={agents}
        conn={conn}
      />
      {store && saveErrors[store.file.id] && (
        <div role="alert" className="shrink-0 border-b border-line bg-panel px-3 py-1.5 text-sm">
          Spark couldn't save this file: {saveErrors[store.file.id]}. Your changes are kept while Spark stays open and
          saving retries with your next edit.
        </div>
      )}
      {store ? (
        <Editor key={store.file.id} store={store} agents={agents} calls={calls} />
      ) : (
        <Home conn={conn} token={token} onOpen={openFile} onNew={newFile} error={error} />
      )}
    </div>
  );
}
