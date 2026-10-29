import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "e2e",
  globalSetup: "./e2e/global-setup.ts",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [["list"]],
  use: { trace: "retain-on-failure" },
  projects: [
    { name: "cli", testMatch: /cli\.spec\.ts/ },
    // Frame-time benchmarks only run when asked for: `bun run bench`.
    ...(process.env.SPARK_DOCS ? [{ name: "docs", testMatch: /\.docs\.ts/, use: { ...devices["Desktop Chrome"], viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 } }] : []),
    ...(process.env.SPARK_BENCH ? [{ name: "bench", testMatch: /\.bench\.ts/, use: { ...devices["Desktop Chrome"], viewport: { width: 1440, height: 900 } } }] : []),
    { name: "chromium", testIgnore: /cli\.spec\.ts/, use: { ...devices["Desktop Chrome"], viewport: { width: 1440, height: 900 } } },
    { name: "webkit", testIgnore: /cli\.spec\.ts/, use: { ...devices["Desktop Safari"], viewport: { width: 1440, height: 900 } } },
  ],
});
