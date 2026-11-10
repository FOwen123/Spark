import { expect, test } from "@playwright/test";
import { connectAgent } from "./mcp";
import { appUrl, newFile } from "./util";

test("connect an agent from the home screen", async ({ page }) => {
  await page.goto(appUrl());
  await page.getByRole("button", { name: "Connect your agent" }).click();
  await page.getByTestId("connect-cursor").click();
  const steps = page.getByTestId("connect-steps");
  // The manual route shows exactly what one click would write.
  await steps.getByRole("button", { name: "Or, set it up manually" }).click();
  await expect(steps).toContainText(".cursor/mcp.json");
  await expect(page.getByTestId("connect-manual")).toContainText('"mcpServers"');
  await expect(page.getByTestId("connect-manual")).toContainText('"spark"');
  const connected = (await steps.getByRole("button", { name: "Disconnect" }).count()) > 0;
  await steps.getByRole("button", { name: connected ? "Disconnect" : "Add to Cursor" }).click();
  await expect(page.getByTestId("connect-result")).toContainText(".cursor/mcp.json");
  if (!connected) {
    await expect(page.getByTestId("connect-result")).toContainText('"spark"');
    await expect(page.getByTestId("connect-cursor").getByLabel("Connected")).toBeVisible();
    // Put it back so the other browser run starts from the same state.
    await steps.getByRole("button", { name: "Disconnect" }).click();
    await expect(page.getByTestId("connect-cursor").getByLabel("Connected")).toHaveCount(0);
  }
});

test("the inspector opens the connect dialog when nothing is selected", async ({ page }) => {
  await newFile(page);
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Connect more agents" }).click();
  await expect(page.getByRole("dialog", { name: "Connect your agent" })).toBeVisible();
  await expect(page.getByTestId("connect-claude-code")).toBeVisible();
});

test("the agent panel runs a local agent CLI and logs tool calls", async ({ page }) => {
  await newFile(page);
  await page.keyboard.press("ControlOrMeta+j");
  const panel = page.getByRole("complementary", { name: "Agent panel" });
  await expect(panel).toBeVisible();
  await expect(panel.getByLabel("Agent CLI")).toHaveValue("claude");
  await panel.getByLabel("Prompt").fill("Make a hero section");
  await panel.getByRole("button", { name: "Run" }).click();
  const out = panel.getByTestId("agent-output");
  await expect(out).toContainText("Spark MCP attached");
  await expect(out).toContainText("▸ get_basic_info");
  await expect(out).toContainText("✓ All done");
  await expect(out).toContainText("Finished.");

  const agent = await connectAgent("Logger");
  await agent.callTool({ name: "get_basic_info", arguments: {} });
  await expect(panel.getByRole("list", { name: "Tool calls" })).toContainText("get_basic_info");
  await expect(panel).toContainText("Logger");
  await agent.close();
});

test("a running agent keeps its Stop button when the panel closes and reopens", async ({ page }) => {
  await newFile(page);
  await page.keyboard.press("ControlOrMeta+j");
  const panel = page.getByRole("complementary", { name: "Agent panel" });
  await panel.getByLabel("Prompt").fill("Make a hero section, take your time");
  await panel.getByRole("button", { name: "Run" }).click();
  await expect(panel.getByTestId("agent-output")).toContainText("▸ get_basic_info");
  await page.keyboard.press("ControlOrMeta+j");
  await expect(panel).toHaveCount(0);
  await page.keyboard.press("ControlOrMeta+j");
  await expect(panel.getByTestId("agent-output")).toContainText("▸ get_basic_info");
  await panel.getByRole("button", { name: "Stop" }).click();
  await expect(panel.getByTestId("agent-output")).toContainText("Stopped.");
  await expect(panel.getByRole("button", { name: "Run" })).toBeVisible();
});
