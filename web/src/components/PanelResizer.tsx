import { setPrefs, usePrefs } from "../lib/prefs";

const LIMITS = { left: [160, 480], right: [280, 480] } as const;

/** Drag strip on a side panel's inner edge. Dragging sets the CSS variable directly and saves the width on release. */
export function PanelResizer({ side }: { side: "left" | "right" }) {
  const key = side === "left" ? "leftW" : "rightW";
  const [min, max] = LIMITS[side];
  const width = usePrefs()[key];
  const clamp = (w: number) => Math.round(Math.min(max, Math.max(min, w)));
  return (
    // biome-ignore lint/a11y/useSemanticElements: a focusable separator is the ARIA pattern for a splitter; <hr> can't take focus
    <div
      role="separator"
      tabIndex={0}
      aria-orientation="vertical"
      aria-label={side === "left" ? "Resize pages and layers" : "Resize inspector"}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={width}
      onPointerDown={(e) => {
        const el = e.currentTarget;
        const panel = (el.parentElement as HTMLElement).getBoundingClientRect();
        let w = panel.width;
        el.setPointerCapture(e.pointerId);
        const move = (ev: PointerEvent) => {
          w = clamp(side === "left" ? ev.clientX - panel.left : panel.right - ev.clientX);
          document.documentElement.style.setProperty(`--${side}-w`, `${w}px`);
        };
        el.addEventListener("pointermove", move);
        el.addEventListener("lostpointercapture", () => {
          el.removeEventListener("pointermove", move);
          setPrefs({ [key]: w });
        }, { once: true });
      }}
      onKeyDown={(e) => {
        const step = (e.key === "ArrowRight" ? 8 : e.key === "ArrowLeft" ? -8 : 0) * (side === "left" ? 1 : -1);
        if (!step) return;
        e.preventDefault();
        e.stopPropagation();
        setPrefs({ [key]: clamp(width + step) });
      }}
      className={`absolute inset-y-0 z-10 w-1.5 cursor-col-resize outline-none after:absolute after:inset-y-0 after:w-px hover:after:bg-fg focus-visible:after:bg-fg active:after:bg-fg ${
        side === "left" ? "right-0 after:right-0" : "left-0 after:left-0"
      }`}
    />
  );
}
