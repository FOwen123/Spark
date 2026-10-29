// Builds the showcase design through MCP, saves it as fixtures/demo/Showcase.spark and captures
// the README screenshots. Opt-in: `bun run screenshots`.
import { copyFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { LANDING, MOBILE } from "../fixtures/demo/showcase";
import { callJson, connectAgent } from "./mcp";
import { newFile } from "./util";

const out = (name: string) => join(__dirname, "..", "docs", "images", name);

test("showcase screenshots", async ({ page }) => {
  await newFile(page);
  const agent = await connectAgent("Claude Code");
  const info = await callJson<{ fileId: string; artboards: { id: string }[] }>(agent, "get_basic_info");
  await callJson(agent, "rename_resource", { resourceId: info.fileId, name: "Showcase" });
  await callJson(agent, "delete_nodes", { nodeIds: [info.artboards[0].id] });
  for (const screen of [LANDING, MOBILE]) {
    const board = await callJson<{ id: string }>(agent, "create_artboard", { name: screen.name, styles: screen.styles });
    for (const html of screen.sections) await callJson(agent, "write_html", { targetNodeId: board.id, html });
  }
  await callJson(agent, "create_tokens", {
    tokens: [
      { name: "--color-ink", value: "#0c0a09" },
      { name: "--color-paper", value: "#fafaf9" },
      { name: "--color-moss", value: "#6f9a87" },
      { name: "--radius-card", value: "18px" },
    ],
  });
  await page.keyboard.press("Shift+Digit1");
  await page.waitForTimeout(600);

  // Light theme, a card selected, with the agent still working.
  await page.getByRole("treeitem", { name: /Atlas — Landing/ }).getByRole("button", { name: "Expand" }).click();
  await page.locator('[data-kind="text"]', { hasText: "Plan the trip together" }).click();
  await page.waitForTimeout(400);
  await page.screenshot({ path: out("editor-light.png") });

  // Dark theme with the agent panel open.
  await page.getByRole("button", { name: "Use dark theme" }).click();
  await page.keyboard.press("ControlOrMeta+j");
  await expect(page.getByRole("complementary", { name: "Agent panel" })).toBeVisible();
  await callJson(agent, "get_tree_summary", { nodeId: (await callJson<{ artboards: { id: string }[] }>(agent, "get_basic_info")).artboards[1].id });
  await page.waitForTimeout(600);
  await page.screenshot({ path: out("editor-dark.png") });
  await page.getByRole("button", { name: "Use light theme" }).click();

  await callJson(agent, "finish_working_on_nodes");
  await agent.close();
  // Spark folds its write-ahead log into the file about two seconds after edits stop.
  await page.waitForTimeout(3500);
  // Keep the design in the repo for anyone who wants to open it.
  const files = join(process.env.SPARK_DIR ?? "", "files");
  const name = readdirSync(files).find((f) => f.startsWith("Untitled") && f.endsWith(".spark"));
  if (!name) throw new Error("no saved file");
  copyFileSync(join(files, name), join(__dirname, "..", "fixtures", "demo", "Showcase.spark"));
});
