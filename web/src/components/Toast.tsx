import { useEffect, useState } from "react";

/** The one toast: a short message that fades after a few seconds. Messages ending in "…" stay until replaced. */
export function Toast({ children }: { children: string | null }) {
  return children ? (
    <output data-testid="toast" className="fixed bottom-6 left-1/2 z-50 max-w-[80vw] -translate-x-1/2 truncate rounded-popover border border-line bg-panel px-3 py-2 text-base shadow-popover">
      {children}
    </output>
  ) : null;
}

/** App-wide toasts from `showToast`. */
export function AppToast() {
  const [text, setText] = useState<string | null>(null);
  useEffect(() => {
    const on = (e: Event) => setText((e as CustomEvent<string>).detail);
    window.addEventListener("spark:toast", on);
    return () => window.removeEventListener("spark:toast", on);
  }, []);
  useEffect(() => {
    if (!text) return;
    const t = setTimeout(() => setText(null), 2500);
    return () => clearTimeout(t);
  }, [text]);
  return <Toast>{text}</Toast>;
}
