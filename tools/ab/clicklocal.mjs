// Can Playwright click a link in Obscura at all, on a page we control?
// If this fails, the click bug is reproducible offline and WB is irrelevant.

import { spawn } from 'node:child_process';
import http from 'node:http';
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
    const s = net.createServer();
    s.once('error', fail);
    s.listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => done(p)); });
  });
}

const fixturePort = await freePort();
const hits = [];
const fixture = http.createServer((req, res) => {
  hits.push(req.url);
  res.writeHead(200, { 'content-type': 'text/html' });
  if (req.url.startsWith('/catalog/')) {
    res.end('<!doctype html><html><body><h1>card</h1><p>id 1193913221</p></body></html>');
  } else {
    let cards = '';
    for (let i = 0; i < 60; i++) {
      cards += `<article class="card"><a href="/catalog/${i}/detail.aspx" id="c${i}">` +
               `<img src="/i.webp"><span>Product ${i}</span></a></article>`;
    }
    res.end(`<!doctype html><html><head><style>.card{position:relative}</style></head>
             <body><div class="feed">${cards}
             <article class="card"><a href="/catalog/999/detail.aspx" id="spa">routed</a></article>
             </div>
             <script>
               document.getElementById('spa').addEventListener('click', function(e) {
                 e.preventDefault();
                 history.pushState({}, '', '/catalog/999/detail.aspx');
               });
             </script>
             </body></html>`);
  }
});
await new Promise(r => fixture.listen(fixturePort, '127.0.0.1', r));
const base = `http://127.0.0.1:${fixturePort}`;

const port = await freePort();
const child = spawn(obscuraBin, ['--stealth', 'serve', '--port', String(port)], {
  cwd: root,
  env: { ...process.env, OBSCURA_NAV_TIMEOUT_MS: '60000', OBSCURA_ALLOW_PRIVATE_NETWORK: '1' },
  stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true,
});
child.stdout.on('data', () => {});
child.stderr.on('data', () => {});

try {
  const deadline = Date.now() + 30000;
  for (;;) {
    try { const r = await fetch(`http://127.0.0.1:${port}/json/version`); if (r.ok) break; } catch {}
    if (Date.now() > deadline) throw new Error('obscura did not start');
    await new Promise(r => setTimeout(r, 200));
  }
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  const context = await browser.newContext();
  const page = await context.newPage();
  await page.goto(base, { waitUntil: 'load', timeout: 60000 });

  // What does the main world say, and what does the utility world say? If the
  // two disagree about where an element is, only one of them can be right and
  // Playwright believes the other one.
  const main = await page.evaluate(() => {
    const a = document.getElementById('c7');
    const r = a.getBoundingClientRect();
    const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    const view = a.ownerDocument && a.ownerDocument.defaultView;
    const cs = view && view.getComputedStyle && view.getComputedStyle(a);
    return { x: r.left, y: r.top, w: r.width, h: r.height,
             hit: hit ? hit.id || hit.tagName : 'null',
             hasOwnerDoc: !!a.ownerDocument, hasDefaultView: !!view,
             hasGCS: !!(view && view.getComputedStyle),
             cs: cs ? { visibility: cs.visibility, display: cs.display } : null };
  });
  console.log('main world  :', JSON.stringify(main));

  console.log('utility bbox:', JSON.stringify(await page.locator('#c7').boundingBox().catch(e => String(e).slice(0,120))));
  console.log('utility eval:', JSON.stringify(await page.locator('#c7').evaluate(el => {
    const r = el.getBoundingClientRect();
    return { w: r.width, h: r.height, cv: typeof el.checkVisibility === 'function' ? el.checkVisibility({checkOpacity:false,checkVisibilityCSS:false}) : 'none' };
  }).catch(e => String(e).slice(0,120))));
  try {
    await page.locator('#c7').click({ timeout: 10000 });
    console.log('after click, href seen by page:', await page.evaluate(() => location.href));
    for (let i=0;i<20;i++){ if(page.url().includes('/catalog/')) break; await new Promise(r=>setTimeout(r,300)); }
    console.log('click        : OK, now at', page.url());
    console.log('server saw   :', JSON.stringify(hits));
    await page.goBack().catch(() => {});
    await page.goto(base, { waitUntil: 'load', timeout: 60000 });
    await page.locator('#spa').click({ timeout: 10000 });
    for (let i = 0; i < 20; i++) {
      if (page.url().includes('/catalog/999/')) break;
      await new Promise(r => setTimeout(r, 300));
    }
    console.log('spa route    :', page.url().includes('/catalog/999/') ? 'OK ' + page.url() : 'FAILED, still at ' + page.url());
  } catch (e) {
    console.log('LOG:', JSON.stringify(e.log || e.message, null, 1).slice(0, 2000));
    console.log('click        : FAILED', String(e).split('\n')[0].slice(0, 140));
  }

  await context.close();
  await browser.close();
} finally {
  child.kill();
  fixture.close();
}
