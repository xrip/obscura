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
