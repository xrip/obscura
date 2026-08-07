# ab — Chrome vs Obscura, same scenario, both over CDP

| script | what it does |
|---|---|
| `ab.mjs` | one URL in both engines, side by side |
| `journey.mjs` | home page → click a random product card → back → repeat |
| `clicklocal.mjs` | the click path against a local fixture, offline and in seconds |
| `chrome-raw.mjs` | real Chrome over raw CDP with the automation tells switched off |
| `engines.mjs` | launching and tearing down the two engines; the others import it |

```
node tools/ab/ab.mjs <url> [--needle text] [--match regexp]
                           [--only chrome|obscura] [--wait seconds] [--proxy url] [--headed]
node tools/ab/journey.mjs [--site wb|ozon|avito] [--cards n]
                           [--only chrome|obscura] [--proxy url] [--goto] [--headed]
node tools/ab/clicklocal.mjs
node tools/ab/chrome-raw.mjs [--cards n] [--headed] [--proxy url]
```

Obscura is built from `target/release/obscura.exe` and launched with `--stealth`
on a free port, then killed. Nothing is written to disk.

## Why both sides go through Playwright

Because a difference then means a difference in the engine, not in the way it
was measured — and because it puts Obscura's CDP surface under the same load a
real client applies.

That second part is not theoretical. Stealth mode once reported **9 of its own
173 requests** over CDP, because scripted fetch and XHR take a different
transport in that build and it recorded nothing. Reading `page.on('request')`
led straight to the wrong conclusion ("Obscura never loads the page's chunks")
until `RUST_LOG=obscura_js::ops=debug` showed the real count. Anything this
harness cannot see is worth checking against the debug log before believing.

## Things that have cost real time here

- **Read Playwright's step log before theorising.** `locator.click` failing
  prints *which* precondition failed. Three wrong hypotheses and six commits went
  by before anyone read `"element is not visible"`, which named the bug exactly.
- **Reproduce offline.** `clicklocal.mjs` runs the whole click path against a
  local fixture in about four seconds. Every diagnosis that mattered came from
  it, not from a live site — live sites throttle, and a throttled run looks
  exactly like a fingerprinting failure.
- **Check the exit IP.** `journey.mjs` prints it. `HTTPS_PROXY` in the shell sent
  runs through an exit they never asked for, and per-site "blocks" measured
  before that was noticed are not trustworthy.
- **A body length that differs by an order of magnitude** usually means one side
  got a different document, not a different render. Check the navigation status
  before reading anything into the rest.

## Prerequisites

- `cargo build --release -p obscura-cli --features stealth`
- Playwright at `target/test-fixtures/playwright`
- Chrome installed, reachable by Playwright as `channel: 'chrome'`
