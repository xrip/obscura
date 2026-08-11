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
                           [--trace-challenge] [--screenshot-dir path]
node tools/ab/journey.mjs [--site wb|ozon|avito] [--cards n]
                           [--only chrome|obscura] [--proxy url] [--goto] [--headed]
                           [--profile-dir path] [--trace-challenge] [--clean-host]
node tools/ab/clicklocal.mjs
node tools/ab/chrome-raw.mjs [--site wb|ozon] [--cards n] [--headed]
                             [--only chrome|obscura] [--proxy url] [--clean-host]
                             [--trace-network] [--dump-dir path]
                             [--chrome-bin path] [--profile profile-id]
                             [--trace-challenge]
                             [--replay captured-challenge.html]
                             [--trace-replay-helpers]
```

`chrome-raw.mjs --dump-dir` writes the loaded home document, document response,
script inventory, and a small summary to the named disposable directory. Keep
that directory outside the repository because live documents may contain
short-lived site data.

`--replay` serves a captured challenge only on loopback and replaces its result
endpoint with a local recorder. The recorder prints field lengths and SHA-256
hashes, never challenge or fingerprint values. This is useful for comparing the
same challenge bytecode in raw Chrome and Obscura without sending a replay to
the live site.

`--trace-replay-helpers` is valid only with `--replay`. It records the challenge
helper function names plus argument and result shapes and string lengths. It
does not record values, cookies, tokens, or the fingerprint body.

Obscura is built from `target/release/obscura.exe` and launched with `--stealth`
on a free port, then killed. Nothing is written to disk unless
`--screenshot-dir` is passed. With that option, `ab.mjs` saves the visible
viewport after the scenario completes, while the page is still open, as
`<path>/chrome.png` and `<path>/obscura.png`.

For WB and Ozon URLs, screenshot mode waits up to `--wait` seconds for the
real storefront: at least three visible product links on the home page, or a
non-challenge product document. If the page still says it is checking the
browser, no screenshot is written.

For a manual WB/Ozon render check, use a separate disposable directory for
each site:

```
node tools/ab/ab.mjs https://www.wildberries.ru/ --screenshot-dir C:\Temp\obscura-ab-wb
node tools/ab/ab.mjs https://www.ozon.ru/ --screenshot-dir C:\Temp\obscura-ab-ozon
```

Use `--only chrome` or `--only obscura` when only one image is needed. The
directory is created on demand and may contain sensitive live page data.

On Windows, use `--clean-host` when the terminal or agent process was itself
started with a proxy. Explorer then launches Chrome or Obscura outside that
process tree, using the same network path as ordinary interactive Chrome. It
cannot be combined with `--proxy`. Chrome uses a fresh temporary profile which
is removed after the run.

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
- **Check the exit IP.** `journey.mjs` prints it. If the parent process itself
  was started with `HTTPS_PROXY`, removing the variable from children may still
  leave them on that process tree's route. Use `--clean-host`; per-site blocks
  measured on the wrong exit are not trustworthy.
- **A body length that differs by an order of magnitude** usually means one side
  got a different document, not a different render. Check the navigation status
  before reading anything into the rest.

## Prerequisites

- `cargo build --release -p obscura-cli --features stealth`
- Playwright at `target/test-fixtures/playwright`
- Chrome installed, reachable by Playwright as `channel: 'chrome'`
