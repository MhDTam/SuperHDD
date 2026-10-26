import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "tests/ui",
  outputDir: "output/playwright/results",
  reporter: [
    ["list"],
    ["json", { outputFile: "output/playwright/results.json" }],
  ],
  use: {
    baseURL: "http://127.0.0.1:1420",
    headless: true,
    viewport: { width: 1440, height: 940 },
    launchOptions: {
      executablePath:
        "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
    },
    screenshot: "only-on-failure",
  },
  webServer: {
    command: "npm run dev",
    url: "http://127.0.0.1:1420",
    reuseExistingServer: true,
    timeout: 30000,
  },
});
