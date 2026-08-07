# Handoff

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
