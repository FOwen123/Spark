import { expect, test } from "vitest";
import { comboLabel, comboOf, commandFor } from "./keymap";

const key = (code: string, key: string, mods: Partial<{ meta: boolean; ctrl: boolean; shift: boolean; alt: boolean }> = {}) => ({
  code,
  key,
  metaKey: !!mods.meta,
  ctrlKey: !!mods.ctrl,
  shiftKey: !!mods.shift,
  altKey: !!mods.alt,
});

test("mod is Cmd on macOS and Ctrl elsewhere", () => {
  expect(comboOf(key("KeyZ", "z", { meta: true }), true)).toBe("mod+z");
  expect(comboOf(key("KeyZ", "z", { ctrl: true }), false)).toBe("mod+z");
  expect(comboOf(key("KeyZ", "z", { ctrl: true }), true)).toBe("ctrl+z");
});

test("Option-modified letters still match by physical key", () => {
  expect(commandFor(key("KeyG", "©", { meta: true, alt: true }), true)).toBe("groupInFrame");
  expect(commandFor(key("Digit1", "!", { shift: true }), true)).toBe("zoomFit");
  expect(commandFor(key("Equal", "=", { meta: true }), true)).toBe("zoomIn");
});

test("shortcut table covers the spec", () => {
  const mac = true;
  expect(commandFor(key("KeyV", "v"), mac)).toBe("tool.move");
  expect(commandFor(key("KeyD", "d", { meta: true }), mac)).toBe("duplicate");
  expect(commandFor(key("KeyZ", "z", { meta: true, shift: true }), mac)).toBe("redo");
  expect(commandFor(key("Enter", "Enter", { shift: true }), mac)).toBe("selectParent");
  expect(commandFor(key("ArrowLeft", "ArrowLeft", { shift: true }), mac)).toBe("nudgeBig.left");
  expect(commandFor(key("KeyA", "A", { shift: true }), mac)).toBe("addFlex");
  expect(commandFor(key("KeyJ", "j", { meta: true }), mac)).toBe("panel.agent");
  expect(commandFor(key("Digit0", "0", { meta: true }), mac)).toBe("zoom100");
});

test("labels", () => {
  expect(comboLabel("mod+shift+z", true)).toBe("⌘⇧Z");
  expect(comboLabel("mod+shift+z", false)).toBe("Ctrl+Shift+Z");
});

test("the desktop app's native menus own their shortcuts", () => {
  const undo = key("KeyZ", "z", { meta: true });
  expect(commandFor(undo, true, false)).toBe("undo");
  expect(commandFor(undo, true, true)).toBeUndefined();
  expect(commandFor(key("KeyV", "v"), true, true)).toBe("tool.move");
});
