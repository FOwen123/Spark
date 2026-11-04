import * as Menu from "@radix-ui/react-dropdown-menu";
import { Check, ChevronDown } from "lucide-react";
import { comboFor } from "../lib/keymap";
import { CANVAS_PREFS, setPrefs, usePrefs } from "../lib/prefs";
import { menuItem } from "./CanvasMenu";
import { Keycaps } from "./Tip";

const ITEMS: [string, string][] = [
  ["Zoom in", "zoomIn"],
  ["Zoom out", "zoomOut"],
  ["Zoom to fit", "zoomFit"],
  ["Zoom to selection", "zoomSelection"],
  ["Zoom to 100%", "zoom100"],
];

/** The zoom readout in the inspector header, with zoom actions and canvas preferences. */
export function ZoomMenu({ zoom, run }: { zoom: number; run: (cmd: string) => void }) {
  const p = usePrefs();
  return (
    <Menu.Root>
      <Menu.Trigger data-testid="zoom" className="flex h-6 items-center gap-1 rounded-control px-2 font-mono text-sm hover:bg-hover data-[state=open]:bg-hover">
        {Math.round(zoom * 100)}%
        <ChevronDown size={12} />
      </Menu.Trigger>
      <Menu.Portal>
        <Menu.Content align="end" sideOffset={4} aria-label="Zoom menu" className="z-50 min-w-[230px] rounded-popover border border-line bg-panel p-1 shadow-popover">
          {ITEMS.map(([label, cmd]) => {
            const combo = comboFor(cmd);
            return (
              <Menu.Item key={cmd} className={menuItem} onSelect={() => run(cmd)}>
                <span className="flex-1">{label}</span>
                {combo && <Keycaps combo={combo} />}
              </Menu.Item>
            );
          })}
          <Menu.Separator className="my-1 h-px bg-line" />
          {CANVAS_PREFS.map(([key, label]) => (
            <Menu.CheckboxItem key={key} checked={p[key] as boolean} onCheckedChange={(v) => setPrefs({ [key]: v })} onSelect={(e) => e.preventDefault()} className={menuItem}>
              <span className="grid size-3 place-items-center">
                <Menu.ItemIndicator>
                  <Check size={12} />
                </Menu.ItemIndicator>
              </span>
              {label}
            </Menu.CheckboxItem>
          ))}
        </Menu.Content>
      </Menu.Portal>
    </Menu.Root>
  );
}
