import * as ContextMenu from "@radix-ui/react-context-menu";
import { ChevronRight } from "lucide-react";
import { textIds } from "../lib/clipboard";
import type { DocStore } from "../lib/doc";
import { comboFor, comboLabel } from "../lib/keymap";
import { KindIcon } from "./LeftPanel";

export const menuItem = "flex h-6 cursor-default items-center gap-4 rounded-control px-2 text-sm outline-none data-[disabled]:text-fg-3 data-[highlighted]:bg-selected data-[highlighted]:text-selected-fg";
const ALIGNS = [["left", "Left"], ["hcenter", "Horizontal center"], ["right", "Right"], ["top", "Top"], ["vmiddle", "Vertical middle"], ["bottom", "Bottom"]];
const sep = <ContextMenu.Separator className="my-1 h-px bg-line" />;

export function Item(props: { label: string; cmd?: string; shortcut?: string; onSelect: () => void; disabled?: boolean }) {
  const combo = props.shortcut ?? (props.cmd ? comboFor(props.cmd) : undefined);
  return (
    <ContextMenu.Item className={menuItem} onSelect={props.onSelect} disabled={props.disabled}>
      <span className="flex-1">{props.label}</span>
      {combo && <span className="font-mono text-xs opacity-60">{comboLabel(combo)}</span>}
    </ContextMenu.Item>
  );
}

function Sub(props: { label: string; disabled?: boolean; children: React.ReactNode }) {
  return (
    <ContextMenu.Sub>
      <ContextMenu.SubTrigger className={menuItem} disabled={props.disabled}>
        <span className="flex-1">{props.label}</span>
        <ChevronRight size={12} />
      </ContextMenu.SubTrigger>
      <ContextMenu.Portal>
        <ContextMenu.SubContent className="z-50 min-w-[180px] rounded-popover border border-line bg-panel p-1 shadow-popover">{props.children}</ContextMenu.SubContent>
      </ContextMenu.Portal>
    </ContextMenu.Sub>
  );
}

const ADJUST: [string, string, number][] = [
  ["Increase font size", "fontSize", 1],
  ["Decrease font size", "fontSize", -1],
  ["Heavier weight", "fontWeight", 1],
  ["Lighter weight", "fontWeight", -1],
  ["Increase line height", "lineHeight", 1],
  ["Decrease line height", "lineHeight", -1],
  ["Increase letter spacing", "letterSpacing", 1],
  ["Decrease letter spacing", "letterSpacing", -1],
];

/** Right-click menu for layers. `under` lists the layers at the click point; `empty` means the click hit nothing. */
export function CanvasMenu(props: {
  store: DocStore;
  children: React.ReactNode;
  run: (cmd: string) => void;
  onPaste: (here: boolean) => void;
  onCopy: (cut: boolean) => void;
  onCode: (format: "html" | "jsx" | "tailwind") => void;
  under?: string[];
  empty?: boolean;
}) {
  const { store, run } = props;
  const sel = store.selection;
  const content = "z-50 min-w-[220px] rounded-popover border border-line bg-panel p-1 shadow-popover";
  const boards = store.kids(store.page);
  // Selects the next or previous artboard (wrapping) and brings it into view.
  const stepBoard = (d: number) => {
    const at = boards.indexOf(sel[0] ? (store.artboardOf(sel[0]) ?? "") : "");
    const next = boards[(at < 0 ? (d > 0 ? 0 : boards.length - 1) : at + d + boards.length) % boards.length];
    if (!next) return;
    store.select([next]);
    run("zoomSelection");
  };
  if (props.empty) {
    return (
      <ContextMenu.Root>
        <ContextMenu.Trigger asChild>{props.children}</ContextMenu.Trigger>
        <ContextMenu.Portal>
          <ContextMenu.Content className={content} aria-label="Canvas menu">
            <Item label="Paste" shortcut="mod+v" onSelect={() => props.onPaste(false)} />
            <Item label="Paste here" onSelect={() => props.onPaste(true)} />
            {sep}
            <Item label="Select next artboard" onSelect={() => stepBoard(1)} disabled={!boards.length} />
            <Item label="Select previous artboard" onSelect={() => stepBoard(-1)} disabled={!boards.length} />
            <Item label="Collapse all layers" onSelect={() => window.dispatchEvent(new CustomEvent("spark:collapse-layers"))} />
          </ContextMenu.Content>
        </ContextMenu.Portal>
      </ContextMenu.Root>
    );
  }
  const first = store.nodes.get(sel[0]);
  const imageHash = first?.style.fills?.find((f) => f.type === "image")?.asset;
  const hasText = textIds(store, sel).length > 0;
  const none = sel.length === 0;
  const nodes = sel.map((id) => store.nodes.get(id));
  const hidden = nodes.length > 0 && nodes.every((n) => n?.hidden);
  const locked = nodes.length > 0 && nodes.every((n) => n?.locked);
  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild>{props.children}</ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className={content} aria-label="Canvas menu">
          {!!props.under?.length && (
            <>
              <Sub label="Select layer">
                {props.under.map((id) => {
                  const n = store.nodes.get(id);
                  return (
                    n && (
                      <ContextMenu.Item key={id} className={menuItem} onSelect={() => store.select([id])}>
                        <KindIcon n={n} />
                        <span className="flex-1 truncate">{n.name}</span>
                      </ContextMenu.Item>
                    )
                  );
                })}
              </Sub>
              {sep}
            </>
          )}
          <Item label="Copy" shortcut="mod+c" onSelect={() => props.onCopy(false)} disabled={none} />
          <Item label="Paste" shortcut="mod+v" onSelect={() => props.onPaste(false)} />
          <Item label="Paste here" onSelect={() => props.onPaste(true)} />
          <Item label="Paste on top" cmd="pasteOnTop" onSelect={() => run("pasteOnTop")} />
          <Item label="Paste to replace" cmd="pasteReplace" onSelect={() => run("pasteReplace")} disabled={none} />
          <Item label="Copy styles" cmd="copyStyle" onSelect={() => run("copyStyle")} disabled={sel.length !== 1} />
          <Item label="Paste styles" cmd="pasteStyle" onSelect={() => run("pasteStyle")} disabled={none} />
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
          <Item label="Ungroup" cmd="ungroup" onSelect={() => run("ungroup")} disabled={!nodes.some((n) => n?.kind === "frame")} />
          {sep}
          <Item label="Select parent" cmd="selectParent" onSelect={() => run("selectParent")} disabled={none} />
          <Item label="Select children" cmd="selectChildren" onSelect={() => run("selectChildren")} disabled={none} />
          <ContextMenu.Sub>
            <ContextMenu.SubTrigger className={menuItem} disabled={none}>
              <span className="flex-1">Align</span>
              <ChevronRight size={12} />
            </ContextMenu.SubTrigger>
            <ContextMenu.Portal>
              <ContextMenu.SubContent className="z-50 min-w-[180px] rounded-popover border border-line bg-panel p-1 shadow-popover">
                {ALIGNS.map(([dir, label]) => (
                  <Item key={dir} label={label} cmd={`align.${dir}`} onSelect={() => run(`align.${dir}`)} />
                ))}
              </ContextMenu.SubContent>
            </ContextMenu.Portal>
          </ContextMenu.Sub>
          {sep}
          <Sub label="Copy as" disabled={none}>
            <Item label="HTML" onSelect={() => props.onCode("html")} />
            <Item label="JSX" cmd="copyJsx" onSelect={() => props.onCode("jsx")} />
            <Item label="Tailwind" cmd="copyTailwind" onSelect={() => props.onCode("tailwind")} />
            <Item label="PNG" cmd="copyPng" onSelect={() => run("copyPng")} />
            <Item label="SVG" onSelect={() => run("copySvg")} />
          </Sub>
          <Item label="Copy link" onSelect={() => run("copyLink")} disabled={none} />
          {hasText && (
            <Sub label="Adjust text">
              {ADJUST.map(([label, prop, dir]) => (
                <Item key={label} label={label} onSelect={() => run(`adjustText.${prop}:${dir}`)} />
              ))}
            </Sub>
          )}
          {imageHash && <Item label="Open image in new tab" onSelect={() => window.open(`/assets/${imageHash}`, "_blank")} />}
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
