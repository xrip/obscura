// Real Chrome, driven over raw CDP, with the automation tells switched off.
//
//   node tools/ab/chrome-raw.mjs [--cards 3] [--headed] [--proxy url]
//
// Playwright is convenient but it announces itself: it sends Runtime.enable on
// every page, and Chrome launched for automation carries
// --enable-automation and the AutomationControlled blink feature. Any of those
// is enough for a site to treat the session differently, which makes "real
// Chrome" a poor control exactly when the control matters.
//
// So this launches Chrome itself and speaks CDP over the websocket by hand:
// Page.navigate and Runtime.evaluate only, never Runtime.enable.

import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const HOME = 'https://www.wildberries.ru/';

function parseArgs(argv) {
  const opts = { cards: 3, headed: false, wait: 20 };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--cards') opts.cards = Number(argv[++i]);
    else if (argv[i] === '--headed') opts.headed = true;
    else if (argv[i] === '--proxy') opts.proxy = argv[++i];
    else if (argv[i] === '--wait') opts.wait = Number(argv[++i]);
  }
  return opts;
}
const opts = parseArgs(process.argv.slice(2));

const CHROME_CANDIDATES = [
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
];
const chromePath = CHROME_CANDIDATES.find(p => {
  try { return !!require('node:fs').statSync(p); } catch { return false; }
}) || CHROME_CANDIDATES[0];

const profileDir = mkdtempSync(join(tmpdir(), 'chrome-raw-'));
const port = 9333 + Math.floor(Math.random() * 500);
const args = [
  `--remote-debugging-port=${port}`,
  `--user-data-dir=${profileDir}`,
  // The three things that make a launched Chrome look launched.
  '--disable-blink-features=AutomationControlled',
  '--no-first-run',
  '--no-default-browser-check',
  '--no-service-autorun',
  '--password-store=basic',
];
if (!opts.headed) args.push('--headless=new');
if (opts.proxy) {
  const parsed = new URL(opts.proxy);
  args.push(`--proxy-server=${parsed.protocol}//${parsed.host}`);
}
args.push('about:blank');

const chrome = spawn(chromePath, args, { stdio: 'ignore', windowsHide: true });

const sleep = ms => new Promise(done => setTimeout(done, ms));

async function targetWebSocket() {
  const deadline = Date.now() + 30000;
  for (;;) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      const page = list.find(t => t.type === 'page');
      if (page?.webSocketDebuggerUrl) return page.webSocketDebuggerUrl;
    } catch { /* not up yet */ }
    if (Date.now() > deadline) throw new Error('chrome did not expose a page target');
    await sleep(200);
  }
}

let nextId = 0;
function connect(url) {
  const socket = new WebSocket(url);
  const pending = new Map();
  socket.addEventListener('message', event => {
    const message = JSON.parse(event.data);
    const waiter = pending.get(message.id);
    if (waiter) { pending.delete(message.id); waiter(message); }
  });
  const ready = new Promise((done, fail) => {
    socket.addEventListener('open', done);
    socket.addEventListener('error', fail);
  });
  const send = (method, params = {}) => new Promise((done, fail) => {
    const id = ++nextId;
    pending.set(id, message => message.error
      ? fail(new Error(`${method}: ${message.error.message}`))
      : done(message.result));
    socket.send(JSON.stringify({ id, method, params }));
  });
  return { ready, send, close: () => socket.close() };
}

// Runtime.evaluate works without Runtime.enable; that is the whole point.
async function evaluate(cdp, expression) {
  const result = await cdp.send('Runtime.evaluate', {
    expression,
    returnByValue: true,
    awaitPromise: true,
  });
  return result?.result?.value;
}

async function navigate(cdp, url) {
  await cdp.send('Page.navigate', { url });
  // No Page.lifecycleEvent without Page.enable, so poll readyState instead.
  const deadline = Date.now() + 90000;
  for (;;) {
    await sleep(500);
    const state = await evaluate(cdp, 'document.readyState + "|" + location.href');
    if (typeof state === 'string' && state.startsWith('complete') &&
        !state.endsWith('|about:blank')) return;
    if (Date.now() > deadline) throw new Error(`navigation to ${url} did not complete`);
  }
}

const productId = url => (url.match(/\/catalog\/(\d+)\/detail/) || [])[1];

try {
  const cdp = connect(await targetWebSocket());
  await cdp.ready;

  console.log('automation tells:');
  console.log('   navigator.webdriver =',
    await evaluate(cdp, 'String(navigator.webdriver)'));

  await navigate(cdp, HOME);
  let links = [];
  for (let second = 1; second <= opts.wait; second++) {
    await sleep(1000);
    links = await evaluate(cdp,
      'JSON.stringify([...document.querySelectorAll(\'a[href*="/catalog/"][href*="/detail.aspx"]\')].map(a => a.href))');
    links = JSON.parse(links || '[]');
    if (links.length >= 3) break;
  }
  const unique = [...new Set(links.filter(productId))];
  console.log(`home: ${unique.length} product links`);

  const picked = [];
  while (picked.length < Math.min(opts.cards, unique.length)) {
    const candidate = unique[Math.floor(Math.random() * unique.length)];
    if (!picked.includes(candidate)) picked.push(candidate);
  }

  let opened = 0;
  for (const url of picked) {
    const id = productId(url);
    await sleep(1500 + Math.random() * 2000);
    try {
      await navigate(cdp, url);
      let at = null;
      for (let second = 1; second <= opts.wait; second++) {
        await sleep(1000);
        const found = await evaluate(cdp,
          `(document.body ? document.body.innerText : '').includes(${JSON.stringify(id)})`);
        if (found) { at = second; break; }
      }
      const length = await evaluate(cdp,
        "(document.body ? document.body.innerText.replace(/\\s+/g, ' ') : '').length");
      if (at !== null) opened += 1;
      console.log(`card ${id}: ${at !== null ? `opened after ${at}s` : 'NEVER rendered'} (${length} chars)`);
    } catch (error) {
      console.log(`card ${id}: FAILED ${String(error).slice(0, 140)}`);
    }
  }
  console.log(`${opened}/${picked.length} cards opened`);
} finally {
  // Ask Chrome to shut itself down before killing anything. On Windows the
  // spawned process is only a launcher stub: child.kill() reaps the stub and
  // leaves the real browser running, which with --headed means a window the
  // user has to close by hand. Browser.close takes the whole tree down.
  try { await cdp.send('Browser.close'); } catch { /* already gone */ }
  try { cdp.close(); } catch { /* already closed */ }
  await sleep(500);
  chrome.kill();
  // Backstop for the stub case: kill anything still holding this run's
  // throwaway profile. Scoped to that directory so a user's own Chrome, and
  // any other run of this script, are never touched.
  if (process.platform === 'win32') {
    try {
      const { execFileSync } = await import('node:child_process');
      const leaf = profileDir.split(/[\/]/).pop();
      execFileSync('powershell', ['-NoProfile', '-Command',
        `Get-CimInstance Win32_Process -Filter "Name='chrome.exe'" |` +
        ` Where-Object { $_.CommandLine -like '*${leaf}*' } |` +
        ` ForEach-Object { taskkill /PID $_.ProcessId /T /F }`,
      ], { stdio: 'ignore' });
    } catch { /* best effort */ }
  }
  try { rmSync(profileDir, { recursive: true, force: true }); } catch { /* best effort */ }
}
