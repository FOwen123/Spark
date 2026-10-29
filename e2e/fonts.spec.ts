import { expect, test } from "@playwright/test";
import { call, callJson, connectAgent } from "./mcp";
import { newFile } from "./util";

test("a Google Fonts family named in a design is downloaded and drawn", async ({ page }) => {
  await newFile(page);
  const agent = await connectAgent("e2e-fonts");
  try {
    const info = await callJson<{ artboards: { id: string }[] }>(agent, "get_basic_info");
    const html = `<p style="font-family:'Spark Test Mono', serif;font-size:20px;margin:0">Downloaded font</p>`;
    await call(agent, "write_html", { targetNodeId: info.artboards[0].id, html });
    await expect(page.locator('[data-kind="text"]', { hasText: "Downloaded font" })).toBeVisible();
    // The browser loads a face only for text it draws, so a loaded face means the text uses it.
    await expect
      .poll(() => page.evaluate(() => [...document.fonts].some((f) => f.family.replace(/["']/g, "") === "Spark Test Mono" && f.status === "loaded")))
      .toBe(true);
  } finally {
    await agent.close();
  }
});
