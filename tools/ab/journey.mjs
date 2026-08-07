// A storefront journey, run against real Chrome and against Obscura.
//
//   node tools/ab/journey.mjs [options]
//
//     --site <name>      wb (default), ozon or avito
//     --cards <n>        product cards to visit (default 3)
//     --only <engine>    "chrome" or "obscura"
//     --proxy <url>      send both engines through the same proxy
//     --wait <seconds>   how long to wait for a card to render (default 20)
//     --goto             navigate by URL instead of clicking the card
//     --headed           show the Chrome window
//
// Open the home page, pick N product links at random, and visit each one.
//
// Why a journey and not a deep link: arriving cold on a product URL is not what
// a person does, and repeating one deep link is the fastest way to get an IP
// rate limited — which then looks exactly like a fingerprinting failure. A card
// reached from the home page also has a referer, a warm connection and the
// cookies the site just set, so a failure here is much more likely to be ours.
//
// Cards are reached by clicking them, not by navigating to their URL. A click
// runs the site's own router and fires the pointer events a behavioural
// detector watches; a goto skips all of it and tests much less than it looks
// like it does. --goto keeps the weaker path for comparison.
//
// A card counts as opened only if its own id appears in the rendered text. That
// is the field that fails when the page hydrates but the card does not, and it
// cannot be satisfied by the site chrome alone.

import { runIn, tryEvaluate, evaluated } from './engines.mjs';
// What differs between storefronts: where the home page is, what a card link
// looks like, and where the product id hides in its URL.
const SITES = {
  wb: {
    home: 'https://www.wildberries.ru/',
    cardLink: 'a[href*="/catalog/"][href*="/detail.aspx"]',
    idFrom: url => (url.match(/\/catalog\/(\d+)\/detail/) || [])[1],
    // The href fragment that says the browser is on this card.
    onCard: id => `/catalog/${id}/`,
    cardLinkFor: id => `a[href*="/catalog/${id}/"]`,
  },
  ozon: {
    home: 'https://www.ozon.ru/',
    cardLink: 'a[href*="/product/"]',
    // Ozon slugs end in the id: /product/some-name-1902651403/
    idFrom: url => (url.match(/\/product\/[^/?#]*?-(\d+)(?:[/?#]|$)/) || [])[1],
    onCard: id => `-${id}`,
    cardLinkFor: id => `a[href*="-${id}"]`,
  },
  avito: {
    home: 'https://www.avito.ru/',
    // Item links are ordinary hrefs; what marks them is the trailing id, so the
    // selector is deliberately loose and idFrom does the filtering.
    cardLink: 'a[href*="_"]',
    idFrom: url => (url.match(/_(\d{6,})(?:[/?#]|$)/) || [])[1],
    onCard: id => `_${id}`,
    cardLinkFor: id => `a[href*="_${id}"]`,
  },
};

function parseArgs(argv) {
  const opts = { cards: 3, wait: 20, headed: false, site: 'wb' };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--headed') opts.headed = true;
    else if (arg === '--cards') opts.cards = Number(argv[++i]);
    else if (arg === '--only') opts.only = argv[++i];
    else if (arg === '--proxy') opts.proxy = argv[++i];
    else if (arg === '--wait') opts.wait = Number(argv[++i]);
    else if (arg === '--goto') opts.goto = true;
    else if (arg === '--site') opts.site = argv[++i];
  }
  return opts;
}
const opts = parseArgs(process.argv.slice(2));
const site = SITES[opts.site];
if (!site) {
  console.error(`unknown --site ${opts.site}; expected one of ${Object.keys(SITES).join(', ')}`);
  process.exit(2);
}
const HOME = site.home;

const productId = url => site.idFrom(url);

async function journey(page, log) {
  const steps = [];
  let requests = 0;
  page.on('request', () => { requests += 1; });

  // Which IP the site actually sees. A blocked run and a proxied run look the
  // same from here otherwise, and guessing which one happened wastes far more
  // time than asking.
  try {
    await page.goto('https://api.ipify.org/?format=json', { waitUntil: 'load', timeout: 30000 });
    const seen = evaluated(await tryEvaluate(page, () => document.body.innerText.trim().slice(0, 80)));
    log(`exit ip: ${seen}`);
  } catch (error) {
    log(`exit ip: unavailable (${String(error).split('\n')[0].slice(0, 60)})`);
  }
  requests = 0;

  await page.goto(HOME, { waitUntil: 'load', timeout: 90000 });
  // The home page fills its rails after hydration, so wait for links rather
  // than assuming the first paint has them.
  let links = [];
  for (let second = 1; second <= opts.wait; second++) {
    await new Promise(done => setTimeout(done, 1000));
    links = evaluated(await tryEvaluate(page, selector =>
      [...document.querySelectorAll(selector)].map(a => a.href), site.cardLink)) || [];
    if (links.length >= 3) break;
  }
  const unique = [...new Set(links.filter(productId))];
  steps.push({ step: 'home', links: unique.length, requests, ok: unique.length >= opts.cards });
  log(`home: ${unique.length} product links, ${requests} requests`);
  if (!unique.length) return steps;

  // One card at a time, returning home in between. The home feed reshuffles on
  // every visit, so ids picked up front are gone by the second card — which
  // reads as a click failure and is really a stale locator.
  const visited = new Set();
  for (let round = 0; round < opts.cards; round++) {
    if (round > 0) {
      await page.goto(HOME, { waitUntil: 'load', timeout: 90000 });
      await new Promise(done => setTimeout(done, 2000));
    }

    // Only cards the page currently shows, and only ones we have not used.
    const candidates = (evaluated(await tryEvaluate(page, selector =>
      [...document.querySelectorAll(selector)]
        .filter(a => {
          const r = a.getBoundingClientRect();
          return r.width > 0 && r.height > 0;
        })
        .map(a => a.href), site.cardLink)) || [])
      .filter(u => productId(u));
    const fresh = candidates.filter(u => !visited.has(productId(u)));
    if (!fresh.length) {
      // Distinguish "the page came back empty" from "we have already used
      // everything it offered". The first is the site refusing us and the
      // second is the journey running out of material; they need opposite
      // reactions and used to print the same line.
      const reason = candidates.length
        ? `all ${candidates.length} cards already visited`
        : 'the home page came back with no cards';
      steps.push({ step: 'card', ok: false, failure: reason });
      log(`card: skipped, ${reason}`);
      continue;
    }
    const url = fresh[Math.floor(Math.random() * fresh.length)];
    const id = productId(url);
    visited.add(id);

    // A person reads before clicking, and back to back navigations are what
    // gets an IP throttled.
    await new Promise(done => setTimeout(done, 1500 + Math.random() * 2000));
    let opened = null;
    let bodyLength = 0;
    let failure = null;
    let how = opts.goto ? 'goto' : 'click';
    try {
      if (opts.goto) {
        await page.goto(url, { waitUntil: 'load', timeout: 90000 });
      } else {
        const card = page.locator(site.cardLinkFor(id)).first();
        await card.scrollIntoViewIfNeeded({ timeout: 15000 });
        await new Promise(done => setTimeout(done, 400 + Math.random() * 600));
        await card.click({ timeout: 15000 });
        for (let tick = 0; tick < 40; tick++) {
          if (page.url().includes(site.onCard(id))) break;
          await new Promise(done => setTimeout(done, 500));
        }
        if (!page.url().includes(site.onCard(id))) {
          how = 'click(no-nav)';
          throw new Error(`click did not reach the card, still at ${page.url().slice(0, 60)}`);
        }
      }
      for (let second = 1; second <= opts.wait; second++) {
        await new Promise(done => setTimeout(done, 1000));
        const found = evaluated(await tryEvaluate(page, needle =>
          (document.body ? document.body.innerText : '').includes(needle), id));
        if (found) { opened = second; break; }
      }
      if (process.env.JOURNEY_LADDER) {
        for (const [name, fn] of [
          ['42', () => 42],
          ['"x"', () => 'x'],
          ['true', () => true],
          ['body?', () => !!document.body],
          ['innerText.length', () => document.body.innerText.length],
          ['String(len)', () => String(document.body.innerText.length)],
          ['replace().length', () => document.body.innerText.replace(/\s+/g, ' ').length],
        ]) {
          const r = await tryEvaluate(page, fn);
          log(`  ladder ${name.padEnd(18)} ${typeof evaluated(r)} ${JSON.stringify(evaluated(r))?.slice(0, 40)}`);
        }
      }
      const measured = await tryEvaluate(page, () =>
        (document.body ? document.body.innerText.replace(/\s+/g, ' ') : '').length);
      bodyLength = measured.gaveUp ? 'still navigating' : measured.value;
    } catch (error) {
      failure = String(error).split('\n')[0].slice(0, 140);
    }
    steps.push({ step: 'card', id, how, opened, bodyLength, failure, requests, ok: opened !== null });
    log(`card ${id} via ${how}: ${failure ? `FAILED ${failure}` : opened !== null
      ? `opened after ${opened}s (${bodyLength} chars)` : `NEVER rendered (${bodyLength} chars)`}` +
      `  [${requests} requests total]`);
  }
  return steps;
}

let failed = 0;
for (const engine of opts.only ? [opts.only] : ['chrome', 'obscura']) {
  console.log(`\n=== ${engine}`);
  try {
    const steps = await runIn(engine, opts, page =>
      journey(page, line => console.log('   ' + line)));
    const cards = steps.filter(s => s.step === 'card');
    const opened = cards.filter(s => s.ok).length;
    console.log(`   ${opened}/${cards.length} cards opened`);
    if (opened < cards.length || !cards.length) failed += 1;
  } catch (error) {
    console.log('   THREW ' + String(error).split('\n')[0].slice(0, 300));
    failed += 1;
  }
}
process.exit(failed ? 1 : 0);
