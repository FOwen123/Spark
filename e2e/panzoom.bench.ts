import { execFileSync } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { appUrl } from "./util";

// Pans and zooms around a 5,000-node page while recording frame times with requestAnimationFrame.
test("pan and zoom frame times on a 5,000-node page", async ({ page }) => {
  const dir = mkdtempSync(join(tmpdir(), "spark-bench-"));
  const file = join(dir, "Bench.spark");
  execFileSync(process.env.SPARK_BIN ?? "", ["demo", file, "--nodes", "5000"]);
  const url = new URL(appUrl());
  await page.goto(`${url.origin}/?open=${encodeURIComponent(file)}${url.hash}`);
  await expect(page.locator('[data-kind="frame"][data-node]').first()).toBeVisible();
  const canvas = await page.getByTestId("canvas").boundingBox();
  if (!canvas) throw new Error("no canvas");
  await page.mouse.move(canvas.x + canvas.width / 2, canvas.y + canvas.height / 2);
  await page.waitForTimeout(500);

  await page.evaluate(() => {
    const w = window as unknown as { frames_: number[]; peak_: number };
    w.frames_ = [];
    w.peak_ = 0;
    let last = performance.now();
    const tick = (t: number) => {
      if (w.frames_.length % 5 === 0) w.peak_ = Math.max(w.peak_, document.querySelectorAll("[data-node]").length);
      w.frames_.push(t - last);
      last = t;
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
  // Zoom in (mounting artboard contents), pan across, zoom back out.
  await page.keyboard.down("Control");
  for (let i = 0; i < 30; i++) {
    await page.mouse.wheel(0, -40);
    await page.waitForTimeout(16);
  }
  await page.keyboard.up("Control");
  for (let i = 0; i < 60; i++) {
    await page.mouse.wheel(60, 25);
    await page.waitForTimeout(16);
  }
  await page.keyboard.down("Control");
  for (let i = 0; i < 30; i++) {
    await page.mouse.wheel(0, 40);
    await page.waitForTimeout(16);
  }
  await page.keyboard.up("Control");
  const frames = (await page.evaluate(() => (window as unknown as { frames_: number[] }).frames_)).slice(2);
  const sorted = [...frames].sort((a, b) => a - b);
  const avg = frames.reduce((a, b) => a + b, 0) / frames.length;
  const p95 = sorted[Math.floor(sorted.length * 0.95)];
  const peak = await page.evaluate(() => (window as unknown as { peak_: number }).peak_);
  const report = `frames ${frames.length}, average ${avg.toFixed(1)} ms (${(1000 / avg).toFixed(0)} fps), p95 ${p95.toFixed(1)} ms, worst ${sorted[sorted.length - 1].toFixed(1)} ms, most nodes in the DOM ${peak}`;
  console.log(report);
  test.info().annotations.push({ type: "pan-zoom", description: report });
});
