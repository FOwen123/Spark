import * as Dialog from "@radix-ui/react-dialog";
import { Search, X } from "lucide-react";
import { useState } from "react";
import { KEYMAP, comboLabel } from "../lib/keymap";

/** Section a command is listed under: the first group with a matching prefix. */
const GROUPS: [string, string[]][] = [
  ["Tools", ["tool."]],
  ["Tabs and files", ["tab.", "app."]],
  ["View", ["zoom", "panel."]],
  ["Arrange", ["bring", "send", "nudge"]],
  ["Edit", ["undo", "redo", "duplicate", "delete", "rename", "group", "addFlex", "toggle", "select", "escape"]],
  ["Export and code", ["export", "copy"]],
];

const LABELS: Record<string, string> = {
  zoomFit: "Zoom to fit",
  zoomSelection: "Zoom to selection",
  zoom100: "Zoom to 100%",
  escape: "Deselect",
  "tab.last": "Go to last tab",
  "tab.next": "Next tab",
  "tab.prev": "Previous tab",
  "tab.new": "New file tab",
  "tab.reopen": "Reopen closed tab",
  "app.home": "Go to Home",
  "app.shortcuts": "Keyboard shortcuts",
  "panel.agent": "Agent panel",
};

function label(cmd: string): string {
  const [head, rest = ""] = cmd.split(".");
  if (LABELS[cmd]) return LABELS[cmd];
  if (head === "tool") return `${rest[0].toUpperCase()}${rest.slice(1)} tool`;
  if (cmd === "tab.1") return "Go to tab 1–8";
  if (head.startsWith("nudge")) return `Nudge ${rest}${head === "nudgeBig" ? " (big)" : ""}`;
  return head.replace(/([A-Z0-9])/g, " $1").replace(/^./, (c) => c.toUpperCase());
}

function describe(cmd: string): { group: string; label: string } {
  const group = GROUPS.find(([, prefixes]) => prefixes.some((p) => cmd.startsWith(p)))?.[0] ?? "Other";
  return { group, label: label(cmd) };
}

/** Every shortcut in the keymap, grouped, with the key labels for this platform. */
function entries() {
  const byCommand = new Map<string, string[]>();
  for (const [combo, cmd] of Object.entries(KEYMAP)) byCommand.set(cmd, [...(byCommand.get(cmd) ?? []), comboLabel(combo)]);
  // Tabs 1–8 share one row.
  return [...byCommand]
    .filter(([cmd]) => !/^tab\.[2-8]$/.test(cmd))
    .map(([cmd, keys]) => ({ cmd, ...describe(cmd), keys: cmd === "tab.1" ? [`${keys[0]}–8`] : keys }));
}

export function ShortcutsDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const [q, setQ] = useState("");
  const needle = q.trim().toLowerCase();
  const shown = entries().filter((e) => !needle || `${e.group} ${e.label} ${e.keys.join(" ")}`.toLowerCase().includes(needle));
  const groups = [...GROUPS.map(([g]) => g), "Other"].map((g) => [g, shown.filter((e) => e.group === g)] as const).filter(([, list]) => list.length);
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-bg/60" />
        <Dialog.Content
          aria-describedby={undefined}
          className="fixed top-1/2 left-1/2 z-50 flex h-[min(640px,85vh)] w-[min(560px,92vw)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-popover border border-line bg-panel shadow-popover"
        >
          <div className="flex h-11 shrink-0 items-center gap-2 border-b border-line px-3">
            <Dialog.Title className="text-base font-medium">Keyboard shortcuts</Dialog.Title>
            <div className="flex-1" />
            <Dialog.Close aria-label="Close" className="grid size-7 place-items-center rounded-control hover:bg-hover">
              <X size={14} />
            </Dialog.Close>
          </div>
          <label className="flex h-9 shrink-0 items-center gap-2 border-b border-line px-3 text-fg-3">
            <Search size={14} strokeWidth={1.5} />
            <input
              // biome-ignore lint/a11y/noAutofocus: the search is the point of the dialog
              autoFocus
              value={q}
              onChange={(e) => setQ(e.target.value)}
              placeholder="Search shortcuts"
              aria-label="Search shortcuts"
              className="min-w-0 flex-1 bg-transparent text-base text-fg outline-none placeholder:text-fg-3"
            />
          </label>
          <div className="min-h-0 flex-1 overflow-auto px-3 pb-3 spark-scroll">
            {groups.length === 0 && <p className="py-6 text-center text-base text-fg-3">No shortcuts match.</p>}
            {groups.map(([group, list]) => (
              <section key={group} aria-label={group}>
                <h3 className="mt-3 mb-1 text-sm font-medium text-fg-3 uppercase">{group}</h3>
                {list.map((e) => (
                  <div key={e.cmd} className="flex h-7 items-center gap-3 text-base">
                    <span className="flex-1 truncate">{e.label}</span>
                    {e.keys.map((k) => (
                      <kbd key={k} className="rounded border border-line px-1.5 font-mono text-xs text-fg-2">
                        {k}
                      </kbd>
                    ))}
                  </div>
                ))}
              </section>
            ))}
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
