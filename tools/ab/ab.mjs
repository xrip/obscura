// Run one scenario against real Chrome and against Obscura, both over CDP, and
// print the two side by side.
//
//   node tools/ab/ab.mjs <url> [options]
//
//     --needle <text>    text to wait for in document.innerText, and time
//     --match <regexp>   list responses whose URL matches this
//     --only <engine>    "chrome" or "obscura"
//     --wait <seconds>   how long to wait for the needle/storefront (default 20)
//     --proxy <url>      send both engines through the same proxy
//     --headed           show the Chrome window
//     --trace-challenge  log safe WB VM and create-token shapes
//     --screenshot-dir <path>  write one viewport PNG per engine
//
// Obscura is launched with --stealth on a free port and killed afterwards.
// OBSCURA_PROXY is used when --proxy is absent. Nothing is written to disk.
//
// Both sides go through Playwright so the scenario cannot drift between them,
// and so that anything Obscura fails to report over CDP shows up as a
// difference rather than as a silent gap. That matters: stealth mode once
// reported 9 of its own 173 requests, which made every comparison misleading
// until it was found.

import { runIn, tryEvaluate, evaluated } from './engines.mjs';
import { installChallengeTrace, readChallengeTrace } from './challenge-trace.mjs';
import { mkdirSync } from 'node:fs';
import { join, resolve } from 'node:path';

const STOREFRONTS = {
  wb: {
    host: 'wildberries.ru',
    homeSelector: 'a[href*="/catalog/"][href*="/detail.aspx"]',
  },
  ozon: {
    host: 'ozon.ru',
    homeSelector: 'a[href*="/product/"]',
  },
};

function storefrontFor(url) {
  try {
    const host = new URL(url).hostname.toLowerCase();
    return Object.values(STOREFRONTS).find(site =>
      host === site.host || host.endsWith(`.${site.host}`));
  } catch {
    return null;
  }
}

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
    else if (arg === '--trace-challenge') opts.traceChallenge = true;
    else if (arg === '--screenshot-dir') opts.screenshotDir = resolve(argv[++i]);
    else rest.push(arg);
  }
  opts.url = rest[0];
  return opts;
}

const opts = parseArgs(process.argv.slice(2));
if (!opts.url) {
  console.error('usage: node tools/ab/ab.mjs <url> [--needle text] [--match regexp]' +
                ' [--only chrome|obscura] [--wait seconds] [--proxy url] [--headed]' +
                ' [--trace-challenge] [--screenshot-dir path]');
  process.exit(2);
}

async function scenario(page) {
  const traffic = [];
  const errors = [];
  let requests = 0;

  if (opts.traceChallenge) await installChallengeTrace(page);

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
      const found = evaluated(await tryEvaluate(page, needle =>
        (document.body ? document.body.innerText : '').includes(needle), opts.needle));
      if (found) { needleAfter = second; break; }
    }
  }

  let challenge = {};
  if (opts.traceChallenge) {
    for (let second = 1; second <= opts.wait; second++) {
      await new Promise(done => setTimeout(done, 1000));
      challenge = await readChallengeTrace(page, tryEvaluate, evaluated);
      if (challenge.vmfp?.length && challenge.token?.length >= 2) break;
    }
  }

  const bodyLength = evaluated(await tryEvaluate(page, () =>
    (document.body ? document.body.innerText.replace(/\s+/g, ' ') : '').length));

  let challengeState = null;
  if (opts.traceChallenge) {
    challengeState = evaluated(await tryEvaluate(page, () => {
      const token = (`; ${document.cookie}`).split('; x_wbaas_token=')[1]?.split(';')[0] || '';
      return {
        url: location.href.split('?')[0],
        title: document.title,
        h1: document.querySelector('h1')?.innerText || '',
        text: (document.body?.innerText || '').replace(/\s+/g, ' ').slice(0, 220),
        readyState: document.readyState,
        tokenCookieLength: token.length,
        reloadType: typeof document.location.reload,
        thresholdPresent: localStorage.getItem('x_wbaas_token_treshold') !== null,
      };
    }));
  }

  return {
    needleAfter, bodyLength, requests, traffic, errors, challenge, challengeState,
    elapsed: Date.now() - started,
  };
}

async function captureScreenshot(page, result, engine) {
  if (!opts.screenshotDir) return result;
  const storefront = storefrontFor(opts.url);
  let screenshotReadiness = null;
  if (storefront) {
    const started = Date.now();
    let state = null;
    const maxWait = Math.max(0, Number.isFinite(opts.wait) ? opts.wait : 20);
    for (;;) {
      state = evaluated(await tryEvaluate(page, ({ homeSelector }) => {
        const text = (document.body?.innerText || '').replace(/\s+/g, ' ').trim();
        const lower = text.toLowerCase();
        const blocked = [
          'проверяем браузер', 'checking your browser', 'checking browser',
          'just a moment', 'verify you are human', 'доступ ограничен',
          'access denied',
        ].some(marker => lower.includes(marker));
        const path = location.pathname.toLowerCase();
        const isProduct = /\/catalog\/\d+(?:\/|$)/.test(path) ||
          /\/product\/[^/?#]*-\d+(?:\/|$)/.test(path);
        const homeLinks = [...document.querySelectorAll(homeSelector)].filter(link => {
          const rect = link.getBoundingClientRect();
          return rect.width > 0 && rect.height > 0;
        }).length;
        const h1 = document.querySelector('h1')?.innerText?.trim() || '';
        const ready = !blocked && (isProduct
          ? text.length >= 300 && (h1.length > 0 || text.length >= 600)
          : homeLinks >= 3);
        return {
          ready,
          mode: isProduct ? 'product' : 'home',
          blocked,
          homeLinks,
          bodyLength: text.length,
          title: document.title,
        };
      }, storefront));
      if (state?.ready || Date.now() >= started + maxWait * 1000) break;
      await new Promise(done => setTimeout(done, 1000));
    }
    screenshotReadiness = {
      ready: !!state?.ready,
      mode: state?.mode || 'unknown',
      waited: Math.round((Date.now() - started) / 1000),
      state,
    };
    if (!screenshotReadiness.ready) {
      return { ...result, screenshotReadiness, screenshotSkipped: true };
    }
  }
  mkdirSync(opts.screenshotDir, { recursive: true });
  const screenshotPath = join(opts.screenshotDir, `${engine}.png`);
  await page.screenshot({ path: screenshotPath });
  return { ...result, screenshotPath, screenshotReadiness };
}

const engines = opts.only ? [opts.only] : ['chrome', 'obscura'];
for (const engine of engines) {
  try {
    const out = await runIn(engine, opts, scenario,
      (page, result) => captureScreenshot(page, result, engine));
    const needle = opts.needle ? `  needle=${out.needleAfter ?? 'NEVER'}s` : '';
    console.log(`\n=== ${engine}  ${out.elapsed}ms${needle}` +
                `  bodyLength=${out.bodyLength}  requests=${out.requests}`);
    if (opts.match) {
      console.log(`   matched responses: ${out.traffic.length}`);
      for (const line of out.traffic.slice(0, 30)) console.log('     ' + line);
      if (out.traffic.length > 30) console.log(`     ... and ${out.traffic.length - 30} more`);
    }
    if (opts.traceChallenge) {
      console.log(`   challenge trace: ${JSON.stringify(out.challenge)}`);
      console.log(`   challenge state: ${JSON.stringify(out.challengeState)}`);
    }
    if (out.screenshotReadiness) {
      console.log(`   storefront: ${out.screenshotReadiness.ready ? 'ready' : 'NOT READY'}` +
                  ` (${out.screenshotReadiness.mode}, waited ${out.screenshotReadiness.waited}s)`);
      if (!out.screenshotReadiness.ready) {
        console.log(`   final state: ${JSON.stringify(out.screenshotReadiness.state)}`);
      }
    }
    if (out.screenshotPath) console.log(`   screenshot: ${out.screenshotPath}`);
    if (out.screenshotSkipped) console.log('   screenshot skipped: storefront was not ready');
    const unique = [...new Set(out.errors)];
    for (const line of unique.slice(0, 12)) console.log('   ' + line);
    if (unique.length > 12) console.log(`   ... and ${unique.length - 12} more errors`);
    if (!unique.length) console.log('   (no errors)');
  } catch (error) {
    console.log(`\n=== ${engine} THREW ${String(error).slice(0, 400)}`);
  }
}
