import { expect, test } from "@playwright/test";
import { camera, newFile } from "./util";

test("create a file, see its artboard, pan and zoom, and reload", async ({ page }) => {
  await newFile(page);
  const board = page.locator("[data-node]").first();
  await expect(board).toHaveCSS("width", "1440px");
  await expect(page.getByRole("treeitem", { name: /Desktop/ })).toBeVisible();

  const canvas = page.getByTestId("canvas");
  const box = await canvas.boundingBox();
  if (!box) throw new Error("no canvas");
  await expect.poll(async () => (await camera(page))[0]).toBeGreaterThan(0.1);
  const [z0] = await camera(page);

  // Mod+wheel zooms around the cursor.
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.keyboard.down("Control");
  await page.mouse.wheel(0, -200);
  await page.keyboard.up("Control");
  await expect.poll(async () => (await camera(page))[0]).toBeGreaterThan(z0);
  await expect(page.getByTestId("zoom")).not.toHaveText(`${Math.round(z0 * 100)}%`);

  // Plain wheel pans.
  const [, x1, y1] = await camera(page);
  await page.mouse.wheel(120, 80);
  await expect.poll(async () => (await camera(page))[1]).toBeCloseTo(x1 - 120, 0);
  expect((await camera(page))[2]).toBeCloseTo(y1 - 80, 0);

  // Space + drag pans too.
  const [, x2, y2] = await camera(page);
  await page.keyboard.down("Space");
  await page.mouse.move(box.x + 300, box.y + 300);
  await page.mouse.down();
  await page.mouse.move(box.x + 350, box.y + 260, { steps: 4 });
  await page.mouse.up();
  await page.keyboard.up("Space");
  await expect.poll(async () => (await camera(page))[1]).toBeCloseTo(x2 + 50, 0);
  expect((await camera(page))[2]).toBeCloseTo(y2 - 40, 0);

  // Shift+1 zooms to fit again.
  await page.keyboard.press("Shift+Digit1");
  await expect.poll(async () => (await camera(page))[0]).toBeCloseTo(z0, 2);

  await page.reload();
  await expect(page.getByTestId("file-tab")).toHaveCount(1);
  await expect(page.locator("[data-node]").first()).toHaveCSS("width", "1440px");
  await expect(page.getByRole("treeitem", { name: /Desktop/ })).toBeVisible();
});

test("clicking an empty artboard selects it and the HUD shows its size", async ({ page }) => {
  await newFile(page);
  const board = page.locator("[data-node]").first();
  const b = await board.boundingBox();
  if (!b) throw new Error("no board");
  await page.mouse.click(b.x + b.width / 2, b.y + b.height / 2);
  await expect(page.getByRole("treeitem", { name: /Desktop/ })).toHaveAttribute("aria-selected", "true");
  // Click on empty canvas clears the selection.
  const canvas = await page.getByTestId("canvas").boundingBox();
  if (!canvas) throw new Error("no canvas");
  await page.mouse.click(canvas.x + 10, canvas.y + canvas.height - 10);
  await expect(page.getByRole("treeitem", { name: /Desktop/ })).toHaveAttribute("aria-selected", "false");
});

test("zoomed-out artboards keep their preview until they are tiny", async ({ page }) => {
  await newFile(page);
  const board = page.locator('[data-testid="camera"] > [data-node]').first();
  await page.keyboard.press("Shift+Digit1");
  let sawPreview = false;
  for (let i = 0; i < 12; i++) {
    await page.keyboard.press("ControlOrMeta+Minus");
    const box = await board.boundingBox();
    if (!box) throw new Error("no artboard");
    const size = Math.max(box.width, box.height);
    // Previews mount when the gesture settles; shells are the fallback only below 32 px.
    if (size >= 40) {
      await expect(board).not.toHaveAttribute("data-shell", "");
      sawPreview ||= size < 160;
    } else if (size < 28) {
      await expect(board).toHaveAttribute("data-shell", "");
      break;
    }
  }
  expect(sawPreview).toBe(true);
});
