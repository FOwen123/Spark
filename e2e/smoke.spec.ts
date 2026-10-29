import { expect, test } from "@playwright/test";
import { appUrl } from "./util";

test("spark serve serves the web UI", async ({ page }) => {
  await page.goto(appUrl());
  await expect(page.getByRole("heading", { name: "Spark" })).toBeVisible();
  await expect(page.getByRole("button", { name: "New file" }).first()).toBeVisible();
});

test("Spark logos stay visible in light and dark themes", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("spark-theme", "light"));
  await page.goto(appUrl());
  const home = page.getByRole("main").getByRole("img", { name: "Spark logo" });
  const tab = page.getByRole("tab", { name: "Home" }).getByRole("img", { name: "Spark logo" });
  await expect(home).toHaveCSS("width", "48px");
  await expect(tab).toHaveCSS("width", "20px");
  for (const logo of [home, tab]) {
    await expect(logo).toBeVisible();
    await expect(logo).toHaveCSS("background-color", "rgb(0, 0, 0)");
  }
  const asset = await page.request.get(new URL("/brand/spark.svg", page.url()).href);
  expect(asset.ok()).toBeTruthy();
  expect(asset.headers()["content-type"]).toContain("image/svg+xml");
  await page.getByRole("button", { name: "Use dark theme" }).click();
  for (const logo of [home, tab]) await expect(logo).toHaveCSS("background-color", "rgb(255, 255, 255)");
  await page.getByRole("button", { name: "Use light theme" }).click();
  for (const logo of [home, tab]) await expect(logo).toHaveCSS("background-color", "rgb(0, 0, 0)");
});
