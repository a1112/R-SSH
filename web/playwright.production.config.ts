import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  testMatch: 'production.spec.ts',
  timeout: 60_000,
  retries: 0,
  workers: 1,
  outputDir: '../evidence/production-web/results',
  reporter: [
    ['list'],
    ['json', { outputFile: '../evidence/production-web/playwright.json' }],
  ],
  projects: [{ name: 'chromium', use: { browserName: 'chromium', headless: true } }],
  use: { screenshot: 'only-on-failure', trace: 'retain-on-failure' },
});
