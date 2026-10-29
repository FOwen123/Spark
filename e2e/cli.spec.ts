import { execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdtempSync, readFileSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";

// `spark connect` against fixture configs in throwaway homes. These don't need a browser, so
// they run once, in the "cli" project.
const FIXTURES = join(__dirname, "..", "fixtures", "agents", "home");
const AGENTS = ["claude-code", "codex", "cursor", "vscode", "copilot", "opencode", "gemini", "claude-desktop"];

function freshHome() {
  const home = mkdtempSync(join(tmpdir(), "spark-home-"));
  cpSync(FIXTURES, home, { recursive: true });
  return home;
}

function spark(args: string[], home: string) {
  return execFileSync(process.env.SPARK_BIN ?? "", args, { env: { ...process.env, SPARK_CONFIG_DIR: join(home, ".spark-config"), SPARK_NO_OPEN: "1" }, encoding: "utf8" });
}

test.describe("spark connect", () => {
  for (const agent of AGENTS) {
    test(`--dry-run for ${agent} shows a diff and writes nothing`, () => {
      const home = freshHome();
      const before = readFileSync(join(home, ".codex/config.toml"), "utf8");
      const out = spark(["connect", agent, "--dry-run", "--home", home], home);
      expect(out).toContain("Would edit");
      expect(out).toContain("mcp");
      expect(readFileSync(join(home, ".codex/config.toml"), "utf8")).toBe(before);
      expect(existsSync(join(home, ".spark-config", "spark.mcpb"))).toBe(false);
    });
  }

  test("writes keep comments, are idempotent and private", () => {
    const home = freshHome();
    spark(["connect", "opencode", "--home", home], home);
    const oc = readFileSync(join(home, ".config/opencode/opencode.jsonc"), "utf8");
    expect(oc).toContain("// Keep this comment: opencode reads JSONC");
    expect(oc).toContain("// trailing comment");
    expect(oc).toContain('"spark"');
    expect(spark(["connect", "opencode", "--home", home], home)).toContain("nothing to change");

    spark(["connect", "codex", "--home", home], home);
    const toml = readFileSync(join(home, ".codex/config.toml"), "utf8");
    expect(toml).toContain('model = "example-model"   # the default model');
    expect(toml).toContain("[mcp_servers.spark]");

    const fresh = mkdtempSync(join(tmpdir(), "spark-empty-home-"));
    spark(["connect", "gemini", "--home", fresh], fresh);
    expect(statSync(join(fresh, ".gemini/settings.json")).mode & 0o777).toBe(0o600);

    spark(["disconnect", "codex", "--home", home], home);
    expect(readFileSync(join(home, ".codex/config.toml"), "utf8")).not.toContain("spark");
  });
});


test("spark export renders a demo file headlessly", () => {
  const dir = mkdtempSync(join(tmpdir(), "spark-export-"));
  const file = join(dir, "demo.spark");
  execFileSync(process.env.SPARK_BIN ?? "", ["demo", file, "--nodes", "120"]);
  for (const format of ["png", "jpg", "webp", "svg", "pdf", "html", "tailwind"]) {
    const out = execFileSync(process.env.SPARK_BIN ?? "", ["export", file, "--format", format, "--out", dir], { encoding: "utf8" });
    const path = out.trim().split("\n")[0];
    expect(existsSync(path)).toBe(true);
    expect(statSync(path).size).toBeGreaterThan(100);
  }
  const png = readFileSync(join(dir, "Overview 1.png"));
  expect(png.readUInt32BE(16)).toBe(1440);
  const json = JSON.parse(execFileSync(process.env.SPARK_BIN ?? "", ["export", file, "--json"], { encoding: "utf8" }));
  expect(json.pages[0].children.length).toBeGreaterThan(0);
  expect(execFileSync(process.env.SPARK_BIN ?? "", ["export", file, "--tokens", "theme"], { encoding: "utf8" })).toContain("@theme {");
});
