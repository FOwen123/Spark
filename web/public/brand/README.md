# Spark logo

`spark.svg` is the vector master: a sharp hourglass.
The lower half reuses the upper path rotated 180 degrees, so both halves have
exactly the same size and shape with opposite bends.

- `spark.svg` and `spark-white.svg`: transparent black and white marks.
- `spark-light.svg` and `spark-dark.svg`: black on white and white on black tiles.
- PNG tiles are available at 16, 32, 64, 128, 256, 512 and 1024 pixels.

The UI masks the vector with its foreground color to follow Spark's theme.
The browser favicon follows the browser's color scheme; the desktop icon uses
the white mark on black. The root README selects the mark for GitHub's theme.

After editing the master, run `bun run icons` from the repository root to rebuild
all variants, the favicon and the desktop PNG, ICO and ICNS icons. This uses the
Tauri CLI already installed by `bun install`.
