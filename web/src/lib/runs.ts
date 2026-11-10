// Agent CLI runs started from this window, one per file. They live here rather than in the agent
// panel because the panel unmounts when it closes or the tab changes, while the process keeps going.

import { useSyncExternalStore } from "react";
import type { Conn, ServerMsg } from "./conn";

export type RunLine = { kind: "out" | "err" | "info"; text: string };
export type RunStatus = "starting" | "running" | "stopping" | "exited" | "stopped" | "failed";
export type AgentRun = { id?: number; status: RunStatus; lines: RunLine[] };

const runs = new Map<string, AgentRun>();
/** Messages for a run whose id hasn't come back yet (output can beat the reply). */
const early = new Map<number, ServerMsg[]>();
const subs = new Set<() => void>();
let listening: Conn | null = null;

export const isActive = (r: AgentRun | undefined) => !!r && (r.status === "starting" || r.status === "running" || r.status === "stopping");

function set(file: string, run: AgentRun) {
  runs.set(file, run);
  for (const fn of subs) fn();
}

function onMessage(m: ServerMsg) {
  if (m.t !== "run") return;
  const id = m.runId as number;
  const found = [...runs].find(([, r]) => r.id === id);
  if (!found) {
    early.set(id, [...(early.get(id) ?? []), m]);
    return;
  }
  const [file, r] = found;
  if (m.kind === "done") {
    const stopped = r.status === "stopping";
    const text = stopped ? "Stopped." : m.code === 0 ? "Finished." : `Exited with code ${m.code ?? "?"}.`;
    set(file, { ...r, status: stopped ? "stopped" : "exited", lines: [...r.lines, { kind: "info", text }] });
  } else {
    set(file, { ...r, lines: [...r.lines.slice(-500), { kind: m.kind === "err" ? "err" : "out", text: String(m.line) }] });
  }
}

export async function startRun(conn: Conn, file: string, agent: string, prompt: string, name: string) {
  if (listening !== conn) {
    listening = conn;
    conn.on(onMessage);
  }
  set(file, { status: "starting", lines: [{ kind: "info", text: `Running ${name}…` }] });
  try {
    const { runId } = await conn.request<{ runId: number }>({ t: "agentRun", file, agent, prompt });
    set(file, { ...(runs.get(file) as AgentRun), id: runId, status: "running" });
    for (const m of early.get(runId) ?? []) onMessage(m);
    early.delete(runId);
  } catch (e) {
    set(file, { status: "failed", lines: [{ kind: "err", text: (e as Error).message }] });
  }
}

export function stopRun(conn: Conn, file: string) {
  const r = runs.get(file);
  if (r?.id === undefined || r.status !== "running") return;
  set(file, { ...r, status: "stopping" });
  conn.request({ t: "agentStop", runId: r.id }).catch(() => {});
}

export function useAgentRun(file: string): AgentRun | undefined {
  return useSyncExternalStore(
    (fn) => {
      subs.add(fn);
      return () => subs.delete(fn);
    },
    () => runs.get(file),
  );
}
