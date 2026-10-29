import { Fragment } from "react";
import { Frame, Hand, Image, MousePointer2, Square, Type } from "lucide-react";

export type Tool = "move" | "frame" | "rect" | "text" | "image" | "hand";

const TOOLS: { id: Tool; label: string; key: string; Icon: typeof Frame }[] = [
  { id: "move", label: "Move", key: "V", Icon: MousePointer2 },
  { id: "hand", label: "Hand", key: "H", Icon: Hand },
  { id: "frame", label: "Frame", key: "F", Icon: Frame },
  { id: "rect", label: "Rectangle", key: "R", Icon: Square },
  { id: "text", label: "Text", key: "T", Icon: Type },
  { id: "image", label: "Image", key: "I", Icon: Image },
];

export function Toolbar({ tool, onTool }: { tool: Tool; onTool: (t: Tool) => void }) {
  return (
    <nav aria-label="Tools" className="flex w-[var(--toolbar-w)] shrink-0 flex-col items-center gap-0.5 border-r border-line bg-panel py-1">
      {TOOLS.map(({ id, label, key, Icon }) => (
        <Fragment key={id}>
          {/* Navigation tools sit above the tools that create layers. */}
          {id === "frame" && <div className="my-1 h-px w-5 bg-line" />}
          <button
            type="button"
            title={`${label} (${key})`}
            aria-label={label}
            aria-pressed={tool === id}
            onClick={() => onTool(id)}
            className="grid h-8 w-9 place-items-center"
          >
            <span className={`grid size-7 place-items-center rounded-control ${tool === id ? "bg-selected text-selected-fg" : "text-fg-2 hover:bg-hover"}`}>
              <Icon size={16} strokeWidth={1.5} />
            </span>
          </button>
        </Fragment>
      ))}
    </nav>
  );
}
