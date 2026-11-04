// Keyboard shortcuts. Combos are written like "mod+shift+z"; `mod` is Cmd on macOS and Ctrl elsewhere.

export type KeyLike = { key: string; code: string; metaKey: boolean; ctrlKey: boolean; shiftKey: boolean; altKey: boolean };

export const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

const CODE_NAMES: Record<string, string> = {
  Equal: "=",
  Minus: "-",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Slash: "/",
  Period: ".",
  Comma: ",",
  Quote: "'",
  Space: "space",
  NumpadAdd: "=",
  NumpadSubtract: "-",
};

/** Physical key name: letters and digits by position so Alt/Option doesn't change them. */
export function keyName(e: KeyLike): string {
  if (/^Key[A-Z]$/.test(e.code)) return e.code.slice(3).toLowerCase();
  if (/^Digit\d$/.test(e.code)) return e.code.slice(5);
  if (CODE_NAMES[e.code]) return CODE_NAMES[e.code];
  return e.key.length === 1 ? e.key.toLowerCase() : e.key.toLowerCase();
}

export function comboOf(e: KeyLike, mac = isMac): string {
  const parts: string[] = [];
  if (mac ? e.metaKey : e.ctrlKey) parts.push("mod");
  if (mac && e.ctrlKey) parts.push("ctrl");
  if (e.altKey) parts.push("alt");
  if (e.shiftKey) parts.push("shift");
  parts.push(keyName(e));
  return parts.join("+");
}

export const KEYMAP: Record<string, string> = {
  v: "tool.move",
  f: "tool.frame",
  r: "tool.rect",
  t: "tool.text",
  i: "tool.image",
  h: "tool.hand",
  "mod+j": "panel.agent",
  "mod+z": "undo",
  "mod+shift+z": "redo",
  "mod+y": "redo",
  "mod+d": "duplicate",
  "mod+a": "selectAll",
  "mod+alt+g": "groupInFrame",
  "shift+a": "addFlex",
  enter: "selectChildren",
  "shift+enter": "selectParent",
  escape: "escape",
  backspace: "delete",
  delete: "delete",
  "shift+1": "zoomFit",
  "shift+2": "zoomSelection",
  "mod+0": "zoom100",
  "mod+=": "zoomIn",
  "mod+shift+=": "zoomIn",
  "mod+-": "zoomOut",
  arrowup: "nudge.up",
  arrowdown: "nudge.down",
  arrowleft: "nudge.left",
  arrowright: "nudge.right",
  "shift+arrowup": "nudgeBig.up",
  "shift+arrowdown": "nudgeBig.down",
  "shift+arrowleft": "nudgeBig.left",
  "shift+arrowright": "nudgeBig.right",
  "mod+]": "bringForward",
  "mod+[": "sendBackward",
  "]": "bringToFront",
  "[": "sendToBack",
  "mod+shift+h": "toggleHidden",
  "mod+shift+l": "toggleLocked",
  "mod+r": "rename",
  "mod+shift+e": "export",
  "shift+'": "togglePixelGrid",
  "mod+'": "togglePixelGrid",
  "mod+shift+'": "toggleSnapPixel",
  "shift+g": "toggleLayoutGuides",
  "alt+t": "copyTailwind",
  "alt+r": "copyJsx",
  "mod+alt+c": "copyStyle",
  "mod+alt+v": "pasteStyle",
  "mod+shift+v": "pasteOnTop",
  "mod+shift+r": "pasteReplace",
  "mod+shift+c": "copyPng",
  "mod+f": "findLayers",
  ".": "toggleUI",
  "mod+\\": "toggleUI",
  "shift+.": "toggleFloating",
  // Tabs and app-level shortcuts (handled in App.tsx). Cycling uses the physical Ctrl key on every platform.
  ...Object.fromEntries([1, 2, 3, 4, 5, 6, 7, 8].map((n) => [`mod+${n}`, `tab.${n}`])),
  "mod+9": "tab.last",
  [isMac ? "ctrl+tab" : "mod+tab"]: "tab.next",
  [isMac ? "ctrl+shift+tab" : "mod+shift+tab"]: "tab.prev",
  "mod+t": "tab.new",
  "mod+shift+t": "tab.reopen",
  "mod+shift+d": "app.home",
  [isMac ? "ctrl+shift+/" : "mod+shift+/"]: "app.shortcuts",
  // Arrange: align, distribute, rotate and flip.
  "alt+a": "align.left",
  "alt+h": "align.hcenter",
  "alt+d": "align.right",
  "alt+w": "align.top",
  "alt+v": "align.vmiddle",
  "alt+s": "align.bottom",
  "alt+shift+h": "distribute.x",
  "alt+shift+v": "distribute.y",
  "shift+h": "transform.flipH",
  "shift+v": "transform.flipV",
  // Keyboard selection and structure.
  tab: "selectNext",
  "shift+tab": "selectPrev",
  n: "boardNext",
  "shift+n": "boardPrev",
  "mod+shift+a": "selectOthers",
  "\\": "selectParent",
  "mod+enter": "selectChildren",
  "mod+shift+g": "ungroup",
  "mod+backspace": "ungroup",
  "mod+alt+arrowup": "sendBackward",
  "mod+alt+arrowdown": "bringForward",
  // Aliases.
  a: "tool.frame",
  "shift+f": "groupInFrame",
  "mod+shift+k": "tool.image",
  // Zoom.
  "0": "zoom100",
  "shift+0": "zoom100",
  "1": "zoomFit",
  "2": "zoomSelection",
  "=": "zoomIn",
  "shift+=": "zoomIn",
  "-": "zoomOut",
  // Pages, panels and colours.
  pagedown: "page.next",
  pageup: "page.prev",
  "alt+1": "leftTab.design",
  "alt+2": "leftTab.theme",
  "alt+l": "collapseLayers",
  "alt+c": "toggleClip",
  "shift+x": "swapColors",
};

/** Shortcuts the desktop app's native menus handle; the page leaves them alone there. */
export const MENU_OWNED = new Set(["mod+z", "mod+shift+z", "mod+d", "mod+=", "mod+shift+=", "mod+-", "mod+0", "mod+j", "mod+shift+e"]);

export const desktop = typeof location !== "undefined" && (new URLSearchParams(location.search).has("desktop") || sessionStorage.getItem("spark-desktop") === "1");
if (desktop) sessionStorage.setItem("spark-desktop", "1");

export function commandFor(e: KeyLike, mac = isMac, inDesktop = desktop): string | undefined {
  const combo = comboOf(e, mac);
  if (inDesktop && MENU_OWNED.has(combo)) return undefined;
  return KEYMAP[combo];
}

/** Human label for a combo, e.g. "⌘⇧Z" on macOS or "Ctrl+Shift+Z" elsewhere. */
export function comboLabel(combo: string, mac = isMac): string {
  const parts = combo.split("+");
  const key = parts.pop() ?? "";
  const k = key.length === 1 ? key.toUpperCase() : key === "backspace" ? "⌫" : key === "enter" ? "↵" : key[0].toUpperCase() + key.slice(1);
  if (mac) {
    const sym: Record<string, string> = { mod: "⌘", ctrl: "⌃", alt: "⌥", shift: "⇧" };
    return parts.map((p) => sym[p]).join("") + k;
  }
  const names: Record<string, string> = { mod: "Ctrl", ctrl: "Ctrl", alt: "Alt", shift: "Shift" };
  return [...parts.map((p) => names[p]), k].join("+");
}

/** The first combo bound to a command, for menus. */
export function comboFor(command: string): string | undefined {
  return Object.keys(KEYMAP).find((c) => KEYMAP[c] === command);
}
