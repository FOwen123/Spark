import { type Page, expect, test } from "@playwright/test";
import { newFile } from "./util";

async function boardBox(page: Page) {
  const b = await page.locator("[data-node]").first().boundingBox();
  if (!b) throw new Error("no artboard");
  return b;
}

async function drag(page: Page, from: [number, number], to: [number, number]) {
  await page.mouse.move(...from);
  await page.mouse.down();
  await page.mouse.move(to[0], to[1], { steps: 6 });
  await page.mouse.up();
}

const layer = (page: Page, name: string | RegExp) => page.getByRole("treeitem", { name });

test("draw, edit in the inspector, undo and redo, and persist", async ({ page }) => {
  await newFile(page);
  const b = await boardBox(page);

  // Frame tool: drag inside the artboard.
  await page.keyboard.press("f");
  await expect(page.getByRole("button", { name: "Frame", exact: true })).toHaveAttribute("aria-pressed", "true");
  await drag(page, [b.x + 40, b.y + 40], [b.x + 240, b.y + 160]);
  await expect(layer(page, /^Frame$/)).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("button", { name: "Move", exact: true })).toHaveAttribute("aria-pressed", "true");

  // Edit its width in the inspector.
  const w = page.getByTestId("field-w");
  await w.fill("300");
  await w.press("Enter");
  const frame = page.locator('[data-kind="frame"] [data-kind="frame"]').first();
  await expect(frame).toHaveCSS("width", "300px");

  // Set a solid fill colour through the hex field.
  const hex = page.getByLabel("Fill hex").first();
  await hex.fill("ff0000");
  await hex.press("Enter");
  await expect(frame).toHaveCSS("background-color", "rgb(255, 0, 0)");

  // Text tool: click, type, commit.
  await page.keyboard.press("t");
  await page.mouse.click(b.x + 60, b.y + 300);
  await expect(page.locator('[data-kind="text"][contenteditable]')).toBeFocused();
  await page.keyboard.type("Hello Spark");
  await page.keyboard.press("Escape");
  const text = page.locator('[data-kind="text"]').first();
  await expect(text).toHaveText("Hello Spark");
  await expect(layer(page, "Hello Spark")).toBeVisible();

  // Typography edit.
  await layer(page, "Hello Spark").click();
  const size = page.getByTestId("field-font-size");
  await size.fill("32");
  await size.press("Enter");
  await expect(text).toHaveCSS("font-size", "32px");

  // Undo the font size, then redo it.
  await page.keyboard.press("ControlOrMeta+z");
  await expect(text).toHaveCSS("font-size", "16px");
  await page.keyboard.press("ControlOrMeta+Shift+z");
  await expect(text).toHaveCSS("font-size", "32px");

  // Reload: everything persisted.
  await page.reload();
  await expect(page.locator('[data-kind="text"]').first()).toHaveText("Hello Spark");
  await expect(page.locator('[data-kind="text"]').first()).toHaveCSS("font-size", "32px");
  await expect(page.locator('[data-kind="frame"] [data-kind="frame"]').first()).toHaveCSS("background-color", "rgb(255, 0, 0)");
});

test("select, move, duplicate, resize, nudge and delete", async ({ page }) => {
  await newFile(page);
  const b = await boardBox(page);
  await page.keyboard.press("r");
  await drag(page, [b.x + 50, b.y + 50], [b.x + 150, b.y + 150]);
  const rect = page.locator('[data-kind="rect"]').first();
  await expect(rect).toBeVisible();
  const x0 = Number(await page.getByTestId("field-x").inputValue());

  // Nudge right by 1 and by 8.
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("Shift+ArrowRight");
  await expect(page.getByTestId("field-x")).toHaveValue(String(x0 + 9));

  // Drag to move.
  const r0 = await rect.boundingBox();
  if (!r0) throw new Error("no rect");
  await drag(page, [r0.x + r0.width / 2, r0.y + r0.height / 2], [r0.x + r0.width / 2 + 120, r0.y + r0.height / 2 + 80]);
  await expect.poll(async () => (await rect.boundingBox())?.x ?? 0).toBeGreaterThan(r0.x + 60);

  // Option-drag duplicates.
  const r1 = await rect.boundingBox();
  if (!r1) throw new Error("no rect");
  await page.keyboard.down("Alt");
  await drag(page, [r1.x + 10, r1.y + 10], [r1.x + 10, r1.y + 200]);
  await page.keyboard.up("Alt");
  await expect(page.locator('[data-kind="rect"]')).toHaveCount(2);

  // Resize from the bottom-right handle with Shift keeps the ratio.
  await expect(page.getByTestId("field-w")).not.toHaveValue("");
  const sel = await page.locator('[data-kind="rect"]').nth(1).boundingBox();
  if (!sel) throw new Error("no copy");
  const wBefore = Number(await page.getByTestId("field-w").inputValue());
  const hBefore = Number(await page.getByTestId("field-h").inputValue());
  await page.keyboard.down("Shift");
  await drag(page, [sel.x + sel.width, sel.y + sel.height], [sel.x + sel.width + 60, sel.y + sel.height + 10]);
  await page.keyboard.up("Shift");
  await expect.poll(async () => Number(await page.getByTestId("field-w").inputValue())).toBeGreaterThan(wBefore);
  const wAfter = Number(await page.getByTestId("field-w").inputValue());
  const hAfter = Number(await page.getByTestId("field-h").inputValue());
  expect(wAfter / hAfter).toBeCloseTo(wBefore / hBefore, 1);

  // Mod+D duplicates, Delete removes.
  await page.keyboard.press("ControlOrMeta+d");
  await expect(page.locator('[data-kind="rect"]')).toHaveCount(3);
  await page.keyboard.press("Backspace");
  await expect(page.locator('[data-kind="rect"]')).toHaveCount(2);

  // Marquee inside the artboard selects both rectangles.
  await page.mouse.click(b.x + b.width - 20, b.y + b.height - 20);
  await drag(page, [b.x + b.width - 10, b.y + b.height - 10], [b.x + 20, b.y + 20]);
  await expect(page.getByRole("treeitem", { selected: true })).toHaveCount(2);
});

test("copy and paste, copy as code, layers and tokens", async ({ page }) => {
  await newFile(page);
  const b = await boardBox(page);
  await page.keyboard.press("r");
  await drag(page, [b.x + 30, b.y + 30], [b.x + 130, b.y + 90]);
  await expect(page.locator('[data-kind="rect"]')).toHaveCount(1);

  // Clipboard round trip through the Spark format.
  await page.keyboard.press("ControlOrMeta+c");
  await page.keyboard.press("ControlOrMeta+v");
  await expect(page.locator('[data-kind="rect"]')).toHaveCount(2);

  // Paste HTML from another app becomes layers.
  await page.evaluate(() => {
    const dt = new DataTransfer();
    dt.setData("text/html", '<div style="display:flex;gap:8px;padding:12px;background:#eeeeee"><p style="font-size:18px">Pasted card</p></div>');
    document.dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true }));
  });
  await expect(page.locator('[data-kind="text"]', { hasText: "Pasted card" })).toBeVisible();

  // Context menu → Copy as → JSX shows the generated code.
  const card = page.locator('[data-kind="text"]', { hasText: "Pasted card" });
  await card.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Copy as" }).hover();
  await page.getByRole("menuitem", { name: "JSX" }).click();
  await expect(page.getByTestId("code")).toContainText("Pasted card");
  await page.keyboard.press("Escape");

  // Layer rename and hide.
  const rectLayer = layer(page, "Rectangle").first();
  await rectLayer.dblclick();
  await page.getByRole("textbox", { name: "Name" }).fill("Hero image");
  await page.keyboard.press("Enter");
  await expect(layer(page, "Hero image")).toBeVisible();
  await layer(page, "Hero image").hover();
  await layer(page, "Hero image").getByRole("button", { name: "Hide" }).click();
  await expect(layer(page, "Hero image")).toHaveClass(/opacity-50/);

  // Tokens: add a color token and an alias, then export CSS.
  await page.keyboard.press("Escape");
  await page.mouse.click(b.x - 30, b.y + b.height + 30);
  await page.getByRole("button", { name: "Theme", exact: true }).click();
  await page.getByRole("button", { name: "Add token" }).click();
  await page.getByLabel("New token name").fill("brand");
  await page.getByLabel("New token value").fill("#112233");
  await page.getByRole("button", { name: "Add", exact: true }).click();
  await expect(page.getByTestId("token-row")).toHaveCount(1);
  await page.getByRole("button", { name: "Add token" }).click();
  await page.getByLabel("New token name").fill("accent");
  await page.getByLabel("New token value").fill("var(--color-brand)");
  await page.getByRole("button", { name: "Add", exact: true }).click();
  await expect(page.getByTestId("token-row")).toHaveCount(2);
  // Rows show names without the group prefix; renaming keeps it.
  const brand = page.locator('input[aria-label="Token name"][title="--color-brand"]');
  await expect(brand).toHaveValue("brand");
  await brand.fill("primary");
  await brand.press("Enter");
  await expect(page.locator('input[aria-label="Token name"][title="--color-primary"]')).toHaveValue("primary");
  await page.getByRole("button", { name: "Export tokens as CSS" }).click();
  await expect(page.getByTestId("code")).toContainText("--color-primary: #112233;");
  await expect(page.getByTestId("code")).toContainText("--color-accent: var(--color-brand);");
  await expect(page.getByTestId("code")).toContainText("@theme {");
});

test("Escape during a drag puts the layer back and leaves undo alone", async ({ page }) => {
  await newFile(page);
  const b = await boardBox(page);
  await page.keyboard.press("r");
  await drag(page, [b.x + 50, b.y + 50], [b.x + 150, b.y + 150]);
  const rect = page.locator('[data-kind="rect"]').first();
  const r0 = await rect.boundingBox();
  if (!r0) throw new Error("no rect");
  await page.mouse.move(r0.x + r0.width / 2, r0.y + r0.height / 2);
  await page.mouse.down();
  await page.mouse.move(r0.x + r0.width / 2 + 120, r0.y + r0.height / 2 + 80, { steps: 6 });
  await expect.poll(async () => (await rect.boundingBox())?.x ?? 0).toBeGreaterThan(r0.x + 60);
  await page.keyboard.press("Escape");
  await page.mouse.up();
  await expect.poll(async () => (await rect.boundingBox())?.x ?? 0).toBeCloseTo(r0.x, 0);
  // Escape didn't also climb to the parent: the rectangle is still selected.
  await expect(layer(page, /Rectangle/)).toHaveAttribute("aria-selected", "true");
  // The next undo removes the rectangle itself; the abandoned drag left no step behind.
  await page.keyboard.press("ControlOrMeta+z");
  await expect(rect).toHaveCount(0);
});
