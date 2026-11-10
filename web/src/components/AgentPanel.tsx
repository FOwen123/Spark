import { Bot, Play, Plug, Square, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { DocStore } from "../lib/doc";
import { isActive, startRun, stopRun, useAgentRun } from "../lib/runs";
import type { Agent } from "../lib/types";
import { ConnectDialog } from "./ConnectDialog";

type Cli = { id: string; name: string; connected: boolean; needs: string | null };
export type ToolCall = { agent: string; tool: string; ms: number; ok: boolean; error?: string; at: number };

/** Readable lines from an agent's output (Claude Code's stream-json events, or plain text). */
function readable(line: string): string | null {
  if (!line.startsWith("{")) return line;
  try {
    const e = JSON.parse(line);
    if (e.type === "assistant") {
      return (e.message?.content ?? [])
        .map((c: { type: string; text?: string; name?: string }) => (c.type === "text" ? c.text : c.type === "tool_use" ? `▸ ${String(c.name).replace(/^mcp__spark__/, "")}` : ""))
        .filter(Boolean)
        .join("\n");
    }
    if (e.type === "result") return e.result ? `✓ ${e.result}` : null;
    return null;
  } catch {
    return line;
  }
}

export function AgentPanel({ store, agents, calls, onClose }: { store: DocStore; agents: Agent[]; calls: ToolCall[]; onClose: () => void }) {
  const [clis, setClis] = useState<Cli[]>([]);
  const [cli, setCli] = useState("");
  const [prompt, setPrompt] = useState("");
  const run = useAgentRun(store.file.id);
  const running = isActive(run);
  const [connect, setConnect] = useState(false);
  const out = useRef<HTMLDivElement>(null);

  useEffect(() => {
    store.conn.request<Cli[]>({ t: "agentDetect" }).then((list) => {
      setClis(list);
      setCli((c) => c || list[0]?.id || "");
    });
  }, [store]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: scroll to the newest line
  useEffect(() => {
    out.current?.scrollTo({ top: out.current.scrollHeight });
  }, [run?.lines]);

  const chosen = clis.find((c) => c.id === cli);
  const start = () => {
    if (prompt.trim() && chosen && !running) startRun(store.conn, store.file.id, cli, prompt, chosen.name);
  };

  return (
    <aside aria-label="Agent panel" className="flex w-[320px] shrink-0 flex-col border-l border-line bg-panel">
      <div className="flex h-[var(--inspector-header-h)] shrink-0 items-center gap-2 border-b border-line px-3">
        <Bot size={15} strokeWidth={1.5} />
        <h2 className="flex-1 text-base font-medium">Agents</h2>
        <button type="button" aria-label="Close agent panel" onClick={onClose} className="grid size-6 place-items-center rounded-control hover:bg-hover">
          <X size={14} />
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto spark-scroll">
        <section className="border-b border-line p-3">
          <h3 className="mb-2 text-base font-medium">Connected</h3>
          {agents.length === 0 ? <p className="text-sm text-fg-3">No agent is connected right now.</p> : null}
          <ul className="flex flex-col gap-1">
            {agents.map((a) => (
              <li key={a.id} className="flex items-center gap-2 text-base">
                <Bot size={13} strokeWidth={1.5} /> {a.name}
                <span className="text-sm text-fg-3">{a.transport}</span>
              </li>
            ))}
          </ul>
          <button type="button" onClick={() => setConnect(true)} className="mt-2 flex h-7 w-full items-center justify-center gap-1.5 rounded-control border border-line text-sm hover:bg-hover">
            <Plug size={13} /> Connect your agent
          </button>
        </section>
        <section className="border-b border-line p-3">
          <h3 className="mb-2 text-base font-medium">Run an agent here</h3>
          {clis.length === 0 ? (
            <p className="text-sm text-fg-3">No agent CLI found on your PATH (claude, codex, opencode, gemini, cursor-agent).</p>
          ) : (
            <>
              <select aria-label="Agent CLI" value={cli} onChange={(e) => setCli(e.target.value)} className="mb-2 h-6 w-full rounded-control bg-control px-1 text-sm">
                {clis.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.name}
                  </option>
                ))}
              </select>
              {chosen && !chosen.connected && <p className="mb-2 text-sm text-fg-2">Connect {chosen.name} first so it can reach Spark.</p>}
              <textarea
                aria-label="Prompt"
                value={prompt}
                onChange={(e) => setPrompt(e.target.value)}
                onKeyDown={(e) => {
                  e.stopPropagation();
                  if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) start();
                }}
                placeholder="Design a pricing page with three plans…"
                className="h-20 w-full resize-none rounded-control bg-control p-2 text-base outline-none"
              />
              <button
                type="button"
                onClick={() => (running ? stopRun(store.conn, store.file.id) : start())}
                className="mt-1 flex h-7 w-full items-center justify-center gap-1.5 rounded-control bg-selected text-sm text-selected-fg"
              >
                {running ? <Square size={12} /> : <Play size={12} />} {running ? "Stop" : "Run"}
              </button>
            </>
          )}
          {run && (
            <div ref={out} data-testid="agent-output" className="mt-2 max-h-64 overflow-auto rounded-control bg-control p-2 font-mono text-xs leading-4 whitespace-pre-wrap select-text spark-scroll">
              {run.lines.map((l, i) => {
                const text = l.kind === "out" ? readable(l.text) : l.text;
                return (
                  text && (
                    // biome-ignore lint/suspicious/noArrayIndexKey: an append-only log
                    <div key={i} className={l.kind === "err" ? "text-fg-2" : l.kind === "info" ? "text-fg-3" : ""}>
                      {text}
                    </div>
                  )
                );
              })}
            </div>
          )}
        </section>
        <section className="p-3">
          <h3 className="mb-2 text-base font-medium">Tool calls</h3>
          {calls.length === 0 && <p className="text-sm text-fg-3">Calls agents make to Spark appear here.</p>}
          <ol className="flex flex-col gap-0.5" aria-label="Tool calls">
            {[...calls].reverse().map((c) => (
              <li key={`${c.at}-${c.tool}`} className="flex items-baseline gap-2 font-mono text-xs" title={c.error}>
                <span className={c.ok ? "" : "line-through"}>{c.tool}</span>
                <span className="truncate text-fg-3">{c.agent}</span>
                <span className="ml-auto shrink-0 text-fg-3">{c.ms} ms</span>
              </li>
            ))}
          </ol>
        </section>
      </div>
      <ConnectDialog conn={store.conn} open={connect} onOpenChange={setConnect} />
    </aside>
  );
}
