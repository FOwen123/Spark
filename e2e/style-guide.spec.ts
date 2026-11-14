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

test("unsaved guide text survives agent edits and switching files", async ({ page }) => {
  await newFile(page);
  const agent = await connectAgent("e2e-style-guide-draft");
  try {
    await page.getByRole("button", { name: "Guide", exact: true }).click();
    const field = page.getByRole("textbox", { name: "Style guide" });
    await field.fill("- Mine");
    // An agent writes while the user is still typing; the user's text stays in the field.
    await call(agent, "set_style_guide", { text: "- Agent" });
    await expect(field).toHaveValue("- Mine");
    await expect(field).toBeFocused();
    await field.blur();
    const { fileId } = await callJson<{ fileId: string }>(agent, "get_basic_info", {});
    const guide = async () => (await callJson<{ styleGuide: string }>(agent, "get_basic_info", { fileId })).styleGuide;
    await expect.poll(guide).toBe("- Mine");

    // Leaving the file from the keyboard (Mod+T opens a new file) keeps what was typed.
    await field.fill("- Mine\n- Kept");
    const tabs = await page.getByRole("tab").count();
    await page.keyboard.press("ControlOrMeta+T");
    await expect(page.getByRole("tab")).toHaveCount(tabs + 1);
    await expect.poll(guide).toBe("- Mine\n- Kept");
  } finally {
    await agent.close();
  }
});
