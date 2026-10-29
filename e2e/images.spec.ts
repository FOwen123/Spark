import { type Server, createServer } from "node:http";
import { expect, test } from "@playwright/test";
import { callJson, connectAgent } from "./mcp";
import { newFile } from "./util";

// A 2×2 opaque PNG.
const PNG = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAFklEQVR4nGNkYPj/n4GBgYmBgYGBAQAj4QH/Hn5+cAAAAABJRU5ErkJggg==";

async function boardId(agent: Awaited<ReturnType<typeof connectAgent>>) {
  const info = await callJson<{ artboards: { id: string }[] }>(agent, "get_basic_info");
  return info.artboards[0].id;
}

test("an image request placeholder is fulfilled by the agent", async ({ page }) => {
  await newFile(page);
  const agent = await connectAgent("Image agent");
  try {
    const board = await boardId(agent);
    await callJson(agent, "write_html", {
      targetNodeId: board,
      html: '<img layer-name="Hero photo" src="spark-gen://agent?prompt=A%20quiet%20studio%20desk&aspect_ratio=16:9" style="width:480px">',
    });
    const placeholder = page.locator('[data-image-state="pending"]');
    await expect(placeholder).toContainText("Waiting for an image");

    const reqs = await callJson<{ requests: { requestId: string; prompt: string; aspectRatio: string }[] }>(agent, "list_image_requests");
    expect(reqs.requests).toHaveLength(1);
    expect(reqs.requests[0]).toMatchObject({ prompt: "A quiet studio desk", aspectRatio: "16:9" });

    await callJson(agent, "fulfill_image_request", { requestId: reqs.requests[0].requestId, base64: PNG });
    await expect(placeholder).toHaveCount(0);
    const photo = page.locator('[data-node][data-kind="rect"]').first();
    await expect(photo).toHaveCSS("background-image", /\/assets\/[0-9a-f]{32}/);
    await expect(photo).toHaveCSS("width", "480px");
    await expect(photo).toHaveCSS("height", "270px");
  } finally {
    await agent.close();
  }
});

test("a built-in provider generates through a mock HTTP API", async ({ page }) => {
  const seen: { auth?: string; body?: Record<string, unknown> }[] = [];
  const mock: Server = createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      seen.push({ auth: req.headers.authorization, body: JSON.parse(body || "{}") });
      res.setHeader("content-type", "application/json");
      res.end(JSON.stringify({ data: [{ b64_json: PNG }] }));
    });
  });
  await new Promise<void>((r) => mock.listen(0, "127.0.0.1", r));
  const port = (mock.address() as { port: number }).port;
  try {
    await newFile(page);
    // Enable OpenAI in Settings, pointed at the mock, with a key.
    await page.getByRole("button", { name: "Settings" }).click();
    const row = page.getByTestId("provider-openai");
    const enable = row.getByLabel("Enable OpenAI Images");
    if (!(await enable.isChecked())) await enable.check();
    await row.getByLabel("OpenAI Images API key").fill("sk-test-123");
    await row.getByLabel("OpenAI Images API key").press("Enter");
    await expect(row).toContainText("Key saved");
    await row.getByLabel("OpenAI Images base URL").fill(`http://127.0.0.1:${port}`);
    await row.getByLabel("OpenAI Images model").click();
    await page.keyboard.press("Escape");

    const agent = await connectAgent("Gen agent");
    try {
      const board = await boardId(agent);
      await callJson(agent, "write_html", { targetNodeId: board, html: '<img src="spark-gen://openai?prompt=A%20paper%20plane&aspect_ratio=1:1" style="width:200px">' });
      const img = page.locator('[data-node][data-kind="rect"]').first();
      await expect(img).toHaveCSS("background-image", /\/assets\//, { timeout: 10_000 });
      expect(seen[0].auth).toBe("Bearer sk-test-123");
      expect(seen[0].body).toMatchObject({ model: "gpt-image-1", size: "1024x1024" });
      expect(String(seen[0].body?.prompt)).toContain("A paper plane");
    } finally {
      await agent.close();
    }
  } finally {
    mock.close();
  }
});
