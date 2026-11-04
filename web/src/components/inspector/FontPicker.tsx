import * as Popover from "@radix-ui/react-popover";
import { ChevronDown, Star } from "lucide-react";
import { useContext, useEffect, useMemo, useRef, useState } from "react";
import { StoreContext } from "./fields";

const BUNDLED = ["Geist", "Geist Mono", "system-ui", "serif", "sans-serif", "monospace"];
const FAVORITES_KEY = "spark-font-favorites";
const ROW = 24;
const HEIGHT = 224;

type Catalog = { installed: string[]; google: string[] };
let catalog: Promise<Catalog> | undefined;
/** Installed and Google Fonts family names from the server, fetched once per page load. */
const loadCatalog = () =>
  (catalog ??= fetch("/fonts/list")
    .then((r) => (r.ok ? (r.json() as Promise<Catalog>) : { installed: [], google: [] }))
    .catch(() => ({ installed: [], google: [] })));

const readFavorites = (): string[] => {
  try {
    return JSON.parse(localStorage.getItem(FAVORITES_KEY) ?? "[]");
  } catch {
    return [];
  }
};

/** Renders only the rows in view, so a catalog of 1500+ families stays cheap. */
function VirtualList(props: { items: string[]; row: (item: string) => React.ReactNode }) {
  const [top, setTop] = useState(0);
  const ref = useRef<HTMLDivElement>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: a new list starts at the top
  useEffect(() => {
    if (ref.current) ref.current.scrollTop = 0;
    setTop(0);
  }, [props.items]);
  const first = Math.max(0, Math.floor(top / ROW) - 3);
  const last = Math.min(props.items.length, Math.ceil((top + HEIGHT) / ROW) + 3);
  return (
    <div ref={ref} onScroll={(e) => setTop(e.currentTarget.scrollTop)} style={{ height: HEIGHT }} className="overflow-auto spark-scroll" data-testid="font-list">
      <div style={{ height: props.items.length * ROW, position: "relative" }}>
        {props.items.slice(first, last).map((item, i) => (
          <div key={item} style={{ position: "absolute", top: (first + i) * ROW, height: ROW, left: 0, right: 0 }}>
            {props.row(item)}
          </div>
        ))}
      </div>
    </div>
  );
}

/** A font family field that opens a searchable list: installed + Google fonts, favorites, font tokens. */
export function FontPicker(props: { value: string; onPick: (family: string) => void }) {
  const store = useContext(StoreContext);
  const [open, setOpen] = useState(false);
  const [tab, setTab] = useState<"fonts" | "favorites" | "tokens">("fonts");
  const search = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  const [favorites, setFavorites] = useState(readFavorites);
  const [fonts, setFonts] = useState<Catalog>({ installed: [], google: [] });
  useEffect(() => {
    if (open) loadCatalog().then(setFonts);
  }, [open]);

  const all = useMemo(() => {
    const seen = new Set<string>();
    const out: { name: string; google: boolean }[] = [];
    for (const [names, google] of [[BUNDLED, false], [fonts.installed, false], [fonts.google, true]] as const) {
      for (const name of names) {
        if (!seen.has(name.toLowerCase())) {
          seen.add(name.toLowerCase());
          out.push({ name, google });
        }
      }
    }
    return out;
  }, [fonts]);
  const googleOnly = useMemo(() => new Set(all.filter((f) => f.google).map((f) => f.name)), [all]);

  const q = query.trim().toLowerCase();
  const match = (name: string) => !q || name.toLowerCase().includes(q);
  // biome-ignore lint/correctness/useExhaustiveDependencies: `match` only depends on `q`
  const names = useMemo(() => all.map((f) => f.name).filter(match), [all, q]);
  const favs = favorites.filter(match);
  const fontTokens = (store?.tokens ?? []).filter((t) => t.name.startsWith("--font-") && !t.name.startsWith("--font-weight-") && match(t.name));

  const toggleFavorite = (name: string) => {
    const next = favorites.includes(name) ? favorites.filter((f) => f !== name) : [...favorites, name];
    setFavorites(next);
    localStorage.setItem(FAVORITES_KEY, JSON.stringify(next));
  };
  const pick = (family: string) => {
    props.onPick(family);
    setOpen(false);
  };
  const row = (name: string) => (
    <div className="flex h-6 items-center gap-1 rounded-control pr-1 hover:bg-hover" data-testid="font-row">
      <button type="button" onClick={() => pick(name)} className={`flex h-6 min-w-0 flex-1 items-center gap-2 px-2 text-left ${name === props.value ? "font-medium" : ""}`}>
        <span className="min-w-0 flex-1 truncate">{name}</span>
        {googleOnly.has(name) && <span className="text-xs text-fg-3">Google</span>}
      </button>
      <button type="button" aria-label={`${favorites.includes(name) ? "Unfavorite" : "Favorite"} ${name}`} aria-pressed={favorites.includes(name)} onClick={() => toggleFavorite(name)} className="grid size-5 shrink-0 place-items-center text-fg-3 hover:text-fg">
        <Star size={12} fill={favorites.includes(name) ? "currentColor" : "none"} />
      </button>
    </div>
  );
  const tabs = [
    ["fonts", "Fonts"],
    ["favorites", "Favorites"],
    ["tokens", "Tokens"],
  ] as const;
  const shown = props.value.startsWith("var(") ? props.value.replace(/^var\(--(?:font-)?|\)$/g, "") : props.value;

  return (
    <Popover.Root open={open} onOpenChange={(o) => { setOpen(o); if (o) setQuery(""); }}>
      <Popover.Trigger type="button" aria-label="Font family" className="flex h-6 w-full items-center gap-1 rounded-control bg-control px-1.5 text-left text-sm">
        <span className="min-w-0 flex-1 truncate">{shown}</span>
        <ChevronDown size={12} className="shrink-0 text-fg-3" />
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content role="dialog" aria-label="Font picker" onEscapeKeyDown={(e) => e.stopPropagation()} onOpenAutoFocus={(e) => { e.preventDefault(); search.current?.focus(); }} align="start" sideOffset={4} collisionPadding={8} className="z-50 w-64 rounded-popover border border-line bg-panel p-1 text-sm shadow-popover outline-none">
          <input ref={search} aria-label="Search fonts" placeholder="Search fonts" value={query} onChange={(e) => setQuery(e.target.value)} className="mb-1 h-6 w-full rounded-control bg-control px-2 outline-none" />
          <div role="tablist" className="mb-1 flex gap-1">
            {tabs.map(([id, label]) => (
              <button key={id} type="button" role="tab" aria-selected={tab === id} onClick={() => setTab(id)} className={`h-6 flex-1 rounded-control ${tab === id ? "bg-selected text-selected-fg" : "bg-control hover:bg-hover"}`}>
                {label}
              </button>
            ))}
          </div>
          {tab === "fonts" && <VirtualList items={names} row={row} />}
          {tab === "favorites" &&
            (favs.length ? <VirtualList items={favs} row={row} /> : <p className="px-2 py-3 text-fg-3">Star a font to keep it here.</p>)}
          {tab === "tokens" &&
            (fontTokens.length ? (
              <div className="max-h-56 overflow-auto spark-scroll">
                {fontTokens.map((t) => (
                  <button key={t.name} type="button" onClick={() => pick(`var(${t.name})`)} className="flex h-6 w-full items-center gap-2 rounded-control px-2 text-left hover:bg-hover">
                    <span className="min-w-0 flex-1 truncate font-mono">{t.name.replace(/^--font-/, "")}</span>
                    <span className="truncate text-fg-3">{t.value.split(",")[0]}</span>
                  </button>
                ))}
              </div>
            ) : (
              <p className="px-2 py-3 text-fg-3">No font tokens in this file.</p>
            ))}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
