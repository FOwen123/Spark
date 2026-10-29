import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

const server = process.env.SPARK_URL ?? "http://127.0.0.1:29890";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  build: { assetsDir: "static" },
  server: {
    proxy: {
      "/ui": { target: server.replace(/^http/, "ws"), ws: true },
      "/api": server,
      "/assets": server,
      "/fonts": server,
    },
  },
  test: { include: ["src/**/*.test.ts"] },
});
