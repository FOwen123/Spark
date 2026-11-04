import { expect, test } from "@playwright/test";
import { newFile } from "./util";

test("pages are renamed by double-click and from their right-click menu", async ({ page }) => {
  await newFile(page);
  const pages = page.getByRole("region", { name: "Pages" });
  const name = page.getByRole("textbox", { name: "Name" });

  await pages.getByRole("button", { name: "Page 1", exact: true }).dblclick();
  await name.fill("Landing");
  await name.press("Enter");
  await expect(pages.getByRole("button", { name: "Landing" })).toBeVisible();

  await pages.getByRole("button", { name: "Landing" }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Rename page" }).click();
  await name.fill("Onboarding");
  await name.press("Enter");
  await expect(pages.getByRole("button", { name: "Onboarding" })).toBeVisible();

  await page.reload();
  await expect(page.getByRole("region", { name: "Pages" }).getByRole("button", { name: "Onboarding" })).toBeVisible();
});

test("a page's right-click menu adds a divider below it and deletes it", async ({ page }) => {
  await newFile(page);
  const pages = page.getByRole("region", { name: "Pages" });
  await pages.getByRole("button", { name: "Add page" }).click();
  await pages.getByRole("button", { name: "Page 1", exact: true }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Add divider" }).click();
  await expect(pages.getByRole("listitem")).toHaveCount(3);
  await expect(pages.getByRole("listitem").nth(1)).toHaveAttribute("aria-label", "Divider");

  await pages.getByRole("button", { name: /^Page 2/ }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Delete page" }).click();
  await expect(pages.getByRole("button", { name: /^Page 2/ })).toHaveCount(0);
  await pages.getByRole("button", { name: "Page 1", exact: true }).click({ button: "right" });
  await expect(page.getByRole("menuitem", { name: "Delete page" })).toBeDisabled();
});
