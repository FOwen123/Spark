import { readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { type Page, expect, test } from "@playwright/test";
import { call, callJson, connectAgent } from "./mcp";
import { newFile } from "./util";

const SHOTS = "test-results/shots/inspector";

/** A new file whose first artboard holds `html`; returns the agent that wrote it. */
async function scene(page: Page, html: string) {
  await newFile(page);
  const agent = await connectAgent("Inspector agent");
  const info = await callJson<{ artboards: { id: string }[] }>(agent, "get_basic_info");
  await call(agent, "write_html", { targetNodeId: info.artboards[0].id, html });
  return agent;
}

/** Selects a layer by name in the layers panel and returns its canvas element. */
async function select(page: Page, name: string) {
  const tree = page.getByRole("tree", { name: "Layers" });
  await expect(tree.getByRole("treeitem").first()).toBeVisible();
  for (let i = 0; i < 6 && (await tree.getByRole("button", { name: "Expand", exact: true }).count()); i++) {
    await tree.getByRole("button", { name: "Expand", exact: true }).first().click();
  }
  const row = page.getByRole("treeitem", { name: new RegExp(`(^|\\s)${name}(\\s|$)`) }).first();
  await row.click();
  return page.locator(`[data-node="${await row.getAttribute("data-layer")}"]`);
}

const section = (page: Page, title: string) => page.getByRole("region", { name: title, exact: true });

test("constraints keep a child anchored when its parent is resized, and sizes can be relative", async ({ page }) => {
  const agent = await scene(
    page,
    `<div layer-name="Parent" style="position:absolute;left:100px;top:100px;width:400px;height:300px;background:#eee">
       <div layer-name="Child" style="position:absolute;left:50px;top:40px;width:100px;height:60px;background:#f00"></div>
     </div>`,
  );
  try {
    const parent = await select(page, "Parent");
    const child = await select(page, "Child");
    const offset = () => child.evaluate((el) => [(el as HTMLElement).offsetLeft, (el as HTMLElement).offsetWidth]);
    const resizeParent = async (w: number) => {
      await select(page, "Parent");
      await page.getByTestId("field-w").fill(String(w));
      await page.getByTestId("field-w").press("Enter");
      await expect(parent).toHaveCSS("width", `${w}px`);
      await select(page, "Child");
    };
    expect(await offset()).toEqual([50, 100]);

    // Right: the distance to the parent's right edge stays 250px.
    await page.getByLabel("Horizontal constraint").selectOption("end");
    await expect(child).toHaveCSS("right", "250px");
    await resizeParent(600);
    expect(await offset()).toEqual([250, 100]);

    // Left and right: the child stretches.
    await page.getByLabel("Horizontal constraint").selectOption("stretch");
    await expect(page.getByLabel("Horizontal constraint")).toHaveValue("stretch");
    await resizeParent(800);
    expect(await offset()).toEqual([250, 300]);

    // Center: stays centred.
    await page.getByLabel("Horizontal constraint").selectOption("center");
    await resizeParent(500);
    expect(await offset()).toEqual([100, 300]);

    // Scale: left and width follow the parent in proportion (100/500 = 20%, 300/500 = 60%).
    await page.getByLabel("Horizontal constraint").selectOption("scale");
    await resizeParent(1000);
    expect(await offset()).toEqual([200, 600]);

    // Vertical: bottom keeps its gap.
    await page.getByLabel("Vertical constraint").selectOption("end");
    await expect(child).toHaveCSS("bottom", "200px");

    // Relative sizing: 600px of a 1000px parent is 60%; editing the percentage resizes it.
    await page.getByLabel("width sizing").selectOption("percent");
    await expect(page.getByTestId("field-w")).toHaveValue("60%");
    await page.getByTestId("field-w").fill("25");
    await page.getByTestId("field-w").press("Enter");
    await expect(child).toHaveCSS("width", "250px");
  } finally {
    await agent.close();
  }
});

test("flex layouts get reverse, baseline, wrap and align-content", async ({ page }) => {
  const agent = await scene(
    page,
    `<div layer-name="Row" style="position:absolute;left:50px;top:50px;width:300px;height:200px;display:flex;flex-direction:row;gap:8px;background:#eee">
       <div style="width:100px;height:40px;background:#f00"></div><div style="width:100px;height:40px;background:#0f0"></div>
     </div>`,
  );
  try {
    const row = await select(page, "Row");
    await page.getByRole("button", { name: "Reverse direction" }).click();
    await expect(row).toHaveCSS("flex-direction", "row-reverse");
    await page.getByRole("button", { name: "Column", exact: true }).click();
    await expect(row).toHaveCSS("flex-direction", "column-reverse");
    await page.getByRole("button", { name: "Row", exact: true }).click();
    await page.getByRole("button", { name: "Reverse direction" }).click();
    await expect(row).toHaveCSS("flex-direction", "row");
    await page.getByRole("button", { name: "Align baseline" }).click();
    await expect(row).toHaveCSS("align-items", "baseline");
    await expect(page.getByLabel("Align content")).toHaveCount(0);
    await page.getByRole("button", { name: "Wrap", exact: true }).click();
    await expect(row).toHaveCSS("flex-wrap", "wrap");
    await page.getByLabel("Align content").selectOption("space-between");
    await expect(row).toHaveCSS("align-content", "space-between");
  } finally {
    await agent.close();
  }
});

test("fill, stroke and effect controls write the matching CSS", async ({ page }) => {
  const agent = await scene(
    page,
    `<div layer-name="Source" style="position:absolute;left:20px;top:20px;width:80px;height:80px;background:#00f"></div>
     <div layer-name="Box" style="position:absolute;left:200px;top:100px;width:200px;height:120px;background:#ff0000"></div>`,
  );
  try {
    const box = await select(page, "Box");

    // Corner radius slider.
    await page.getByLabel("Corner radius slider").fill("20");
    await expect(box).toHaveCSS("border-radius", "20px");
    await expect(page.getByTestId("field-radius")).toHaveValue("20");

    // Conic gradient: reverse, rotate, distribute, color space.
    const fill = section(page, "Fill");
    await fill.getByLabel("Fill type").selectOption("conic");
    await expect(box).toHaveCSS("background-image", /^conic-gradient\((from 0deg, )?rgb\(255, 0, 0\)/);
    await fill.getByRole("button", { name: "Reverse stops" }).click();
    await expect(box).toHaveCSS("background-image", /^conic-gradient\((from 0deg, )?rgb\(0, 0, 0\)/);
    await fill.getByRole("button", { name: "Rotate 90°" }).click();
    await expect(box).toHaveCSS("background-image", /from 90deg/);
    await fill.getByLabel("Interpolation color space").selectOption("oklch");
    await expect(box).toHaveCSS("background-image", /from 90deg in oklch/);
    const lastStop = fill.getByTitle("Stop position").last().locator("input");
    await lastStop.fill("30");
    await lastStop.press("Enter");
    await expect(box).toHaveCSS("background-image", /30%/);
    await fill.getByRole("button", { name: "Distribute stops evenly" }).click();
    await expect(box).toHaveCSS("background-image", /100%\)$/);
    await fill.getByLabel("Fill type").selectOption("linear");
    await expect(box).toHaveCSS("background-image", /^linear-gradient\(/);

    // Capture a layer into an image fill.
    await section(page, "Fill").getByRole("button", { name: "Capture from layer" }).click();
    await page.getByRole("dialog", { name: "Capture from layer" }).getByRole("button", { name: /Source/ }).click();
    await expect(fill.getByLabel("Fill type").first()).toHaveValue("image");
    await expect(box).toHaveCSS("background-image", /^url\(".*\/assets\//);

    // Per-side border widths.
    await section(page, "Border").getByRole("button", { name: "Add border" }).click();
    await page.getByRole("button", { name: "Per-side widths" }).click();
    await page.getByTitle("Border top width").locator("input").fill("6");
    await page.getByTitle("Border top width").locator("input").press("Enter");
    await expect(box).toHaveCSS("border-top-width", "6px");
    await expect(box).toHaveCSS("border-bottom-width", "1px");

    // Outline, separate from the border.
    await section(page, "Outline").getByRole("button", { name: "Add outline" }).click();
    await expect(box).toHaveCSS("outline-style", "solid");
    await page.getByTitle("Outline width").locator("input").fill("3");
    await page.getByTitle("Outline width").locator("input").press("Enter");
    await page.getByTitle("Outline offset").locator("input").fill("2");
    await page.getByTitle("Outline offset").locator("input").press("Enter");
    await page.getByLabel("Outline style").selectOption("dashed");
    await expect(box).toHaveCSS("outline", /3px.*dashed|dashed.*3px/);
    await expect(box).toHaveCSS("outline-offset", "2px");
    await expect(box).toHaveCSS("border-top-width", "6px");

    // Filters sit beside blur and compose into one CSS filter.
    // Each edit composes from the current filters, so let it land before the next one.
    const edits: [string, string, RegExp][] = [
      ["Layer blur", "4", /^blur\(4px\)$/],
      ["Brightness", "150", /brightness\(1\.5\)/],
      ["Hue rotate", "90", /hue-rotate\(90deg\)/],
      ["Sepia", "40", /sepia\(0\.4\)/],
    ];
    for (const [title, value, css] of edits) {
      await page.getByTitle(title).locator("input").fill(value);
      await page.getByTitle(title).locator("input").press("Enter");
      await expect(box).toHaveCSS("filter", css.source.startsWith("^") ? css : new RegExp(`blur\\(4px\\).*${css.source}`));
    }
    await expect(box).toHaveCSS("filter", /blur\(4px\) brightness\(1\.5\) hue-rotate\(90deg\) sepia\(0\.4\)/);

    // Shadow presets.
    await section(page, "Shadow").getByRole("button", { name: "Shadow presets" }).click();
    await page.getByRole("button", { name: "Medium" }).click();
    await expect(box).toHaveCSS("box-shadow", /0px 4px 12px/);
    await section(page, "Shadow").getByRole("button", { name: "Shadow presets" }).click();
    await page.getByRole("button", { name: "Inner" }).click();
    await expect(box).toHaveCSS("box-shadow", /inset/);
  } finally {
    await agent.close();
  }
});

test("color fields bind and detach tokens, and the eyedropper samples the canvas", async ({ page }) => {
  // No native EyeDropper: the fallback renders the artboard and reads the pixel.
  await page.addInitScript(() => {
    (window as unknown as { EyeDropper?: unknown }).EyeDropper = undefined;
    delete (window as unknown as { EyeDropper?: unknown }).EyeDropper;
  });
  const agent = await scene(
    page,
    `<div layer-name="Red" style="position:absolute;left:20px;top:20px;width:200px;height:200px;background:#ff0000"></div>
     <div layer-name="Target" style="position:absolute;left:300px;top:20px;width:100px;height:100px;background:#0000ff"></div>`,
  );
  try {
    await call(agent, "create_tokens", { tokens: [{ name: "--color-brand", value: "#112233" }] });
    const target = await select(page, "Target");
    const fill = section(page, "Fill");
    await fill.getByRole("button", { name: "Fill tokens" }).click();
    await page.getByRole("button", { name: "Bind --color-brand" }).click();
    await expect(target).toHaveCSS("background-color", "rgb(17, 34, 51)");
    await expect(fill.getByTitle("--color-brand")).toBeVisible();
    await fill.getByRole("button", { name: "Detach Fill token" }).click();
    await expect(fill.getByLabel("Fill hex")).toHaveValue("112233");
    await expect(target).toHaveCSS("background-color", "rgb(17, 34, 51)");
    await expect(fill.getByRole("button", { name: "Detach Fill token" })).toHaveCount(0);

    // Eyedropper: click the red layer on the canvas.
    const red = page.locator(`[data-node="${await page.getByRole("treeitem", { name: /Red/ }).getAttribute("data-layer")}"]`);
    const box = (await red.boundingBox())!;
    await fill.getByRole("button", { name: "Fill eyedropper" }).click();
    await expect(page.getByTestId("eyedropper-overlay")).toBeVisible();
    await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
    await expect(fill.getByLabel("Fill hex")).toHaveValue("ff0000");
    await expect(target).toHaveCSS("background-color", "rgb(255, 0, 0)");

    // Escape cancels.
    await fill.getByRole("button", { name: "Fill eyedropper" }).click();
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("eyedropper-overlay")).toHaveCount(0);

    // The page canvas color takes tokens too.
    await page.keyboard.press("Escape");
    await page.mouse.click(280, 500);
    await page.getByRole("button", { name: "Canvas color tokens" }).click();
    await page.getByRole("button", { name: "Bind --color-brand" }).click();
    await expect(page.getByRole("button", { name: "Detach Canvas color token" })).toBeVisible();
    await page.getByRole("button", { name: "Detach Canvas color token" }).click();
    await expect(page.getByLabel("Canvas color hex")).toHaveValue("112233");
  } finally {
    await agent.close();
  }
});

test("text controls: vertical alignment, wrap, truncation, decoration and font features", async ({ page }) => {
  const agent = await scene(
    page,
    `<p layer-name="Copy" style="position:absolute;left:40px;top:40px;width:200px;height:120px;margin:0;font-size:18px">A long line of text that should wrap onto several lines in this box 0123</p>`,
  );
  try {
    const text = await select(page, "Copy");
    await page.getByRole("button", { name: "Align text middle" }).click();
    await expect(text).toHaveCSS("align-content", "center");
    await page.getByRole("button", { name: "Align text bottom" }).click();
    await expect(text).toHaveCSS("align-content", "end");
    await page.getByLabel("Text wrap").selectOption("balance");
    await expect(text).toHaveCSS("text-wrap-style", "balance");
    await page.getByLabel("Text wrap").selectOption("pretty");
    await expect(text).toHaveCSS("text-wrap-style", "pretty");

    await page.getByLabel("Truncate text").click();
    await expect(page.getByLabel("Truncate text")).toBeChecked();
    await expect(text).toHaveCSS("-webkit-line-clamp", "1");
    await expect(text).toHaveCSS("text-overflow", "ellipsis");
    await page.getByTitle("Max lines").locator("input").fill("2");
    await page.getByTitle("Max lines").locator("input").press("Enter");
    await expect(text).toHaveCSS("-webkit-line-clamp", "2");
    await page.getByLabel("Truncate text").click();
    await expect(text).toHaveCSS("-webkit-line-clamp", "none");

    await page.getByLabel("Decoration", { exact: true }).selectOption("overline");
    await expect(text).toHaveCSS("text-decoration-line", "overline");
    await page.getByLabel("Decoration style").selectOption("wavy");
    await expect(text).toHaveCSS("text-decoration-style", "wavy");

    await page.getByRole("button", { name: "Ligatures" }).click();
    await expect(text).toHaveCSS("font-variant-ligatures", "none");
    await page.getByRole("button", { name: "Tabular numbers" }).click();
    await page.getByRole("button", { name: "Slashed zero" }).click();
    await page.getByRole("button", { name: "Fractions" }).click();
    await expect(text).toHaveCSS("font-variant-numeric", /(?=.*tabular-nums)(?=.*slashed-zero)(?=.*diagonal-fractions)/);
    await page.getByRole("button", { name: "Tabular numbers" }).click();
    await expect(text).toHaveCSS("font-variant-numeric", /^(?!.*tabular-nums)/);
  } finally {
    await agent.close();
  }
});

test("font picker searches installed and Google fonts, stars favorites and lists font tokens", async ({ page }) => {
  const agent = await scene(page, `<p layer-name="Copy" style="position:absolute;left:40px;top:40px;margin:0;font-size:18px">Picker text</p>`);
  try {
    await call(agent, "create_tokens", { tokens: [{ name: "--font-display", value: "Geist Mono, monospace" }] });
    const text = await select(page, "Copy");
    await page.getByRole("button", { name: "Font family" }).click();
    const picker = page.getByRole("dialog", { name: "Font picker" });
    await picker.getByLabel("Search fonts").fill("spark test");
    await picker.getByRole("button", { name: "Favorite Spark Test Mono" }).click();
    await picker.getByRole("tab", { name: "Favorites" }).click();
    await expect(picker.getByTestId("font-row")).toHaveCount(1);
    await picker.getByRole("tab", { name: "Fonts" }).click();
    // Picking a Google family downloads it through /fonts/css and draws with it.
    await picker.getByRole("button", { name: "Spark Test Mono" }).first().click();
    await expect(text).toHaveCSS("font-family", /Spark Test Mono/);
    await expect
      .poll(() => page.evaluate(() => [...document.fonts].some((f) => f.family.replace(/["']/g, "") === "Spark Test Mono" && f.status === "loaded")))
      .toBe(true);

    // Favorites persist across reopening (local UI settings).
    await page.getByRole("button", { name: "Font family" }).click();
    await page.getByRole("dialog", { name: "Font picker" }).getByRole("tab", { name: "Favorites" }).click();
    await expect(page.getByTestId("font-row")).toHaveCount(1);
    await page.getByRole("dialog", { name: "Font picker" }).getByRole("tab", { name: "Tokens" }).click();
    await page.getByRole("button", { name: /display/ }).click();
    await expect(page.getByRole("button", { name: "Font family" })).toContainText("display");
  } finally {
    await agent.close();
  }
});

test("font picker stays fast with thousands of families", async ({ page }) => {
  const names = Array.from({ length: 2500 }, (_, i) => `Family ${String(i).padStart(4, "0")}`);
  await page.route("**/fonts/list", (r) => r.fulfill({ json: { installed: ["Local One"], google: names } }));
  const agent = await scene(page, `<p layer-name="Copy" style="position:absolute;left:40px;top:40px;margin:0">x</p>`);
  try {
    await select(page, "Copy");
    await page.getByRole("button", { name: "Font family" }).click();
    const list = page.getByTestId("font-list");
    await expect(list.getByTestId("font-row").first()).toBeVisible();
    expect(await list.getByTestId("font-row").count()).toBeLessThan(40);
    // The catalog may still be arriving, so scroll again until the last family is in view.
    await expect(async () => {
      await list.evaluate((el) => el.scrollTo(0, el.scrollHeight));
      await expect(list.getByRole("button", { name: /^Family 2499/ })).toBeVisible({ timeout: 500 });
    }).toPass();
    expect(await list.getByTestId("font-row").count()).toBeLessThan(40);
    await page.getByLabel("Search fonts").fill("Family 1234");
    await expect(list.getByTestId("font-row")).toHaveCount(1);
  } finally {
    await agent.close();
  }
});

test("selection colors list every color with counts and replace it in one undo step", async ({ page }) => {
  const agent = await scene(
    page,
    `<div layer-name="A" style="position:absolute;left:20px;top:20px;width:80px;height:80px;background:#ff0000;border:2px solid #ff0000"></div>
     <div layer-name="B" style="position:absolute;left:200px;top:20px;width:80px;height:80px;background:#ff0000;border:2px solid #0000ff"></div>`,
  );
  try {
    const a = await select(page, "A");
    const b = page.locator(`[data-node="${await page.getByRole("treeitem", { name: /(^|\s)B(\s|$)/ }).getAttribute("data-layer")}"]`);
    await page.getByRole("treeitem", { name: /(^|\s)B(\s|$)/ }).click({ modifiers: ["Shift"] });
    const rows = section(page, "Selection colors").getByTestId("selection-color");
    await expect(rows).toHaveCount(2);
    await expect(rows.nth(0)).toContainText("3");
    await expect(rows.nth(1)).toContainText("1");

    const hex = section(page, "Selection colors").getByLabel("Selection color 1 hex");
    await hex.fill("00ff00");
    await hex.press("Enter");
    for (const el of [a, b]) await expect(el).toHaveCSS("background-color", "rgb(0, 255, 0)");
    await expect(a).toHaveCSS("border-top-color", "rgb(0, 255, 0)");
    await expect(b).toHaveCSS("border-top-color", "rgb(0, 0, 255)");

    await page.keyboard.press("ControlOrMeta+z");
    for (const el of [a, b]) await expect(el).toHaveCSS("background-color", "rgb(255, 0, 0)");
    await expect(a).toHaveCSS("border-top-color", "rgb(255, 0, 0)");
  } finally {
    await agent.close();
  }
});

test("export settings: JPEG quality, filename suffix and copy as PNG", async ({ page, context, browserName }) => {
  if (browserName === "chromium") await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  const downloads = join(process.env.SPARK_DIR ?? "", "downloads");
  const listing = () => {
    try {
      return new Set(readdirSync(downloads));
    } catch {
      return new Set<string>();
    }
  };
  const agent = await scene(
    page,
    `<div layer-name="Art" style="position:absolute;left:20px;top:20px;width:400px;height:300px;background:linear-gradient(135deg,#ff0080,#00ffcc,#2000ff)"></div>`,
  );
  try {
    await select(page, "Art");
    const exp = section(page, "Export");
    const rows = exp.getByTestId("export-preset");
    for (let i = 0; i < 2; i++) {
      await exp.getByRole("button", { name: "Add export" }).click();
      await expect(rows).toHaveCount(i + 1);
    }
    for (const [i, [suffix, quality]] of [["-hq", "95"], ["-lq", "5"]].entries()) {
      // A controlled select shows the saved value once the edit lands; wait so the next edit builds on it.
      await rows.nth(i).getByLabel("Export scale").selectOption("1x");
      await expect(rows.nth(i).getByLabel("Export scale")).toHaveValue("1x");
      await rows.nth(i).getByLabel("Export format").selectOption("jpg");
      await expect(rows.nth(i).getByLabel("Export format")).toHaveValue("jpg");
      await rows.nth(i).getByRole("button", { name: "Export settings" }).click();
      const pop = page.getByRole("dialog", { name: "Export settings" });
      await pop.getByTitle("JPEG quality").locator("input").fill(quality);
      await pop.getByTitle("JPEG quality").locator("input").press("Enter");
      // Shows the saved value again once the edit has landed; the next edit builds on it.
      await expect(pop.getByTitle("JPEG quality").locator("input")).toHaveValue(quality);
      await pop.getByLabel("File name suffix").fill(suffix);
      await pop.getByLabel("File name suffix").press("Enter");
      await page.screenshot({ path: `${SHOTS}/export-settings-${i}.png` });
      // Close and reopen: the popover remounts from the saved preset, so this waits for the edit to land.
      await rows.nth(i).getByRole("button", { name: "Export settings" }).click();
      await rows.nth(i).getByRole("button", { name: "Export settings" }).click();
      await expect(page.getByRole("dialog", { name: "Export settings" }).getByLabel("File name suffix")).toHaveValue(suffix);
      await expect(page.getByRole("dialog", { name: "Export settings" }).getByTitle("JPEG quality").locator("input")).toHaveValue(quality);
      await rows.nth(i).getByRole("button", { name: "Export settings" }).click();
    }
    const before = listing();
    await exp.getByRole("button", { name: /^Export Art/ }).click();
    await expect(page.getByTestId("toast")).toContainText("Exported 2 files");
    const files = [...listing()].filter((f) => !before.has(f));
    const size = (suffix: string) => statSync(join(downloads, files.find((f) => f.includes(`Art${suffix}`)) ?? `missing${suffix}`)).size;
    expect(size("-lq")).toBeLessThan(size("-hq") / 2);

    await exp.getByRole("button", { name: "Copy as PNG" }).click();
    await expect(exp.getByRole("button", { name: /Copied|Copy failed/ })).toContainText("Copied");
    if (browserName === "chromium") {
      const type = await page.evaluate(async () => (await (await navigator.clipboard.read())[0].getType("image/png")).type);
      expect(type).toBe("image/png");
    }
  } finally {
    await agent.close();
  }
});

test("screenshots of the new inspector controls, light and dark", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "screenshots are taken once");
  await page.setViewportSize({ width: 1440, height: 2400 });
  const agent = await scene(
    page,
    `<div layer-name="Frame" style="position:absolute;left:40px;top:40px;width:300px;height:200px;background:linear-gradient(135deg,#ff0080,#2000ff);border:2px solid #000;border-radius:12px;box-shadow:0 4px 12px #0000001f"></div>
     <p layer-name="Copy" style="position:absolute;left:400px;top:40px;width:240px;height:120px;margin:0;font-size:18px;color:#112233">Text with tokens 0123</p>`,
  );
  try {
    await call(agent, "create_tokens", { tokens: [{ name: "--color-brand", value: "#112233" }, { name: "--font-display", value: "Geist Mono, monospace" }] });
    for (const theme of ["light", "dark"]) {
      if (theme === "dark") await page.getByRole("button", { name: "Use dark theme" }).click();
      await select(page, "Frame");
      await section(page, "Fill").getByLabel("Fill type").selectOption("conic");
      if (theme === "light") {
        await page.getByRole("button", { name: "Add outline" }).click();
        await page.getByRole("button", { name: "Per-side widths" }).click();
      }
      const inspector = page.getByRole("complementary", { name: "Inspector" });
      await inspector.screenshot({ path: `${SHOTS}/inspector-frame-${theme}.png` });
      await section(page, "Fill").getByLabel("Fill type").selectOption("solid");
      await section(page, "Fill").getByRole("button", { name: "Fill tokens" }).click();
      await page.getByRole("dialog", { name: "Fill tokens" }).screenshot({ path: `${SHOTS}/token-list-${theme}.png` });
      await page.keyboard.press("Escape");
      await section(page, "Shadow").getByRole("button", { name: "Shadow presets" }).click();
      await page.getByRole("dialog", { name: "Shadow presets" }).screenshot({ path: `${SHOTS}/shadow-presets-${theme}.png` });
      await page.keyboard.press("Escape");
      await section(page, "Fill").getByRole("button", { name: "Capture from layer" }).click();
      await page.getByRole("dialog", { name: "Capture from layer" }).screenshot({ path: `${SHOTS}/capture-layer-${theme}.png` });
      await page.keyboard.press("Escape");

      await select(page, "Copy");
      await inspector.screenshot({ path: `${SHOTS}/inspector-text-${theme}.png` });
      await page.getByRole("button", { name: "Font family" }).click();
      await page.getByRole("dialog", { name: "Font picker" }).screenshot({ path: `${SHOTS}/font-picker-${theme}.png` });
      await page.keyboard.press("Escape");
    }
  } finally {
    await agent.close();
  }
});
