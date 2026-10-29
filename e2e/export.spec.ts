import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { PDFDocument } from "pdf-lib";
import { callJson, connectAgent } from "./mcp";
import { newFile } from "./util";

const downloads = () => join(process.env.SPARK_DIR ?? "", "downloads");
const listing = () => {
  try {
    return new Set(readdirSync(downloads()));
  } catch {
    return new Set<string>();
  }
};
const newFiles = (before: Set<string>) => [...listing()].filter((f) => !before.has(f));
const pngSize = (b: Buffer) => [b.readUInt32BE(16), b.readUInt32BE(20)];

test("export presets from the inspector produce valid files in every format", async ({ page }) => {
  await newFile(page);
  const agent = await connectAgent("Export agent");
  try {
    const info = await callJson<{ artboards: { id: string }[] }>(agent, "get_basic_info");
    const board = info.artboards[0].id;
    await callJson(agent, "write_html", {
      targetNodeId: board,
      html: '<div style="position:absolute;left:80px;top:80px;width:600px;padding:40px;background:linear-gradient(135deg,#111,#555);border-radius:24px"><h1 style="color:#fff;font-size:56px;margin:0">Hello export</h1></div>',
    });

    await page.getByRole("treeitem", { name: /Desktop/ }).click();
    const exportSection = page.getByRole("region", { name: "Export" });
    for (let i = 0; i < 5; i++) await exportSection.getByRole("button", { name: "Add export" }).click();
    const rows = exportSection.getByTestId("export-preset");
    await expect(rows).toHaveCount(5);
    const formats = ["png", "jpg", "webp", "svg", "pdf"];
    for (const [i, f] of formats.entries()) await rows.nth(i).getByLabel("Export format").selectOption(f);
    await rows.nth(0).getByLabel("Export scale").selectOption("2x");
    await rows.nth(1).getByLabel("Export scale").selectOption("1x");

    const before = listing();
    await exportSection.getByRole("button", { name: "Export Desktop" }).click();
    await expect(page.getByTestId("toast")).toContainText("Exported 5 files");
    const files = newFiles(before);
    const find = (ext: string) => {
      const name = files.find((f) => f.endsWith(`.${ext}`));
      if (!name) throw new Error(`no .${ext} in ${files.join(", ")}`);
      return readFileSync(join(downloads(), name));
    };
    expect(files.find((f) => f.endsWith(".png"))).toMatch(/^Desktop@2x( \(\d+\))?\.png$/);

    const png = find("png");
    expect(png.subarray(1, 4).toString()).toBe("PNG");
    expect(pngSize(png)).toEqual([2880, 2048]);
    const jpg = find("jpg");
    expect([jpg[0], jpg[1], jpg[jpg.length - 2], jpg[jpg.length - 1]]).toEqual([0xff, 0xd8, 0xff, 0xd9]);
    const webp = find("webp");
    expect(webp.subarray(0, 4).toString()).toBe("RIFF");
    expect(webp.subarray(8, 12).toString()).toBe("WEBP");
    const svg = find("svg").toString();
    expect(svg).toMatch(/^<svg xmlns="http:\/\/www.w3.org\/2000\/svg"[^>]*width="1440" height="1024"/);
    expect(svg).toContain("Hello export");
    expect(svg).toContain("<linearGradient");
    const pdf = await PDFDocument.load(find("pdf"));
    expect(pdf.getPageCount()).toBe(1);
    expect(pdf.getPage(0).getSize()).toEqual({ width: 1440, height: 1024 });

    // MCP export at a target width, and a combined PDF of two artboards.
    const second = await callJson<{ id: string }>(agent, "create_artboard", { name: "Second", styles: { width: "400px", height: "300px" } });
    const before2 = listing();
    const out = await callJson<{ files: { path: string }[] }>(agent, "export", { nodes: { [board]: [{ format: "png", scale: "512w" }] } });
    expect(pngSize(readFileSync(out.files[0].path))).toEqual([512, 364]);
    const combined = await callJson<{ path: string }>(agent, "export_combined_pdf", { nodeIds: [second.id, board], name: "Deck" });
    const deck = await PDFDocument.load(readFileSync(combined.path));
    expect(deck.getPageCount()).toBe(2);
    // Ordered top to bottom, then left to right: both at y=0, so the leftmost (Desktop) first.
    expect(deck.getPage(0).getSize()).toEqual({ width: 1440, height: 1024 });
    expect(newFiles(before2).length).toBe(2);
    expect(statSync(combined.path).size).toBeGreaterThan(1000);
  } finally {
    await agent.close();
  }
});
