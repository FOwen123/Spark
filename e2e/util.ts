import { type Page, expect } from "@playwright/test";

/** The server URL including the `#token=` fragment printed by `spark serve`. */
export const appUrl = () => process.env.SPARK_URL ?? "";

export async function newFile(page: Page) {
  await page.goto(appUrl());
  await page.getByRole("button", { name: "New file" }).first().click();
  await expect(page.getByTestId("file-tab").last()).toHaveAttribute("aria-selected", "true");
  await expect(page.locator("[data-node]").first()).toBeVisible();
}

/** Camera transform of the canvas as [zoom, x, y]. */
export async function camera(page: Page): Promise<[number, number, number]> {
  const t = await page.getByTestId("camera").evaluate((el) => (el as HTMLElement).style.transform);
  const m = t.match(/matrix3d\(([^)]+)\)/);
  const v = (m?.[1] ?? "").split(",").map(Number);
  return [v[0], v[12], v[13]];
}
