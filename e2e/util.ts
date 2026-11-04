import { type Page, expect } from "@playwright/test";

/** The server URL including the `#token=` fragment printed by `spark serve`. */
export const appUrl = () => process.env.SPARK_URL ?? "";

export async function newFile(page: Page) {
  await page.goto(appUrl());
  await page.getByRole("button", { name: "New file" }).first().click();
  await expect(page.getByTestId("file-tab").last()).toHaveAttribute("aria-selected", "true");
  const board = page.locator("[data-node]").first();
  await expect(board).toBeVisible();
  // Opening fits the camera on the next frame; wait before tests read screen coordinates.
  await expect.poll(async () => {
    const b = await board.boundingBox();
    const c = await page.getByTestId("canvas").boundingBox();
    return !!b && !!c && b.x >= c.x && b.y >= c.y && b.x + b.width <= c.x + c.width && b.y + b.height <= c.y + c.height;
  }).toBe(true);
  await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
}

/** Camera transform of the canvas as [zoom, x, y]. */
export async function camera(page: Page): Promise<[number, number, number]> {
  const t = await page.getByTestId("camera").evaluate((el) => (el as HTMLElement).style.transform);
  const m = t.match(/matrix3d\(([^)]+)\)/);
  const v = (m?.[1] ?? "").split(",").map(Number);
  return [v[0], v[12], v[13]];
}
