import { mkdirSync } from "node:fs";
import { type Page, expect, test } from "@playwright/test";
import { camera, newFile } from "./util";

const SHOTS = "/tmp/paper-parity/c";
mkdirSync(SHOTS, { recursive: true });

type Box = { x: number; y: number; width: number; height: number };

async function boardBox(page: Page): Promise<Box> {
  const b = await page.locator("[data-node]").first().boundingBox();
  if (!b) throw new Error("no artboard");
  return b;
}

async function drag(page: Page, from: [number, number], to: [number, number]) {
  await page.mouse.move(...from);
  await page.mouse.down();
  await page.mouse.move(to[0], to[1], { steps: 8 });
  await page.mouse.up();
}

/** Draws a frame or rect with its tool and returns it selected. */
async function draw(page: Page, tool: "f" | "r", from: [number, number], to: [number, number]) {
  await page.keyboard.press(tool);
  await drag(page, from, to);
  // The tool goes back to Move once the server has created the node; let it be laid out and measured.
  await expect(page.getByRole("button", { name: "Move", exact: true })).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator('[role="treeitem"][aria-selected="true"]')).toHaveCount(1);
  // A new node takes a moment before the browser hit-tests it.
  const at = [(from[0] + to[0]) / 2, (from[1] + to[1]) / 2];
  await expect
    .poll(() =>
      page.evaluate(([x, y]) => {
        const hit = document.elementFromPoint(x, y)?.closest("[data-node]");
        return !!hit && hit !== document.querySelector("[data-node]");
      }, at),
    )
    .toBe(true);
}

/** How many HUD pixels in a viewport box are drawn (not transparent). */
async function ink(page: Page, b: Box) {
  return page.getByTestId("hud").evaluate((c: HTMLCanvasElement, b) => {
    const r = c.getBoundingClientRect();
    const k = c.width / r.width;
    const [x, y] = [Math.round((b.x - r.left) * k), Math.round((b.y - r.top) * k)];
    const d = c.getContext("2d")?.getImageData(x, y, Math.max(1, Math.round(b.width * k)), Math.max(1, Math.round(b.height * k))).data ?? [];
    let n = 0;
    for (let i = 3; i < d.length; i += 4) if (d[i]) n++;
    return n;
  }, b);
}

const field = (page: Page, title: string) => page.getByTitle(title, { exact: true }).locator("input");
const num = async (page: Page, title: string) => Number.parseFloat(await field(page, title).inputValue());
const nodeStyle = (page: Page, sel: string, prop: "left" | "top") => page.locator(sel).first().evaluate((el, p) => (el as HTMLElement).style[p as "left"], prop);

async function shots(page: Page, name: string) {
  // Switch the theme in place (a click would blur an open field), shoot both, and restore.
  const first = (await page.evaluate(() => document.documentElement.dataset.theme)) ?? "light";
  const other = first === "dark" ? "light" : "dark";
  for (const theme of [first, other]) {
    await page.evaluate((t) => {
      document.documentElement.dataset.theme = t;
      window.dispatchEvent(new Event("spark:theme"));
    }, theme);
    await page.waitForTimeout(150);
    if (theme !== first || name) await page.screenshot({ path: `${SHOTS}/${name}-${theme}.png` });
  }
}

test("Alt measures the distance from the selection to what it hovers", async ({ page }) => {
  await newFile(page);
  const b = await boardBox(page);
  await draw(page, "r", [b.x + 80, b.y + 80], [b.x + 180, b.y + 160]);
  await draw(page, "r", [b.x + 320, b.y + 120], [b.x + 440, b.y + 220]);
  await page.mouse.click(b.x + 130, b.y + 120);
  const hover: [number, number] = [b.x + 380, b.y + 170];
  const gap: Box = { x: b.x + 190, y: b.y + 110, width: 120, height: 40 };

  await page.mouse.move(...hover);
  expect(await ink(page, gap)).toBe(0);
  await page.keyboard.down("Alt");
  await page.mouse.move(hover[0] + 1, hover[1] + 1);
  await expect.poll(() => ink(page, gap)).toBeGreaterThan(40);
  await shots(page, "measure");
  await page.keyboard.up("Alt");
  await expect.poll(() => ink(page, gap)).toBe(0);
});

test("flex frames show padding and gap areas with handles that edit them in one undo step", async ({ page }) => {
  await newFile(page);
  const b = await boardBox(page);
  await draw(page, "f", [b.x + 60, b.y + 60], [b.x + 460, b.y + 260]);
  await draw(page, "r", [b.x + 90, b.y + 100], [b.x + 190, b.y + 200]);
  await draw(page, "r", [b.x + 250, b.y + 100], [b.x + 350, b.y + 200]);
  await page.getByRole("treeitem", { name: /Frame$/ }).last().click();
  await page.keyboard.press("Shift+a");
  await expect(page.getByRole("button", { name: "Flex", exact: true })).toHaveAttribute("aria-pressed", "true");

  const frame = page.locator('[data-kind="frame"] [data-kind="frame"]').first();
  const fb = await frame.boundingBox();
  if (!fb) throw new Error("no frame");
  const [z] = await camera(page);
  // Padding areas are drawn inside the frame's edge.
  expect(await ink(page, { x: fb.x + fb.width / 2 - 40, y: fb.y + 2, width: 80, height: 6 })).toBeGreaterThan(20);

  const pt = await num(page, "Padding top");
  const top = Math.max((pt * z) / 2, 12);
  await drag(page, [fb.x + fb.width / 2, fb.y + top], [fb.x + fb.width / 2, fb.y + top + 24]);
  const after = await num(page, "Padding top");
  expect(after).toBe(Math.round(pt + 24 / z));
  await page.keyboard.press("ControlOrMeta+z");
  await expect.poll(() => num(page, "Padding top")).toBe(pt);

  // The gap handle sits between the two children.
  const kids = page.locator('[data-kind="frame"] [data-kind="frame"] > [data-kind="rect"]');
  const [a, c] = await Promise.all([kids.nth(0).boundingBox(), kids.nth(1).boundingBox()]);
  if (!a || !c) throw new Error("no children");
  const gap = await num(page, "Gap");
  const gx = (a.x + a.width + c.x) / 2;
  await drag(page, [gx, fb.y + fb.height / 2], [gx + 30, fb.y + fb.height / 2]);
  expect(await num(page, "Gap")).toBe(Math.round(gap + 30 / z));
  await page.keyboard.press("ControlOrMeta+z");
  await expect.poll(() => num(page, "Gap")).toBe(gap);

  // Children get dashed outlines.
  expect(await ink(page, { x: a.x - 2, y: a.y + 20, width: 4, height: 40 })).toBeGreaterThan(10);
  await shots(page, "flex");
});

test("pixel grid and snap to pixel toggle from the keyboard", async ({ page }) => {
  await newFile(page);
  const b = await boardBox(page);
  await draw(page, "r", [b.x + 100, b.y + 100], [b.x + 200, b.y + 180]);
  const rect = '[data-kind="rect"]';

  // Snap to pixel is on: a drag lands on whole pixels; off keeps the fraction.
  await drag(page, [b.x + 150, b.y + 140], [b.x + 233, b.y + 177]);
  await expect.poll(() => nodeStyle(page, rect, "left")).toMatch(/^-?\d+px$/);
  expect(await nodeStyle(page, rect, "top")).toMatch(/^-?\d+px$/);
  await page.keyboard.press("ControlOrMeta+Shift+'");
  const rb = await page.locator(rect).first().boundingBox();
  if (!rb) throw new Error("no rect");
  await drag(page, [rb.x + 30, rb.y + 30], [rb.x + 30 + 41, rb.y + 30 + 17]);
  await expect.poll(() => nodeStyle(page, rect, "left")).toMatch(/\./);
  await page.keyboard.press("ControlOrMeta+Shift+'");

  // The grid shows from 800%: toggled with Shift+' and Mod+'.
  const canvas = await page.getByTestId("canvas").boundingBox();
  if (!canvas) throw new Error("no canvas");
  // Deselect, and look at canvas outside the artboard.
  await page.mouse.click(canvas.x + canvas.width - 20, canvas.y + canvas.height - 20);
  const spot: Box = { x: canvas.x + canvas.width - 150, y: canvas.y + canvas.height - 150, width: 100, height: 100 };
  expect(await ink(page, spot)).toBe(0);
  for (let i = 0; i < 6; i++) await page.keyboard.press("ControlOrMeta+=");
  await expect.poll(async () => (await camera(page))[0]).toBeGreaterThanOrEqual(8);
  await expect.poll(() => ink(page, spot)).toBeGreaterThan(100);
  await shots(page, "pixel-grid");
  await page.keyboard.press("Shift+'");
  await expect.poll(() => ink(page, spot)).toBe(0);
  await page.keyboard.press("ControlOrMeta+'");
  await expect.poll(() => ink(page, spot)).toBeGreaterThan(100);
});

test("layout guides live in the file, draw on the frame and hide with Shift+G", async ({ page }) => {
  await newFile(page);
  await page.getByRole("treeitem", { name: /Desktop/ }).click();
  const section = page.getByRole("region", { name: "Layout guides" });
  await section.getByRole("button", { name: "Add layout guides" }).click();
  await expect(section.getByTestId("layout-guide")).toHaveCount(1);
  const b = await boardBox(page);
  const strip: Box = { x: b.x + b.width / 2 - 20, y: b.y + 100, width: 40, height: 30 };
  await page.keyboard.press("Escape");
  await expect.poll(() => ink(page, strip)).toBeGreaterThan(100);
  await page.keyboard.press("Shift+g");
  await expect.poll(() => ink(page, strip)).toBe(0);
  await page.keyboard.press("Shift+g");
  await expect.poll(() => ink(page, strip)).toBeGreaterThan(100);

  // Edit, add a grid, and keep them across a reload.
  await page.getByRole("treeitem", { name: /Desktop/ }).click();
  const count = section.getByTitle("Count").locator("input");
  await count.fill("4");
  await count.press("Enter");
  await expect(count).toHaveValue("4");
  await section.getByRole("button", { name: "Add layout guides" }).click();
  await expect(section.getByTestId("layout-guide")).toHaveCount(2);
  await section.getByRole("combobox", { name: "Guide type" }).last().selectOption("grid");
  await shots(page, "layout-guides");
  await page.reload();
  await page.getByRole("treeitem", { name: /Desktop/ }).click();
  await expect(section.getByTestId("layout-guide")).toHaveCount(2);
  await expect(section.getByTitle("Count").locator("input")).toHaveValue("4");
  await section.getByRole("button", { name: "Remove layout guide" }).first().click();
  await section.getByRole("button", { name: "Remove layout guide" }).first().click();
  await expect(section.getByTestId("layout-guide")).toHaveCount(0);
});

test("dragging outside a corner rotates, Shift snaps to 15 degrees", async ({ page }) => {
  await newFile(page);
  const b = await boardBox(page);
  await draw(page, "r", [b.x + 200, b.y + 200], [b.x + 300, b.y + 280]);
  const r = await page.locator('[data-kind="rect"]').first().boundingBox();
  if (!r) throw new Error("no rect");
  const corner: [number, number] = [r.x + r.width + 10, r.y + r.height + 10];
  const cx = r.x + r.width / 2;
  const cy = r.y + r.height / 2;

  await page.mouse.move(...corner);
  await expect.poll(() => page.getByTestId("canvas").evaluate((el) => (el as HTMLElement).style.cursor)).toContain("svg");
  // Quarter turn clockwise: from the bottom-right corner to the bottom-left.
  const from = Math.atan2(corner[1] - cy, corner[0] - cx);
  const to = from + Math.PI / 2 + 0.1;
  const radius = Math.hypot(corner[0] - cx, corner[1] - cy);
  await page.mouse.down();
  await page.mouse.move(cx + radius * Math.cos(to), cy + radius * Math.sin(to), { steps: 10 });
  await page.mouse.up();
  const angle = await num(page, "Rotation");
  expect(angle).toBeGreaterThan(85);
  expect(angle).toBeLessThan(100);
  await shots(page, "rotate");

  // Undo is one step; with Shift the angle lands on a multiple of 15.
  await page.keyboard.press("ControlOrMeta+z");
  await expect.poll(() => num(page, "Rotation")).toBe(0);
  const r2 = await page.locator('[data-kind="rect"]').first().boundingBox();
  if (!r2) throw new Error("no rect");
  const c2 = [r2.x + r2.width / 2, r2.y + r2.height / 2];
  const start: [number, number] = [r2.x + r2.width + 10, r2.y + r2.height + 10];
  await page.keyboard.down("Shift");
  await page.mouse.move(...start);
  await page.mouse.down();
  await page.mouse.move(start[0] - 30, start[1] + 25, { steps: 8 });
  await page.mouse.up();
  await page.keyboard.up("Shift");
  const snapped = await num(page, "Rotation");
  expect(snapped).not.toBe(0);
  expect(Math.abs(snapped % 15)).toBe(0);
  expect(c2.length).toBe(2);
});

test("Z zooms: drag a rectangle, click in, Alt+click out", async ({ page }) => {
  await newFile(page);
  const canvas = await page.getByTestId("canvas").boundingBox();
  if (!canvas) throw new Error("no canvas");
  await expect.poll(async () => (await camera(page))[0]).toBeGreaterThan(0.1);
  const [z0] = await camera(page);
  const at: [number, number] = [canvas.x + 400, canvas.y + 300];

  await page.mouse.move(...at);
  await page.keyboard.down("z");
  await expect.poll(() => page.getByTestId("canvas").evaluate((el) => (el as HTMLElement).style.cursor)).toBe("zoom-in");
  await page.mouse.click(...at);
  await expect.poll(async () => (await camera(page))[0]).toBeCloseTo(z0 * 2, 2);
  await page.keyboard.down("Alt");
  await expect.poll(() => page.getByTestId("canvas").evaluate((el) => (el as HTMLElement).style.cursor)).toBe("zoom-out");
  await page.mouse.click(...at);
  await expect.poll(async () => (await camera(page))[0]).toBeCloseTo(z0, 2);
  await page.keyboard.up("Alt");

  await drag(page, [at[0], at[1]], [at[0] + 80, at[1] + 50]);
  await expect.poll(async () => (await camera(page))[0]).toBeGreaterThan(z0 * 4);
  await page.keyboard.up("z");
  await expect.poll(() => page.getByTestId("canvas").evaluate((el) => (el as HTMLElement).style.cursor)).toBe("");
});

test("double-clicking an artboard's name above it renames it in place", async ({ page }) => {
  await newFile(page);
  const b = await boardBox(page);
  await page.mouse.dblclick(b.x + 20, b.y - 9);
  const name = page.getByRole("textbox", { name: "Name" });
  await expect(name).toHaveValue("Desktop");
  await shots(page, "artboard-rename");
  await name.fill("Landing hero");
  await name.press("Enter");
  await expect(page.getByRole("treeitem", { name: /Landing hero/ })).toBeVisible();
  await expect(name).toHaveCount(0);
});
