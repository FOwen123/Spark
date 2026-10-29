// Rebuild the theme variants and desktop icons from the vector master.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const brand = join(root, "web/public/brand");
const source = readFileSync(join(brand, "spark.svg"), "utf8");
const white = source.replace('fill="#000"', 'fill="#fff"');
const background = (svg, color) => svg.replace(/(<svg[^>]*>)/, `$1\n  <rect width="1024" height="1024" rx="224" fill="${color}"/>`);
const light = background(source, "#fff");
const dark = background(white, "#000");
const temporary = mkdtempSync(join(tmpdir(), "spark-icons-"));

function icons(input, output, sizes = []) {
  execFileSync("bun", ["run", "--cwd", "apps/desktop", "tauri", "icon", input, "--output", output, ...sizes.flatMap((size) => ["--png", String(size)])], {
    cwd: root,
    stdio: "inherit",
  });
}

try {
  writeFileSync(join(brand, "spark-white.svg"), white);
  writeFileSync(join(brand, "spark-light.svg"), light);
  writeFileSync(join(brand, "spark-dark.svg"), dark);
  writeFileSync(join(root, "apps/desktop/icon.svg"), dark);
  writeFileSync(join(root, "web/public/favicon.svg"), light.replace(/(<svg[^>]*>)/, "$1\n  <style>@media (prefers-color-scheme: dark) { rect { fill: #000; } g { fill: #fff; } }</style>"));
  for (const theme of ["light", "dark"]) {
    const sizes = [16, 32, 64, 128, 256, 512, 1024];
    const output = join(temporary, theme);
    icons(join(brand, `spark-${theme}.svg`), output, sizes);
    for (const size of sizes) copyFileSync(join(output, `${size}x${size}.png`), join(brand, `spark-${theme}-${size}.png`));
  }
  const desktop = join(temporary, "desktop");
  icons(join(root, "apps/desktop/icon.svg"), desktop);
  for (const name of ["32x32.png", "128x128.png", "128x128@2x.png", "icon.png", "icon.ico", "icon.icns"]) {
    copyFileSync(join(desktop, name), join(root, "apps/desktop/src-tauri/icons", name));
  }
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
