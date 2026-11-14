import { expect, test } from "@playwright/test";
import { call, callJson, connectAgent } from "./mcp";
import { newFile } from "./util";

test("starter tokens bring light and dark themes, and users add their own", async ({ page }) => {
  await newFile(page);
  const agent = await connectAgent("e2e-themes");
  try {
    await page.keyboard.press("Escape");
    await page.getByRole("button", { name: "Theme", exact: true }).click();
    await page.getByRole("button", { name: "Add starter tokens" }).click();
    await expect(page.getByTestId("token-row")).toHaveCount(19);
    await expect(page.getByText("Radius", { exact: true })).toBeVisible();

    // An alias declared outside the themed frame still follows the theme inside it.
    await call(agent, "create_tokens", { tokens: [{ name: "--color-surface", value: "var(--color-background)" }] });
    const board = await callJson<{ id: string }>(agent, "create_artboard", {
      name: "Themed",
      styles: { width: "200px", height: "100px", backgroundColor: "var(--color-surface)" },
    });
    const el = page.locator(`[data-node="${board.id}"]`);
    await expect(el).toHaveCSS("background-color", "oklch(1 0 0)");

    // The inspector picks the theme for the selected frame.
    await el.click({ position: { x: 100, y: 50 } });
    await page.getByRole("combobox", { name: "Theme", exact: true }).last().selectOption("dark");
    await expect(el).toHaveCSS("background-color", "oklch(0.145 0 0)");
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
    expect(pixel[0]).toBeLessThan(20);

    // A theme of the user's own: create it, give the background a value, use it.
    const bar = page.getByLabel("Theme", { exact: true }).first();
    await bar.selectOption("+new");
    await page.getByRole("textbox", { name: "Theme name" }).fill("sepia");
    await page.getByRole("button", { name: "Add theme" }).click();
    const bg = page.getByRole("textbox", { name: "--color-background value" });
    await expect(bg).toHaveValue("");
    await bg.fill("#f4ecd8");
    await bg.press("Enter");
    await expect
      .poll(async () => (await callJson<{ tokens: { themes?: Record<string, string> }[] }>(agent, "get_tokens", { namePattern: "--color-background" })).tokens[0].themes)
      .toEqual({ dark: "oklch(0.145 0 0)", sepia: "#f4ecd8" });
    await call(agent, "update_styles", { updates: [{ nodeIds: [board.id], theme: "sepia" }] });
    await expect(el).toHaveCSS("background-color", "rgb(244, 236, 216)");

    // Renaming the theme carries the frame along; deleting it puts the frame back on the base values.
    await page.getByRole("button", { name: "Rename theme" }).click();
    await page.getByRole("textbox", { name: "Theme name" }).fill("paper");
    await page.getByRole("button", { name: "Rename" }).click();
    await expect(el).toHaveAttribute("data-theme", "paper");
    await expect(el).toHaveCSS("background-color", "rgb(244, 236, 216)");
    await page.getByRole("button", { name: "Delete theme" }).click();
    await expect(el).not.toHaveAttribute("data-theme");
    await expect(el).toHaveCSS("background-color", "oklch(1 0 0)");
  } finally {
    await agent.close();
  }
});
