# ab — Chrome vs Obscura, same scenario, both over CDP

```
node tools/ab/ab.mjs <url> [--needle text] [--match regexp]
                           [--only chrome|obscura] [--wait seconds] [--headed]
```

Loads one URL in the real system Chrome and in Obscura, drives both through
Playwright, and prints the two runs side by side: time to a text needle, body
length, request count, matched responses, and console/page errors.

Obscura is built from `target/release/obscura.exe` and launched with
`--stealth` on a free port, then killed. `OBSCURA_PROXY` is honoured. Nothing is
written to disk.

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

## Prerequisites

- `cargo build --release -p obscura-cli --features stealth`
- Playwright at `target/test-fixtures/playwright`
- Chrome installed, reachable by Playwright as `channel: 'chrome'`

## Example

```
node tools/ab/ab.mjs https://example.com/product/1 \
  --needle "in stock" --match "api|\.json"
```

```
=== chrome   7431ms  needle=7s  bodyLength=1313  requests=208
=== obscura  9204ms  needle=1s  bodyLength=25348  requests=175
```

A body length that differs by an order of magnitude usually means one side got
a different document, not a different render. Check the navigation status
before reading anything into the rest.
