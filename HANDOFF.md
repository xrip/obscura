# Handoff

## Rebuild in progress (branch `webgl+webgpu-v2`)

The old branch `webgl+webgpu` is frozen at `e1562bf` (tag `pre-rebuild`). The
abandoned merge of upstream's render drop is kept, with conflict markers, on
branch `merge-attempt-snapshot`. The fork is being rebuilt stage by stage on top
of upstream. Everything below this section still describes the old branch and is
rewritten at the end of the rebuild.

Ports come from tag `port-src` = `ce18b78`.

### Upstream baseline, measured before any fork code landed

At `97124ed`, on Windows 11:

```
cargo build --release -p obscura-cli --bins --features stealth      # exit 0
cargo nextest run --release --workspace --features stealth --no-fail-fast
```

**1075 tests: 1045 passed, 30 failed, 4 skipped.** All 30 failures are
upstream's, on a clean checkout, before we added anything. Any failure outside
this list is ours.

| count | failure | cause |
|---|---|---|
| 17 | `obscura-render dom::tests::*` | layout and text shaping differ on Windows. The vendored `taffy` and `cosmic-text` patches are active (`cargo tree` resolves taffy to `vendor/taffy`), so this is not a misconfigured checkout. |
| 11 | `obscura-render::layout_test *` | same cause. Example: `legacy_center_keeps_block_flow_and_centers_descendants` asserts a centered table box and gets `Rect { x: 0.0, y: 80.0, width: 400.0, height: 40.0 }`. |
| 1 | `obscura-net wreq_client::tests::stealth_client_decodes_gzip_response` | the test's own fixture is on `127.0.0.1` and the SSRF gate blocks it. `OBSCURA_ALLOW_PRIVATE_NETWORK=1` fixes this one but then breaks two `ssrf_tests`, so the gate stays on and the test needs a per-test allowance. This is what `e1562bf` fixed on the old branch; re-derive it in stage 6. |
| 1 | `obscura-cdp::max_connections_cap max_connections_refuses_then_recovers` | pre-existing, also failed on the old branch. |

`live_product_smoke` is not in this list because it is a fork test and has not
landed yet. It was already failing before this work, verified at `f508c5b`.

### `obscura-cli::mcp_client` is flaky under load

The whole binary is timing-sensitive, not one test in it. Each test spawns the
`obscura` binary, drives it over MCP, and asserts on a page it just navigated
to. Under a saturated box the navigation has not finished when the next command
runs. Two different tests have flaked so far:

- `test_wait_for_selector` polls for an `h1` on a hardcoded 5 second budget.
  Alone it takes 0.04s; at full parallelism it lands near position 1103 of 1109,
  beside the 5-6s `obscura-js runtime::tests`, and expires at exactly 5.132s.
- `test_evaluate` reads `document.title` from a local `TestPageServer` page and
  gets `""` instead of `"Example Domain"`.

It is contention, not a regression: run alone the binary has been 16/16 on eight
consecutive runs. It flakes at `--test-threads 8` too, just less often. Compare
failure *sets* against the baseline rather than counts, and re-run the binary
alone before treating anything in it as a signal.

### Stage 2 result (profiles, workbench, GeoIP, transport identity)

`cargo nextest run --release --workspace --features stealth --no-fail-fast
--test-threads 8`: **1105 tests run, 30 failed.** Same count as the baseline,
and the set differs by exactly two entries, both in our favour:

- `stealth_client_decodes_gzip_response` is now **fixed**. The stealth path
  hardcoded `validate_url(url, false)`, so `--allow-private-network` never
  reached it and its own `127.0.0.1` fixture was unreachable. It now passes the
  client's flag, and `obscura-net` is 83/83.
- `mcp_client test_wait_for_selector` appears, and is the known flake above.

Identity verified on the wire against a local echo server: the plain and the
stealth path both send the profile's Windows Chrome 145 `user-agent`,
`sec-ch-ua` and `sec-ch-ua-platform`. No Linux Chrome string on either path.

Roughly 400 lines of fork code were **deleted rather than ported**, because
upstream now does the same work under different names (`ResourceRequest`,
`fetch_with_profile`, `request_referrer`, `request_fetch_site`, gzip decoding).
That also means our old call sites in `page.rs`, `module_loader.rs`, `ops.rs`
and `runtime.rs` need no porting at all, which shrinks stages 4 and 5.

Stage 2 is closed. Final run: **1106 tests, 1076 passed, 30 failed, 4 skipped**,
the same count as the baseline. The set differs by two: the gzip test is fixed,
and our own `live_product_smoke` now runs and fails (see below).

#### `live_product_smoke` needs stage 4, not stage 2

All three sites answer with an anti-bot challenge: Wildberries a browser check,
Ozon a JavaScript proof-of-work, Avito a SHA-256 proof-of-work with
`blocked: true`. None of them is a profile problem, and the identity on the wire
was verified correct.

The cause is the settle, now measured offline rather than inferred. A local
fixture whose only content is a 10ms `setTimeout` chain, so 200 ticks is about
two seconds of page time:

| budget | ticks | page time |
|---|---|---|
| `settle(500)` | 44 | ~440ms |
| `settle(2000)` | 57 | ~570ms |
| CLI, `OBSCURA_DYNAMIC_SCRIPT_SETTLE_MS` 1000 / 5000 / 15000 | 57 / 57 / 58 | ~650ms |

Page time plateaus around 600ms however much is asked for. `settle` returns once
V8 looks momentarily idle, and a pending timer chain does not count as busy, so
a proof-of-work needing two seconds cannot finish. The sliced settle loop in
`ce18b78` is the fix, and it lands in stage 4.

`crates/obscura-browser/tests/fork_settle_budget.rs` pins this as an offline
gate. It is `#[ignore]`d, so it stays out of the failure set; drop the attribute
when stage 4 lands.

**But the settle is not the whole story.** Upstream already ships the escape
hatch: `OBSCURA_STRICT_SETTLE=1` swaps the quiescence heuristic for the full
budget, and it works - the same fixture goes from 57 ticks in 647ms to 434 ticks
in 5001ms. Yet with strict settle on, Ozon and Wildberries still return their
challenge pages. The fork's sliced settle loop may therefore be unnecessary:
test against `OBSCURA_STRICT_SETTLE` before porting it.

#### What actually blocked Ozon: `performance` was an object literal

With strict settle on, the Ozon challenge threw:

```
TypeError: performance[b[127]].toJSON is not a function
```

Upstream builds `performance` as a plain object with every method as an own
property, so `constructor.name` was `Object`, `Object.prototype.toString` gave
`[object Object]`, and no `toJSON` existed on `performance`, `.timing` or
`.navigation`. Chrome has all of them. `performance.navigation` was missing
outright.

`js/fork_performance.js` reshapes the object in place at the new
`/* __OBSCURA_FORK_LATE_PAGE_INIT__ */` marker, after upstream assigns
`timeOrigin`/`timing`/`memory` per page. In place, never replaced: bootstrap
hands the same reference to other realms and reassigns those fields on every
navigation. Covered by
`crates/obscura-browser/tests/fork_performance_interface.rs`.

Result: the exception is gone, the console is clean, and Ozon's challenge now
runs to completion and clears its own challenge element. It still does not pass
- the page ends on Ozon's "Похоже, нет соединения" with no clearance cookie -
so there is at least one more gap behind this one. Wildberries is unchanged.

#### A/B against real Chrome: the discriminator is headless, not fingerprint

`tools/ab` needs `playwright-core` at `target/test-fixtures/playwright`; there
is no setup script, so install it there by hand (`npm install
playwright-core@1.62.1`). The system Chrome is used via `channel: 'chrome'`, so
no browser download is needed.

Offline first, `clicklocal.mjs`: click geometry agrees between the main and
utility worlds, the click navigates, and the server sees the request. One real
failure remains, `spa route: FAILED` - a page that routes itself through
history is not recorded as navigated. That is fork commit `d7dca7a`, still
unported.

The important run is Wildberries with `chrome-raw.mjs`, which drives the real
Chrome over raw CDP using `Page.navigate` and `Runtime.evaluate` only, never
`Runtime.enable`, with `--disable-blink-features=AutomationControlled`:

| engine | Wildberries home | cards |
|---|---|---|
| Chrome `--headless=new`, tells off | 0 product links | 0/0 |
| **Chrome headful, same flags** | **28 product links** | **2/2 opened** |
| Obscura `--stealth` | 0 product links, 40 requests | 0/0 |

**That conclusion was wrong, and the correction matters more than the result.**
Reading the actual body each engine receives, rather than counting links:

| engine | what Wildberries returns |
|---|---|
| Chrome headful | the real shop page, 19 scripts |
| Chrome headless | *"Подозрительная активность... Новая попытка через 00:55"* |
| Obscura | the `no-js-title` browser-check page |

The headless run is **rate limited**, not fingerprint-blocked: that page is a
timed retry tied to the exit IP, produced after a session of repeated requests
from this address. So headless and headful were not a controlled comparison at
all, and "the discriminator is headless" does not follow from it. Obscura gets a
third, different page again, so it is not the same failure either.

This is exactly what `tools/ab/README.md` warns about: a throttled run looks
exactly like a fingerprinting failure. Before drawing any conclusion from these
three sites, let the IP cool down or use another exit, and always read the
returned body rather than a link count.

#### What Wildberries actually runs, and why this is not a quick fix

Obscura loads the challenge page and every one of its scripts, with **no
console errors**, and is issued the `x_wbaas_token` cookie:

```
__wbaas/challenges/antibot/__static/v2/index-Bob5L-dt.js
__wbaas/challenges/antibot/statics/challenge-solver_v1.0.8.js
__wbaas/challenges/antibot/statics/behavior-tracker_v1.0.3.js
__wbaas/challenges/antibot/statics/challenge_vm_fp_v1.8.0_68686d45.js
__wbaas/challenges/antibot/__static/v2/browser-check.js
```

So this is not the Ozon situation, where a missing `toJSON` threw and stopped
the challenge dead. The scripts run to completion and the verdict is still no.

Three named components, and they want different things:

- `challenge_vm_fp` is a VM-based fingerprint, a bytecode interpreter probing
  many surfaces at once. This is what the measured surface work targets, and
  where further probe-driven fixes pay off.
- `behavior-tracker` wants mouse movement, scrolling and timing. The engine
  emits none: there is no synthetic input at all outside an explicit CDP
  `Input.dispatchMouseEvent`. Headful Chrome passes partly because a real
  window produces incidental input; headless Chrome does not pass either.
- `challenge-solver` presumably gates on both.

**That "needs a behavioural layer" reading was also wrong.** Running
`live_product_smoke` against the *old fork build* settles it:

| | Wildberries | Ozon | Avito | wall |
|---|---|---|---|---|
| old fork (`pre-rebuild`) | **passes** | fails | fails | 58s |
| this branch | fails | fails | fails | 9s, 36s with strict settle |

Wildberries passed on the fork with no behavioural input, so it is reachable by
porting, not by new features. Note also that the fork's own suite never had this
test fully green: Ozon and Avito failed there too, which is what its HANDOFF
recorded. Parity with the fork means Wildberries green and the other two red.

Two candidates were tested and eliminated:

- **Settle budget.** `OBSCURA_STRICT_SETTLE=1` takes the run from 9s to 36s of
  real page time and Wildberries still fails, so the sliced settle loop alone is
  not the difference.
- **Self-routing.** `d7dca7a` is now ported (`fork_virtual_url.rs`) and
  `clicklocal.mjs` is green on both steps offline, but Wildberries is unchanged.

**Frame realms are not the answer either, and that is now measured.** The
Wildberries challenge page has **no iframes and no shadow roots**, so the whole
frame-realm port (`bc1cd60`, `e43e651`, `f11e748`, `ba3d9d8`, `99426aa`,
`ce18b78`) cannot be what makes this test pass. Port it for Turnstile, not for
this.

What is actually true of the challenge page here:

- All five challenge scripts load and **execute**. Their globals are present:
  `LOAD_START`, `ANTI_SDK_WB_START_TIME`, `ANTI_SDK_WB_1695184013`,
  `ANALY_S_WB_KEY`, and `__vmfp` with `{bundle, getExported, run}`.
- No console errors anywhere on the page.
- `IS_OUTDATED_BROWSER` is `false`. `browser-check.js` only tests
  `Chrome/(\d+) < 80`, so the UA passes it.
- The `x_wbaas_token` cookie is set.
- `document.body` stays at 773 bytes and `readyState` reaches `complete`.

Eliminated by measurement, each with the command that did it:

| candidate | test | result |
|---|---|---|
| settle budget | `OBSCURA_STRICT_SETTLE=1`, 9s -> 36s | unchanged |
| more time | `--wait 30` with strict settle | unchanged, still 773 bytes |
| self-routing | `d7dca7a` ported, `clicklocal.mjs` green | unchanged |
| frames / shadow DOM | counted on the live page | 0 iframes, 0 shadow hosts |
| feature/version gate | read `IS_OUTDATED_BROWSER` and its source | false, not a gate |

### The block is transport-level, not JavaScript

The controlled comparison, run from the same exit IP within the same minute,
using the journey the fork's own harness defines (home page, then click three
product cards):

```
node tools/ab/chrome-raw.mjs --cards 3 --headed     28 product links, 3/3 cards
node tools/ab/journey.mjs --site wb --cards 3 --only obscura      0 links
```

Real Chrome passes and Obscura does not, at the same moment from the same
address, so this is not throttling and not the IP.

**And Obscura receives the 773-byte challenge page as the initial HTML
response**, before a single line of page script runs. The server decides on the
request. That rules out the entire JavaScript surface as the cause: the DOM
work, the interface shapes, the codec answers, `__vmfp`, the settle loop and the
frame realms are all downstream of a decision that has already been made.

So the discriminator is in what goes on the wire: the TLS ClientHello, the
HTTP/2 settings and pseudo-header order, or the request headers themselves.
`crates/obscura-net/src/wreq_client.rs` and `transport_profile.rs` are where
that lives.

#### Measured header differences

Against a local echo server, real Chrome versus Obscura `--stealth`. Caveat
before acting on any of it: this was **HTTP/1.1 to a plain server**, while
Wildberries is HTTP/2 over TLS, and both header order and presence differ by
protocol. Re-measure over HTTP/2 before treating these as the cause.

| header | Chrome | Obscura |
|---|---|---|
| `accept-encoding` | `gzip, deflate, br, zstd` | `zstd,gzip,deflate,br` |
| `accept-language` | `ru-RU,ru;q=0.9,en-US;q=0.8,en;q=0.7` | `en-US,en;q=0.9` |
| `connection` | `keep-alive` | absent |
| `priority` | absent | present |
| `sec-ch-ua` GREASE | `"Not=A?Brand"` | `"Not:A-Brand"` |

**`accept-encoding` is the strongest lead.** Order and spacing are exactly what
transport fingerprinting reads, and ours is both reordered and unspaced. It
comes from the wreq emulation profile, not from our code, so check whether
wreq's Chrome145 emulation is what emits it before changing anything.

Do **not** "fix" the GREASE brand from this table. The local Chrome is version
151 and our profile claims 145; Chromium's GREASE punctuation is derived per
major version, so a difference here is expected and `chrome_client_hints`
already implements that algorithm. Verify against a real Chrome 145 before
touching it.

`priority` on HTTP/1.1 and the missing `connection` header are both
protocol-shape mismatches worth checking over HTTP/2.

**Next diagnostic.** Capture and compare the two requests rather than guessing:
the JA3/JA4 and HTTP/2 fingerprint of Chrome versus the wreq emulation profile
we select, and the exact header list and order each sends. `chrome_transport_profile`
maps the profile's Chrome major to the nearest wreq profile and warns when it is
not exact, so start by checking whether the selected profile's major has an
exact wreq transport at all.

The JavaScript surface work in this branch is still correct and still needed -
it is what the probe diff measures - but it cannot move this test.

Plain `ab.mjs` is a weaker control and should not be read as "we beat Chrome":
it launches Chrome through Playwright with the automation tells left on. Under
it, Chrome got HTTP 403 from Ozon (186 byte body) and HTTP 498 from Wildberries
(17 bytes) while Obscura received the full challenge documents. That says
Playwright-Chrome is more detectable than Obscura, nothing more.

The identity itself is not implicated. Verified directly with proxies cleared:
exit IP `46.160.251.166` on both the plain and stealth paths, and a
cross-surface probe in which `userAgent`, `appVersion`, `platform`, `vendor`,
`userAgentData` brands and platform, the WebGL ANGLE/D3D11 AMD renderer, screen
metrics and `Europe/Moscow` all agree, with `navigator.webdriver` false, five
plugins and `pdfViewerEnabled` true. Wildberries is reached over the IPv6 exit
(`2001:470:...`), which is this machine's normal route.

### Stage 3 result (WebGL and WebGPU identity)

**1109 tests, 1079 passed, 30 failed.** Same count as the baseline, same two
differences as stage 2: the gzip test fixed, `live_product_smoke` red pending
stage 4. Three fork tests added, no new failures.

The whole graphics layer costs **three lines in `bootstrap.js`**, all comment
markers. `crates/obscura-js/build.rs` splices the fork modules in at build time:

| file | role |
|---|---|
| `js/graphics.js` | the facade: canvas, WebGL, WebGL2, WebGPU |
| `js/graphics_api_v145.js` | generated Chrome 145 IDL constants and arities |
| `js/graphics_shim.js` | the few helpers upstream's bootstrap does not have |
| `js/graphics_page_init.js` | picks up the profile, per page |
| `src/graphics.rs` | the Rust side of the profile handoff |

Verified against a live page: WebGL reports the profile's
`ANGLE (AMD, AMD Radeon(TM) Graphics (0x0000164C) Direct3D11 vs_5_0 ps_5_0, D3D11)`
with 35 extensions, WebGL2 reports 32 extensions and `MAX_TEXTURE_SIZE` 16384,
and `navigator.gpu.requestAdapter()` yields a `GPUAdapter` with 9 features,
`maxBufferSize` 2147483648 and `info.vendor` `amd`, `architecture` `rdna-2` —
consistent with the WebGL renderer, which is the point.

#### The facade only exists when a profile does

Upstream returns `null` from `getContext('webgl')` on purpose: a shim that
reports success while every draw is a no-op makes applications take the WebGL
path and render nothing. Their test
`unavailable_webgl_context_does_not_claim_success` guards it.

The fork reverses that, which broke the test. The fix was not to edit the test.
Every value the facade reports is read from the fingerprint profile, so with no
profile loaded it would be a context backed by nothing, which is exactly what
upstream objects to. `_canvasGetContext` now returns `null` unless a profile is
present, and `navigator.gpu` is likewise absent. Upstream's test constructs a
runtime with no profile, so it passes unchanged, and `runtime.rs` needed no edit
at all. `crates/obscura-browser/tests/fork_graphics_identity.rs` covers our half.

#### Interface objects must not be enumerable

Everything graphics.js puts on the global goes through `_graphicsDefineGlobal`
in `js/graphics_shim.js`, which defines it `enumerable: false` as WebIDL
requires. A plain `globalThis.X = C` lands enumerable, and
`Object.keys(window)` containing `WebGLRenderingContext` or `GPUAdapter` is a
one-line detection.

Upstream gets this only for the names in its `_preHideInternals` list, which
pre-declares them non-enumerable so a later plain assignment updates the value
alone. `WebGL2RenderingContext` is in that list; `WebGLRenderingContext` is not,
and neither is any WebGPU interface. `HTMLImageElement` is enumerable upstream
today and we do not touch it.

Verified: `Object.keys(globalThis)` leaks none of the graphics interfaces.

Two other observations from the same probe, neither a stage 3 problem:

- `isSecureContext` is missing from the engine entirely. Chrome has it on every
  page. Deliberately left alone until the stealth tests are green.
- `HTMLImageElement` is enumerable on the global, which is upstream's.

### The method that works: diff the two engines

Porting by reading commits missed things repeatedly. What works is measuring.

```
git worktree add /c/tmp/obscura-old pre-rebuild
cd /c/tmp/obscura-old && CARGO_TARGET_DIR=/c/tmp/obscura-old-target   cargo build --release -p obscura-cli --bins --features stealth
```

Then run the same probe against both binaries and diff the JSON:

```
obscura fetch about:blank --eval "$(cat /c/tmp/probe/surface.js)"
```

The probe captures every own global with its descriptor, the prototype shape of
the interfaces anti-bot code reads, navigator/screen/window values, the
toString of the usual builtins, error stacks, and the WebGL identity. Compare
*sets and shapes*, not values: screen size, deviceMemory, hardwareConcurrency
and devicePixelRatio vary legitimately because the profile rotates per run.

This is how the enumerability gap was found, and it is the only way to be sure a
port is complete. Keep `/c/tmp/probe/surface.js` or rebuild it from this note.

### Closed by measurement, in `js/fork_*.js`

| gap | before | after |
|---|---|---|
| interface objects in `Object.keys(window)` | 138 | 0 |
| `navigator` own properties | 25 | 0 |
| `screen` own properties | 9 | 0 |
| `toString.call(screen)` | `[object Object]` | `[object Screen]` |
| `Navigator`, `Permissions`, `MediaDevices`, `ScreenOrientation`, `NavigatorUAData`, `Screen`, `HTMLDocument`, `HTMLEmbedElement`, `HTMLSourceElement`, `NavigatorManagedData`, `ProtectedAudience` | absent | present, non-enumerable |
| `performance` | object literal, 3 timing fields | `Performance` + `PerformanceTiming`, 21 fields |
| `setTimeout.toString()` | `function () {...}` | `function setTimeout() {...}` |
| `chrome.runtime` | present | undefined |
| `isSecureContext` | absent | boolean |
| `new HTMLCanvasElement()` | no throw | Illegal constructor |

`bootstrap.js` carries **5 marker comments** for all of it and nothing else.

### Still open, measured and ranked

1. **`Element.prototype` and `HTMLElement.prototype` leak 20 engine privates**
   (`_renderBoxGeometry`, `_loadIframeSrc`, `_popoverAttrValue`, ...). Visible to
   `Object.getOwnPropertyNames`, so hiding from enumeration is not enough; they
   need to move off the public prototype. Upstream's, and the old fork build
   leaked 11 of its own, so this was never solved there either.
2. **`EventTarget.prototype` carries 51 `Node` constants and `Node.prototype`
   carries `addEventListener`/`removeEventListener`/`dispatchEvent`.** In Chrome
   those belong to exactly one of the two. Structural, upstream's.
3. **`window[0]`..`window[49]`** exist as frame-index accessors with no frames.
   Chrome has none. `Object.getOwnPropertyNames(window).filter(isNumeric)`.
4. `Navigator.prototype` exposes 27 members against the fork's 45, because
   upstream keeps the spoofed ones on an intermediate prototype.
5. `history` is non-enumerable here and enumerable in Chrome.
6. `Deno` is still reachable; see below.
7. `Worker` is a shim that evaluates in the page isolate, not a thread.

#### Deferred to stage 5: hiding `Deno` from the page

A page that can see `Deno` is not Chrome. The fork used to delete
`globalThis.Deno`, which needs bootstrap's own `Deno.core` calls to resolve
through an alias. That part is cheap: bootstrap's IIFE takes one `const Deno =
globalThis.Deno;` and the whole change is two marker comments plus a fork-owned
module, with no rename of bootstrap's ~80 call sites.

What blocks it is `runtime.rs`, which injects 33 scripts referencing
`Deno.core.ops` into the global scope, outside the IIFE. Eight upstream tests
fail immediately, and the render-gated instrumentation at `runtime.rs`
9098-9477 would break under `--features render` in stage 6. Making it work
means editing ~33 sites in a file upstream rewrites constantly, which is the
opposite of what this rebuild is for. It was implemented, measured, and
reverted. Revisit in stage 5 with a single Rust-side choke point for injected
scripts, not a rename.

---

Branch `webgl+webgpu`, at `6a02338`, with local child-frame CDP changes.

## What this project is

Obscura is a headless browser written in Rust: its own DOM, V8 through
`deno_core`, and a CDP server so Puppeteer and Playwright can drive it as a
drop-in for Chrome. It is aimed at scraping and agent work, so a page must not
merely load — it must be indistinguishable enough from Chrome that anti-bot
services treat it the same.

Crates: `obscura-dom` (tree), `obscura-js` (V8, ~9,700-line `js/bootstrap.js`
shim, frame realms), `obscura-net` (HTTP, TLS impersonation under the `stealth`
feature), `obscura-browser` (`Page`, lifecycle, profiles), `obscura-cdp` (CDP
server), `obscura-cli`, `obscura` (library facade), `obscura-mcp`.

## The work in this session: child frames

The goal was to make Cloudflare Turnstile work. That turned out to be a frames
problem end to end, and the fixes are general — every embedded widget uses the
same mechanisms.

**Result: Turnstile issues a token**, verified against the real service,
covered by `crates/obscura-browser/tests/live_turnstile_smoke.rs`, ~11s, 3/3
stable. The message sequence matches Chrome's exactly:

```
init -> requestExtraParams -> translationInit -> food -> complete{token}
```

### What was wrong, and why each fix

Ordered as they blocked. Every one of them failed *silently*, which is the
theme: Cloudflare's script ignores what it does not trust and reports nothing.

1. **`globalThis.parent = globalThis` in every realm.** A frame's
   `parent.postMessage(token)` was delivered to the frame itself, and
   `parent === window` told every widget in the frame it was the top document.
   Fixed with a host-routed cross-realm bridge: both sides hand a message to
   the host, which dispatches it into the target realm. A frame learns its own
   id and its parent's *before* its bootstrap runs, because `parent`/`top` are
   installed during init and one script taking the top-level branch is enough
   to change everything after it.

2. **Frames were attached ~10s late.** They were only picked up after the
   page's post-load settle (`OBSCURA_DYNAMIC_SCRIPT_SETTLE_MS`, default
   10,000). The challenge frame's first message arrived long after `api.js`
   had moved on. Frames are now attached and their messages delivered on a
   100ms cadence *inside* both settle loops — which is why the event loop is
   driven in slices and `self.js` is re-borrowed each turn. A page with no
   frames settles exactly as before.

3. **A frame's document never got `DOMContentLoaded` or `load`,** so anything
   a widget defers until then never ran.

4. **`event.isTrusted` was false** — the one that actually mattered. The bridge
   built the `MessageEvent` from script. `api.js` gates every message from its
   own frame on `isTrusted && source === iframe.contentWindow`. Fixed by
   marking host-delivered messages trusted through the existing
   `__obscura_markTrusted`. A script-built event must still report `false`;
   answering `true` for everything is a trivial bot tell, and both halves are
   tested.

   This also required **stable `contentWindow` identity across the frame's
   load**: an embedder captures that reference when it creates the iframe and
   compares it against `event.source` later, so handing out a fresh object on
   load broke the comparison.

Supporting changes: `MessageEvent` gained `origin`/`source`/`ports`/
`lastEventId` (a handler with none of these drops the message);
`window.postMessage` to one's own window went from a no-op to real async
delivery.

### Design decisions worth knowing

- **One isolate, many `v8::Context`s.** A frame realm is a second context in the
  page's isolate. An earlier attempt used a second isolate and was abandoned
  because objects cannot cross isolates. Ops resolve which realm called them
  via `RealmStates` and `scope.get_entered_or_microtask_context()` — not
  `get_current_context()`, which reports the realm the op was bound in.
- **`Page::frames` is declared before `Page::js`.** A realm holds a V8 handle
  into that isolate and fields drop in declaration order. Reordering them
  aborts the process.
- **Messages are JSON, not structured clone.** Structured clone cannot cross
  realms here. JSON covers what postMessage is used for; anything it cannot
  encode throws `DataCloneError` rather than arriving silently as `null`.
- **Frames were unobservable**, which is how "the frame rendered nothing"
  survived as a theory when the widget had in fact rendered. `Page::frame_urls`
  and `Page::evaluate_in_frame` open them up, and `OBSCURA_FRAME_PRELOAD` runs
  a script inside a frame realm before the frame's own scripts — the only point
  an instrument can watch them. It is off unless set: it runs arbitrary source
  inside a frame, so it is a debugging tool, not a page feature.

## What works

- Cross-origin frames run their own scripts against their own DOM and origin,
  inheriting the page's browser identity (a frame reporting a different UA or
  GPU than its parent is an instant tell).
- postMessage in both directions, with correct `origin`, `source`, `isTrusted`.
- `parent` / `top` in a framed document; the page still reports itself as top.
- Nested frames drain (the op queues onto the page's state whichever realm
  called it).
- `Page.getFrameTree` now includes the live child-frame hierarchy, and
  navigation emits `Page.frameAttached`, `Page.frameNavigated` and
  `Page.frameStoppedLoading` for each child. A local Playwright 1.62.1 check
  sees both the main page and its iframe through `page.frames()`.
- Turnstile end to end.

## What does not work, and what is unproven

- **Executing a real Turnstile proof-of-work is unproven.** The always-pass
  sitekey `1x00000000000000000000AA` **short-circuits** — verified by capturing
  the worker from real Chrome. It serves the full 265 KB framework but returns
  a fixed token with a trivial worker; the real PoW never launches. What is
  proven is the protocol, the frame lifecycle and token delivery. No real
  challenge could be observed from this machine at all: every
  challenge-triggering sitekey returns HTTP 400 "invalid sitekey" from this IP.
- **The managed widget** (e.g. `turnstile-test.vercel.app`) yields no token.
  It reaches `translationInit`, passes Turnstile's whole capability gate inside
  the frame realm, and builds its widget, then emits `overrunBegin` where
  Chrome emits `interactiveBegin`. **Real headless Chrome gets no token there
  either** — it waits for a human click. Next lead: `api.js` reports one caught
  error from `runImplicitRender`, driven by our `<load-events>` script.
- **CDP frame use is not complete.** Playwright can now see `page.frames()`,
  but CDP execution context ids are not mapped to frame realms, so evaluation
  through a child `Frame` still runs in the main realm. A later main-frame
  navigation also does not emit `Page.frameDetached` for removed children.
- **`_IframeDocument` is still what a parent reads through `contentDocument`** —
  a regex-built shim, not the frame's real document.
- **No same-origin synchronous DOM access between realms**, and no `frames[]`
  indexing.
- `_RemoteWindow` implements 12 of the 13 properties on the HTML spec's
  cross-origin allowlist; `location` is absent, so `parent.location` is
  `undefined` where a browser exposes a restricted Location. Small
  fingerprinting tell.

## Open question: is `"you"==="bot"` a honeypot?

Turnstile's capability gate builds a Blob worker whose entire source is the
string `"you"==="bot"`, constructs a `Worker` from it, revokes the URL and
terminates it immediately. Raised as a suspicion that this is bait rather than
a probe. **Unresolved.** The evidence, both ways:

*Reads as a plain capability probe:* it sits inside a function whose only
outputs are "supported"/"unsupported"; the worker is terminated at once and
nothing reads a result; the expression evaluates to `false` and has no side
effect; it is one link in a feature-detection chain (`ReadableStream.pipeTo`,
`BigInt`, `crypto.getRandomValues`, `performance.getEntries`,
`PerformanceObserver`).

*Reads as bait:* a pure probe would use `0` or `''` — the taunt is a choice.
And as a probe it is weak, because an engine that only *pretends* to support
Workers passes it trivially, which is exactly the shape of a trap for engines
that special-case, log, or refuse unusual worker sources.

**The real risk it points at, regardless of intent:** `globalThis.Worker` in
`bootstrap.js` (~line 8437) is a shim that evaluates the worker source in the
page's own isolate inside a Proxy scope. It is not a thread. Nothing today
observes worker *behaviour* — this gate only checks constructibility — so we
pass. Any future check that makes a worker do real work, or that times it, or
that looks for true concurrency, would fail. Treat "our Worker is not a worker"
as live technical debt, and do not read the current pass as evidence that
Workers are implemented.

## Verification

Windows 11, PowerShell/Git Bash. `cargo nextest` is required.

```
cargo build --release -p obscura-cli --features stealth      # exit 0
cargo nextest run --workspace --features stealth --no-fail-fast
cargo nextest run --workspace --no-fail-fast                 # live tests excluded
```

Recorded results at `cb869cb`:

| run | result |
|---|---|
| `--features stealth` | **541 tests, 538 passed, 3 failed, 3 skipped** |
| plain | **534 tests, 532 passed, 2 failed, 3 skipped** |

Current local child-frame CDP change, after `cargo clean`:

- `cargo build --release -p obscura-cli`: exit 0.
- `cargo nextest run --workspace --no-fail-fast`: **536 tests, 534 passed,
  2 failed, 3 skipped**. The two failures are the same CDP failures below.
- The two new CDP frame-tree/event tests and all six `frame_messaging` tests
  pass.
- A local Playwright 1.62.1 check against one page with one iframe reports both
  URLs from `page.frames()`.
- The obstacle course was not run because the companion `obscura-benchmark`
  checkout is not present beside this repository.

The three failures, all pre-existing and none caused by this work:

- `obscura-cdp::max_connections_cap max_connections_refuses_then_recovers`
- `obscura-cdp::concurrent_connections_heavy_page concurrent_connections_heavy_page_do_not_abort_v8`
- `obscura-browser::live_product_smoke live_product_cards_load_with_the_selected_profile`
  — **verified to fail identically at `f508c5b`**, before any of this work, via
  a baseline worktree.

### The live product smoke, specifically

Do not read it as a stealth regression. Measured this session:

- Not an environment proxy leak. `HTTPS_PROXY` is set in the shell, but both
  HTTP clients call `.no_proxy()` unconditionally
  (`obscura-net/src/client.rs:545`, `wreq_client.rs:214`); a proxy is used only
  when passed via `--proxy`/`OBSCURA_PROXY`. Measured: obscura's exit IP is the
  real one with `HTTPS_PROXY` set.
- Not the IP: through a proxy, Avito passes and Wildberries fails the same way.
- Not timing: raising the settle from 10s to 30s changes nothing.
- Not the fingerprint profile: pinning the test's profile over CDP still
  renders the page correctly.
- The engine renders that page fine over CDP (25 KB of text, product id
  present) while **headless Chrome gets an error page** on the same URL at the
  same moment. The failure is narrow: on the direct-library path the rendered
  text lacks the numeric product id, which Wildberries puts further down the
  page. Wildberries also no longer serves `application/ld+json` or an `h1`
  there, so the test's `title` check already relies on markup that is gone.

## Where to look

| file | why |
|---|---|
| `crates/obscura-js/src/frame.rs` | `FrameRealm`: construction, load events, message delivery |
| `crates/obscura-js/js/bootstrap.js` | the bridge (search `postMessage between browsing contexts`), `_RemoteWindow`, `_IframeWindow`, `_loadIframeSrc`, `Worker` (~8437), `_trustedEvents` (~5923) |
| `crates/obscura-browser/src/page.rs` | `attach_pending_frames`, `deliver_frame_messages`, `settle`, and the `execute_scripts` post-load loop |
| `crates/obscura-js/src/ops.rs` | `RealmStates`, `realm_state`, `op_frame_document_ready`, `op_post_frame_message` |
| `crates/obscura/tests/frame_messaging.rs` | 6 offline tests; each fails without its fix |
| `crates/obscura-browser/tests/live_turnstile_smoke.rs` | the live guard |
| `crates/obscura/examples/turnstile_probe.rs` | diagnosis: frame internals, capability gate, config, shadow content |
| `tools/ab/turnstile.mjs` | Chrome vs Obscura on the same widget |

## Next, by priority

1. **Finish CDP frame use.** Route child execution contexts to their frame
   realms, then emit `Page.frameDetached` when a navigation removes old
   children. Discovery is done: Playwright now sees `page.frames()`.
2. **Replace the `_IframeDocument` shim** with the frame realm's real document
   behind `contentDocument`, and add same-origin access. The shim regex-strips
   `<head>` and is a visible divergence from a browser.
3. **Decide what to do about Workers** (see the honeypot section). Either
   implement them properly or record deliberately that they are a shim and
   accept the ceiling.
4. **The managed Turnstile widget**: chase the `runImplicitRender` error, and
   note that reaching `interactiveBegin` buys parity with Chrome, not a token —
   that widget needs an interactive click in Chrome too.
5. **`live_product_smoke`**: either update the Wildberries case to markup the
   site still serves, or split "did the engine render it" from "did the site
   serve it", so the test stops being read as a stealth signal.

## Constraints and risks

- **Proxy credentials must never be written into git, logs, tests or docs.**
  They are passed at runtime only, via `--proxy` / `OBSCURA_PROXY`. Honoured
  throughout; the commits were scanned before this handoff.
- Wildberries/Ozon/Avito results are heavily IP-dependent and throttle rapidly.
  Reproduce offline before concluding anything from them.
- `tools/ab/*` strips proxy variables from child environments deliberately; a
  proxy left in the shell once sent a run through an exit it never asked for.
- Live tests are gated behind `--features stealth` so they stay out of a plain
  run. Turnstile does **not** need stealth — it passes without TLS
  impersonation — so that gate means "live network test" here, not "needs
  stealth".
- The settle loop now used by every page was restructured. It preserves the old
  early-exit semantics, but it is the highest-blast-radius change in this work.
- Deobfuscated Turnstile sources and protocol notes were left outside the repo,
  in `C:\Temp\turnstile-analysis`, `C:\Temp\turnstile-interactive` and
  `C:\Temp\turnstile-wasm`. They are scratch, not dependencies.

## Settled: no WebAssembly, no GPU

Checked three independent ways, because it decides whether this line of work is
worth continuing: the blob worker captured from real Chrome (13 bytes); the
265 KB challenge document; and the resolved string table, which is where this
obfuscator keeps every string it can reference. No `WebAssembly`, `wasm`,
`WebGL`, `WebGPU`, `getContext`, `toDataURL`, `getImageData`, `readPixels` or
`OffscreenCanvas` anywhere. The proof-of-work is plain JavaScript using
`BigUint64` arithmetic. There is no native kernel to reimplement, and WebGPU
support is not on the critical path for Turnstile.
