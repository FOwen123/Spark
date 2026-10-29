import { expect, test } from "@playwright/test";
import { callJson, connectAgent } from "./mcp";
import { appUrl } from "./util";

test("a spark:// link opens the file on its page with the node selected", async ({ page }) => {
  const agent = await connectAgent("Link agent");
  try {
    const file = await callJson<{ fileId: string }>(agent, "create_file", { name: "Linked" });
    const page2 = await callJson<{ pageId: string }>(agent, "create_page", { fileId: file.fileId, name: "Second page" });
    const board = await callJson<{ id: string }>(agent, "create_artboard", { fileId: file.fileId, pageId: page2.pageId, name: "Target board", styles: { width: "400px", height: "300px" } });
    const link = `spark://file/${file.fileId}/${page2.pageId}?node=${board.id}`;
    const url = new URL(appUrl());
    await page.goto(`${url.origin}/?open=${encodeURIComponent(link)}${url.hash}`);
    await expect(page.getByTestId("file-tab").filter({ hasText: "Linked" })).toHaveAttribute("aria-selected", "true");
    await expect(page.getByRole("button", { name: "Second page" })).toHaveAttribute("aria-current", "true");
    await expect(page.getByRole("treeitem", { name: "Target board" })).toHaveAttribute("aria-selected", "true");
  } finally {
    await agent.close();
  }
});
