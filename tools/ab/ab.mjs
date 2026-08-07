// Run one scenario against real Chrome and against Obscura, both over CDP, and
// print the two side by side.
//
//   node tools/ab/ab.mjs <url> [options]
//
//     --needle <text>    text to wait for in document.innerText, and time
//     --match <regexp>   list responses whose URL matches this
//     --only <engine>    "chrome" or "obscura"
//     --wait <seconds>   how long to wait for the needle (default 20)
//     --proxy <url>      send both engines through the same proxy
//     --headed           show the Chrome window
//
// Obscura is launched with --stealth on a free port and killed afterwards.
// OBSCURA_PROXY is used when --proxy is absent. Nothing is written to disk.
//
// Both sides go through Playwright so the scenario cannot drift between them,
// and so that anything Obscura fails to report over CDP shows up as a
// difference rather than as a silent gap. That matters: stealth mode once
// reported 9 of its own 173 requests, which made every comparison misleading
// until it was found.

import { spawn } from 'node:child_process';
import net from 'node:net';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const root = resolve(import.meta.dirname, '..', '..');
const obscuraBin = join(root, 'target', 'release', 'obscura.exe');
const playwrightPath = join(root, 'target', 'test-fixtures', 'playwright',
  'node_modules', 'playwright-core', 'index.mjs');

function parseArgs(argv) {
  const opts = { wait: 20, headed: false };
  const rest = [];
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--headed') opts.headed = true;
    else if (arg === '--needle') opts.needle = argv[++i];
    else if (arg === '--match') opts.match = new RegExp(argv[++i]);
    else if (arg === '--only') opts.only = argv[++i];
    else if (arg === '--wait') opts.wait = Number(argv[++i]);
    else if (arg === '--proxy') opts.proxy = argv[++i];
    else rest.push(arg);
  }
  opts.url = rest[0];
  return opts;
}

const opts = parseArgs(process.argv.slice(2));
if (!opts.url) {
  console.error('usage: node tools/ab/ab.mjs <url> [--needle text] [--match regexp]' +
                ' [--only chrome|obscura] [--wait seconds] [--proxy url] [--headed]');
  process.exit(2);
}

const { chromium } = await import(pathToFileURL(playwrightPath).href);

// A navigation part way through an evaluate destroys the execution context.
// That is ordinary on a site that redirects itself, not a result, so retry.
async function tryEvaluate(page, fn, arg) {
  for (let attempt = 0; attempt < 5; attempt++) {
    try {
      return await page.evaluate(fn, arg);
    } catch (error) {
      if (!String(error).includes('Execution context was destroyed')) throw error;
      await new Promise(done => setTimeout(done, 500));
    }
  }
  return null;
}

async function scenario(page) {
  const traffic = [];
  const errors = [];
  let requests = 0;

  page.on('request', () => { requests += 1; });
  page.on('response', response => {
    if (opts.match && opts.match.test(response.url())) {
      traffic.push(`${response.status()} ${response.url().slice(0, 140)}`);
    }
  });
  page.on('requestfailed', request => {
    if (opts.match && opts.match.test(request.url())) {
      traffic.push(`FAILED(${request.failure()?.errorText}) ${request.url().slice(0, 140)}`);
    }
  });
  page.on('pageerror', error => errors.push('pageerror: ' + String(error).slice(0, 200)));
  page.on('console', message => {
    if (message.type() === 'error') errors.push('console: ' + message.text().slice(0, 200));
  });

  const started = Date.now();
  await page.goto(opts.url, { waitUntil: 'load', timeout: 90000 });

  let needleAfter = null;
  if (opts.needle) {
    for (let second = 1; second <= opts.wait; second++) {
      await new Promise(done => setTimeout(done, 1000));
      const found = await tryEvaluate(page, needle =>
        (document.body ? document.body.innerText : '').includes(needle), opts.needle);
      if (found) { needleAfter = second; break; }
    }
  }

  const bodyLength = await tryEvaluate(page, () =>
    (document.body ? document.body.innerText.replace(/\s+/g, ' ') : '').length);

  return { needleAfter, bodyLength, requests, traffic, errors, elapsed: Date.now() - started };
}

function proxyForPlaywright(raw) {
  if (!raw) return undefined;
  const parsed = new URL(raw);
  const proxy = { server: `${parsed.protocol}//${parsed.host}` };
  if (parsed.username) proxy.username = decodeURIComponent(parsed.username);
  if (parsed.password) proxy.password = decodeURIComponent(parsed.password);
  return proxy;
}

async function withChrome() {
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

async function withObscura() {
  const port = await freePort();
  const child = spawn(obscuraBin, ['--stealth', 'serve', '--port', String(port)], {
    cwd: root,
    // Explicit, not inherited. A proxy sitting in the shell sends a run through
    // an exit it never asked for, and the result then reads as the site
    // treating this machine differently rather than as a different IP.
    // HTTPS_PROXY is the one that actually bit: it was set here, only some
    // request paths honoured it, and runs silently disagreed about their own
    // exit address.
    env: (() => {
      const env = { ...process.env, OBSCURA_NAV_TIMEOUT_MS: '90000' };
      for (const name of ['OBSCURA_PROXY', 'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY',
                          'http_proxy', 'https_proxy', 'all_proxy']) {
        delete env[name];
      }
      if (opts.proxy) env.OBSCURA_PROXY = opts.proxy;
      return env;
    })(),
    stdio: ['ignore', 'pipe', 'pipe'],
    windowsHide: true,
  });
  child.stdout.on('data', () => {});
  child.stderr.on('data', () => {});
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

const engines = opts.only ? [opts.only] : ['chrome', 'obscura'];
for (const engine of engines) {
  try {
    const out = engine === 'chrome' ? await withChrome() : await withObscura();
    const needle = opts.needle ? `  needle=${out.needleAfter ?? 'NEVER'}s` : '';
    console.log(`\n=== ${engine}  ${out.elapsed}ms${needle}` +
                `  bodyLength=${out.bodyLength}  requests=${out.requests}`);
    if (opts.match) {
      console.log(`   matched responses: ${out.traffic.length}`);
      for (const line of out.traffic.slice(0, 30)) console.log('     ' + line);
      if (out.traffic.length > 30) console.log(`     ... and ${out.traffic.length - 30} more`);
    }
    const unique = [...new Set(out.errors)];
    for (const line of unique.slice(0, 12)) console.log('   ' + line);
    if (unique.length > 12) console.log(`   ... and ${unique.length - 12} more errors`);
    if (!unique.length) console.log('   (no errors)');
  } catch (error) {
    console.log(`\n=== ${engine} THREW ${String(error).slice(0, 400)}`);
  }
}
