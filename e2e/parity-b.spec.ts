import { mkdirSync } from "node:fs";
import { type Page, expect, test } from "@playwright/test";
import { newFile } from "./util";

// A 2×2 opaque PNG.
const PNG = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAFklEQVR4nGNkYPj/n4GBgYmBgYGBAQAj4QH/Hn5+cAAAAABJRU5ErkJggg==";

const rects = (page: Page) => page.locator('[data-kind="rect"]');
const layer = (page: Page, name: string | RegExp) => page.getByRole("treeitem", { name });
const selected = (page: Page) => page.getByRole("treeitem", { selected: true });

async function boardBox(page: Page) {
  const b = await page.locator("[data-node]").first().boundingBox();
  if (!b) throw new Error("no artboard");
  return b;
}

async function drawRect(page: Page, from: [number, number], to: [number, number]) {
  await page.keyboard.press("r");
  await page.mouse.move(...from);
  await page.mouse.down();
  await page.mouse.move(to[0], to[1], { steps: 6 });
  await page.mouse.up();
  // The tool switches back to Move once the server has created the rectangle.
  await expect(page.getByRole("button", { name: "Move", exact: true })).toHaveAttribute("aria-pressed", "true");
}

async function setFill(page: Page, hex: string) {
  const field = page.getByLabel("Fill hex").first();
  await field.fill(hex);
  await field.press("Enter");
}

async function pasteHtml(page: Page, html: string) {
  await page.evaluate((h) => {
    const dt = new DataTransfer();
    dt.setData("text/html", h);
    document.dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true }));
  }, html);
}

/** Saves a screenshot for review when PARITY_SHOTS names a folder. */
async function shot(page: Page, name: string) {
  const dir = process.env.PARITY_SHOTS;
  if (!dir) return;
  mkdirSync(dir, { recursive: true });
  for (const theme of ["light", "dark"]) {
    await page.evaluate((t) => {
      document.documentElement.dataset.theme = t;
    }, theme);
    await page.screenshot({ path: `${dir}/${name}-${theme}.png` });
  }
  await page.evaluate(() => {
    document.documentElement.dataset.theme = "light";
  });
}

test.beforeEach(async ({ context, browserName }) => {
  if (browserName === "chromium") await context.grantPermissions(["clipboard-read", "clipboard-write"]);
});

test("copy styles and paste styles are one undo step", async ({ page }) => {
  await newFile(page);
  const b = await boardBox(page);
  await drawRect(page, [b.x + 30, b.y + 30], [b.x + 130, b.y + 90]);
  await setFill(page, "ff0000");
  const radius = page.getByTestId("field-radius");
  await radius.fill("12");
  await radius.press("Enter");
  await drawRect(page, [b.x + 200, b.y + 30], [b.x + 300, b.y + 90]);
  await expect(rects(page)).toHaveCount(2);
  const [a, c] = [rects(page).nth(0), rects(page).nth(1)];
  const width = await c.evaluate((el) => getComputedStyle(el).width);

  await a.click();
  await page.keyboard.press("ControlOrMeta+Alt+c");
  await c.click();
  await page.keyboard.press("ControlOrMeta+Alt+v");
  await expect(c).toHaveCSS("background-color", "rgb(255, 0, 0)");
  await expect(c).toHaveCSS("border-radius", "12px");
  // Position and size are not part of a style.
  await expect(c).toHaveCSS("width", width);

  await page.keyboard.press("ControlOrMeta+z");
  await expect(c).toHaveCSS("background-color", "rgb(217, 217, 217)");
  await expect(c).toHaveCSS("border-radius", "0px");

  // The menu items do the same.
  await c.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Paste styles" }).click();
  await expect(c).toHaveCSS("background-color", "rgb(255, 0, 0)");
});

test("paste on top keeps the copied position; paste to replace swaps the selection", async ({ page }) => {
  await newFile(page);
  const b = await boardBox(page);
  await drawRect(page, [b.x + 30, b.y + 30], [b.x + 130, b.y + 90]);
  await setFill(page, "ff0000");
  await drawRect(page, [b.x + 200, b.y + 200], [b.x + 300, b.y + 290]);
  const [a, c] = [rects(page).nth(0), rects(page).nth(1)];
  const x = page.getByTestId("field-x");
  const y = page.getByTestId("field-y");

  await a.click();
  const [ax, ay] = [await x.inputValue(), await y.inputValue()];
  await page.keyboard.press("ControlOrMeta+c");
  // Move the original, then paste on top: the copy appears where the original used to be.
  await page.keyboard.press("Shift+ArrowRight");
  await page.keyboard.press("Shift+ArrowRight");
  await expect(x).not.toHaveValue(ax);
  await page.keyboard.press("ControlOrMeta+Shift+v");
  await expect(rects(page)).toHaveCount(3);
  await expect.poll(async () => Number(await x.inputValue())).toBeCloseTo(Number(ax), 1);
  await expect.poll(async () => Number(await y.inputValue())).toBeCloseTo(Number(ay), 1);

  // Replace the grey rectangle with the red one: same count, red fill, grey one's position.
  await c.click();
  const [cx, cy] = [await x.inputValue(), await y.inputValue()];
  await page.keyboard.press("ControlOrMeta+Shift+r");
  await expect(rects(page)).toHaveCount(3);
  await expect.poll(async () => Number(await x.inputValue())).toBeCloseTo(Number(cx), 1);
  await expect.poll(async () => Number(await y.inputValue())).toBeCloseTo(Number(cy), 1);
  await expect(selected(page)).toHaveCount(1);
  for (const i of [0, 1, 2]) await expect(rects(page).nth(i)).toHaveCSS("background-color", "rgb(255, 0, 0)");

  // One undo step brings the grey rectangle back.
  await page.keyboard.press("ControlOrMeta+z");
  await expect(rects(page)).toHaveCount(3);
  await expect.poll(async () => rects(page).evaluateAll((els) => els.filter((e) => getComputedStyle(e).backgroundColor === "rgb(217, 217, 217)").length)).toBe(1);
});

test("the node menu: select layer, copy as, copy link, open image", async ({ page, browserName }) => {
  await newFile(page);
  const b = await boardBox(page);
  await drawRect(page, [b.x + 30, b.y + 30], [b.x + 130, b.y + 90]);
  const rect = rects(page).first();

  // Select layer lists what is under the pointer, topmost first, and selects the pick.
  await rect.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Select layer" }).hover();
  const sub = page.getByRole("menu").last();
  await expect(sub.getByRole("menuitem")).toHaveCount(2);
  await expect(sub.getByRole("menuitem").first()).toContainText("Rectangle");
  await shot(page, "select-layer");
  await sub.getByRole("menuitem").nth(1).click();
  await expect(selected(page)).toHaveCount(1);
  await expect(selected(page)).not.toContainText("Rectangle");

  // Copy as PNG / SVG and Copy link go to the system clipboard (Playwright's WebKit can't grant it).
  await rect.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Copy as" }).hover();
  await shot(page, "copy-as");
  if (browserName === "chromium") {
    await page.getByRole("menuitem", { name: "SVG" }).click();
    await expect(page.getByTestId("toast")).toHaveText("Copied SVG");
    expect(await page.evaluate(() => navigator.clipboard.readText())).toMatch(/^<svg[\s>]/);

    await rect.click({ button: "right" });
    await page.getByRole("menuitem", { name: "Copy as" }).hover();
    await page.getByRole("menuitem", { name: "PNG" }).click();
    await expect(page.getByTestId("toast")).toHaveText("Copied PNG");
    const types = await page.evaluate(async () => (await navigator.clipboard.read()).flatMap((i) => i.types));
    expect(types).toContain("image/png");

    await page.keyboard.press("ControlOrMeta+Shift+c");
    await expect(page.getByTestId("toast")).toHaveText("Copied PNG");

    await rect.click({ button: "right" });
    await page.getByRole("menuitem", { name: "Copy link" }).click();
    await expect(page.getByTestId("toast")).toHaveText("Copied link");
    expect(await page.evaluate(() => navigator.clipboard.readText())).toMatch(/^spark:\/\/file\/[^/]+\/p-[^/?]+\?node=\S+$/);
  } else {
    await page.keyboard.press("Escape");
    await page.keyboard.press("Escape");
  }

  // An image layer offers "Open image in new tab".
  await page.getByTestId("image-input").setInputFiles({ name: "dot.png", mimeType: "image/png", buffer: Buffer.from(PNG, "base64") });
  await expect(layer(page, "dot")).toBeVisible();
  const image = rects(page).last();
  await expect(image).toHaveCSS("background-image", /\/assets\//);
  await image.click({ button: "right" });
  const popup = page.waitForEvent("popup");
  await page.getByRole("menuitem", { name: "Open image in new tab" }).click();
  expect((await popup).url()).toMatch(/\/assets\/[0-9a-f]{32}/);
  // A plain rectangle has no such item.
  await rect.click({ button: "right" });
  await expect(page.getByRole("menuitem", { name: "Open image in new tab" })).toHaveCount(0);
});

test("adjust text steps size, weight and spacing", async ({ page }) => {
  await newFile(page);
  await pasteHtml(page, '<div style="display:flex;padding:12px;background:#eeeeee"><p style="font-size:18px;font-weight:400;line-height:1.2">Adjust me</p></div>');
  const text = page.locator('[data-kind="text"]', { hasText: "Adjust me" });
  await expect(text).toBeVisible();
  const adjust = async (item: string) => {
    await text.click({ button: "right" });
    await page.getByRole("menuitem", { name: "Adjust text" }).hover();
    await page.getByRole("menuitem", { name: item }).click();
  };
  await adjust("Increase font size");
  await expect(text).toHaveCSS("font-size", "19px");
  await adjust("Heavier weight");
  await expect(text).toHaveCSS("font-weight", "500");
  await adjust("Lighter weight");
  await adjust("Lighter weight");
  await expect(text).toHaveCSS("font-weight", "300");
  await adjust("Increase line height");
  await expect(text).toHaveCSS("line-height", /^24\.(7|69)/);
  await adjust("Increase letter spacing");
  await expect(text).toHaveCSS("letter-spacing", "0.19px");
  await text.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Adjust text" }).hover();
  await shot(page, "adjust-text");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");

  // Not offered for a layer without text.
  await page.keyboard.press("Escape");
  const b = await boardBox(page);
  await drawRect(page, [b.x + 30, b.y + 200], [b.x + 130, b.y + 260]);
  await rects(page).first().click({ button: "right" });
  await expect(page.getByRole("menuitem", { name: "Adjust text" })).toHaveCount(0);
});

test("empty canvas has its own short menu; layer rows share the node menu", async ({ page }) => {
  await newFile(page);
  const b = await boardBox(page);
  await drawRect(page, [b.x + 30, b.y + 30], [b.x + 130, b.y + 90]);
  await drawRect(page, [b.x + 200, b.y + 30], [b.x + 300, b.y + 90]);

  // Right-click outside every artboard.
  await page.mouse.click(b.x - 30, b.y + b.height + 30, { button: "right" });
  const menu = page.getByRole("menu", { name: "Canvas menu" });
  await expect(menu.getByRole("menuitem")).toHaveCount(5);
  for (const name of ["Paste", "Paste here", "Select next artboard", "Select previous artboard", "Collapse all layers"]) {
    await expect(menu.getByText(name, { exact: true })).toBeVisible();
  }
  await expect(menu.getByText("Copy", { exact: true })).toHaveCount(0);
  await shot(page, "empty-canvas");
  await menu.getByRole("menuitem", { name: "Select next artboard" }).click();
  await expect(selected(page)).toHaveCount(1);
  await expect(selected(page)).not.toContainText("Rectangle");

  // Collapse all layers hides the rectangles' rows and keeps them hidden while they stay selected.
  await expect(layer(page, "Rectangle")).toHaveCount(2);
  await page.mouse.click(b.x - 30, b.y + b.height + 30, { button: "right" });
  await menu.getByRole("menuitem", { name: "Collapse all layers" }).click();
  await expect(layer(page, "Rectangle")).toHaveCount(0);

  // Expand again by selecting a rectangle; right-click a layer row: it gets selected and the node menu opens.
  await rects(page).nth(0).click({ modifiers: ["ControlOrMeta"] });
  await expect(layer(page, "Rectangle")).toHaveCount(2);
  await layer(page, "Rectangle").nth(0).click({ button: "right" });
  await expect(menu.getByText("Copy", { exact: true })).toBeVisible();
  await expect(menu.getByRole("menuitem", { name: "Select layer" })).toHaveCount(0);
  await shot(page, "layer-row");
  // An open menu hides the page from the accessibility tree, so look at the rows directly.
  const rows = page.locator('[data-layer][aria-selected="true"]');
  await expect(rows).toHaveCount(1);
  await expect(rows).toContainText("Rectangle");
  await page.keyboard.press("Escape");

  // Right-clicking a row that is already part of a multi-selection keeps the selection.
  await layer(page, "Rectangle").nth(1).click({ modifiers: ["Shift"] });
  await expect(selected(page)).toHaveCount(2);
  await layer(page, "Rectangle").nth(1).click({ button: "right" });
  await expect(menu).toBeVisible();
  await expect(rows).toHaveCount(2);
});
