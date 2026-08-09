// Run a JS expression in the real Chrome and print the result.
//
//   node tools/ab/probe-chrome.mjs <file.js> [--headed]
//
// Raw CDP: Page.navigate and Runtime.evaluate only, never Runtime.enable, and
// AutomationControlled disabled. Same posture as chrome-raw.mjs, so a difference
// against Obscura is a difference in the engine and not in the driver.
//
// Chrome is always shut down over CDP before the process is killed; on Windows
// child.kill() reaps only the launcher stub and would leave a window open.
import { spawn, execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, readFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { WebSocket } from 'node:http';

const file = process.argv[2];
const headed = process.argv.includes('--headed');
const urlArg = (process.argv.includes('--url') && process.argv[process.argv.indexOf('--url') + 1]) || 'about:blank';
const sizeArg = process.argv.includes('--window-size') && process.argv[process.argv.indexOf('--window-size') + 1];
if (!file) { console.error('usage: probe-chrome.mjs <file.js> [--headed]'); process.exit(2); }
const expression = readFileSync(file, 'utf8');

const CHROME_CANDIDATES = [
  'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  'C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe',
];
const chromePath = CHROME_CANDIDATES.find(p => existsSync(p));
if (!chromePath) { console.error('Chrome not found'); process.exit(2); }

const profileDir = mkdtempSync(join(tmpdir(), 'chrome-probe-'));
const port = 9800 + Math.floor(Math.random() * 500);
const args = [
  `--remote-debugging-port=${port}`,
  `--user-data-dir=${profileDir}`,
  '--remote-allow-origins=*',
  '--disable-blink-features=AutomationControlled',
  '--no-first-run',
  '--no-default-browser-check',
  '--no-service-autorun',
  '--password-store=basic',
];
if (!headed) args.push('--headless=new');
if (sizeArg) args.push(`--window-size=${sizeArg}`);

const chrome = spawn(chromePath, args, { stdio: 'ignore', windowsHide: true });
const sleep = ms => new Promise(done => setTimeout(done, ms));

async function targetUrl() {
  const deadline = Date.now() + 30000;
  for (;;) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      const page = list.find(t => t.type === 'page' && t.webSocketDebuggerUrl);
      if (page) return page.webSocketDebuggerUrl;
    } catch { /* not up yet */ }
    if (Date.now() > deadline) throw new Error('Chrome did not expose a page target');
    await sleep(200);
  }
}

let cdp;
try {
  const { WebSocket: WS } = await import('ws').catch(() => ({ WebSocket: globalThis.WebSocket }));
  const url = await targetUrl();
  const socket = new (WS || globalThis.WebSocket)(url);
  let nextId = 1;
  const pending = new Map();
  await new Promise((ok, bad) => { socket.onopen = ok; socket.onerror = bad; });
  socket.onmessage = ev => {
    const msg = JSON.parse(typeof ev.data === 'string' ? ev.data : ev.data.toString());
    const entry = pending.get(msg.id);
    if (entry) { pending.delete(msg.id); entry(msg); }
  };
  cdp = {
    send: (method, params = {}) => new Promise((resolve, reject) => {
      const id = nextId++;
      pending.set(id, msg => (msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result)));
      socket.send(JSON.stringify({ id, method, params }));
    }),
    close: () => socket.close(),
  };

  await cdp.send('Page.navigate', { url: urlArg });
  await sleep(urlArg === 'about:blank' ? 1200 : 9000);
  const result = await cdp.send('Runtime.evaluate', {
    expression, returnByValue: true, awaitPromise: true,
  });
  if (result.exceptionDetails) {
    console.error('evaluate threw:', JSON.stringify(result.exceptionDetails.exception));
    process.exitCode = 1;
  } else {
    console.log(result.result.value);
  }
} finally {
  try { await cdp?.send('Browser.close'); } catch { /* already gone */ }
  try { cdp?.close(); } catch { /* already closed */ }
  await sleep(400);
  chrome.kill();
  try {
    const leaf = profileDir.split(/[\\/]/).pop();
    execFileSync('powershell', ['-NoProfile', '-Command',
      `Get-CimInstance Win32_Process -Filter "Name='chrome.exe'" |` +
      ` Where-Object { $_.CommandLine -like '*${leaf}*' } |` +
      ` ForEach-Object { taskkill /PID $_.ProcessId /T /F }`,
    ], { stdio: 'ignore' });
  } catch { /* best effort */ }
  try { rmSync(profileDir, { recursive: true, force: true }); } catch { /* best effort */ }
}
