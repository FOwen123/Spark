import { type ChildProcess, spawn } from "node:child_process";
import { chmodSync, cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";

// Starts `spark serve` on a free port with a throwaway data dir for the whole run.
export default async function globalSetup() {
  const dir = mkdtempSync(join(tmpdir(), "spark-e2e-"));
  const bin = join(__dirname, "..", "target", "debug", process.platform === "win32" ? "spark.exe" : "spark");
  // Agent configs live in a throwaway home seeded from the fixtures; never the real one.
  const home = join(dir, "home");
  cpSync(join(__dirname, "..", "fixtures", "agents", "home"), home, { recursive: true });
  // A fake `claude` CLI on PATH for the agent panel.
  const fakeBin = join(dir, "bin");
  mkdirSync(fakeBin);
  writeFileSync(join(fakeBin, "claude"), FAKE_CLAUDE);
  chmodSync(join(fakeBin, "claude"), 0o755);
  // Google Fonts stand-in: one family, "Spark Test Mono", served from the bundled Geist Mono.
  const fontFile = readFileSync(join(__dirname, "..", "web", "node_modules", "@fontsource-variable", "geist-mono", "files", "geist-mono-latin-wght-normal.woff2"));
  const fonts = createServer((req, res) => {
    const path = new URL(req.url ?? "/", "http://x").pathname;
    if (path === "/metadata/fonts") return res.end(`)]}'\n${JSON.stringify({ familyMetadataList: [{ family: "Spark Test Mono", fonts: { "400": {} }, axes: [] }] })}`);
    if (path === "/css2") return res.end(`@font-face { font-family: 'Spark Test Mono'; src: url(${fontsUrl}/s/test/mono.woff2) format('woff2'); }`);
    if (path === "/s/test/mono.woff2") return res.end(fontFile);
    res.statusCode = 404;
    res.end();
  });
  await new Promise<void>((r) => fonts.listen(0, "127.0.0.1", r));
  const fontsUrl = `http://127.0.0.1:${(fonts.address() as { port: number }).port}`;
  const child: ChildProcess = spawn(bin, ["serve", "--port", "0"], {
    env: {
      ...process.env,
      SPARK_CONFIG_DIR: join(dir, "config"),
      SPARK_FILES_DIR: join(dir, "files"),
      SPARK_HOME_DIR: home,
      SPARK_DOWNLOADS_DIR: join(dir, "downloads"),
      // Deleted files go to a throwaway folder instead of the real Trash.
      SPARK_TRASH_DIR: join(dir, "trash"),
      SPARK_NO_OPEN: "1",
      // Provider keys go to a throwaway file instead of the real keychain.
      SPARK_KEYRING_FILE: join(dir, "keys.json"),
      SPARK_GOOGLE_FONTS_URL: fontsUrl,
      PATH: `${fakeBin}:${process.env.PATH}`,
    },
    stdio: ["ignore", "pipe", "inherit"],
  });
  const url = await new Promise<string>((resolve, reject) => {
    let out = "";
    child.stdout?.on("data", (chunk) => {
      out += chunk;
      const m = out.match(/running at (http:\/\/\S+)/);
      if (m) resolve(m[1]);
    });
    child.on("exit", (code) => reject(new Error(`spark serve exited with ${code}`)));
  });
  process.env.SPARK_URL = url;
  process.env.SPARK_DIR = dir;
  process.env.SPARK_BIN = bin;
  return async () => {
    child.kill();
    fonts.close();
    rmSync(dir, { recursive: true, force: true });
  };
}

const FAKE_CLAUDE = `#!/bin/sh
echo '{"type":"system","subtype":"init"}'
for a in "$@"; do
  case "$a" in --mcp-config) echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Spark MCP attached"}]}}';; esac
done
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Reading the file"},{"type":"tool_use","name":"mcp__spark__get_basic_info"}]}}'
echo '{"type":"result","result":"All done"}'
`;
