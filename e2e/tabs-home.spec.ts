import { type Page, expect, test } from "@playwright/test";
import { appUrl } from "./util";

const uid = () => Math.random().toString(36).slice(2, 8);
const bar = (page: Page) => page.locator("header");
const tabs = (page: Page) => page.getByTestId("file-tab");
const row = (page: Page, name: string) => page.getByRole("listitem").filter({ has: page.getByText(name, { exact: true }) });

/** Opens a new file in a tab and renames it through the tab's menu. */
async function addFile(page: Page, name: string) {
  const before = await tabs(page).count();
  await bar(page).getByRole("button", { name: "New file" }).click();
  await expect(tabs(page)).toHaveCount(before + 1);
  const tab = tabs(page).last();
  await expect(tab).toHaveAttribute("aria-selected", "true");
  await tab.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Rename" }).click();
  await bar(page).getByLabel("Name", { exact: true }).fill(name);
  await page.keyboard.press("Enter");
  await expect(tab).toHaveText(name);
}

// "Copy link" writes to the system clipboard, which bundled Chromium refuses without the permission.
test.beforeEach(async ({ context, browserName }) => {
  if (browserName === "chromium") await context.grantPermissions(["clipboard-read", "clipboard-write"]);
});

test("tabs: context menu, drag to reorder, reopen and hotkeys", async ({ page }) => {
  await page.goto(appUrl());
  const [a, b, c] = [`a-${uid()}`, `b-${uid()}`, `c-${uid()}`];
  for (const n of [a, b, c]) await addFile(page, n);
  await expect(tabs(page)).toHaveText([a, b, c]);

  // Drag the first tab onto the right half of the last one.
  const box = (await tabs(page).nth(2).boundingBox())!;
  await tabs(page).nth(0).dragTo(tabs(page).nth(2), { targetPosition: { x: box.width - 4, y: box.height / 2 } });
  await expect(tabs(page)).toHaveText([b, c, a]);

  // Mod+N jumps to tab N, Mod+9 to the last; Ctrl+Tab cycles through Home too.
  await page.keyboard.press("ControlOrMeta+1");
  await expect(tabs(page).nth(0)).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("ControlOrMeta+9");
  await expect(tabs(page).nth(2)).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("Control+Tab");
  await expect(page.getByRole("tab", { name: "Home" })).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("Control+Shift+Tab");
  await expect(tabs(page).nth(2)).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("ControlOrMeta+Shift+D");
  await expect(page.getByRole("tab", { name: "Home" })).toHaveAttribute("aria-selected", "true");

  // Copy link.
  await tabs(page).nth(1).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Copy link" }).click();
  await expect(page.getByTestId("toast")).toHaveText("Link copied");

  // Close others keeps one tab; Mod+Shift+T brings the closed ones back, newest first.
  await tabs(page).nth(0).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Close others" }).click();
  await expect(tabs(page)).toHaveText([b]);
  await page.keyboard.press("ControlOrMeta+Shift+T");
  await expect(tabs(page)).toHaveText([b, a]);
  await page.keyboard.press("ControlOrMeta+Shift+T");
  await expect(tabs(page)).toHaveText([b, a, c]);

  await tabs(page).nth(0).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Close tabs to the right" }).click();
  await expect(tabs(page)).toHaveText([b]);
  await tabs(page).nth(0).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Close all" }).click();
  await expect(tabs(page)).toHaveCount(0);
  await expect(page.getByRole("tab", { name: "Home" })).toHaveAttribute("aria-selected", "true");

  // Mod+T opens a new file tab.
  await page.keyboard.press("ControlOrMeta+T");
  await expect(tabs(page)).toHaveCount(1);
  await expect(tabs(page).first()).toHaveAttribute("aria-selected", "true");
});

test("home: search, views, folders, move and the file menu", async ({ page }) => {
  await page.goto(appUrl());
  const name = `File ${uid()}`;
  await addFile(page, name);
  await tabs(page).first().hover();
  await page.getByRole("button", { name: `Close ${name}` }).click();
  await page.keyboard.press("ControlOrMeta+Shift+D");
  await expect(row(page, name)).toBeVisible();

  // Search filters by name.
  await page.getByLabel("Search files").fill(name.toLowerCase().slice(0, 9));
  await expect(row(page, name)).toBeVisible();
  await page.getByLabel("Search files").fill(`nothing-${uid()}`);
  await expect(page.getByText("No files match your search.")).toBeVisible();
  await page.getByLabel("Search files").fill("");

  // The view choice sticks across reloads.
  await page.getByRole("button", { name: "Grid view" }).click();
  await page.reload();
  await expect(page.getByRole("button", { name: "Grid view" })).toHaveAttribute("aria-pressed", "true");
  await page.getByRole("button", { name: "List view" }).click();

  // New folder, then move the file into it from the file menu.
  const folder = `Folder ${uid()}`;
  await page.getByRole("button", { name: "New folder" }).click();
  await page.getByLabel("Name", { exact: true }).fill(folder);
  await page.keyboard.press("Enter");
  const open = page.getByRole("button", { name: `Open folder ${folder}` });
  await expect(open).toBeVisible();
  await row(page, name).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Move to…" }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toContainText(`Move ${name} to…`);
  await dialog.getByRole("option", { name: folder }).click();
  await expect(dialog.getByTestId("move-path")).toContainText(folder);
  await dialog.getByRole("button", { name: "Move here" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(row(page, name)).toHaveCount(0);
  await open.click();
  await expect(row(page, name)).toBeVisible();

  // Rename, duplicate and copy the link from the menu.
  const renamed = `Renamed ${uid()}`;
  await row(page, name).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Rename" }).click();
  await page.getByLabel("Name", { exact: true }).fill(renamed);
  await page.keyboard.press("Enter");
  await expect(row(page, renamed)).toBeVisible();
  await row(page, renamed).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Duplicate" }).click();
  await expect(row(page, `${renamed} copy`)).toBeVisible();
  await row(page, renamed).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Copy link" }).click();
  await expect(page.getByTestId("toast")).toHaveText("Link copied");

  // Open in a new tab keeps Home on screen; Open switches to the file.
  await row(page, renamed).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Open in new tab" }).click();
  await expect(tabs(page)).toHaveText([renamed]);
  await expect(page.getByRole("tab", { name: "Home" })).toHaveAttribute("aria-selected", "true");
  await row(page, `${renamed} copy`).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Open", exact: true }).click();
  await expect(tabs(page).last()).toHaveAttribute("aria-selected", "true");

  // Archive and Trash from the menu use the same commands as the buttons.
  await tabs(page).last().hover();
  await page.getByRole("button", { name: `Close ${renamed} copy` }).click();
  await page.getByRole("tab", { name: "Home" }).click();
  await open.click();
  await tabs(page).first().hover();
  await page.getByRole("button", { name: `Close ${renamed}` }).click();
  await row(page, renamed).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Archive" }).click();
  await expect(page.getByText(`Moved ${renamed} to `)).toContainText("/Archive/");
  await row(page, `${renamed} copy`).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Move to Trash" }).click();
  await expect(page.getByText(`Moved ${renamed} copy to the Trash`)).toBeVisible();
});

test("home: the first-run callout points to the agent connect flow and stays dismissed", async ({ page }) => {
  await page.goto(appUrl());
  const callout = page.getByRole("region", { name: "Get started" });
  await expect(callout).toContainText("Try a first prompt");
  await callout.getByRole("button", { name: "Set up an agent" }).click();
  await expect(page.getByRole("dialog", { name: "Connect your agent" })).toBeVisible();
  await page.keyboard.press("Escape");
  await callout.getByRole("button", { name: "Dismiss" }).click();
  await expect(callout).toHaveCount(0);
  await page.reload();
  await expect(page.getByRole("button", { name: "Connect your agent" })).toBeVisible();
  await expect(callout).toHaveCount(0);
});

test("keyboard shortcuts dialog is generated from the keymap; commands can toast", async ({ page }) => {
  await page.goto(appUrl());
  await page.keyboard.press("Control+Shift+/");
  const dialog = page.getByRole("dialog", { name: "Keyboard shortcuts" });
  await expect(dialog).toBeVisible();
  for (const label of ["Move tool", "Undo", "Reopen closed tab", "Go to tab 1–8", "Zoom to fit"]) await expect(dialog.getByText(label, { exact: true })).toBeVisible();
  await dialog.getByLabel("Search shortcuts").fill("reopen");
  await expect(dialog.getByText("Reopen closed tab")).toBeVisible();
  await expect(dialog.getByText("Undo", { exact: true })).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);

  await bar(page).getByRole("button", { name: "New file" }).click();
  await expect(page.locator("[data-node]").first()).toBeVisible();
  await page.locator("[data-node]").first().click();
  await page.keyboard.press("Shift+1");
  await expect(page.getByTestId("toast")).toHaveText("Zoom to fit");
});
