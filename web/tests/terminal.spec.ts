import { expect, test } from '@playwright/test';
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { createInterface } from 'node:readline';

let server: ChildProcessWithoutNullStreams;
let bootstrapUrl: string;
let serverStderr = '';

type FunctionalSnapshot = {
  schema: number;
  terminalText: string;
  cursorRow: number;
  cursorColumn: number;
  cols: number;
  rows: number;
  connectionState: string;
};

async function installLoopbackOnlyNetworkPolicy(
  page: import('@playwright/test').Page,
): Promise<void> {
  await page.route('**/*', async (route) => {
    const host = new URL(route.request().url()).hostname;
    if (host === 'localhost' || host === '127.0.0.1' || host === '[::1]') {
      await route.continue();
    } else {
      await route.abort('blockedbyclient');
    }
  });
}

async function snapshot(page: import('@playwright/test').Page): Promise<FunctionalSnapshot> {
  return page.evaluate(() => {
    const observer = (window as typeof window & {
      __RSSH_FUNCTIONAL_SNAPSHOT__?: () => FunctionalSnapshot;
    }).__RSSH_FUNCTIONAL_SNAPSHOT__;
    if (!observer) {
      throw new Error('functional Web observer is unavailable');
    }
    return observer();
  });
}

test.beforeAll(async () => {
  server = spawn('../target/debug/rssh-web', [
    '--listen',
    '127.0.0.1:0',
    '--web-root',
    'dist',
  ]);
  server.stderr.on('data', (chunk: Buffer) => {
    serverStderr += chunk.toString('utf8');
  });
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

test.afterAll(async () => {
  server.kill('SIGTERM');
  await new Promise<void>((resolve, reject) => {
    const timeout = setTimeout(() => {
      server.kill('SIGKILL');
      reject(new Error(`rssh-web did not stop within cleanup deadline: ${serverStderr}`));
    }, 10_000);
    server.once('exit', () => {
      clearTimeout(timeout);
      resolve();
    });
  });
});

test('redeems the bootstrap ticket and opens a real PTY session', async ({ page, request, browserName }) => {
  await installLoopbackOnlyNetworkPolicy(page);
  await page.setViewportSize({ width: 960, height: 620 });
  const response = await page.goto(bootstrapUrl);
  expect(response?.url()).not.toContain('token=');
  expect(response?.headers()['cache-control']).toContain('no-store');
  expect(response?.headers()['referrer-policy']).toBe('no-referrer');
  expect(response?.headers()['content-security-policy']).toContain("frame-ancestors 'none'");
  await expect(page.locator('#connection-status')).toHaveText('Connected');
  await expect(page.locator('#terminal')).toBeVisible();

  const replay = await request.get(bootstrapUrl, { maxRedirects: 0 });
  expect(replay.status()).toBe(401);
  expect(replay.headers()['cache-control']).toContain('no-store');
  expect(replay.headers()['referrer-policy']).toBe('no-referrer');
  if (browserName === 'chromium') {
    await page.context().grantPermissions(['clipboard-read', 'clipboard-write'], {
      origin: new URL(bootstrapUrl).origin,
    });
  }
  const terminal = page.locator('#terminal');
  const bounds = await terminal.boundingBox();
  if (!bounds) {
    throw new Error('terminal has no browser layout box');
  }
  await page.mouse.click(bounds.x + 40, bounds.y + 40);
  await page.keyboard.type('echo web-keyboard-probe');
  await page.keyboard.press('Enter');
  await expect
    .poll(async () => (await snapshot(page)).terminalText, { timeout: 15_000 })
    .toContain('web-keyboard-probe');

  await page.evaluate(() => navigator.clipboard.writeText('echo web-clipboard-probe'));
  await page.keyboard.press('Control+V');
  await page.keyboard.press('Enter');
  await expect
    .poll(async () => (await snapshot(page)).terminalText, { timeout: 15_000 })
    .toContain('web-clipboard-probe');

  const before = await snapshot(page);
  await page.setViewportSize({ width: 720, height: 480 });
  await expect
    .poll(async () => await snapshot(page), { timeout: 15_000 })
    .not.toMatchObject({ cols: before.cols, rows: before.rows });

  await page.keyboard.type('exit');
  await page.keyboard.press('Enter');
  await expect(page.locator('#connection-status')).toContainText('Exited');
  await page.locator('#restart').click();
  await expect(page.locator('#connection-status')).toHaveText('Connected');
  expect((await snapshot(page)).connectionState).toBe('open');
});
