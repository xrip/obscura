// Launching the two engines, so the scripts that compare them don't each carry
// their own copy of it. Four of them did, and the proxy handling drifted between
// copies — which is how a run ended up going through an exit it never asked for.

import { spawn } from 'node:child_process';
import net from 'node:net';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export const root = resolve(import.meta.dirname, '..', '..');
const obscuraBin = join(root, 'target', 'release', 'obscura.exe');
const playwrightPath = join(root, 'target', 'test-fixtures', 'playwright',
  'node_modules', 'playwright-core', 'index.mjs');

export const { chromium } = await import(pathToFileURL(playwrightPath).href);

export function freePort() {
  return new Promise((done, fail) => {
    const server = net.createServer();
    server.once('error', fail);
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      server.close(() => done(port));
    });
  });
}

// Playwright wants the credentials split out of the URL, which also keeps them
// off Chrome's command line.
export function proxyForPlaywright(raw) {
  if (!raw) return undefined;
  const parsed = new URL(raw);
  const proxy = { server: `${parsed.protocol}//${parsed.host}` };
  if (parsed.username) proxy.username = decodeURIComponent(parsed.username);
  if (parsed.password) proxy.password = decodeURIComponent(parsed.password);
  return proxy;
}

// A proxy sitting in the shell sends a run through an exit it never asked for,
// and the result then reads as the site treating this machine differently
// rather than as a different IP. HTTPS_PROXY is the one that actually bit.
export function childEnv(proxy, extra = {}) {
  const env = { ...process.env, ...extra };
  for (const name of ['OBSCURA_PROXY', 'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY',
                      'http_proxy', 'https_proxy', 'all_proxy']) {
    delete env[name];
  }
  if (proxy) env.OBSCURA_PROXY = proxy;
  return env;
}

// A navigation part way through an evaluate destroys the execution context.
// Ordinary on a site that redirects itself, so retry rather than lose the step.
// Returns { value } or { gaveUp }: a bare null could not be told apart from a
// legitimately null result, and typeof null === 'object' once disguised a give
// up as an unserialisable value for long enough to send me after the wrong bug.
export async function tryEvaluate(page, fn, arg) {
  for (let attempt = 0; attempt < 5; attempt++) {
    try {
      return { value: await page.evaluate(fn, arg) };
    } catch (error) {
      if (!String(error).includes('Execution context was destroyed')) throw error;
      await new Promise(done => setTimeout(done, 500));
    }
  }
  return { gaveUp: true };
}

export const evaluated = result => (result && 'value' in result ? result.value : undefined);

/// Runs `scenario(page)` in the real system Chrome.
export async function withChrome(opts, scenario) {
  const browser = await chromium.launch({
    channel: 'chrome',
    headless: !opts.headed,
    proxy: proxyForPlaywright(opts.proxy),
  });
  try {
    const context = await browser.newContext();
    return await scenario(await context.newPage());
  } finally {
    await browser.close();
  }
}

/// Runs `scenario(page)` in Obscura, started with --stealth on a free port and
/// killed afterwards.
export async function withObscura(opts, scenario) {
  const port = await freePort();
  const child = spawn(obscuraBin, ['--stealth', 'serve', '--port', String(port)], {
    cwd: root,
    env: childEnv(opts.proxy, { OBSCURA_NAV_TIMEOUT_MS: '90000', ...(opts.env || {}) }),
    stdio: ['ignore', 'pipe', 'pipe'],
    windowsHide: true,
  });
  child.stdout.on('data', () => {});
  child.stderr.on('data', opts.onStderr || (() => {}));
  try {
    const deadline = Date.now() + 30000;
    for (;;) {
      try {
        const probe = await fetch(`http://127.0.0.1:${port}/json/version`);
        if (probe.ok) break;
      } catch { /* not up yet */ }
      if (Date.now() > deadline) throw new Error('obscura did not start');
      await new Promise(done => setTimeout(done, 200));
    }
    const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
    const context = await browser.newContext();
    const result = await scenario(await context.newPage());
    await context.close();
    await browser.close();
    return result;
  } finally {
    child.kill();
  }
}

export const runIn = (engine, opts, scenario) =>
  (engine === 'chrome' ? withChrome : withObscura)(opts, scenario);
