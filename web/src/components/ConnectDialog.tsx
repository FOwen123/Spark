import * as Dialog from "@radix-ui/react-dialog";
import { Check, Copy, Ellipsis, Minus, Plus, X } from "lucide-react";
import { useEffect, useState } from "react";
import antigravity from "../assets/agent-logos/antigravity.svg";
import claude from "../assets/agent-logos/claude.svg";
import cursor from "../assets/agent-logos/cursor.svg";
import copilot from "../assets/agent-logos/githubcopilot.svg";
import openai from "../assets/agent-logos/openai.svg";
import opencode from "../assets/agent-logos/opencode.svg";
import vscode from "../assets/agent-logos/vscode.svg";
import type { Conn } from "../lib/conn";

/** `path` and `snippet` describe setting the agent up by hand; Claude Desktop has neither. */
type Status = { id: string; name: string; connected: boolean; installed: boolean; path: string | null; snippet: string | null };
type Change = { agent: string; path: string; changed: boolean; diff: string; note?: string };

/** The agents the dialog lists, in order. "Claude" covers Claude Code and, when installed, Claude Desktop. */
const ORDER = ["claude-code", "codex", "cursor", "opencode", "antigravity", "copilot", "vscode", "other"];
const NAMES: Record<string, string> = { "claude-code": "Claude" };
const LOGOS: Record<string, string> = { "claude-code": claude, codex: openai, cursor, opencode, antigravity, copilot, vscode };

const PROMPTS = [
  "What's on the canvas in Spark right now?",
  "Design a pricing page with three plans in Spark.",
  "Tidy up the spacing in the frame I have selected in Spark.",
];

/** Brand marks render in the text color, like the rest of Spark's grayscale chrome. */
function AgentLogo({ id }: { id: string }) {
  const src = LOGOS[id];
  if (!src) return <Ellipsis aria-hidden size={16} strokeWidth={1.5} className="shrink-0" />;
  return <span aria-hidden className="size-4 shrink-0 bg-current" style={{ mask: `url("${src}") center / contain no-repeat` }} />;
}

function CopyButton({ text, label }: { text: string; label: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={async () => {
        await navigator.clipboard.writeText(text).catch(() => {});
        setCopied(true);
        setTimeout(() => setCopied(false), 1200);
      }}
      className="grid size-7 shrink-0 place-items-center rounded-control text-fg-2 hover:bg-hover hover:text-fg"
    >
      {copied ? <Check size={14} /> : <Copy size={14} />}
    </button>
  );
}

function Snippet({ text }: { text: string }) {
  return (
    <div className="relative rounded-control bg-control">
      <pre className="p-3 pr-10 font-mono text-sm leading-5 whitespace-pre-wrap text-fg [overflow-wrap:anywhere] select-text">{text}</pre>
      <div className="absolute top-1 right-1">
        <CopyButton text={text} label="Copy config" />
      </div>
    </div>
  );
}

function Step(props: { n: number; done?: boolean; title: string; children: React.ReactNode; last?: boolean }) {
  return (
    <li className="relative flex gap-3 pb-6 last:pb-0">
      {!props.last && <span aria-hidden className="absolute top-7 bottom-1 left-[11px] w-px bg-line" />}
      <span
        className={`grid size-6 shrink-0 place-items-center rounded-full text-sm font-medium ${props.done ? "bg-selected text-selected-fg" : "border border-line text-fg-2"}`}
      >
        {props.done ? <Check size={13} strokeWidth={2.5} /> : props.n}
      </span>
      <div className="min-w-0 flex-1 pt-0.5">
        <h3 className="text-base font-medium">{props.title}</h3>
        <div className="mt-2 flex flex-col gap-2.5 text-base text-fg-2">{props.children}</div>
      </div>
    </li>
  );
}

/** Sets an agent up to use Spark over MCP: one click, or the exact config to add by hand. */
export function ConnectDialog({ conn, open, onOpenChange }: { conn: Conn; open: boolean; onOpenChange: (o: boolean) => void }) {
  const [statuses, setStatuses] = useState<Status[]>([]);
  const [selected, setSelected] = useState("claude-code");
  const [manual, setManual] = useState(false);
  const [results, setResults] = useState<Change[]>([]);
  const [error, setError] = useState<string | null>(null);
  const refresh = () => conn.request<Status[]>({ t: "connectStatus" }).then(setStatuses);
  // biome-ignore lint/correctness/useExhaustiveDependencies: reload the list each time the dialog opens
  useEffect(() => {
    if (open) refresh();
  }, [open]);
  const byId = new Map(statuses.map((s) => [s.id, s]));
  const agents = ORDER.flatMap((id) => {
    const s = byId.get(id);
    return s ? [{ ...s, name: NAMES[id] ?? s.name }] : [];
  });
  const desktop = byId.get("claude-desktop");
  const pick = (id: string) => {
    setSelected(id);
    setManual(false);
    setResults([]);
    setError(null);
  };
  // Connecting Claude also installs the Claude Desktop extension when that app is there.
  const run = async (id: string, remove: boolean) => {
    setError(null);
    const ids = id === "claude-code" && desktop && (remove ? desktop.connected : desktop.installed) ? [id, "claude-desktop"] : [id];
    const done: Change[] = [];
    try {
      for (const agent of ids) done.push(await conn.request<Change>({ t: "connect", agent, remove }));
    } catch (e) {
      setError((e as Error).message);
    }
    setResults(done);
    refresh();
  };
  const a = agents.find((x) => x.id === selected);
  const other = a?.id === "other";
  const withDesktop = a?.id === "claude-code" && desktop?.installed;
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-bg/60" />
        <Dialog.Content className="fixed top-1/2 left-1/2 z-50 flex h-[min(600px,88vh)] w-[min(800px,94vw)] -translate-x-1/2 -translate-y-1/2 flex-col overflow-hidden rounded-popover border border-line bg-panel shadow-popover">
          <div className="flex shrink-0 items-start gap-3 border-b border-line px-5 py-4">
            <div className="flex-1">
              <Dialog.Title className="text-[15px] font-medium">Connect your agent</Dialog.Title>
              <Dialog.Description className="mt-0.5 text-base text-fg-2">
                Agents work in Spark through MCP. Pick yours to add Spark to it.
              </Dialog.Description>
            </div>
            <Dialog.Close aria-label="Close" className="-mr-1.5 grid size-7 place-items-center rounded-control text-fg-2 hover:bg-hover hover:text-fg">
              <X size={15} />
            </Dialog.Close>
          </div>
          <div className="flex min-h-0 flex-1">
            <nav aria-label="Agents" className="flex w-52 shrink-0 flex-col gap-0.5 overflow-auto border-r border-line p-2 spark-scroll">
              {agents.map((x) => (
                <button
                  key={x.id}
                  type="button"
                  data-testid={`connect-${x.id}`}
                  aria-current={x.id === selected}
                  onClick={() => pick(x.id)}
                  className={`flex h-8 shrink-0 items-center gap-2.5 rounded-control px-2.5 text-left text-base ${x.id === selected ? "bg-hover font-medium text-fg" : "text-fg-2 hover:bg-hover hover:text-fg"}`}
                >
                  <AgentLogo id={x.id} />
                  <span className="flex-1 truncate">{x.name}</span>
                  {x.connected && <Check size={13} aria-label="Connected" className="shrink-0 text-fg" />}
                </button>
              ))}
            </nav>
            {a && (
              <ol className="min-w-0 flex-1 overflow-auto px-6 py-5 spark-scroll" data-testid="connect-steps">
                <Step n={1} done={a.connected} title={other ? "Add Spark to your agent's MCP config" : `Connect to ${a.name}`}>
                  {other ? (
                    <>
                      <p>Most agents read MCP servers from a JSON file. Add this entry to yours:</p>
                      {a.snippet && <Snippet text={a.snippet} />}
                      <p className="text-sm">Agents name this file and its keys differently, so check your agent's docs and adjust the shape if needed.</p>
                    </>
                  ) : (
                    <>
                      <div className="flex items-center gap-3 rounded-control border border-line py-2.5 pr-2.5 pl-3.5">
                        <p className="flex-1 text-fg">
                          {withDesktop ? "Add Spark to Claude Code and Claude" : `Add Spark to ${a.id === "claude-code" ? "Claude Code" : a.name}`}
                        </p>
                        <button
                          type="button"
                          onClick={() => run(a.id, a.connected)}
                          className={`flex h-7 shrink-0 items-center gap-2 rounded-control px-3 text-sm ${a.connected ? "border border-line text-fg hover:bg-hover" : "bg-selected text-selected-fg"}`}
                        >
                          {!a.connected && <AgentLogo id={a.id} />}
                          {a.connected ? "Disconnect" : `Add to ${a.name}`}
                        </button>
                      </div>
                      {error && <p className="text-fg">⚠ {error}</p>}
                      {results.length > 0 && (
                        <div data-testid="connect-result" className="flex flex-col gap-2">
                          {results.map((r) => (
                            <div key={r.agent}>
                              <p className="text-fg">
                                {r.changed ? "Updated" : "No change needed in"} <span className="font-mono text-sm break-all">{r.path}</span>
                              </p>
                              {r.note && <p className="mt-1 text-sm">{r.note}</p>}
                              {r.diff && (
                                <pre className="mt-2 max-h-40 overflow-auto rounded-control bg-control p-2 font-mono text-xs whitespace-pre text-fg select-text spark-scroll">{r.diff}</pre>
                              )}
                            </div>
                          ))}
                        </div>
                      )}
                      {a.snippet && (
                        <div className="rounded-control border border-line" data-testid="connect-manual">
                          <button
                            type="button"
                            aria-expanded={manual}
                            onClick={() => setManual(!manual)}
                            className="flex h-10 w-full items-center px-3.5 text-left hover:text-fg"
                          >
                            <span className="flex-1">Or, set it up manually</span>
                            {manual ? <Minus size={14} /> : <Plus size={14} />}
                          </button>
                          {manual && (
                            <div className="flex flex-col gap-2 px-3.5 pb-3.5">
                              <p className="text-sm">
                                Add this to <code className="font-mono text-fg">{a.path}</code>, keeping anything already in the file:
                              </p>
                              <Snippet text={a.snippet} />
                            </div>
                          )}
                        </div>
                      )}
                    </>
                  )}
                </Step>
                <Step n={2} title={other ? "Restart your agent" : `Restart ${a.name}`}>
                  <p>Agents load MCP servers when they start. When it connects, it's listed in Spark's agent panel.</p>
                </Step>
                <Step n={3} title="Try a first prompt" last>
                  <p>Keep Spark open with a file, then ask your agent:</p>
                  <ul className="flex flex-col gap-1.5">
                    {PROMPTS.map((p) => (
                      <li key={p} className="flex items-center gap-2 rounded-control border border-line py-0.5 pr-0.5 pl-3 text-fg">
                        <span className="flex-1">{p}</span>
                        <CopyButton text={p} label="Copy prompt" />
                      </li>
                    ))}
                  </ul>
                </Step>
              </ol>
            )}
          </div>
          <div className="flex shrink-0 justify-end border-t border-line px-5 py-3">
            <Dialog.Close className="h-8 rounded-control bg-selected px-4 text-base text-selected-fg">Done</Dialog.Close>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
