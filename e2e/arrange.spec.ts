import { type Page, expect, test } from "@playwright/test";
import { newFile } from "./util";

const SHOTS = "test-results/shots/arrange";

async function drag(page: Page, from: [number, number], to: [number, number]) {
  await page.mouse.move(...from);
  await page.mouse.down();
  await page.mouse.move(to[0], to[1], { steps: 6 });
  await page.mouse.up();
}

/** Draws rectangles inside the artboard at offsets [x, y, w, h] from its top-left. */
async function draw(page: Page, rects: [number, number, number, number][]) {
  const b = await page.locator("[data-node]").first().boundingBox();
  if (!b) throw new Error("no artboard");
  const have = await page.locator('[data-kind="rect"]').count();
  for (const [i, [x, y, w, h]] of rects.entries()) {
    await page.keyboard.press("r");
    await drag(page, [b.x + x, b.y + y], [b.x + x + w, b.y + y + h]);
    await expect(page.locator('[data-kind="rect"]')).toHaveCount(have + i + 1);
  }
  return b;
}

const boxes = async (page: Page) => {
  const out = [];
  for (const el of await page.locator('[data-kind="rect"]').all()) out.push((await el.boundingBox())!);
  return out;
};
/** Row positions of the selected layers in the layers panel (names repeat, so positions identify them). */
const picked = (page: Page) =>
  page.getByRole("treeitem").evaluateAll((els) => els.flatMap((e, i) => (e.getAttribute("aria-selected") === "true" ? [i] : [])));
const layer = (page: Page, name: string | RegExp) => page.getByRole("treeitem", { name });

test("align, distribute, rotate and flip from the buttons and the keyboard", async ({ page }) => {
  await newFile(page);
  const board = await draw(page, [
    [40, 40, 60, 40],
    [160, 100, 80, 70],
    [330, 60, 50, 90],
  ]);
  await page.keyboard.press("ControlOrMeta+a");
  await expect(page.getByRole("button", { name: "Distribute horizontally" })).toBeEnabled();

  // Distribute: equal gaps along each axis.
  await page.keyboard.press("Alt+Shift+h");
  await expect.poll(async () => {
    const [a, b, c] = (await boxes(page)).sort((p, q) => p.x - q.x);
    return Math.abs(b.x - (a.x + a.width) - (c.x - (b.x + b.width))) < 1;
  }).toBe(true);
  await page.keyboard.press("Alt+Shift+v");
  await expect.poll(async () => {
    const [a, b, c] = (await boxes(page)).sort((p, q) => p.y - q.y);
    return Math.abs(b.y - (a.y + a.height) - (c.y - (b.y + b.height))) < 1;
  }).toBe(true);

  // Align the selection to its combined bounds, by shortcut and by button.
  await page.keyboard.press("Alt+a");
  await expect.poll(async () => Math.max(...(await boxes(page)).map((b) => b.x)) - Math.min(...(await boxes(page)).map((b) => b.x))).toBeLessThan(1);
  await page.keyboard.press("Alt+d");
  await expect.poll(async () => new Set((await boxes(page)).map((b) => Math.round(b.x + b.width))).size).toBe(1);
  await page.keyboard.press("Alt+w");
  await expect.poll(async () => new Set((await boxes(page)).map((b) => Math.round(b.y))).size).toBe(1);
  await page.keyboard.press("Alt+s");
  await expect.poll(async () => new Set((await boxes(page)).map((b) => Math.round(b.y + b.height))).size).toBe(1);
  await page.keyboard.press("Alt+h");
  await expect.poll(async () => new Set((await boxes(page)).map((b) => Math.round(b.x + b.width / 2))).size).toBe(1);
  await page.keyboard.press("Alt+v");
  await expect.poll(async () => new Set((await boxes(page)).map((b) => Math.round(b.y + b.height / 2))).size).toBe(1);
  await page.getByRole("button", { name: "Align left" }).click();
  await expect.poll(async () => new Set((await boxes(page)).map((b) => Math.round(b.x))).size).toBe(1);

  // A single layer aligns within its parent.
  await layer(page, /^Rectangle/).first().click();
  await page.keyboard.press("Alt+d");
  const last = page.locator('[data-kind="rect"]').last();
  await expect.poll(async () => Math.round(board.x + board.width - (await last.boundingBox().then((r) => r!.x + r!.width)))).toBe(0);

  // Rotate and flip: buttons and Shift+H / Shift+V.
  const rect = page.locator('[data-kind="rect"]').last();
  await page.getByRole("button", { name: "Rotate 90° clockwise" }).click();
  await expect(rect).toHaveCSS("rotate", "90deg");
  await page.keyboard.press("Shift+h");
  await expect(rect).toHaveCSS("scale", "-1 1");
  await page.keyboard.press("Shift+v");
  await expect(rect).toHaveCSS("scale", "-1");
  await page.getByRole("button", { name: "Flip horizontal" }).click();
  await page.getByRole("button", { name: "Flip vertical" }).click();
  await expect(rect).toHaveCSS("scale", "none");

  // Layers in a flex parent are placed by layout, so the align buttons are off.
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.press("Shift+a");
  await expect(page.getByRole("button", { name: "Flex" })).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("ControlOrMeta+Enter");
  await expect(page.getByRole("button", { name: "Align left" })).toBeDisabled();
});

test("keyboard selection, ungroup, reorder and tool aliases", async ({ page }) => {
  await newFile(page);
  await draw(page, [
    [40, 40, 60, 40],
    [160, 100, 80, 70],
    [330, 60, 50, 90],
  ]);
  const rects = page.locator('[data-kind="rect"]');
  const selected = page.getByRole("treeitem", { selected: true });

  // Tab / Shift+Tab walk the siblings, wrapping; Mod+Shift+A selects the rest.
  await layer(page, /^Rectangle/).last().click();
  const first = await picked(page);
  await page.keyboard.press("Tab");
  await expect.poll(() => picked(page)).not.toEqual(first);
  await expect.poll(async () => (await picked(page)).length).toBe(1);
  await page.keyboard.press("Shift+Tab");
  await expect.poll(() => picked(page)).toEqual(first);
  await page.keyboard.press("ControlOrMeta+Shift+a");
  await expect(selected).toHaveCount(2);

  // Group in frame (Shift+F), then Mod+Shift+G ungroups without moving anything, in one undo step.
  const before = (await Promise.all((await rects.all()).map((r) => r.boundingBox()))).map((b) => [Math.round(b!.x), Math.round(b!.y)]);
  await page.keyboard.press("Shift+f");
  await expect(layer(page, /Frame/)).toHaveCount(1);
  await page.keyboard.press("ControlOrMeta+Shift+g");
  await expect(layer(page, /Frame/)).toHaveCount(0);
  await expect(selected).toHaveCount(2);
  expect((await Promise.all((await rects.all()).map((r) => r.boundingBox()))).map((b) => [Math.round(b!.x), Math.round(b!.y)])).toEqual(before);
  await page.keyboard.press("ControlOrMeta+z");
  await expect(layer(page, /Frame/)).toHaveCount(1);
  await page.keyboard.press("ControlOrMeta+Backspace");
  await expect(layer(page, /Frame/)).toHaveCount(0);

  // \ selects the parent, Mod+Enter its children again.
  await layer(page, /^Rectangle/).first().click();
  await page.keyboard.press("\\");
  await expect.poll(() => picked(page)).toEqual([0]);
  await page.keyboard.press("ControlOrMeta+Enter");
  await expect(selected).toHaveCount(3);

  // Mod+Alt+Arrow moves one step among the siblings: Up earlier, Down later (the list shows later layers first).
  await layer(page, /^Rectangle/).first().click();
  const top = await picked(page);
  await page.keyboard.press("ControlOrMeta+Alt+ArrowUp");
  await expect.poll(() => picked(page)).toEqual([top[0] + 1]);
  await page.keyboard.press("ControlOrMeta+Alt+ArrowDown");
  await expect.poll(() => picked(page)).toEqual(top);

  // Aliases and the zoom keys.
  await page.keyboard.press("a");
  await expect(page.getByRole("button", { name: "Frame", exact: true })).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("v");
  const zoom = page.getByTestId("zoom");
  await page.keyboard.press("0");
  await expect(zoom).toHaveText("100%");
  await page.keyboard.press("-");
  await expect(zoom).toHaveText("50%");
  await page.keyboard.press("=");
  await expect(zoom).toHaveText("100%");
  await page.keyboard.press("1");
  await expect(zoom).not.toHaveText("100%");
  await page.keyboard.press("Shift+0");
  await expect(zoom).toHaveText("100%");
  await page.keyboard.press("ControlOrMeta+Shift+k");
  await expect(page.getByTestId("image-input")).toBeAttached();

  // Alt+C toggles clip content on a selected frame; Shift+X swaps fill and border colours.
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");
  await page.keyboard.press("1");
  await expect.poll(async () => Math.round((await page.locator("[data-node]").first().boundingBox())!.width)).toBeLessThan(900);
  await page.keyboard.press("f");
  const b = (await page.locator("[data-node]").first().boundingBox())!;
  await drag(page, [b.x + 40, b.y + 300], [b.x + 240, b.y + 420]);
  const clip = page.getByRole("checkbox", { name: "Clip content" });
  await expect(clip).not.toBeChecked();
  await page.keyboard.press("Alt+c");
  await expect(clip).toBeChecked();
  await page.keyboard.press("Alt+c");
  await expect(clip).not.toBeChecked();
  await page.getByRole("button", { name: "Add border" }).click();
  await expect(page.getByLabel("Border hex")).toHaveValue(/000000/i);
  await page.keyboard.press("Shift+x");
  await expect(page.getByLabel("Fill hex").first()).toHaveValue(/000000/i);
  await expect(page.getByLabel("Border hex")).toHaveValue(/ffffff/i);
});

test("artboard, page, and panel shortcuts", async ({ page }) => {
  await newFile(page);
  const selected = page.getByRole("treeitem", { selected: true });
  const pages = page.getByRole("region", { name: "Pages" });

  // Duplicate the artboard, then N / Shift+N move between the two.
  await layer(page, /./).first().click();
  await page.keyboard.press("ControlOrMeta+d");
  await expect(page.locator("[data-node]").first()).toBeVisible();
  await page.keyboard.press("Escape");
  await page.keyboard.press("n");
  const a = await picked(page);
  await page.keyboard.press("n");
  await expect.poll(() => picked(page)).not.toEqual(a);
  await expect.poll(async () => (await picked(page)).length).toBe(1);
  await page.keyboard.press("Shift+n");
  await expect.poll(() => picked(page)).toEqual(a);

  // PageDown / PageUp switch pages.
  await pages.getByRole("button", { name: "Add page" }).click();
  await expect(pages.getByRole("button", { name: /^Page 2/ })).toHaveAttribute("aria-current", "true");
  await page.keyboard.press("PageUp");
  await expect(pages.getByRole("button", { name: "Page 1", exact: true })).toHaveAttribute("aria-current", "true");
  await page.keyboard.press("PageDown");
  await expect(pages.getByRole("button", { name: /^Page 2/ })).toHaveAttribute("aria-current", "true");

  // Alt+2 / Alt+1 switch the left panel; Alt+L collapses the layer rows.
  await page.keyboard.press("Alt+2");
  await expect(page.getByRole("button", { name: "Theme", exact: true })).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Alt+1");
  await expect(page.getByRole("button", { name: "Design", exact: true })).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("PageUp");
  // Artboard navigation left the camera on the copy; fit both before drawing in the original.
  await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
  await page.keyboard.press("1");
  const canvas = (await page.getByTestId("canvas").boundingBox())!;
  await expect.poll(async () => (await page.locator("[data-node]").first().boundingBox())!.x).toBeGreaterThan(canvas.x);
  await draw(page, [[40, 40, 60, 40]]);
  await expect(layer(page, /^Rectangle/)).toHaveCount(1);
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Alt+l");
  await expect(layer(page, /^Rectangle/)).toHaveCount(0);

  // Single-key shortcuts stay out of text fields.
  await pages.getByRole("button", { name: "Page 1", exact: true }).dblclick();
  const name = page.getByRole("textbox", { name: "Name" });
  await name.fill("");
  await page.keyboard.type("an1-0");
  await expect(name).toHaveValue("an1-0");
  await name.press("Escape");
});

test("the arrange controls in the inspector and the right-click menu, light and dark", async ({ page }) => {
  await newFile(page);
  const b = await draw(page, [
    [40, 40, 60, 40],
    [160, 100, 80, 70],
    [330, 60, 50, 90],
  ]);
  await page.keyboard.press("ControlOrMeta+a");
  for (const theme of ["light", "dark"]) {
    await page.evaluate((t) => document.documentElement.setAttribute("data-theme", t), theme);
    await page.getByRole("complementary", { name: "Inspector" }).screenshot({ path: `${SHOTS}/inspector-position-${theme}.png` });
    await page.mouse.click(b.x + 200, b.y + 500, { button: "right" });
    await page.getByRole("menuitem", { name: "Align", exact: true }).hover();
    await expect(page.getByRole("menuitem", { name: "Horizontal center" })).toBeVisible();
    await page.screenshot({ path: `${SHOTS}/canvas-menu-${theme}.png`, clip: { x: b.x + 150, y: b.y + 350, width: 600, height: 400 } });
    await page.getByRole("menuitem", { name: "Vertical middle" }).click();
    await expect(page.getByRole("menu")).toHaveCount(0);
  }
  await page.mouse.click(b.x + 200, b.y + 500, { button: "right" });
  await expect(page.getByRole("menuitem", { name: "Select parent" })).toBeEnabled();
  await page.getByRole("menuitem", { name: "Select parent" }).click();
  await expect(page.getByRole("treeitem", { selected: true })).toHaveCount(1);
});
