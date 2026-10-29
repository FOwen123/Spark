import { existsSync } from "node:fs";
import { basename, join } from "node:path";
import { expect, test } from "@playwright/test";
import { callJson, connectAgent } from "./mcp";
import { newFile } from "./util";

test("archive a file from the recent list", async ({ page }) => {
  await newFile(page);
  const name = `Archive me ${Date.now()}`;
  const agent = await connectAgent("e2e-archive");
  try {
    const info = await callJson<{ fileId: string }>(agent, "get_basic_info");
    await callJson(agent, "rename_resource", { resourceId: info.fileId, name });
  } finally {
    await agent.close();
  }
  await expect(page.getByTestId("file-tab").last()).toContainText(name);
  await page.getByRole("tab", { name: "Home" }).click();
  const row = page.getByRole("listitem").filter({ hasText: name });
  const archive = page.getByRole("button", { name: `Archive ${name}` });

  // A file open in a tab stays put.
  await row.hover();
  await archive.click();
  await expect(page.getByText("is open in a tab. Close it first.")).toBeVisible();
  await expect(row).toBeVisible();

  // Closed, it moves to the Archive folder and leaves the list.
  await page.getByTestId("file-tab").filter({ hasText: name }).hover();
  await page.getByRole("button", { name: `Close ${name}` }).click();
  await page.getByRole("tab", { name: "Home" }).click();
  await row.hover();
  await archive.click();
  await expect(row).toHaveCount(0);
  const moved = page.getByText(`Moved ${name} to `);
  await expect(moved).toBeVisible();
  const path = (await moved.innerText()).replace(`Moved ${name} to `, "");
  expect(path).toContain("/Archive/");
  expect(existsSync(path)).toBe(true);
});

test("delete a file from the recent list to the Trash", async ({ page }) => {
  await newFile(page);
  const name = `Delete me ${Date.now()}`;
  const agent = await connectAgent("e2e-trash");
  try {
    const info = await callJson<{ fileId: string }>(agent, "get_basic_info");
    await callJson(agent, "rename_resource", { resourceId: info.fileId, name });
  } finally {
    await agent.close();
  }
  await expect(page.getByTestId("file-tab").last()).toContainText(name);
  await page.getByTestId("file-tab").filter({ hasText: name }).hover();
  await page.getByRole("button", { name: `Close ${name}` }).click();
  await page.getByRole("tab", { name: "Home" }).click();
  const row = page.getByRole("listitem").filter({ hasText: name });
  const path = (await row.locator(".font-mono").innerText()).trim();
  expect(existsSync(path)).toBe(true);

  await row.hover();
  await page.getByRole("button", { name: `Move ${name} to the Trash` }).click();
  await expect(row).toHaveCount(0);
  await expect(page.getByText(`Moved ${name} to the Trash`)).toBeVisible();
  // The test server's Trash is a throwaway folder (SPARK_TRASH_DIR), never the real one.
  expect(existsSync(path)).toBe(false);
  expect(existsSync(join(process.env.SPARK_DIR ?? "", "trash", basename(path)))).toBe(true);
});
