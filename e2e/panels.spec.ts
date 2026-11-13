import { type Page, expect, test } from "@playwright/test";
import { callJson, connectAgent } from "./mcp";
import { camera, newFile } from "./util";

const SHOTS = "test-results/shots/panels";
/** Screenshots a locator (or the page) in light and dark. */
async function shots(page: Page, name: string, target?: ReturnType<Page["locator"]>) {
  for (const theme of ["light", "dark"]) {
    await page.evaluate((t) => document.documentElement.setAttribute("data-theme", t), theme);
    await (target ?? page).screenshot({ path: `${SHOTS}/${name}-${theme}.png` });
  }
  await page.evaluate(() => document.documentElement.setAttribute("data-theme", "light"));
}

const pagesPanel = (page: Page) => page.getByRole("region", { name: "Pages" });
const row = (page: Page, name: string) => pagesPanel(page).locator('[role="button"]').filter({ has: page.getByText(name, { exact: true }) });
const rows = (page: Page) => pagesPanel(page).locator('li [role="button"]');

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
});

test("page rows: icons, duplicate, reorder, current checkmark, collapse, agent dot", async ({ page }) => {
  await newFile(page);
  const pages = pagesPanel(page);

  // The current page row has a checkmark and stays bold.
  const first = row(page, "Page 1");
  await expect(first.getByTestId("page-current")).toBeVisible();
  await expect(first).toHaveCSS("font-weight", /5\d\d|[6-9]\d\d/);

  // Icon submenu.
  await first.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Icon" }).hover();
  await expect(page.getByRole("menu", { name: "Icon" }).getByRole("menuitem")).toHaveCount(17);
  await shots(page, "page-icon-menu");
  await page.getByRole("menuitem", { name: "Pie chart" }).click();
  await expect(first.locator("svg.lucide-chart-pie")).toBeVisible();

  // Duplicate: copies the page and its artboards, one undo step.
  await first.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Duplicate page" }).click();
  const copy = row(page, "Page 1 copy");
  await expect(copy).toBeVisible();
  await expect(copy.locator("svg.lucide-chart-pie")).toBeVisible();
  await expect(copy.getByTestId("page-current")).toBeVisible();
  await expect(page.locator("[data-node]").first()).toBeVisible();
  await page.getByRole("button", { name: "Add page" }).click();
  await expect(rows(page)).toHaveCount(3);
  await page.getByRole("button", { name: "Add page" }).click();
  await page.keyboard.press("ControlOrMeta+z");
  await page.keyboard.press("ControlOrMeta+z");
  await expect(rows(page)).toHaveCount(2);
  await page.keyboard.press("ControlOrMeta+z");
  await expect(rows(page)).toHaveCount(1);

  // Drag a row to reorder (copy comes back with redo, then moves above the original).
  await page.keyboard.press("ControlOrMeta+Shift+z");
  await expect(rows(page)).toHaveCount(2);
  const order = () => rows(page).evaluateAll((els) => els.map((e) => e.textContent?.trim()));
  expect(await order()).toEqual(["Page 1", "Page 1 copy"]);
  await rows(page).nth(1).dragTo(rows(page).nth(0), { targetPosition: { x: 40, y: 3 } });
  await expect.poll(order).toEqual(["Page 1 copy", "Page 1"]);
  await page.reload();
  await expect.poll(order).toEqual(["Page 1 copy", "Page 1"]);

  // Collapse the section; it stays collapsed after a reload.
  await shots(page, "pages-panel", page.getByRole("complementary", { name: "Pages and layers" }));
  await pages.getByRole("button", { name: "Pages", exact: true }).click();
  await expect(rows(page)).toHaveCount(0);
  await page.reload();
  await expect(pagesPanel(page).getByRole("button", { name: "Pages", exact: true })).toHaveAttribute("aria-expanded", "false");
  await expect(rows(page)).toHaveCount(0);
  await pagesPanel(page).getByRole("button", { name: "Pages", exact: true }).click();
  await expect(rows(page)).toHaveCount(2);

  // An agent working on this page's artboard puts a dot on the row.
  const agent = await connectAgent("dot-agent");
  try {
    await callJson(agent, "create_artboard", { name: "Agent board", styles: { width: "200px", height: "100px" } });
    await expect(pages.getByTestId("page-busy")).toHaveCount(1);
    await shots(page, "page-agent-dot", page.getByRole("complementary", { name: "Pages and layers" }));
    await callJson(agent, "finish_working_on_nodes");
    await expect(pages.getByTestId("page-busy")).toHaveCount(0);
  } finally {
    await agent.close();
  }
});

test("layer search filters by name, Enter selects the first match, Esc closes", async ({ page }) => {
  await newFile(page);
  const agent = await connectAgent("search-agent");
  try {
    const board = await callJson<{ id: string }>(agent, "create_artboard", { name: "Landing", styles: { width: "400px", height: "300px" } });
    await callJson(agent, "write_html", {
      targetNodeId: board.id,
      html: '<div layer-name="Hero"><p layer-name="Headline">Hi</p><p layer-name="Subtitle">There</p></div><div layer-name="Footer"><p layer-name="Copyright">c</p></div>',
    });
    await callJson(agent, "finish_working_on_nodes");
  } finally {
    await agent.close();
  }
  const layer = (name: string) => page.getByRole("treeitem", { name });
  await expect(layer("Landing")).toBeVisible();

  await page.getByTestId("canvas").click({ position: { x: 5, y: 5 } });
  await page.keyboard.press("ControlOrMeta+f");
  const search = page.getByRole("textbox", { name: "Search layers" });
  await expect(search).toBeFocused();
  await search.fill("head");
  // The match and its ancestors show; siblings and other branches don't.
  await expect(layer("Headline")).toBeVisible();
  await expect(layer("Hero")).toBeVisible();
  await expect(layer("Landing")).toBeVisible();
  await expect(layer("Subtitle")).toHaveCount(0);
  await expect(layer("Footer")).toHaveCount(0);
  await shots(page, "layer-search", page.getByRole("complementary", { name: "Pages and layers" }));
  await search.press("Enter");
  await expect(layer("Headline")).toHaveAttribute("aria-selected", "true");
  await search.press("Escape");
  await expect(search).toHaveCount(0);
  await expect(layer("Footer")).toBeVisible();
});

test("the file name opens the main menu; sidebar, hide-UI and floating modes", async ({ page }) => {
  await newFile(page);
  const commands: string[] = [];
  await page.exposeFunction("seen", (c: string) => commands.push(c));
  await page.evaluate(() => window.addEventListener("spark:command", (e) => (window as unknown as { seen: (c: string) => void }).seen((e as CustomEvent<string>).detail)));

  const fileMenu = page.getByRole("button", { name: "File menu" });
  await fileMenu.click();
  const menu = page.getByRole("menu", { name: "File menu" });
  await expect(menu.getByRole("menuitem")).toHaveText([/Rename file/, /Duplicate file/, /Keyboard shortcuts/, /Settings/]);
  await shots(page, "file-menu", page.locator("body"));
  await menu.getByRole("menuitem", { name: "Keyboard shortcuts" }).click();
  await expect(page.getByRole("dialog", { name: "Keyboard shortcuts" })).toBeVisible();
  await page.keyboard.press("Escape");

  // View toggles flip their checkbox and announce themselves by command name.
  await fileMenu.click();
  const pixel = menu.getByRole("menuitemcheckbox", { name: "Pixel grid" });
  await expect(pixel).toHaveAttribute("aria-checked", "true");
  await pixel.click();
  await expect(pixel).toHaveAttribute("aria-checked", "false");
  await menu.getByRole("menuitemcheckbox", { name: "Snap to pixel" }).click();
  await menu.getByRole("menuitemcheckbox", { name: "Layout guides" }).click();
  expect(commands).toEqual(expect.arrayContaining(["togglePixelGrid", "toggleSnapPixel", "toggleLayoutGuides"]));
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);

  // Settings opens the settings dialog.
  await fileMenu.click();
  await menu.getByRole("menuitem", { name: "Settings" }).click();
  await expect(page.getByRole("dialog", { name: "Settings" })).toBeVisible();
  await page.keyboard.press("Escape");

  // Rename file, and double-click still renames.
  await fileMenu.click();
  await menu.getByRole("menuitem", { name: "Rename file" }).click();
  const name = page.getByRole("textbox", { name: "Name" });
  await name.fill("Renamed");
  await name.press("Enter");
  await expect(page.getByRole("button", { name: "File menu" })).toContainText("Renamed");
  await page.getByRole("button", { name: "File menu" }).dblclick();
  await expect(name).toBeVisible();
  await name.press("Escape");

  // Duplicate file opens a copy in a new tab.
  await fileMenu.click();
  await menu.getByRole("menuitem", { name: "Duplicate file" }).click();
  await expect(page.getByTestId("file-tab").filter({ hasText: "Renamed copy" })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("button", { name: "File menu" })).toContainText("Renamed copy");

  // Sidebar button hides the left panel and a button over the canvas brings it back.
  const left = page.getByRole("complementary", { name: "Pages and layers" });
  await page.getByRole("button", { name: "Hide sidebar" }).click();
  await expect(left).toHaveCount(0);
  await expect(page.getByRole("navigation", { name: "Tools" })).toBeVisible();
  await shots(page, "sidebar-hidden");
  await page.getByRole("button", { name: "Show sidebar" }).click();
  await expect(left).toBeVisible();

  // "." hides everything; "." again shows it.
  const inspector = page.getByRole("complementary", { name: "Inspector" });
  await page.getByTestId("canvas").click({ position: { x: 5, y: 5 } });
  await page.keyboard.press(".");
  await expect(left).toHaveCount(0);
  await expect(inspector).toHaveCount(0);
  await expect(page.getByRole("navigation", { name: "Tools" })).toHaveCount(0);
  await shots(page, "ui-hidden");
  await page.keyboard.press("ControlOrMeta+\\");
  await expect(left).toBeVisible();
  await expect(inspector).toBeVisible();

  // Shift+. floats the panels over the canvas as cards.
  await page.keyboard.press("Shift+.");
  await expect(left.locator("xpath=..")).toHaveCSS("position", "absolute");
  await expect(inspector.locator("xpath=..")).toHaveCSS("position", "absolute");
  await expect(page.getByRole("navigation", { name: "Tools" }).locator("xpath=..")).toHaveCSS("position", "absolute");
  await shots(page, "floating");
  await page.keyboard.press("Shift+.");
  await expect(left.locator("xpath=..")).not.toHaveCSS("position", "absolute");
});

test("toolbar tooltips show the tool name and shortcut keycaps", async ({ page }) => {
  await newFile(page);
  await page.getByRole("button", { name: "Rectangle" }).hover();
  const tip = page.getByRole("tooltip");
  await expect(tip).toContainText("Rectangle");
  await expect(tip.locator("kbd")).toHaveText("R");
  await shots(page, "toolbar-tooltip", page.getByRole("navigation", { name: "Tools" }).locator("xpath=../.."));
});

test("zoom menu closes on Esc and outside click, shows keycaps; canvas preferences apply", async ({ page }) => {
  await newFile(page);
  const zoom = page.getByTestId("zoom");
  const menu = page.getByRole("menu");
  await zoom.click();
  // Zoom to fit shows its first binding, the bare "1" key.
  await expect(menu.getByRole("menuitem", { name: /Zoom to fit/ }).locator("kbd")).toHaveText(["1"]);
  await shots(page, "zoom-menu", page.locator("body"));
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  const box = (await page.getByTestId("canvas").boundingBox()) as { x: number; y: number; width: number; height: number };
  await zoom.click();
  await expect(menu).toBeVisible();
  await page.waitForTimeout(300); // Radix starts listening for outside clicks a tick after opening
  await page.mouse.click(box.x + 5, box.y + 5);
  await expect(menu).toHaveCount(0);

  const wheel = async () => {
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.wheel(0, 100);
  };

  // Default: the wheel pans.
  const [z0, x0, y0] = await camera(page);
  await wheel();
  await expect.poll(async () => (await camera(page))[2]).toBeLessThan(y0);
  expect((await camera(page))[0]).toBe(z0);
  void x0;

  // Scroll wheel zooms (from the zoom menu): scrolling down zooms out; inverted, it zooms in.
  await zoom.click();
  await menu.getByRole("menuitemcheckbox", { name: "Scroll wheel zooms" }).click();
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  const z1 = (await camera(page))[0];
  await wheel();
  await expect.poll(async () => (await camera(page))[0]).toBeLessThan(z1);
  await zoom.click();
  await menu.getByRole("menuitemcheckbox", { name: "Invert zoom direction" }).click();
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  const z2 = (await camera(page))[0];
  await wheel();
  await expect.poll(async () => (await camera(page))[0]).toBeGreaterThan(z2);
  await zoom.click();
  await expect(menu.getByRole("menuitemcheckbox", { name: "Invert zoom direction" })).toHaveAttribute("aria-checked", "true");
  await shots(page, "zoom-menu-prefs", page.locator("body"));
  await page.keyboard.press("Escape");

  // Preferences persist across a reload.
  await page.reload();
  await page.getByTestId("zoom").click();
  await expect(menu.getByRole("menuitemcheckbox", { name: "Scroll wheel zooms" })).toHaveAttribute("aria-checked", "true");
  await menu.getByRole("menuitemcheckbox", { name: "Scroll wheel zooms" }).click();
  await menu.getByRole("menuitemcheckbox", { name: "Invert zoom direction" }).click();
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);

  // Right-click drag pans (and a plain right-click still opens the canvas menu).
  await page.getByTestId("zoom").click();
  await menu.getByRole("menuitemcheckbox", { name: "Right-click drag pans" }).click();
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  const [, px, py] = await camera(page);
  await page.mouse.move(box.x + 300, box.y + 300);
  await page.mouse.down({ button: "right" });
  await page.mouse.move(box.x + 380, box.y + 340, { steps: 4 });
  await page.mouse.up({ button: "right" });
  await expect.poll(async () => (await camera(page)).slice(1)).toEqual([px + 80, py + 40]);
  await expect(page.getByRole("menu", { name: "Canvas menu" })).toHaveCount(0);
  await page.mouse.click(box.x + 300, box.y + 300, { button: "right" });
  await expect(page.getByRole("menu", { name: "Canvas menu" })).toBeVisible();
  await page.keyboard.press("Escape");
});

test("reduce-motion, deep select and nudge steps from Settings", async ({ page }) => {
  await newFile(page);
  const b = (await page.locator("[data-node]").first().boundingBox()) as { x: number; y: number };
  // A rectangle grouped into a frame.
  await page.keyboard.press("r");
  await page.mouse.move(b.x + 100, b.y + 100);
  await page.mouse.down();
  await page.mouse.move(b.x + 200, b.y + 200, { steps: 4 });
  await page.mouse.up();
  const rect = page.getByRole("treeitem", { name: /^Rectangle$/ });
  await expect(rect).toHaveAttribute("aria-selected", "true");
  await page.waitForTimeout(800); // the server needs the rectangle's measured size before it can group it
  await page.mouse.click(b.x + 150, b.y + 150, { button: "right" });
  await page.getByRole("menuitem", { name: "Group in frame" }).click();
  const frame = page.getByRole("treeitem", { name: /Frame$/ });
  await expect(frame).toHaveAttribute("aria-selected", "true");
  const at = async () => {
    const r = (await page.locator('[data-kind="rect"]').first().boundingBox()) as { x: number; y: number; width: number; height: number };
    return [r.x + r.width / 2, r.y + r.height / 2] as const;
  };
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");
  await expect(frame).toHaveAttribute("aria-selected", "false");
  // By default a click selects the artboard's child (the frame).
  await page.mouse.click(...(await at()));
  await expect(frame).toHaveAttribute("aria-selected", "true");

  await page.getByRole("button", { name: "Settings" }).click();
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(dialog.getByRole("region", { name: "Canvas" })).toBeVisible();
  await shots(page, "settings-canvas", page.locator("body"));
  await dialog.getByRole("spinbutton", { name: "Nudge step (px)" }).fill("3");
  await dialog.getByRole("spinbutton", { name: "Big nudge step, with Shift (px)" }).fill("20");
  await dialog.getByRole("checkbox", { name: "Reduce agent animations" }).check();
  await dialog.getByRole("checkbox", { name: "Select deepest layer by default" }).check();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(page.locator("html")).toHaveAttribute("data-reduce-motion", "");

  // Deep select: the same click now picks the rectangle.
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");
  await page.mouse.click(...(await at()));
  await expect(rect).toHaveAttribute("aria-selected", "true");

  // Nudges use the configured steps.
  const x = page.getByTestId("field-x");
  const x0 = Number(await x.inputValue());
  await page.keyboard.press("ArrowRight");
  await expect(x).toHaveValue(String(x0 + 3));
  await page.keyboard.press("Shift+ArrowRight");
  await expect(x).toHaveValue(String(x0 + 23));
});

test("side panels resize from their inner edges and keep the width; section chevrons line up", async ({ page }) => {
  await newFile(page);
  const left = page.getByRole("complementary", { name: "Pages and layers" });
  const right = page.getByRole("complementary", { name: "Inspector" });
  const width = async (panel: typeof left) => (await panel.boundingBox())?.width;

  // The Pages chevron sits in the same column as the layer rows' chevrons.
  const leftOf = (l: ReturnType<Page["locator"]>) => l.evaluate((el) => el.getBoundingClientRect().left);
  const pagesChevron = pagesPanel(page).getByRole("button", { name: "Pages" }).locator("svg");
  const layerChevron = page.getByRole("tree", { name: "Layers" }).locator('button[aria-label="Expand"], button[aria-label="Collapse"]').first();
  expect(await leftOf(pagesChevron)).toBe(await leftOf(layerChevron));

  async function drag(name: string, toX: number) {
    const h = await page.getByRole("separator", { name }).boundingBox();
    if (!h) throw new Error(`no ${name} handle`);
    await page.mouse.move(h.x + h.width / 2, h.y + 300);
    await page.mouse.down();
    await page.mouse.move(toX, h.y + 300, { steps: 6 });
    await page.mouse.up();
  }

  // Dragging clamps to each panel's range, and the canvas takes the rest.
  await drag("Resize pages and layers", 1000);
  await expect.poll(() => width(left)).toBe(480);
  await drag("Resize pages and layers", 0);
  await expect.poll(() => width(left)).toBe(160);
  await drag("Resize inspector", 0);
  await expect.poll(() => width(right)).toBe(480);
  await shots(page, "resized-panels");

  // Arrow keys step the focused handle by 8px.
  await page.getByRole("separator", { name: "Resize inspector" }).focus();
  await page.keyboard.press("ArrowRight");
  await expect.poll(() => width(right)).toBe(472);

  // Widths survive a reload.
  await page.reload();
  await expect.poll(() => width(left)).toBe(160);
  await expect.poll(() => width(right)).toBe(472);
});
