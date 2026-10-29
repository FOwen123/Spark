import { expect, test } from "@playwright/test";
import { call, callJson, connectAgent } from "./mcp";
import { newFile } from "./util";

test("an MCP agent builds an artboard live, reads JSX and takes a screenshot", async ({ page }) => {
  await newFile(page);
  const agent = await connectAgent("e2e-agent");
  try {
    const tools = await agent.listTools();
    expect(tools.tools.map((t) => t.name)).toContain("write_html");

    // No fileId: tools act on the file the user is viewing.
    const info = await callJson<{ fileName: string; artboards: { id: string }[] }>(agent, "get_basic_info");
    expect(info.fileName).toBe("Untitled");

    const board = await callJson<{ id: string }>(agent, "create_artboard", { name: "Agent board", styles: { width: "640px", height: "fit-content", padding: "40px", gap: "16px" } });
    await expect(page.getByRole("treeitem", { name: "Agent board" })).toBeVisible();

    const html = `<div layer-name="Card" style="display:flex;flex-direction:column;gap:8px;padding:24px;background:#f4f4f5;border-radius:12px">
      <h2 style="font-size:28px;margin:0;color:#0a0a0a">Hello from an agent</h2>
      <p style="font-size:15px;color:#52525b;margin:0">Written with write_html.</p>
    </div>`;
    const written = await callJson<{ created: { id: string; name: string }[] }>(agent, "write_html", { targetNodeId: board.id, html });
    expect(written.created[0].name).toBe("Card");
    // It appears live in the open UI (zoom to fit: the new artboard was placed off screen).
    await page.keyboard.press("Shift+Digit1");
    await expect(page.locator('[data-kind="text"]', { hasText: "Hello from an agent" })).toBeVisible();

    // get_jsx reflects what was written.
    const jsx = (await call(agent, "get_jsx", { nodeId: written.created[0].id }))[1].text ?? "";
    expect(jsx).toContain("Hello from an agent");
    expect(jsx).toContain('flexDirection: "column"');
    expect(jsx).toContain('borderRadius: "12px"');
    const tw = (await call(agent, "get_jsx", { nodeId: written.created[0].id, format: "tailwind", includeNodeIds: false }))[1].text ?? "";
    expect(tw).toContain('className="flex flex-col gap-2 p-6 rounded-[12px] bg-[#f4f4f5]"');

    // Screenshots come back as PNG, rendered by the open UI.
    const shot = await call(agent, "get_screenshot", { nodeId: board.id, scale: 1 });
    const image = shot.find((c) => c.type === "image");
    expect(image?.mimeType).toBe("image/png");
    const png = Buffer.from(image?.data ?? "", "base64");
    expect(png.subarray(1, 4).toString()).toBe("PNG");
    expect(png.readUInt32BE(16)).toBe(640);
    expect(png.readUInt32BE(20)).toBeGreaterThan(100);

    // Activity indicators clear when the agent finishes.
    const tab = page.getByTestId("file-tab").last();
    await expect(tab.getByRole("img", { name: "Agent working" })).toBeVisible();
    // With nothing selected, the inspector lists the agent under the page it works on.
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("agents-page")).toContainText("Page 1");
    await expect(page.getByTestId("agents-page")).toContainText("e2e-agent");
    await call(agent, "finish_working_on_nodes");
    await expect(page.getByTestId("agents-page")).toHaveCount(0);
    await expect(tab.getByRole("img", { name: "Agent working" })).toHaveCount(0);

    // One tool call is one undo step.
    await page.keyboard.press("ControlOrMeta+z");
    await expect(page.locator('[data-kind="text"]', { hasText: "Hello from an agent" })).toHaveCount(0);
  } finally {
    await agent.close();
  }
});
