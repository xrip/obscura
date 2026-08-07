// How far a Turnstile widget gets, in real Chrome and in Obscura.
//
//   node tools/ab/turnstile.mjs [url|local] [--only chrome|obscura]
//                              [--wait seconds] [--proxy url] [--headed]
//                              [--outbound] [--verbose <RUST_LOG>]
//
// "local" serves a fixture using Turnstile's dummy sitekey, which always issues
// a token without interaction. That separates the two questions a live site
// answers at once: whether the engine can mechanically complete the flow, and
// whether Cloudflare's risk engine trusts this browser and IP. A live failure
// cannot tell them apart, and guessing which one happened is how the previous
// round of this work lost a day.
//
// Body text says nothing here: the widget is an iframe and renders no text, so
// a page that fully solved and a page that never started look identical from
// document.innerText. What separates them is the token, which Turnstile writes
// into <input name="cf-turnstile-response">. That input being non-empty is the
// only real definition of passing, and it is what this reports.
//
// The stages in between are reported too, because they say *where* it stopped:
// the api.js script loading, window.turnstile appearing, the challenge iframe
// being created, and that iframe having a document with something in it.

import http from 'node:http';

import { runIn, tryEvaluate, evaluated } from './engines.mjs';

// Turnstile's "always passes" test sitekey. Documented as a dummy: it still
// loads the real challenge iframe from Cloudflare, so the whole pipeline runs,
// but it issues a token instead of scoring the visitor.
const DUMMY_SITEKEY = '1x00000000000000000000AA';

const FIXTURE = `<!doctype html>
<html><head><meta charset="utf-8"><title>turnstile fixture</title>
<script src="https://challenges.cloudflare.com/turnstile/v0/api.js" async defer></script>
</head><body>
<form><div class="cf-turnstile" data-sitekey="${DUMMY_SITEKEY}"></div></form>
</body></html>`;

/// Serves the fixture on a free port. Returns its URL and a stop function.
function serveFixture() {
  return new Promise(done => {
    const server = http.createServer((_request, response) => {
      response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
      response.end(FIXTURE);
    });
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      done({ url: `http://127.0.0.1:${port}/`, stop: () => server.close() });
    });
  });
}

function parseArgs(argv) {
  const opts = { wait: 30, headed: false };
  const rest = [];
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--headed') opts.headed = true;
    // Obscura's own log is the only view into what a frame realm did; nothing
    // about a frame that failed to run reaches CDP.
    else if (arg === '--verbose') {
      opts.env = { RUST_LOG: argv[++i] || 'obscura_browser=debug,obscura_js=debug' };
      opts.onStderr = chunk => process.stderr.write(chunk);
    }
    else if (arg === '--outbound') opts.outbound = true;
    else if (arg === '--only') opts.only = argv[++i];
    else if (arg === '--wait') opts.wait = Number(argv[++i]);
    else if (arg === '--proxy') opts.proxy = argv[++i];
    else rest.push(arg);
  }
  opts.url = rest[0] || 'https://turnstile-test.vercel.app/';
  return opts;
}
const opts = parseArgs(process.argv.slice(2));

// Runs in the page. Everything is read defensively: a missing shim must read as
// "absent", not throw and lose the whole report.
function probe() {
  const out = { iframes: [] };
  out.hasApiScript = [...document.querySelectorAll('script[src]')]
    .some(s => s.src.includes('turnstile') && s.src.includes('api.js'));
  out.hasTurnstileGlobal = typeof globalThis.turnstile;
  out.widgets = document.querySelectorAll('.cf-turnstile,[data-sitekey]').length;

  const input = document.querySelector('input[name="cf-turnstile-response"]');
  out.tokenInput = input ? 'present' : 'absent';
  out.token = input && input.value ? input.value : '';

  // Turnstile puts its iframe inside a *closed* shadow root, which
  // querySelectorAll correctly cannot pierce — so searching the light DOM alone
  // reports zero iframes whether the widget worked or never started. The init
  // script recorded every root as it was attached; look there too.
  const roots = globalThis.__abShadowRoots || [];
  out.shadowRoots = roots.length;
  out.messages = (globalThis.__abMessages || []).slice(0, 60);
  const frames = [...document.querySelectorAll('iframe')];
  for (const root of roots) {
    try { frames.push(...root.querySelectorAll('iframe')); } catch { /* gone */ }
  }

  for (const frame of frames) {
    const entry = { src: (frame.getAttribute('src') || '').slice(0, 120) };
    entry.connected = frame.isConnected;
    // Cross-origin in a real browser, so this throws there and must not abort
    // the rest of the report.
    try {
      const doc = frame.contentDocument;
      entry.doc = doc ? 'reachable' : 'null';
      if (doc && doc.body) entry.docBodyChars = doc.body.innerHTML.length;
    } catch { entry.doc = 'cross-origin (correct)'; }
    try { entry.win = frame.contentWindow ? typeof frame.contentWindow.postMessage : 'null'; }
    catch { entry.win = 'cross-origin (correct)'; }
    // Obscura-only bookkeeping: whether the frame document was fetched, and
    // whether it was given a realm. Absent in Chrome, which is the point — it
    // says where an Obscura-side frame stopped.
    if (frame._iframeLoadInfo) entry.loadInfo = JSON.stringify({ ...frame._iframeLoadInfo, url: undefined });
    if (frame._frameId !== undefined) entry.frameId = frame._frameId;
    out.iframes.push(entry);
  }
  return out;
}

async function scenario(page) {
  // Must be installed before any page script runs, so it catches the root
  // Turnstile attaches. A closed root cannot be reached any other way.
  await page.addInitScript(captureOutbound => {
    globalThis.__abShadowRoots = [];
    const attach = Element.prototype.attachShadow;
    Element.prototype.attachShadow = function (init) {
      const root = attach.call(this, init);
      globalThis.__abShadowRoots.push(root);
      return root;
    };

    // The widget's whole conversation with its frame, both directions. Which
    // message an engine fails to send or answer is the actual difference
    // between passing and not; the DOM afterwards only shows that it did not.
    globalThis.__abMessages = [];
    const label = data => {
      try {
        if (typeof data === 'string') return `"${data.slice(0, 60)}"`;
        const { event, source, widgetId, ...rest } = data || {};
        const extra = Object.keys(rest).slice(0, 4).join(',');
        return `${event || '?'}${extra ? ` {${extra}}` : ''}`;
      } catch { return '<unreadable>'; }
    };
    addEventListener('message', e => {
      globalThis.__abMessages.push(`in  ${label(e.data)}  src=${e.source ? 'yes' : 'NONE'}`);
    });

    // Outbound needs the iframe's window intercepted. A cross-origin
    // contentWindow cannot be read from, but postMessage on it can be wrapped.
    //
    // Off by default, and deliberately: wrapping contentWindow in a Proxy stops
    // real Chrome from ever issuing a token, so a run with this on cannot be
    // read as a pass or a fail — only as a record of the sequence.
    const descriptor = captureOutbound && Object.getOwnPropertyDescriptor(
      HTMLIFrameElement.prototype, 'contentWindow');
    if (descriptor && descriptor.get) {
      Object.defineProperty(HTMLIFrameElement.prototype, 'contentWindow', {
        configurable: true,
        get() {
          const win = descriptor.get.call(this);
          if (!win) return win;
          return new Proxy(win, {
            get(target, prop) {
              if (prop === 'postMessage') {
                return function (data, ...rest) {
                  globalThis.__abMessages.push(`out ${label(data)}`);
                  return target.postMessage(data, ...rest);
                };
              }
              const value = target[prop];
              return typeof value === 'function' ? value.bind(target) : value;
            },
          });
        },
      });
    }
  }, opts.outbound || false);

  const errors = [];
  const challengeRequests = [];
  page.on('pageerror', e => errors.push('pageerror: ' + String(e).slice(0, 160)));
  page.on('console', m => {
    if (m.type() === 'error') errors.push('console: ' + m.text().slice(0, 160));
  });
  page.on('response', r => {
    const url = r.url();
    if (url.includes('challenges.cloudflare.com') || url.includes('challenge-platform')) {
      challengeRequests.push(`${r.status()} ${url.slice(0, 110)}`);
    }
  });

  await page.goto(opts.url, { waitUntil: 'load', timeout: 90000 });

  // Poll rather than wait once: the token can arrive many seconds after load,
  // and stopping early would report a failure that had not happened yet.
  let report = null;
  let tokenAfter = null;
  for (let second = 1; second <= opts.wait; second++) {
    await new Promise(done => setTimeout(done, 1000));
    report = evaluated(await tryEvaluate(page, probe)) || report;
    if (report && report.token) { tokenAfter = second; break; }
  }
  return { report, tokenAfter, errors, challengeRequests };
}

const fixture = opts.url === 'local' ? await serveFixture() : null;
if (fixture) {
  opts.url = fixture.url;
  console.log(`fixture on ${fixture.url} with the dummy sitekey`);
}

for (const engine of opts.only ? [opts.only] : ['chrome', 'obscura']) {
  console.log(`\n=== ${engine}`);
  try {
    const out = await runIn(engine, opts, scenario);
    const r = out.report || {};
    console.log(`   api.js script tag : ${r.hasApiScript}`);
    console.log(`   window.turnstile  : ${r.hasTurnstileGlobal}`);
    console.log(`   widget elements   : ${r.widgets}`);
    console.log(`   token input       : ${r.tokenInput}`);
    console.log(`   TOKEN             : ${r.token
      ? `${r.token.slice(0, 24)}... (after ${out.tokenAfter}s)  PASS` : 'empty  FAIL'}`);
    console.log(`   shadow roots      : ${r.shadowRoots}`);
    console.log(`   iframes           : ${(r.iframes || []).length}`);
    for (const f of r.iframes || []) {
      console.log(`     - src=${f.src || '(none)'}`);
      console.log(`       connected=${f.connected} doc=${f.doc}` +
                  `${f.docBodyChars !== undefined ? ` bodyChars=${f.docBodyChars}` : ''}` +
                  ` contentWindow.postMessage=${f.win}`);
      if (f.frameId !== undefined) console.log(`       frameId=${f.frameId}`);
      if (f.loadInfo) console.log(`       loadInfo=${f.loadInfo}`);
    }
    console.log(`   message exchange  : ${(r.messages || []).length}`);
    for (const line of r.messages || []) console.log('     ' + line);
    const seen = [...new Set(out.challengeRequests)];
    console.log(`   challenge requests: ${seen.length}`);
    for (const line of seen.slice(0, 10)) console.log('     ' + line);
    const unique = [...new Set(out.errors)];
    for (const line of unique.slice(0, 8)) console.log('   ' + line);
  } catch (error) {
    console.log('   THREW ' + String(error).split('\n')[0].slice(0, 300));
  }
}
fixture?.stop();
