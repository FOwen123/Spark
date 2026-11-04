import * as ContextMenu from "@radix-ui/react-context-menu";
import { FileText, Moon, Plus, Sun, X } from "lucide-react";
import { useState } from "react";
import { useDoc, type DocStore } from "../lib/doc";
import type { Conn } from "../lib/conn";
import { desktop, isMac } from "../lib/keymap";
import { showToast } from "../lib/toast";
import type { Agent } from "../lib/types";
import { Item } from "./CanvasMenu";
import { Rename } from "./LeftPanel";
import { SettingsButton } from "./SettingsDialog";
import { SparkLogo } from "./SparkLogo";

/** A small 2×3 dot cluster that pulses while an agent works in the file. */
export function BusyDots() {
  return (
    <span className="grid shrink-0 grid-cols-2 gap-[2px]" role="img" aria-label="Agent working">
      {[0, 1, 2, 3, 4, 5].map((i) => (
        <span key={i} className="size-[3px] animate-pulse rounded-full bg-current" style={{ animationDelay: `${(i * 137) % 600}ms` }} />
      ))}
    </span>
  );
}

type TabDrag = { id: string; over: string | null; after: boolean } | null;

function Tab(props: {
  store: DocStore;
  active: boolean;
  busy: boolean;
  /** Ids of all tabs in order, for the menu's close commands. */
  order: string[];
  drag: TabDrag;
  setDrag: (d: TabDrag) => void;
  onSelect: () => void;
  onClose: () => void;
  onCloseMany: (ids: string[], keep?: string) => void;
  onReorder: (id: string, over: string, after: boolean) => void;
}) {
  const { store, active, order, drag } = props;
  useDoc(store);
  const [renaming, setRenaming] = useState(false);
  const id = store.file.id;
  const at = order.indexOf(id);
  const mark = drag?.over === id && drag.id !== id ? (drag.after ? "shadow-[inset_-2px_0_0_var(--spark-fg)]" : "shadow-[inset_2px_0_0_var(--spark-fg)]") : "";
  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild>
        <div
          role="tab"
          aria-selected={active}
          data-testid="file-tab"
          tabIndex={0}
          draggable={!renaming}
          onMouseDown={props.onSelect}
          onKeyDown={(e) => e.key === "Enter" && !renaming && props.onSelect()}
          onDragStart={(e) => {
            e.dataTransfer.effectAllowed = "move";
            e.dataTransfer.setData("text/plain", id);
            props.setDrag({ id, over: null, after: false });
          }}
          onDragOver={(e) => {
            if (!drag) return;
            e.preventDefault();
            const r = e.currentTarget.getBoundingClientRect();
            props.setDrag({ ...drag, over: id, after: e.clientX > r.left + r.width / 2 });
          }}
          onDrop={(e) => {
            e.preventDefault();
            const source = e.dataTransfer.getData("text/plain");
            const r = e.currentTarget.getBoundingClientRect();
            if (order.includes(source) && source !== id) props.onReorder(source, id, e.clientX > r.left + r.width / 2);
            props.setDrag(null);
          }}
          onDragEnd={() => props.setDrag(null)}
          className={`group flex h-7 max-w-[200px] cursor-default items-center gap-2 rounded-control px-2.5 text-base ${mark} ${active ? "bg-hover text-fg" : "text-fg-3 hover:text-fg-2"}`}
        >
          {props.busy ? <BusyDots /> : <FileText size={14} strokeWidth={1.5} className="shrink-0" />}
          {renaming ? (
            <Rename
              value={store.file.name}
              onDone={(v) => {
                setRenaming(false);
                if (v?.trim() && v.trim() !== store.file.name) store.do({ type: "renameFile", name: v.trim() });
              }}
            />
          ) : (
            <span className="truncate">{store.file.name}</span>
          )}
          <button
            type="button"
            aria-label={`Close ${store.file.name}`}
            onMouseDown={(e) => e.stopPropagation()}
            onClick={props.onClose}
            className={`-mr-1 grid size-4 shrink-0 place-items-center rounded hover:bg-line ${active ? "" : "invisible group-hover:visible"}`}
          >
            <X size={12} />
          </button>
        </div>
      </ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className="z-50 min-w-[200px] rounded-popover border border-line bg-panel p-1 shadow-popover" aria-label="Tab menu" onCloseAutoFocus={(e) => e.preventDefault()}>
          <Item label="Rename" onSelect={() => setRenaming(true)} />
          <Item
            label="Copy link"
            onSelect={() =>
              navigator.clipboard.writeText(`spark://file/${id}`).then(
                () => showToast("Link copied"),
                () => showToast("Couldn't copy the link"),
              )
            }
          />
          <ContextMenu.Separator className="my-1 h-px bg-line" />
          <Item label="Close" onSelect={props.onClose} />
          <Item label="Close others" onSelect={() => props.onCloseMany(order.filter((x) => x !== id), id)} disabled={order.length < 2} />
          <Item label="Close tabs to the right" onSelect={() => props.onCloseMany(order.slice(at + 1), id)} disabled={at === order.length - 1} />
          <Item label="Close all" onSelect={() => props.onCloseMany(order)} />
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}

export function AppBar(props: {
  tabs: DocStore[];
  active: string;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onCloseMany: (ids: string[], keep?: string) => void;
  onReorder: (id: string, over: string, after: boolean) => void;
  onNew: () => void;
  theme: string;
  onTheme: () => void;
  agents: Agent[];
  conn: Conn;
}) {
  const [drag, setDrag] = useState<TabDrag>(null);
  const order = props.tabs.map((s) => s.file.id);
  return (
    // In the macOS desktop app this bar is the title bar: empty space drags the window and the left edge
    // leaves room for the window buttons.
    <header
      data-tauri-drag-region
      className={`flex h-[var(--appbar-h)] shrink-0 items-center gap-1 border-b border-line bg-panel px-2 ${desktop && isMac ? "pl-[76px]" : ""}`}
      role="tablist"
    >
      <button
        type="button"
        role="tab"
        aria-label="Home"
        aria-selected={props.active === "home"}
        onMouseDown={() => props.onSelect("home")}
        className={`grid h-7 w-8 place-items-center rounded-control ${props.active === "home" ? "bg-hover text-fg" : "text-fg-3 hover:text-fg-2"}`}
      >
        <SparkLogo size={20} className="text-fg" />
      </button>
      <div className="mx-1 h-4 w-px bg-line" />
      {props.tabs.map((s) => (
        <Tab
          key={s.file.id}
          store={s}
          active={props.active === s.file.id}
          busy={props.agents.some((a) => (a.files[s.file.id]?.length ?? 0) > 0)}
          order={order}
          drag={drag}
          setDrag={setDrag}
          onSelect={() => props.onSelect(s.file.id)}
          onClose={() => props.onClose(s.file.id)}
          onCloseMany={props.onCloseMany}
          onReorder={props.onReorder}
        />
      ))}
      <button type="button" aria-label="New file" onClick={props.onNew} className="grid size-7 place-items-center rounded-control text-fg-3 hover:bg-hover hover:text-fg">
        <Plus size={15} strokeWidth={1.5} />
      </button>
      <div data-tauri-drag-region className="flex-1 self-stretch" />
      <SettingsButton conn={props.conn} />
      <button
        type="button"
        aria-label={props.theme === "dark" ? "Use light theme" : "Use dark theme"}
        onClick={props.onTheme}
        className="grid size-7 place-items-center rounded-control text-fg-3 hover:bg-hover hover:text-fg"
      >
        {props.theme === "dark" ? <Sun size={15} strokeWidth={1.5} /> : <Moon size={15} strokeWidth={1.5} />}
      </button>
    </header>
  );
}
