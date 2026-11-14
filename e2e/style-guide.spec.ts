import { expect, test } from "@playwright/test";
import { call, callJson, connectAgent } from "./mcp";
import { newFile } from "./util";

test("the style guide is shared with agents, kept in the file and exported as DESIGN.md", async ({ page }) => {
  await newFile(page);
  const agent = await connectAgent("e2e-style-guide");
  try {
    await page.getByRole("button", { name: "Guide", exact: true }).click();
    const field = page.getByRole("textbox", { name: "Style guide" });
    await field.fill("# Rules\n- Flat surfaces");
    await field.blur();

    // Agents read the user's rules and record decisions under them.
    await expect.poll(async () => (await callJson<{ styleGuide: string }>(agent, "get_basic_info", {})).styleGuide).toBe("# Rules\n- Flat surfaces");
    await call(agent, "set_style_guide", { decision: "No shadows" });
    await expect(field).toHaveValue("# Rules\n- Flat surfaces\n\n## Decisions\n\n- No shadows\n");

    // Undo is per file, and the guide survives a reload.
    await page.keyboard.press("ControlOrMeta+z");
    await expect(field).toHaveValue("# Rules\n- Flat surfaces");
    await page.keyboard.press("ControlOrMeta+Shift+z");
    await page.reload();
    await page.getByRole("button", { name: "Guide", exact: true }).click();
    await expect(page.getByRole("textbox", { name: "Style guide" })).toHaveValue(/- No shadows/);

    await page.getByRole("button", { name: "Export as DESIGN.md" }).click();
    await expect(page.getByRole("dialog", { name: "DESIGN.md" })).toContainText("- No shadows");
  } finally {
    await agent.close();
  }
});
