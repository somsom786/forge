import { defineConfig } from '@playwright/test';

// SIMULATED end-to-end tests: the React UI in Chromium against the in-browser mock backend (src/lib/devPreview*.ts),
// which `vite` serves in development when no Tauri runtime is present. They exercise the UI's behaviour, keyboard use,
// layout and accessibility. They do NOT exercise the Rust core, real missions, or the Windows WebView2 runtime.
export default defineConfig({
  testDir: 'e2e',
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [['list']],
  use: {
    baseURL: 'http://localhost:1420',
    // The desktop window's minimum size (src-tauri/tauri.conf.json): layouts must hold at this size.
    viewport: { width: 1100, height: 700 },
    trace: 'retain-on-failure',
    launchOptions: { args: ['--disable-gpu'], ...(process.env.PLAYWRIGHT_CHROMIUM_PATH ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_PATH } : {}) },
  },
  webServer: {
    command: 'npx vite --port 1420 --strictPort',
    url: 'http://localhost:1420',
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
