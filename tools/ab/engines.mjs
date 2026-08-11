// Launching the two engines, so the scripts that compare them don't each carry
// their own copy of it. Four of them did, and the proxy handling drifted between
// copies — which is how a run ended up going through an exit it never asked for.

import { execFileSync, spawn } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import net from 'node:net';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export const root = resolve(import.meta.dirname, '..', '..');
const obscuraBin = join(root, 'target', 'release', 'obscura.exe');
const chromeBin = [
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
].find(existsSync);
const playwrightPath = join(root, 'target', 'test-fixtures', 'playwright',
  'node_modules', 'playwright-core', 'index.mjs');

export const { chromium } = await import(pathToFileURL(playwrightPath).href);

const sleep = ms => new Promise(done => setTimeout(done, ms));

function powerShellQuote(value) {
  return `'${String(value).replaceAll("'", "''")}'`;
}

function windowsArgument(value) {
  value = String(value);
  return /[\s"]/.test(value) ? `"${value.replaceAll('"', '\\"')}"` : value;
}

// Shell.Application runs the program through the already-running Explorer
// process. This matters when the agent process itself was started with a proxy:
// deleting variables in a child is not enough to leave that process tree's
// network path, while a normal interactive Chrome already owned by Explorer is
// direct. This is a Windows-only diagnostic control.
function launchViaExplorer(executable, args, visible) {
  if (process.platform !== 'win32') {
    throw new Error('--clean-host is supported only on Windows');
  }
  const argumentString = args.map(windowsArgument).join(' ');
  const script = [
    '$shell = New-Object -ComObject Shell.Application',
    `$shell.ShellExecute(${powerShellQuote(executable)}, ` +
      `${powerShellQuote(argumentString)}, ${powerShellQuote(root)}, 'open', ${visible ? 1 : 0})`,
  ].join('; ');
  execFileSync('powershell.exe', ['-NoProfile', '-Command', script], {
    stdio: 'ignore',
    windowsHide: true,
  });
}

async function waitForCdp(port, name) {
  const deadline = Date.now() + 30000;
  for (;;) {
    try {
      const probe = await fetch(`http://127.0.0.1:${port}/json/version`);
      if (probe.ok) return;
    } catch { /* not up yet */ }
    if (Date.now() > deadline) throw new Error(`${name} did not start`);
    await sleep(200);
  }
}

function stopExplorerProcess(executable, commandLineNeedle) {
  if (process.platform !== 'win32') return;
  const script = [
    `$executable = ${powerShellQuote(resolve(executable))}`,
    'Get-CimInstance Win32_Process | Where-Object { ' +
      '$_.ExecutablePath -eq $executable -and $_.CommandLine -like ' +
      `${powerShellQuote(`*${commandLineNeedle}*`)} } | ` +
      'ForEach-Object { Stop-Process -Id $_.ProcessId -Force }',
  ].join('; ');
  try {
    execFileSync('powershell.exe', ['-NoProfile', '-Command', script], {
      stdio: 'ignore',
      windowsHide: true,
    });
  } catch { /* process may already be gone */ }
}

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

// Remove ordinary proxy variables from normal child launches. If the parent
// process tree itself has the wrong network path, use --clean-host as well.
export function childEnv(proxy, extra = {}) {
  const env = { ...process.env, ...extra };
  for (const name of ['OBSCURA_PROXY', 'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY',
                      'NO_PROXY', 'http_proxy', 'https_proxy', 'all_proxy', 'no_proxy']) {
    delete env[name];
  }
  if (proxy) {
    env.OBSCURA_PROXY = proxy;
  } else {
    // Windows may still have a system proxy after the standard proxy variables
    // are gone. Make a no-proxy A/B run direct by construction.
    env.NO_PROXY = '*';
    env.no_proxy = '*';
  }
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

/// Runs `scenario(page)` in the real system Chrome, then `afterScenario`.
export async function withChrome(opts, scenario, afterScenario) {
  if (opts.cleanHost) {
    if (opts.proxy) throw new Error('--clean-host cannot be combined with --proxy');
    if (opts.profileDir) throw new Error('--clean-host always uses a fresh Chrome profile');
    if (!chromeBin) throw new Error('system Chrome was not found');

    const port = await freePort();
    const profileDir = mkdtempSync(join(tmpdir(), 'obscura-clean-chrome-'));
    let browser;
    try {
      const args = [
        `--remote-debugging-port=${port}`,
        `--user-data-dir=${profileDir}`,
        '--no-first-run',
        '--no-default-browser-check',
        'about:blank',
      ];
      if (!opts.headed) args.unshift('--headless=new');
      launchViaExplorer(chromeBin, args, opts.headed);
      await waitForCdp(port, 'Chrome');
      browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
      const context = browser.contexts()[0] || await browser.newContext();
      const page = context.pages()[0] || await context.newPage();
      const result = await scenario(page);
      return afterScenario ? await afterScenario(page, result) : result;
    } finally {
      try { await browser?.close(); } catch { /* already closed */ }
      await sleep(500);
      stopExplorerProcess(chromeBin, profileDir.split(/[\\/]/).pop());
      try { rmSync(profileDir, { recursive: true, force: true }); } catch {}
    }
  }

  if (opts.profileDir) {
    const context = await chromium.launchPersistentContext(opts.profileDir, {
      channel: 'chrome',
      headless: !opts.headed,
      proxy: proxyForPlaywright(opts.proxy),
      env: childEnv(),
    });
    try {
      const page = await context.newPage();
      const result = await scenario(page);
      return afterScenario ? await afterScenario(page, result) : result;
    } finally {
      await context.close();
    }
  }

  const browser = await chromium.launch({
    channel: 'chrome',
    headless: !opts.headed,
    proxy: proxyForPlaywright(opts.proxy),
    env: childEnv(),
  });
  try {
    const context = await browser.newContext();
    const page = await context.newPage();
    const result = await scenario(page);
    return afterScenario ? await afterScenario(page, result) : result;
  } finally {
    await browser.close();
  }
}

/// Runs `scenario(page)` in Obscura, then `afterScenario`, before teardown.
export async function withObscura(opts, scenario, afterScenario) {
  const port = await freePort();
  if (opts.cleanHost && opts.proxy) {
    throw new Error('--clean-host cannot be combined with --proxy');
  }
  let child;
  let cleanHostDir;
  if (opts.cleanHost) {
    cleanHostDir = mkdtempSync(join(tmpdir(), 'obscura-clean-host-'));
    const launcher = join(cleanHostDir, 'launch.cmd');
    const extra = opts.env || {};
    for (const [name, value] of Object.entries(extra)) {
      if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(name) || /[\r\n]/.test(String(value))) {
        throw new Error(`unsafe clean-host environment entry: ${name}`);
      }
    }
    const lines = [
      '@echo off',
      'set OBSCURA_PROXY=',
      'set HTTP_PROXY=', 'set HTTPS_PROXY=', 'set ALL_PROXY=',
      'set http_proxy=', 'set https_proxy=', 'set all_proxy=',
      'set NO_PROXY=*', 'set no_proxy=*',
      'set OBSCURA_NAV_TIMEOUT_MS=90000',
      ...Object.entries(extra).map(([name, value]) =>
        `set "${name}=${String(value).replaceAll('%', '%%').replaceAll('"', '""')}"`),
      `"${obscuraBin}" --stealth serve --port ${port}`,
    ];
    writeFileSync(launcher, lines.join('\r\n'), 'utf8');
    launchViaExplorer(launcher, [], false);
  } else {
    child = spawn(obscuraBin, ['--stealth', 'serve', '--port', String(port)], {
      cwd: root,
      env: childEnv(opts.proxy, { OBSCURA_NAV_TIMEOUT_MS: '90000', ...(opts.env || {}) }),
      stdio: ['ignore', 'pipe', 'pipe'],
      windowsHide: true,
    });
    child.stdout.on('data', () => {});
    child.stderr.on('data', opts.onStderr || (() => {}));
  }
  try {
    await waitForCdp(port, 'Obscura');
    const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
    const context = await browser.newContext();
    try {
      const page = await context.newPage();
      const result = await scenario(page);
      return afterScenario ? await afterScenario(page, result) : result;
    } finally {
      await context.close();
      await browser.close();
    }
  } finally {
    if (child) child.kill();
    else stopExplorerProcess(obscuraBin, `--port ${port}`);
    if (cleanHostDir) {
      try { rmSync(cleanHostDir, { recursive: true, force: true }); } catch {}
    }
  }
}

export const runIn = (engine, opts, scenario, afterScenario) =>
  (engine === 'chrome' ? withChrome : withObscura)(opts, scenario, afterScenario);
