import { existsSync } from "node:fs";
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
