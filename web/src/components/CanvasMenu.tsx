import * as ContextMenu from "@radix-ui/react-context-menu";
import { ChevronRight } from "lucide-react";
import type { DocStore } from "../lib/doc";
import { comboFor, comboLabel } from "../lib/keymap";

const item = "flex h-6 cursor-default items-center gap-4 rounded-control px-2 text-sm outline-none data-[disabled]:text-fg-3 data-[highlighted]:bg-selected data-[highlighted]:text-selected-fg";
const sep = <ContextMenu.Separator className="my-1 h-px bg-line" />;

function Item(props: { label: string; cmd?: string; shortcut?: string; onSelect: () => void; disabled?: boolean }) {
  const combo = props.shortcut ?? (props.cmd ? comboFor(props.cmd) : undefined);
  return (
    <ContextMenu.Item className={item} onSelect={props.onSelect} disabled={props.disabled}>
      <span className="flex-1">{props.label}</span>
      {combo && <span className="font-mono text-xs opacity-60">{comboLabel(combo)}</span>}
    </ContextMenu.Item>
  );
}

export function CanvasMenu(props: {
  store: DocStore;
  children: React.ReactNode;
  run: (cmd: string) => void;
  onPaste: (here: boolean) => void;
  onCopy: (cut: boolean) => void;
  onCode: (format: "html" | "jsx" | "tailwind") => void;
}) {
  const { store, run } = props;
  const sel = store.selection;
  const none = sel.length === 0;
  const nodes = sel.map((id) => store.nodes.get(id));
  const hidden = nodes.length > 0 && nodes.every((n) => n?.hidden);
  const locked = nodes.length > 0 && nodes.every((n) => n?.locked);
  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild>{props.children}</ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className="z-50 min-w-[220px] rounded-popover border border-line bg-panel p-1 shadow-popover" aria-label="Canvas menu">
          <Item label="Copy" shortcut="mod+c" onSelect={() => props.onCopy(false)} disabled={none} />
          <Item label="Paste" shortcut="mod+v" onSelect={() => props.onPaste(false)} />
          <Item label="Paste here" onSelect={() => props.onPaste(true)} />
          <Item label="Duplicate" cmd="duplicate" onSelect={() => run("duplicate")} disabled={none} />
          <Item label="Delete" shortcut="backspace" onSelect={() => run("delete")} disabled={none} />
          {sep}
          <Item label="Bring forward" cmd="bringForward" onSelect={() => run("bringForward")} disabled={none} />
          <Item label="Send backward" cmd="sendBackward" onSelect={() => run("sendBackward")} disabled={none} />
          <Item label="Bring to front" cmd="bringToFront" onSelect={() => run("bringToFront")} disabled={none} />
          <Item label="Send to back" cmd="sendToBack" onSelect={() => run("sendToBack")} disabled={none} />
          {sep}
          <Item label="Group in frame" cmd="groupInFrame" onSelect={() => run("groupInFrame")} disabled={none} />
          <Item label="Add flex layout" cmd="addFlex" onSelect={() => run("addFlex")} disabled={none} />
          {sep}
          <ContextMenu.Sub>
            <ContextMenu.SubTrigger className={item} disabled={none}>
              <span className="flex-1">Copy as</span>
              <ChevronRight size={12} />
            </ContextMenu.SubTrigger>
            <ContextMenu.Portal>
              <ContextMenu.SubContent className="z-50 min-w-[180px] rounded-popover border border-line bg-panel p-1 shadow-popover">
                <Item label="HTML" onSelect={() => props.onCode("html")} />
                <Item label="JSX" cmd="copyJsx" onSelect={() => props.onCode("jsx")} />
                <Item label="Tailwind" cmd="copyTailwind" onSelect={() => props.onCode("tailwind")} />
              </ContextMenu.SubContent>
            </ContextMenu.Portal>
          </ContextMenu.Sub>
          <Item label="Export…" cmd="export" onSelect={() => run("export")} disabled={none} />
          {sep}
          <Item label={hidden ? "Show" : "Hide"} cmd="toggleHidden" onSelect={() => run("toggleHidden")} disabled={none} />
          <Item label={locked ? "Unlock" : "Lock"} cmd="toggleLocked" onSelect={() => run("toggleLocked")} disabled={none} />
          <Item label="Rename" cmd="rename" onSelect={() => run("rename")} disabled={sel.length !== 1} />
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}
