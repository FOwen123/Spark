import { expect, test } from "@playwright/test";
import { call, callJson, connectAgent } from "./mcp";
import { newFile } from "./util";

test("layers styled with design tokens render with the token values", async ({ page }) => {
  await newFile(page);
  const agent = await connectAgent("e2e-tokens");
  try {
    await call(agent, "create_tokens", {
      tokens: [
        { type: "color", name: "--color-brand", value: "#ff0000" },
        { type: "color", name: "--color-accent", value: "var(--color-brand)" },
      ],
    });
    const board = await callJson<{ id: string }>(agent, "create_artboard", {
      name: "Token board",
      styles: { width: "200px", height: "100px", backgroundColor: "var(--color-accent)" },
    });
    const el = page.locator(`[data-node="${board.id}"]`);
    await expect(el).toHaveCSS("background-color", "rgb(255, 0, 0)");

    // Screenshots and exports render through the same tokens.
    const shot = await call(agent, "get_screenshot", { nodeId: board.id, scale: 1 });
    const data = shot.find((c) => c.type === "image")?.data ?? "";
    const pixel = await page.evaluate(async (b64) => {
      const img = new Image();
      img.src = `data:image/png;base64,${b64}`;
      await img.decode();
      const c = document.createElement("canvas");
      c.width = img.width;
      c.height = img.height;
      const ctx = c.getContext("2d") as CanvasRenderingContext2D;
      ctx.drawImage(img, 0, 0);
      return Array.from(ctx.getImageData(100, 50, 1, 1).data);
    }, data);
    expect(pixel).toEqual([255, 0, 0, 255]);

    // Changing a token restyles every layer that uses it.
    await call(agent, "set_tokens", { tokens: [{ name: "--color-brand", value: "#0000ff" }] });
    await expect(el).toHaveCSS("background-color", "rgb(0, 0, 255)");
  } finally {
    await agent.close();
  }
});

test("a bare namespace token keeps its name when its row is edited", async ({ page }) => {
  await newFile(page);
  const agent = await connectAgent("e2e-bare-token");
  try {
    await call(agent, "create_tokens", { tokens: [{ type: "spacing", name: "--spacing", value: "4px" }] });
    await page.keyboard.press("Escape");
    await page.getByRole("button", { name: "Theme", exact: true }).click();
    const name = page.locator('input[aria-label="Token name"][title="--spacing"]');
    await expect(name).toHaveValue("--spacing");
    await name.focus();
    await name.blur();
    await expect(page.getByTestId("token-row")).toHaveCount(1);
    await expect(name).toHaveValue("--spacing");
  } finally {
    await agent.close();
  }
});
