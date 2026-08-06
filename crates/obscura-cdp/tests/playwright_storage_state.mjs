import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { access, mkdir, readFile, rm } from 'node:fs/promises';
import http from 'node:http';
import net from 'node:net';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const playwrightVersion = '1.62.1';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const fixtureRoot = join(root, 'target', 'test-fixtures', 'playwright');
const playwrightPath = join(fixtureRoot, 'node_modules', 'playwright-core', 'index.mjs');
const statePath = join(fixtureRoot, `storage-state-${process.pid}.json`);
const obscuraBin = process.env.OBSCURA_BIN || join(
  root,
  'target',
  'release',
  process.platform === 'win32' ? 'obscura.exe' : 'obscura',
);

async function exists(path) {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

async function run(command, args) {
  await new Promise((done, fail) => {
    const child = spawn(command, args, {
      cwd: root,
      stdio: 'inherit',
      windowsHide: true,
    });
    child.once('error', fail);
    child.once('exit', code => {
      if (code === 0) done();
      else fail(new Error(`${command} stopped with code ${code}`));
    });
  });
}

async function loadPlaywright() {
  if (!await exists(playwrightPath)) {
    await mkdir(fixtureRoot, { recursive: true });
    await run(process.platform === 'win32' ? 'npm.cmd' : 'npm', [
      'install',
      '--prefix', fixtureRoot,
      `playwright-core@${playwrightVersion}`,
      '--no-save',
      '--no-package-lock',
      '--ignore-scripts',
      '--no-audit',
      '--no-fund',
    ]);
  }
  return import(pathToFileURL(playwrightPath).href);
}

async function freePort() {
  const server = net.createServer();
  await new Promise((done, fail) => {
    server.once('error', fail);
    server.listen(0, '127.0.0.1', done);
  });
  const port = server.address().port;
  await new Promise(done => server.close(done));
  return port;
}

async function listen(server) {
  await new Promise((done, fail) => {
    server.once('error', fail);
    server.listen(0, '127.0.0.1', done);
  });
  return `http://127.0.0.1:${server.address().port}`;
}

async function waitForServer(child, port) {
  for (let attempt = 0; attempt < 100; attempt++) {
    if (child.exitCode !== null) {
      throw new Error(`Obscura stopped with code ${child.exitCode}`);
    }
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/version`);
      if (response.ok) return;
    } catch {}
    await new Promise(done => setTimeout(done, 100));
  }
  throw new Error('Obscura did not become ready');
}

function cookie(value) {
  return {
    name: 'sid',
    value,
    domain: '127.0.0.1',
    path: '/',
    expires: -1,
    httpOnly: false,
    secure: false,
    sameSite: 'Lax',
  };
}

await access(obscuraBin).catch(() => {
  throw new Error(`Build Obscura first: cargo build --release -p obscura-cli\nMissing: ${obscuraBin}`);
});

const { chromium } = await loadPlaywright();
const catalog = JSON.parse(await readFile(
  join(root, 'crates', 'obscura-browser', 'data', 'chrome-windows-v1.json'),
  'utf8',
));
const defaultBase = catalog.baseProfiles.find(
  row => row.id === catalog.defaultComposition.baseId,
);
const browserMajor = defaultBase.browserVersion.split('.')[0];
const profileId = [
  `c${browserMajor}w1`,
  catalog.defaultComposition.baseId,
  catalog.defaultComposition.graphicsId,
  catalog.defaultComposition.screenId,
].join(':');

const firstWeb = http.createServer((_request, response) => {
  response.writeHead(200, { 'content-type': 'text/html' });
  response.end('<!doctype html><title>storage test</title><body>ok</body>');
});
const secondWeb = http.createServer((_request, response) => {
  response.writeHead(200, { 'content-type': 'text/html' });
  response.end('<!doctype html><title>second origin</title><body>ok</body>');
});
const origin = await listen(firstWeb);
const otherOrigin = await listen(secondWeb);
const cdpPort = await freePort();
const obscura = spawn(
  obscuraBin,
  ['--allow-private-network', 'serve', '--port', String(cdpPort)],
  { cwd: root, stdio: 'ignore', windowsHide: true },
);

let browser;
try {
  await waitForServer(obscura, cdpPort);
  browser = await chromium.connectOverCDP(`http://127.0.0.1:${cdpPort}`);
  const rootSession = await browser.newBrowserCDPSession();
  assert.deepEqual(
    await rootSession.send('Obscura.setProfile', { profileId }),
    { profileId },
  );

  const context = await browser.newContext({
    storageState: {
      cookies: [cookie('loaded')],
      origins: [{
        origin,
        localStorage: [{ name: 'token', value: 'loaded-token' }],
      }],
    },
  });
  assert.equal((await context.cookies(origin)).find(item => item.name === 'sid')?.value, 'loaded');

  const page = await context.newPage();
  await page.goto(origin);
  assert.deepEqual(await page.evaluate(() => ({
    cookie: document.cookie,
    token: localStorage.getItem('token'),
  })), { cookie: 'sid=loaded', token: 'loaded-token' });

  await page.evaluate(() => {
    document.cookie = 'sid=worked; path=/';
    localStorage.setItem('token', 'worked-token');
    localStorage.setItem('second', 'two');
    sessionStorage.setItem('temporary', 'page-only');
  });
  const secondPage = await context.newPage();
  await secondPage.goto(`${origin}/second`);
  assert.deepEqual(await secondPage.evaluate(() => ({
    local: localStorage.getItem('token'),
    session: sessionStorage.getItem('temporary'),
  })), { local: 'worked-token', session: null });
  await secondPage.goto(otherOrigin);
  assert.equal(await secondPage.evaluate(() => localStorage.getItem('token')), null);

  const exported = await context.storageState({ path: statePath });
  assert.equal(exported.cookies.find(item => item.name === 'sid')?.value, 'worked');
  const exportedOrigin = exported.origins.find(item => item.origin === origin);
  assert.equal(
    exportedOrigin?.localStorage.find(item => item.name === 'token')?.value,
    'worked-token',
  );

  await context.clearCookies();
  assert.equal((await context.cookies()).length, 0);
  await context.addCookies([
    cookie('added'),
    { name: 'url-cookie', value: 'kept', url: origin },
  ]);
  assert.equal((await context.cookies()).find(item => item.name === 'sid')?.value, 'added');
  await context.clearCookies({ name: 'sid', domain: '127.0.0.1', path: '/' });
  const afterFilteredClear = await context.cookies([origin]);
  assert.equal(afterFilteredClear.some(item => item.name === 'sid'), false);
  assert.equal(afterFilteredClear.find(item => item.name === 'url-cookie')?.value, 'kept');

  await context.setStorageState({
    cookies: [cookie('reset')],
    origins: [{
      origin,
      localStorage: [{ name: 'token', value: 'reset-token' }],
    }],
  });
  await page.goto(`${origin}/reset`);
  assert.deepEqual(await page.evaluate(() => ({
    cookie: document.cookie,
    token: localStorage.getItem('token'),
    second: localStorage.getItem('second'),
  })), { cookie: 'sid=reset', token: 'reset-token', second: null });

  const restored = await browser.newContext({ storageState: statePath });
  const restoredPage = await restored.newPage();
  await restoredPage.goto(`${origin}/restored`);
  assert.deepEqual(await restoredPage.evaluate(() => ({
    cookie: document.cookie,
    token: localStorage.getItem('token'),
  })), { cookie: 'sid=worked', token: 'worked-token' });
  await restoredPage.evaluate(() => localStorage.setItem('token', 'other-context'));

  const contextCheckPage = await context.newPage();
  await contextCheckPage.goto(`${origin}/context-check`);
  assert.equal(
    await contextCheckPage.evaluate(() => localStorage.getItem('token')),
    'reset-token',
  );

  await context.setStorageState(statePath);
  const pathLoadPage = await context.newPage();
  await pathLoadPage.goto(`${origin}/path-load`);
  assert.deepEqual(await pathLoadPage.evaluate(() => ({
    cookie: document.cookie,
    token: localStorage.getItem('token'),
  })), { cookie: 'sid=worked', token: 'worked-token' });

  await restored.close();
  await context.close();
  await browser.close();
  browser = undefined;
  console.log(JSON.stringify({ ok: true, playwrightVersion, profileId }));
} finally {
  if (browser) await browser.close().catch(() => {});
  obscura.kill();
  await Promise.all([
    new Promise(done => firstWeb.close(done)),
    new Promise(done => secondWeb.close(done)),
  ]);
  await rm(statePath, { force: true });
}
