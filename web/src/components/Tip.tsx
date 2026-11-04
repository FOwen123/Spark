import * as Tooltip from "@radix-ui/react-tooltip";
import { comboLabel, isMac } from "../lib/keymap";

/** A shortcut as separate keycaps: "⌘⇧Z" on macOS, "Ctrl+Shift+Z" elsewhere. */
export function Keycaps({ combo, className = "" }: { combo: string; className?: string }) {
  const label = comboLabel(combo);
  const keys = isMac ? [...label] : label.split("+");
  return (
    <span className={`flex gap-0.5 ${className}`}>
      {keys.map((k, i) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: a combo can repeat a key
        <kbd key={i} className="grid h-4 min-w-4 place-items-center rounded border border-current/30 px-1 font-sans text-xs leading-none opacity-70">
          {k}
        </kbd>
      ))}
    </span>
  );
}

/** Styled tooltip with the name and shortcut keycaps. */
export function Tip({ label, combo, side = "right", children }: { label: string; combo?: string; side?: "right" | "bottom"; children: React.ReactNode }) {
  return (
    <Tooltip.Provider delayDuration={400}>
      <Tooltip.Root>
        <Tooltip.Trigger asChild>{children}</Tooltip.Trigger>
        <Tooltip.Portal>
          <Tooltip.Content side={side} sideOffset={6} className="z-50 flex items-center gap-2 rounded-control bg-selected px-2 py-1 text-sm text-selected-fg shadow-popover">
            {label}
            {combo && <Keycaps combo={combo} />}
          </Tooltip.Content>
        </Tooltip.Portal>
      </Tooltip.Root>
    </Tooltip.Provider>
  );
}
