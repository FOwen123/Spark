import { FileText, Moon, Plus, Sun, X } from "lucide-react";
import { useDoc, type DocStore } from "../lib/doc";
import type { Conn } from "../lib/conn";
import { desktop, isMac } from "../lib/keymap";
import type { Agent } from "../lib/types";
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

function Tab({ store, active, busy, onSelect, onClose }: { store: DocStore; active: boolean; busy: boolean; onSelect: () => void; onClose: () => void }) {
  useDoc(store);
  return (
    <div
      role="tab"
      aria-selected={active}
      data-testid="file-tab"
      tabIndex={0}
      onMouseDown={onSelect}
      onKeyDown={(e) => e.key === "Enter" && onSelect()}
      className={`group flex h-7 max-w-[200px] cursor-default items-center gap-2 rounded-control px-2.5 text-base ${active ? "bg-hover text-fg" : "text-fg-3 hover:text-fg-2"}`}
    >
      {busy ? <BusyDots /> : <FileText size={14} strokeWidth={1.5} className="shrink-0" />}
      <span className="truncate">{store.file.name}</span>
      <button
        type="button"
        aria-label={`Close ${store.file.name}`}
        onMouseDown={(e) => e.stopPropagation()}
        onClick={onClose}
        className={`-mr-1 grid size-4 shrink-0 place-items-center rounded hover:bg-line ${active ? "" : "invisible group-hover:visible"}`}
      >
        <X size={12} />
      </button>
    </div>
  );
}

export function AppBar(props: {
  tabs: DocStore[];
  active: string;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onNew: () => void;
  theme: string;
  onTheme: () => void;
  agents: Agent[];
  conn: Conn;
}) {
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
          onSelect={() => props.onSelect(s.file.id)}
          onClose={() => props.onClose(s.file.id)}
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
