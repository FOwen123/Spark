import * as Dialog from "@radix-ui/react-dialog";
import { Check, Copy, X } from "lucide-react";
import { useState } from "react";

/** Shows generated code with a Copy button (copying from a click keeps clipboard access reliable). */
export function CodeDialog(props: { title: string; code: string | null; onClose: () => void; tabs?: { label: string; active: boolean; onClick: () => void }[] }) {
  const [copied, setCopied] = useState(false);
  return (
    <Dialog.Root open={props.code !== null} onOpenChange={(o) => !o && props.onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-bg/60" />
        <Dialog.Content className="fixed top-1/2 left-1/2 z-50 flex max-h-[80vh] w-[min(720px,90vw)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-popover border border-line bg-panel shadow-popover">
          <div className="flex h-11 items-center gap-2 border-b border-line px-3">
            <Dialog.Title className="text-base font-medium">{props.title}</Dialog.Title>
            <div className="flex gap-1">
              {props.tabs?.map((t) => (
                <button key={t.label} type="button" onClick={t.onClick} className={`h-6 rounded-control px-2 text-sm ${t.active ? "bg-selected text-selected-fg" : "hover:bg-hover"}`}>
                  {t.label}
                </button>
              ))}
            </div>
            <div className="flex-1" />
            <button
              type="button"
              onClick={async () => {
                await navigator.clipboard.writeText(props.code ?? "").catch(() => {});
                setCopied(true);
                setTimeout(() => setCopied(false), 1200);
              }}
              className="flex h-7 items-center gap-1.5 rounded-control bg-selected px-2.5 text-sm text-selected-fg"
            >
              {copied ? <Check size={13} /> : <Copy size={13} />} {copied ? "Copied" : "Copy"}
            </button>
            <Dialog.Close aria-label="Close" className="grid size-7 place-items-center rounded-control hover:bg-hover">
              <X size={14} />
            </Dialog.Close>
          </div>
          <Dialog.Description className="sr-only">Generated code</Dialog.Description>
          <pre data-testid="code" className="min-h-0 flex-1 overflow-auto p-3 font-mono text-sm leading-5 whitespace-pre select-text spark-scroll">
            {props.code}
          </pre>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
