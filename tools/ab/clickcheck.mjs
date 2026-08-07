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

  const report = await page.evaluate(async () => {
    const a = document.querySelector('a[href*="/catalog/"][href*="/detail.aspx"]');
    if (!a) return { error: 'no card anchor' };
    a.scrollIntoView();

    const rect1 = a.getBoundingClientRect();
    await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
    const rect2 = a.getBoundingClientRect();

    const cx = rect2.left + rect2.width / 2;
    const cy = rect2.top + rect2.height / 2;
    const hit = document.elementFromPoint(cx, cy);
    const style = getComputedStyle(a);

    // Playwright walks up from the hit element looking for the target.
    let found = false;
    for (let n = hit; n; n = n.parentElement) if (n === a) { found = true; break; }

    return {
      // 1 visible
      rect: { x: rect2.left, y: rect2.top, w: rect2.width, h: rect2.height },
      nonEmpty: rect2.width > 0 && rect2.height > 0,
      visibility: style.visibility,
      display: style.display,
      // 2 stable across two frames
      stable: rect1.left === rect2.left && rect1.top === rect2.top &&
              rect1.width === rect2.width && rect1.height === rect2.height,
      rectBefore: { x: rect1.left, y: rect1.top, w: rect1.width, h: rect1.height },
      // 3 receives events
      hitTag: hit ? (hit.tagName + (hit.id ? '#' + hit.id : '')) : 'null',
      hitIsTargetOrChild: found,
      // 4 enabled
      disabled: a.hasAttribute('disabled'),
      inViewport: cx >= 0 && cy >= 0 &&
                  cx <= (window.innerWidth || 0) && cy <= (window.innerHeight || 0),
      viewport: { w: window.innerWidth, h: window.innerHeight },
      scroll: { x: window.scrollX, y: window.scrollY },
    };
  });
  console.log(JSON.stringify(report, null, 2));

  await context.close();
  await browser.close();
} finally {
  child.kill();
}
