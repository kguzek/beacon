import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/e2e",
  use: { baseURL: process.env.BEACON_WEB_URL ?? "http://127.0.0.1:4173" },
  ...(process.env.BEACON_WEB_URL === undefined && {
    webServer: {
      command: "pnpm preview --port 4173",
      url: "http://127.0.0.1:4173",
    },
  }),
});
