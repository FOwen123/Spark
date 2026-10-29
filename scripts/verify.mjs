// Runs every check the repo requires before a commit. Stops at the first failure.
import { execSync } from "node:child_process";

const steps = [
  ["rustfmt", "cargo fmt --all --check"],
  ["clippy", "cargo clippy --workspace --all-targets -- -D warnings"],
  ["cargo test", "cargo test --workspace"],
  ["tsc", "bun run --cwd web typecheck && bunx tsc --noEmit"],
  ["lint", "bun run --cwd web lint"],
  ["vitest", "bun run --cwd web test"],
  ["web build", "bun run --cwd web build"],
  ["spark binary", "cargo build -p spark-cli"],
  ["e2e", "bunx playwright test"],
];

for (const [name, cmd] of steps) {
  const started = Date.now();
  console.log(`\n▶ ${name}: ${cmd}`);
  try {
    execSync(cmd, { stdio: "inherit" });
  } catch {
    console.error(`\n✗ ${name} failed`);
    process.exit(1);
  }
  console.log(`✓ ${name} (${((Date.now() - started) / 1000).toFixed(1)}s)`);
}
console.log("\nAll checks passed.");
