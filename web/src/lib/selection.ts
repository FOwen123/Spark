import type { DocStore } from "./doc";

/** What a keyboard selection command selects next, or null when it has nothing to do. */
export function selectionFor(store: DocStore, cmd: string): string[] | null {
  const sel = store.selection;
  const last = sel[sel.length - 1];
  const step = (list: string[], from: string | undefined, dir: 1 | -1) => {
    const i = from ? list.indexOf(from) : -1;
    return list.length ? list[i < 0 ? (dir > 0 ? 0 : list.length - 1) : (i + dir + list.length) % list.length] : undefined;
  };
  let id: string | undefined;
  switch (cmd) {
    case "selectNext":
    case "selectPrev":
      id = step(store.kids(last ? (store.nodes.get(last)?.parent ?? store.page) : store.page), last, cmd === "selectNext" ? 1 : -1);
      break;
    case "boardNext":
    case "boardPrev":
      id = step(store.kids(store.page), last && store.artboardOf(last), cmd === "boardNext" ? 1 : -1);
      break;
    case "selectOthers": {
      const parent = last && store.nodes.get(last)?.parent;
      const rest = parent ? store.kids(parent).filter((s) => !sel.includes(s)) : [];
      return rest.length ? rest : null;
    }
  }
  return id ? [id] : null;
}
