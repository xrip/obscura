// Which of Playwright's actionability preconditions does a Wildberries card
// anchor fail in Obscura? Run them by hand, in Playwright's own order.

import { spawn } from 'node:child_process';
import net from 'node:net';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const root = resolve(import.meta.dirname, '..', '..');
const obscuraBin = join(root, 'target', 'release', 'obscura.exe');
const playwrightPath = join(root, 'target', 'test-fixtures', 'playwright',
  'node_modules', 'playwright-core', 'index.mjs');
const { chromium } = await import(pathToFileURL(playwrightPath).href);

function freePort() {
  return new Promise((done, fail) => {
    const server = net.createServer();
    server.once('error', fail);
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      server.close(() => done(port));
    });
  });
}

const port = await freePort();
const child = spawn(obscuraBin, ['--stealth', 'serve', '--port', String(port)], {
  cwd: root, env: { ...process.env, OBSCURA_NAV_TIMEOUT_MS: '90000' },
  stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true,
});
child.stdout.on('data', () => {});
child.stderr.on('data', () => {});

try {
  const deadline = Date.now() + 30000;
  for (;;) {
    try { const r = await fetch(`http://127.0.0.1:${port}/json/version`); if (r.ok) break; } catch {}
    if (Date.now() > deadline) throw new Error('obscura did not start');
    await new Promise(done => setTimeout(done, 200));
  }
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  const context = await browser.newContext();
  const page = await context.newPage();
  await page.goto('https://www.wildberries.ru/', { waitUntil: 'load', timeout: 90000 });
  await new Promise(done => setTimeout(done, 3000));

  const nav = [];
  page.on('framenavigated', f => nav.push('nav:' + f.url().slice(0, 70)));
  page.on('request', r => { if (r.url().includes('/catalog/')) nav.push('req:' + r.url().slice(0, 70)); });
  const report = await page.evaluate(async () => {
    const a = document.querySelector('a[href*="/catalog/"][href*="/detail.aspx"]');
    if (!a) return { error: 'no card anchor' };
    const href = a.getAttribute('href');
    let defaultPrevented = null;
    a.addEventListener('click', e => { defaultPrevented = e.defaultPrevented; }, true);
    const after = new Promise(r => setTimeout(r, 1500));
    a.click();
    await after;
    return { href, defaultPrevented, locationAfter: location.href, historyLen: history.length };
  });
  console.log(JSON.stringify(report, null, 2));
  console.log(JSON.stringify(nav, null, 1));

  await context.close();
  await browser.close();
} finally {
  child.kill();
}
