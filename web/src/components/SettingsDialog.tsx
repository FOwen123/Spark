import * as Dialog from "@radix-ui/react-dialog";
import { Minus, Settings, X } from "lucide-react";
import { useEffect, useState } from "react";
import type { Conn } from "../lib/conn";
import { CANVAS_PREFS, setPrefs, usePrefs } from "../lib/prefs";

type Provider = {
  id: string;
  name: string;
  needsKey: boolean;
  hasKey: boolean;
  enabled: boolean;
  model?: string;
  baseUrl?: string;
  costPerImage?: number;
  defaultModel: string;
  defaultBase: string;
  defaultCost: number;
};
type State = { grantedDirs: string[]; perCallCap: number; monthlyBudget: number | null; spent: number; providers: Provider[] };

const input = "h-7 min-w-0 rounded-control bg-control px-2 text-sm outline-none";

/** Canvas behavior, stored on this device (see lib/prefs.ts). */
function CanvasPrefs() {
  const p = usePrefs();
  const nudge = (key: "nudgeSmall" | "nudgeLarge", label: string) => (
    <label className="flex flex-col gap-1">
      {label}
      <input
        aria-label={label}
        type="number"
        min={0.01}
        step={1}
        value={p[key]}
        className={input}
        onChange={(e) => Number(e.target.value) > 0 && setPrefs({ [key]: Number(e.target.value) })}
      />
    </label>
  );
  return (
    <section aria-label="Canvas" className="mb-5">
      <h3 className="mb-2 text-base font-medium">Canvas</h3>
      <div className="grid grid-cols-2 gap-x-4 gap-y-1.5">
        {CANVAS_PREFS.map(([key, label]) => (
          <label key={key} className="flex items-center gap-2 text-sm">
            <input type="checkbox" checked={p[key] as boolean} onChange={(e) => setPrefs({ [key]: e.target.checked })} />
            {label}
          </label>
        ))}
      </div>
      <div className="mt-3 grid grid-cols-2 gap-3 text-sm text-fg-2">
        {nudge("nudgeSmall", "Nudge step (px)")}
        {nudge("nudgeLarge", "Big nudge step, with Shift (px)")}
      </div>
    </section>
  );
}

export function SettingsButton({ conn }: { conn: Conn }) {
  const [open, setOpen] = useState(false);
  const [s, setS] = useState<State | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [newDir, setNewDir] = useState("");
  const load = () => conn.request<State>({ t: "settingsGet" }).then(setS);
  // The file menu opens this dialog by command.
  useEffect(() => {
    const on = (e: Event) => (e as CustomEvent<string>).detail === "showSettings" && setOpen(true);
    window.addEventListener("spark:command", on);
    return () => window.removeEventListener("spark:command", on);
  }, []);
  // biome-ignore lint/correctness/useExhaustiveDependencies: reload each time the dialog opens
  useEffect(() => {
    if (open) load();
  }, [open]);
  const save = async (msg: Record<string, unknown>) => {
    setError(null);
    try {
      await conn.request({ t: "settingsSet", ...msg });
      await load();
    } catch (e) {
      setError((e as Error).message);
    }
  };
  const saveProvider = (id: string, providerSettings: Record<string, unknown>) => save({ provider: id, providerSettings });
  return (
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Dialog.Trigger aria-label="Settings" className="grid size-7 place-items-center rounded-control text-fg-3 hover:bg-hover hover:text-fg">
        <Settings size={15} strokeWidth={1.5} />
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-bg/60" />
        <Dialog.Content className="fixed top-1/2 left-1/2 z-50 flex max-h-[85vh] w-[min(640px,92vw)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-popover border border-line bg-panel shadow-popover">
          <div className="flex h-11 items-center border-b border-line px-4">
            <Dialog.Title className="flex-1 text-base font-medium">Settings</Dialog.Title>
            <Dialog.Close aria-label="Close" className="grid size-7 place-items-center rounded-control hover:bg-hover">
              <X size={14} />
            </Dialog.Close>
          </div>
          <Dialog.Description className="sr-only">Image generation and file access settings</Dialog.Description>
          {s && (
            <div className="min-h-0 overflow-auto p-4 spark-scroll">
              <CanvasPrefs />
              {error && <p className="mb-3 text-base text-fg-2">⚠ {error}</p>}
              <h3 className="text-base font-medium">Image providers</h3>
              <p className="mt-1 mb-2 text-sm text-fg-2">
                Agents can generate images with providers you enable, using your own keys. Keys are stored in your system keychain, never in files. Agents can always fill image requests with images they make themselves.
              </p>
              <ul className="divide-y divide-line rounded-control border border-line">
                {s.providers.map((p) => (
                  <li key={p.id} className="flex flex-col gap-2 p-3" data-testid={`provider-${p.id}`}>
                    <label className="flex items-center gap-2 text-base">
                      <input
                        type="checkbox"
                        checked={p.enabled}
                        onChange={(e) => {
                          const enabled = e.target.checked;
                          setS({ ...s, providers: s.providers.map((x) => (x.id === p.id ? { ...x, enabled } : x)) });
                          saveProvider(p.id, { enabled });
                        }}
                        aria-label={`Enable ${p.name}`}
                      />
                      {p.name}
                      {p.needsKey && <span className="ml-auto text-sm text-fg-3">{p.hasKey ? "Key saved" : "No key"}</span>}
                    </label>
                    {p.enabled && (
                      <div className="grid grid-cols-2 gap-2">
                        {p.needsKey && (
                          <input
                            type="password"
                            aria-label={`${p.name} API key`}
                            placeholder={p.hasKey ? "Replace key…" : "API key"}
                            className={`${input} col-span-2`}
                            onBlur={(e) => e.target.value && saveProvider(p.id, { key: e.target.value }).then(() => (e.target.value = ""))}
                            onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
                          />
                        )}
                        <input aria-label={`${p.name} model`} placeholder={p.defaultModel || "Model"} defaultValue={p.model ?? ""} className={input} onBlur={(e) => saveProvider(p.id, { model: e.target.value })} />
                        <input aria-label={`${p.name} base URL`} placeholder={p.defaultBase} defaultValue={p.baseUrl ?? ""} className={input} onBlur={(e) => saveProvider(p.id, { baseUrl: e.target.value })} />
                        <label className="col-span-2 flex items-center gap-2 text-sm text-fg-2">
                          Cost per image (USD)
                          <input
                            aria-label={`${p.name} cost per image`}
                            type="number"
                            min={0}
                            step={0.01}
                            placeholder={String(p.defaultCost)}
                            defaultValue={p.costPerImage ?? ""}
                            className={`${input} w-24`}
                            onBlur={(e) => saveProvider(p.id, { costPerImage: e.target.value === "" ? null : Number(e.target.value) })}
                          />
                        </label>
                      </div>
                    )}
                  </li>
                ))}
              </ul>
              <h3 className="mt-5 text-base font-medium">Limits</h3>
              <div className="mt-2 grid grid-cols-2 gap-3 text-sm text-fg-2">
                <label className="flex flex-col gap-1">
                  Images per agent call
                  <input aria-label="Images per call" type="number" min={1} max={100} defaultValue={s.perCallCap} className={input} onBlur={(e) => save({ perCallCap: Number(e.target.value) })} />
                </label>
                <label className="flex flex-col gap-1">
                  Monthly budget (USD, empty for none)
                  <input aria-label="Monthly budget" type="number" min={0} step={1} defaultValue={s.monthlyBudget ?? ""} className={input} onBlur={(e) => save({ monthlyBudget: e.target.value === "" ? null : Number(e.target.value) })} />
                </label>
              </div>
              <p className="mt-2 text-sm text-fg-3">Spent this month: about ${s.spent.toFixed(2)}</p>
              <h3 className="mt-5 text-base font-medium">Folders agents can read images from</h3>
              <p className="mt-1 mb-2 text-sm text-fg-2">Agents may place images from these folders with spark-asset:// paths or insert_image. Nothing outside them is readable.</p>
              <ul className="flex flex-col gap-1" aria-label="Granted folders">
                {s.grantedDirs.map((d) => (
                  <li key={d} className="flex items-center gap-2 font-mono text-sm">
                    <span className="flex-1 truncate">{d}</span>
                    <button type="button" aria-label={`Remove ${d}`} onClick={() => save({ grantedDirs: s.grantedDirs.filter((x) => x !== d) })} className="grid size-6 place-items-center rounded-control hover:bg-hover">
                      <Minus size={13} />
                    </button>
                  </li>
                ))}
              </ul>
              <form
                className="mt-2 flex gap-2"
                onSubmit={(e) => {
                  e.preventDefault();
                  if (newDir.trim()) save({ grantedDirs: [...s.grantedDirs, newDir.trim()] }).then(() => setNewDir(""));
                }}
              >
                <input aria-label="Folder path" placeholder="/absolute/path/to/folder" value={newDir} onChange={(e) => setNewDir(e.target.value)} className={`${input} flex-1 font-mono`} />
                <button type="submit" className="h-7 rounded-control border border-line px-3 text-sm hover:bg-hover">
                  Add folder
                </button>
              </form>
            </div>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
