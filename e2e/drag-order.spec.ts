import { type Page, expect, test } from "@playwright/test";
import { call, callJson, connectAgent } from "./mcp";
import { newFile } from "./util";

// A fast drag on a slow machine can deliver dragover and drop before React re-renders after
// dragstart. These fire all of them in one go, the worst case, and expect the reorder anyway.
async function dragFast(page: Page, rows: string, from: number, to: number) {
  await page.evaluate(
    ({ rows, from, to }) => {
      const els = [...document.querySelectorAll<HTMLElement>(rows)];
      const r = els[to].getBoundingClientRect();
      const at = { bubbles: true, cancelable: true, dataTransfer: new DataTransfer(), clientX: r.left + 40, clientY: r.top + 2 };
      els[from].dispatchEvent(new DragEvent("dragstart", at));
      els[to].dispatchEvent(new DragEvent("dragover", at));
      els[to].dispatchEvent(new DragEvent("drop", at));
      els[from].dispatchEvent(new DragEvent("dragend", at));
    },
    { rows, from, to },
  );
}

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
});

test("a page row dropped before React re-renders still moves", async ({ page }) => {
  await newFile(page);
  await page.getByRole("button", { name: "Add page" }).click();
  const rows = '[aria-label="Pages"] li [role="button"]';
  const order = () => page.locator(rows).evaluateAll((els) => els.map((e) => e.textContent?.trim()));
  await expect.poll(order).toHaveLength(2);
  const [a, b] = await order();
  await dragFast(page, rows, 1, 0);
  await expect.poll(order).toEqual([b, a]);
});

test("a layer row dropped before React re-renders still moves", async ({ page }) => {
  await newFile(page);
  const agent = await connectAgent("Drag agent");
  try {
    const info = await callJson<{ artboards: { id: string }[] }>(agent, "get_basic_info");
    await call(agent, "write_html", { targetNodeId: info.artboards[0].id, html: `<div layer-name="Alpha" style="width:10px;height:10px"></div><div layer-name="Bravo" style="width:10px;height:10px"></div>` });
    const tree = page.getByRole("tree", { name: "Layers" });
    await tree.getByRole("button", { name: "Expand", exact: true }).first().click();
    const rows = '[role="treeitem"][aria-level="2"]';
    const order = () => page.locator(rows).evaluateAll((els) => els.map((e) => e.textContent?.trim()));
    await expect.poll(order).toHaveLength(2);
    const [a, b] = await order();
    await dragFast(page, rows, 1, 0);
    await expect.poll(order).toEqual([b, a]);
  } finally {
    await agent.close();
  }
});
