import { expect, test } from '@playwright/test';
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { createInterface } from 'node:readline';

let server: ChildProcessWithoutNullStreams;
let bootstrapUrl: string;

test.beforeAll(async () => {
  server = spawn('../target/debug/rssh-web', [
    '--listen',
    '127.0.0.1:0',
    '--web-root',
    'dist',
  ]);
  bootstrapUrl = await new Promise<string>((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error('rssh-web did not publish a bootstrap URL')), 15_000);
    createInterface({ input: server.stdout }).on('line', (line) => {
      const match = line.match(/^R-SSH Web terminal: (http:\/\/\S+)$/);
      if (match?.[1]) {
        clearTimeout(timeout);
        resolve(match[1]);
      }
    });
    server.once('exit', (code) => {
      clearTimeout(timeout);
      reject(new Error(`rssh-web exited before startup with code ${String(code)}`));
    });
  });
});

test.afterAll(() => {
  server.kill('SIGTERM');
});

test('redeems the bootstrap ticket and opens a real PTY session', async ({ page, request }) => {
  const response = await page.goto(bootstrapUrl);
  expect(response?.url()).not.toContain('token=');
  await expect(page.locator('#connection-status')).toHaveText('Connected');
  await expect(page.locator('#terminal')).toBeVisible();

  const replay = await request.get(bootstrapUrl, { maxRedirects: 0 });
  expect(replay.status()).toBe(401);
  expect(replay.headers()['cache-control']).toContain('no-store');
  expect(replay.headers()['referrer-policy']).toBe('no-referrer');
});
