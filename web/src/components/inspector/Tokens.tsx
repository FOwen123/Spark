import { Code, Minus, Pencil, Plus } from "lucide-react";
import { useState } from "react";
import { parseColor, toHex } from "../../lib/color";
import type { DocStore } from "../../lib/doc";
import type { Token } from "../../lib/types";
import { CodeDialog } from "../CodeDialog";
import { IconButton, Select } from "./fields";

const TYPES: [string, string][] = [
  ["--color-", "Color"],
  ["--spacing-", "Spacing"],
  ["--radius-", "Radius"],
  ["--font-", "Font family"],
  ["--text-", "Font size"],
  ["--font-weight-", "Font weight"],
  ["--tracking-", "Letter spacing"],
  ["--leading-", "Line height"],
  ["--opacity-", "Opacity"],
  ["--breakpoint-", "Breakpoint"],
  ["--container-", "Container"],
];

// A bare namespace token such as `--radius` belongs to its namespace's group too.
const typeOf = (name: string) => [...TYPES].sort((a, b) => b[0].length - a[0].length).find(([p]) => name.startsWith(p) || name === p.slice(0, -1))?.[0] ?? "--color-";

function TokenRow({ store, token, theme, tokens }: { store: DocStore; token: Token; theme: string; tokens: Map<string, string> }) {
  const { name } = token;
  // With a theme picked, the row edits that theme's value; empty means it uses the base value.
  const value = theme ? (token.themes?.[theme] ?? "") : token.value;
  const type = typeOf(name);
  // A bare namespace token such as `--spacing` keeps its full name.
  const prefix = name.startsWith(type) ? type : "";
  const color = type === "--color-" ? parseColor(value || token.value, tokens) : null;
  const alias = value.match(/^var\((--[\w-]+)\)$/)?.[1];
  return (
    <div className="group relative flex h-7 items-center gap-1" data-testid="token-row">
      {/* The group heading names the type, so rows show the name without its namespace prefix. */}
      <input
        aria-label="Token name"
        defaultValue={name.slice(prefix.length)}
        key={name}
        title={name}
        className="-ml-1 h-6 w-[45%] shrink-0 rounded-control bg-transparent px-1 font-mono text-sm outline-none hover:bg-control focus:bg-control"
        onBlur={(e) => e.target.value.trim() && prefix + e.target.value.trim() !== name && store.do({ type: "setToken", name, newName: prefix + e.target.value.trim() })}
        onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
      />
      <div className="flex h-6 min-w-0 flex-1 items-center gap-1.5 rounded-control px-1 hover:bg-control focus-within:bg-control">
        {color && <span className="size-3.5 shrink-0 rounded-[3px] border border-line" style={{ background: toHex(color) }} />}
        <input
          aria-label={`${name} value`}
          defaultValue={value}
          key={value}
          placeholder={theme ? token.value : undefined}
          title={alias ? `Alias of ${alias}` : value || token.value}
          className={`min-w-0 flex-1 bg-transparent font-mono text-sm outline-none placeholder:text-fg-3 ${alias ? "italic" : ""}`}
          onBlur={(e) => {
            const v = e.target.value.trim();
            if (v !== value && (v || theme)) store.do({ type: "setToken", name, value: v, theme: theme || undefined });
          }}
          onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
        />
      </div>
      <span className="invisible absolute right-0 rounded-control bg-panel group-hover:visible">
        <IconButton
          label={theme ? `Use the base value of ${name} in ${theme}` : `Delete ${name}`}
          onClick={() => store.do(theme ? { type: "setToken", name, value: "", theme } : { type: "deleteToken", name })}
        >
          <Minus size={13} />
        </IconButton>
      </span>
    </div>
  );
}

/** Theme picker: the base values ("Default") or a named theme, plus new, rename and delete. */
function ThemeBar({ store, theme, themes, setTheme }: { store: DocStore; theme: string; themes: string[]; setTheme: (t: string) => void }) {
  // "new" or "rename" while the name field is open.
  const [naming, setNaming] = useState<"new" | "rename" | null>(null);
  const [error, setError] = useState<string | null>(null);
  if (naming)
    return (
      <form
        className="mt-1 flex flex-col gap-1"
        onSubmit={async (e) => {
          e.preventDefault();
          const name = String(new FormData(e.currentTarget).get("theme")).trim();
          if (!/^[a-z0-9]+(-[a-z0-9]+)*$/.test(name) || name === "default") return setError("Use lowercase letters and digits joined by -, like dark or brand-b.");
          if (naming === "rename") await store.do({ type: "renameTheme", name: theme, newName: name });
          setTheme(name);
          setNaming(null);
          setError(null);
        }}
      >
        <input
          name="theme"
          aria-label="Theme name"
          // biome-ignore lint/a11y/noAutofocus: the field opens on request
          autoFocus
          defaultValue={naming === "rename" ? theme : ""}
          placeholder="dark"
          className="h-6 rounded-control bg-control px-1.5 font-mono text-sm outline-none"
        />
        {error && <p className="text-sm text-fg-2">{error}</p>}
        <div className="flex justify-end gap-1">
          <button type="button" onClick={() => setNaming(null)} className="h-6 rounded-control px-2 text-sm hover:bg-hover">
            Cancel
          </button>
          <button type="submit" className="h-6 rounded-control bg-selected px-2 text-sm text-selected-fg">
            {naming === "new" ? "Add theme" : "Rename"}
          </button>
        </div>
      </form>
    );
  return (
    <div className="mt-1 flex items-center gap-1">
      <Select
        label="Theme"
        className="flex-1"
        value={theme}
        options={[["", "Default"], ...themes.map((t): [string, string] => [t, t]), ["+new", "New theme…"]]}
        onChange={(t) => (t === "+new" ? setNaming("new") : setTheme(t))}
      />
      {theme && (
        <>
          <IconButton label="Rename theme" onClick={() => setNaming("rename")}>
            <Pencil size={13} />
          </IconButton>
          <IconButton
            label="Delete theme"
            onClick={async () => {
              await store.do({ type: "deleteTheme", name: theme });
              setTheme("");
            }}
          >
            <Minus size={13} />
          </IconButton>
        </>
      )}
    </div>
  );
}

export function TokensView({ store }: { store: DocStore }) {
  const [adding, setAdding] = useState(false);
  // "" is the base values. A new theme is listed here until one of its values is set.
  const [theme, setTheme] = useState("");
  const themes = [...new Set([...store.tokens.flatMap((t) => Object.keys(t.themes ?? {})), ...(theme ? [theme] : [])])].sort();
  const [prefix, setPrefix] = useState("--color-");
  const [error, setError] = useState<string | null>(null);
  const [css, setCss] = useState<{ theme: boolean; text: string } | null>(null);
  const tokens = new Map(store.tokens.map((t) => [t.name, (theme && t.themes?.[theme]) || t.value]));
  const groups = TYPES.map(([p, label]) => [label, store.tokens.filter((t) => typeOf(t.name) === p)] as const).filter(([, list]) => list.length);
  const exportCss = async (theme: boolean) => setCss({ theme, text: await store.do<string>({ type: "tokensCss", theme }) });
  return (
    <section aria-label="Tokens" className="pt-2 pr-3 pb-3 pl-5">
      <div className="flex h-7 items-center justify-between">
        <h3 className="text-base font-medium">Tokens</h3>
        <div className="flex">
          <IconButton label="Export tokens as CSS" onClick={() => exportCss(true)}>
            <Code size={14} />
          </IconButton>
          <IconButton label="Add token" onClick={() => setAdding(true)}>
            <Plus size={14} />
          </IconButton>
        </div>
      </div>
      {adding && (
        <form
          className="mt-1 flex flex-col gap-1 rounded-control border border-line p-2"
          onSubmit={async (e) => {
            e.preventDefault();
            const f = new FormData(e.currentTarget);
            const name = `${prefix}${String(f.get("name")).trim()}`;
            try {
              await store.do({ type: "setToken", name, value: String(f.get("value")).trim() });
              setAdding(false);
              setError(null);
            } catch (err) {
              setError((err as Error).message);
            }
          }}
        >
          <Select label="Token type" value={prefix} options={TYPES} onChange={setPrefix} />
          <div className="flex h-6 items-center rounded-control bg-control px-1.5 font-mono text-sm">
            <span className="text-fg-3">{prefix}</span>
            <input name="name" aria-label="New token name" required className="min-w-0 flex-1 bg-transparent outline-none" />
          </div>
          <input name="value" aria-label="New token value" required placeholder="#000000 or var(--color-x)" className="h-6 rounded-control bg-control px-1.5 font-mono text-sm outline-none" />
          {error && <p className="text-sm text-fg-2">{error}</p>}
          <div className="flex justify-end gap-1">
            <button type="button" onClick={() => setAdding(false)} className="h-6 rounded-control px-2 text-sm hover:bg-hover">
              Cancel
            </button>
            <button type="submit" className="h-6 rounded-control bg-selected px-2 text-sm text-selected-fg">
              Add
            </button>
          </div>
        </form>
      )}
      {groups.length > 0 && <ThemeBar store={store} theme={theme} themes={themes} setTheme={setTheme} />}
      {groups.length === 0 && !adding && (
        <div className="mt-1 flex flex-col items-start gap-2">
          <p className="text-sm text-fg-3">No tokens yet. Tokens are CSS variables in Tailwind theme namespaces.</p>
          <button type="button" onClick={() => store.do({ type: "addStarterTokens" })} className="h-6 rounded-control bg-control px-2 text-sm hover:bg-hover">
            Add starter tokens
          </button>
          <p className="text-sm text-fg-3">The shadcn/ui neutral colors, with a dark theme.</p>
        </div>
      )}
      {groups.map(([label, list]) => (
        <div key={label} className="mt-2">
          <div className="mb-0.5 text-xs text-fg-3 uppercase">{label}</div>
          {list.map((t) => (
            <TokenRow key={t.name} store={store} token={t} theme={theme} tokens={tokens} />
          ))}
        </div>
      ))}
      <CodeDialog
        title="Tokens as CSS"
        code={css?.text ?? null}
        onClose={() => setCss(null)}
        tabs={[
          { label: "@theme", active: !!css?.theme, onClick: () => exportCss(true) },
          { label: ":root", active: css?.theme === false, onClick: () => exportCss(false) },
        ]}
      />
    </section>
  );
}
