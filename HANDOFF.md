# Handoff

Branch `webgl+webgpu-v2`, 36 commits on top of upstream `origin/main` at
`97124ed`. Working tree clean apart from the files listed under
[Intentionally uncommitted](#intentionally-uncommitted).

## What this project is, and what this fork is for

Obscura is a headless browser written in Rust: its own DOM, V8 through
`deno_core`, a CDP server so Puppeteer and Playwright drive it as a Chrome
drop-in, and (upstream, recently) a native render layer. Upstream lives at
`origin` = `h4ckf0r0day/obscura`; our fork pushes to `xrip` only.

The fork exists to add a **stealth identity layer** upstream does not want:
a catalog of real captured Chrome-on-Windows fingerprint profiles, and the
engine changes that make one selected profile consistent across every surface
a site can read - TLS transport, HTTP headers, `navigator`, `screen`, WebGL,
WebGPU, audio, codecs.

**The target state**, in the project owner's words:

> upstream + our profiles + stealth tests passing + easy upstream sync = WIN

The fork can never be merged upstream (upstream deliberately answers "no" where
we answer "Chrome"), but it must stay cheap to merge *from* upstream forever.

## Why this branch exists

The previous branch `webgl+webgpu` (frozen at tag `pre-rebuild` = `e1562bf`)
diverged so far that upstream's render drop could not be merged: 18 conflicted
files, `bootstrap.js` carrying **+2390/-899** and `runtime.rs` **+2014/-109** of
fork code. The abandoned merge is preserved on branch `merge-attempt-snapshot`
(it contains conflict markers and does not build).

This branch restarts from upstream and re-applies the fork as **isolated
modules**, so the next sync is a plain `git merge origin/main`.

## Current state

| goal criterion | status |
|---|---|
| upstream features still work | **yes** - suite at its recorded baseline every run |
| our profiles | **yes** - 367 base / 427 graphics / 226 screen rows, consistent across surfaces |
| easy upstream sync | **yes** - `bootstrap.js` is **+5/-0**, all fork code in `fork_*` files |
| no duplicated work | **yes** - ~400 lines deleted after finding upstream equivalents |
| **stealth tests passing** | **NO** - `live_product_smoke` is red. This is the open work. |

Fork delta against upstream: **87 files, +16532 / -221**.

## Architecture: how fork code is kept out of upstream's way

This is the part that must not be eroded. Everything else can be rewritten.

### JavaScript: five marker comments, nothing else

`crates/obscura-js/js/bootstrap.js` is a ~14,500-line upstream file that
upstream rewrites constantly. The fork adds **five comment lines** to it and
nothing more:

```
/* __OBSCURA_FORK_EARLY_MODULE__ */     before upstream's performance literal
/* __OBSCURA_FORK_LATE_MODULE__ */      top level, after the DOM classes exist
/* __OBSCURA_GRAPHICS_PAGE_INIT__ */    inside __obscura_init
/* __OBSCURA_FORK_PAGE_INIT_END__ */    last statement of __obscura_init
```
(the fourth marker hosts two modules; five lines total including a blank)

`crates/obscura-js/build.rs` splices the fork modules in at snapshot build time
and **hard-errors if a marker is missing**, so an upstream merge that drops one
fails the build instead of silently shipping a half-stealth engine.

Fork JS modules, all under `crates/obscura-js/js/`:

| file | what it does |
|---|---|
| `fork_performance.js` | `Performance` + `PerformanceTiming` classes, 21 timing fields, `toJSON` |
| `graphics_shim.js` | helpers `graphics.js` needs that upstream's bootstrap lacks |
| `graphics_api_v145.js` | generated Chrome 145 IDL constants and method arities |
| `graphics.js` | the canvas / WebGL / WebGL2 / WebGPU facade (~780 lines) |
| `fork_interfaces.js` | 11 interface constructors upstream never puts on `window` |
| `fork_media_codecs.js` | `canPlayType` answering as Chrome does |
| `fork_console.js` | stops `Error.stack` being read; adds Chrome's console methods |
| `fork_event_target.js` | separates `EventTarget` from `Node` |
| `graphics_page_init.js` | per-page profile hand-off |
| `fork_browser_shape.js` | lifts `navigator`/`screen` members onto their prototypes |
| `fork_audio_memory.js` | `AudioContext` starts suspended; `[object MemoryInfo]` |
| `fork_hide_globals.js` | sweep making interface objects non-enumerable |

### Rust: inherent impls in fork-owned modules

Rust allows an inherent `impl` in any module of the defining crate, so fork
methods live in their own files and the call site still reads naturally:

- `crates/obscura-js/src/graphics.rs` - `set_fingerprint_profile`
- `crates/obscura-js/src/origin_storage.rs` - BrowserContext-scoped localStorage
- `crates/obscura-net/src/transport_profile.rs` - Chrome major to wreq profile
- `crates/obscura-browser/src/fork_virtual_url.rs` - `Page::sync_virtual_url`

Files upstream rewrites hardest, and what the fork costs in each:

```
obscura-js/js/bootstrap.js        +5  -0
obscura-js/src/runtime.rs         +2  -1     (one pub(crate) on a field)
obscura-js/src/ops.rs             +8  -0
obscura-browser/src/page.rs       +28 -27
obscura-js/src/module_loader.rs   +53 -11
obscura-cdp/src/domains/input.rs  +26 -1
```

Compare the old branch: `bootstrap.js` +2390/-899, `runtime.rs` +2014/-109.

### Two rules that produced this, and must be kept

1. **Before porting anything, check whether upstream already does it.**
   ~400 lines were deleted rather than ported once upstream was found to have
   `ResourceRequest`, `fetch_with_profile`, `request_referrer`,
   `request_fetch_site` and gzip decoding. Their call sites in `page.rs`,
   `ops.rs`, `runtime.rs` and `module_loader.rs` then needed no porting at all.
2. **Where upstream and the fork disagree philosophically, gate on the
   profile rather than editing upstream's test.** Upstream returns `null` from
   `getContext('webgl')` and `''` from `canPlayType` on purpose: an engine with
   no GPU or decoder that claims support makes applications take a path that
   renders nothing. Both fork facades therefore appear **only when a fingerprint
   profile is loaded**. Upstream's tests build a runtime without one, so
   `unavailable_webgl_context_does_not_claim_success` and
   `unsupported_media_capabilities_and_readiness_are_honest` both pass
   **unedited**, and `runtime.rs` needed no change for either.

## What works

Verified this session against the real Chrome on this machine, or against the
old fork build:

- **Profile engine.** `obscura profiles list|show|current`. Composed IDs from a
  1.4 MB catalog baked and gzipped at build time by
  `crates/obscura-browser/build.rs`; ~3 ms on top of a 34 ms process start.
- **Identity is consistent across surfaces.** `userAgent`, `appVersion`,
  `platform`, `vendor`, `userAgentData`, client hints on the wire, WebGL
  ANGLE/D3D11 renderer, screen metrics and timezone all agree, `webdriver` is
  false, five plugins, `pdfViewerEnabled` true.
- **Transport identity** follows the profile's Chrome major
  (`transport_profile.rs`), rather than upstream's pinned `Chrome145`.
- **WebGL/WebGPU.** Real parameter tables from the profile; `requestAdapter()`
  yields a `GPUAdapter` whose `vendor`/`architecture` agree with the WebGL
  renderer. Secure-context gated, as in Chrome.
- **Interface surface matches the old fork build.** The probe diff is now
  essentially zero: no missing globals, only `history`/`isSecureContext`
  descriptors and values that vary because the profile rotates.
- **`canPlayType` is byte-identical to real Chrome** on six probes, positive
  and negative.
- Upstream's own failing test `stealth_client_decodes_gzip_response` is
  **fixed** (the stealth path hardcoded `validate_url(url, false)` so
  `--allow-private-network` never reached it).

## What is broken or incomplete

### `live_product_smoke` is red - the open work

`crates/obscura-browser/tests/live_product_smoke.rs` drives Wildberries, Ozon
and Avito. It is the fork's stealth gate and it fails.

Important context: **it was never fully green on the old branch either.** The
old build passes Wildberries and fails Ozon and Avito. Parity with the fork
means WB green and the other two red.

Proven, same URL and same minute, with an **identical TLS fingerprint**:

| build | body | product links | on challenge page |
|---|---|---|---|
| old fork (`pre-rebuild`) | 2,792,132 | **295** | no |
| this branch | 773 | 0 | yes |

So the old engine solves Wildberries' JS challenge and reloads into the real
page; this one does not. It is an engine behaviour difference living in the
still-unported parts of one fork commit.

**Eliminated by measurement.** Do not re-investigate these without new evidence:

| candidate | how it was ruled out |
|---|---|
| TLS fingerprint | `ja3n` and `ja4` identical to the old build; `ja3` varies per run because the emulation shuffles extensions as Chrome does |
| HTTP version | both negotiate HTTP/1.1 |
| transport generally | `peetprint_hash` identical between the two builds |
| frames / shadow DOM | the live challenge page has 0 iframes and 0 shadow roots |
| settle budget | `OBSCURA_STRICT_SETTLE=1` takes the run 9s to 36s, unchanged |
| more time | `--wait 30`, still 773 bytes |
| self-routing | `d7dca7a` ported, `clicklocal.mjs` green, unchanged |
| browser version gate | `IS_OUTDATED_BROWSER` is false; `browser-check.js` only tests Chrome < 80 |
| wreq transport major | profile is 145 and wreq has an exact Chrome145 |
| `accept-encoding` | identical (`gzip, br`) over HTTPS; an earlier HTTP/1.1 reading was misleading |
| JS interface surface | probe diff against the old build is essentially zero |
| IP / throttling | real Chrome passes from the same exit in the same minute |

### Other known gaps

- **`Element.prototype` and `HTMLElement.prototype` leak 20 engine privates**
  (`_renderBoxGeometry`, `_loadIframeSrc`, `_popoverAttrValue`, ...). Visible to
  `Object.getOwnPropertyNames`, so making them non-enumerable is not enough.
  The old fork leaked 11 of its own and still passes WB, so this is not the
  blocker.
- **`window[0]`..`window[49]`** exist as frame-index accessors with no frames.
  Chrome has none.
- **`Deno` is reachable from the page.** `764298d` makes it non-enumerable
  rather than deleting it, which avoids the ~33 `Deno.core` call sites in
  `runtime.rs` that made an earlier deletion attempt fail. Not yet ported.
- **`Worker` is a shim** that evaluates in the page isolate; it is not a thread.
- **Both builds negotiate HTTP/1.1** and advertise only `http/1.1` in ALPN.
  Every real Chrome offers `h2`. A genuine divergence, unrelated to WB.
- `Navigator.prototype` exposes 27 members against the old fork's 45, because
  upstream keeps the spoofed ones on an intermediate prototype.
- `history` is non-enumerable here, enumerable in Chrome.
- Frame realms are **entirely unported** (`bc1cd60`, `e43e651`, `f11e748`,
  `ba3d9d8`, `99426aa`, `ce18b78`). Needed for `live_turnstile_smoke`, which is
  also unported. Not needed for Wildberries.

## Build, run, test

Use `cargo nextest`, never `cargo test`: the engine holds one V8 isolate per
process and `cargo test` runs a whole binary in one process.

```bash
cargo build --release -p obscura-cli --bins --features stealth
cargo build --release -p obscura-cli --bins --features render,stealth

cargo nextest run --release --workspace --features stealth --no-fail-fast --test-threads 8
cargo nextest run --release --workspace --no-fail-fast --test-threads 8
```

`--test-threads 8` matters on a 16-core box: at full parallelism the
`obscura-cli::mcp_client` binary starves and flakes. Three different tests in it
have flaked so far. Run that binary alone before believing any failure from it;
it has been 16/16 on ten consecutive standalone runs.

**Clear the proxy environment first** for anything that must reach the network
directly. The shell normally has `HTTPS_PROXY` set, with credentials:

```bash
unset HTTP_PROXY HTTPS_PROXY ALL_PROXY http_proxy https_proxy all_proxy
```

### Results recorded at this commit

```
cargo build --release -p obscura-cli --bins --features stealth          exit 0
cargo build --release -p obscura-cli --bins --features render,stealth   exit 0

nextest --features stealth   1112 tests: 1081 passed, 31 failed, 6 skipped
nextest plain                1110 tests: 1081 passed, 29 failed, 6 skipped
```

Failure breakdown for the stealth run:

| count | what | ours? |
|---|---|---|
| 28 | `obscura-render` `dom::tests` and `layout_test` | **no** - upstream's, layout and text shaping differ on Windows |
| 1 | `obscura-cdp::max_connections_cap` | **no** - upstream's, pre-existing |
| 1 | `obscura-cli::mcp_client test_evaluate` | **no** - the load flake; 16/16 alone |
| 1 | `obscura-browser::live_product_smoke` | **yes** - the open work |

### The upstream baseline this is compared against

Measured on a clean checkout of `97124ed` **before any fork code landed**:
**1075 tests, 1045 passed, 30 failed, 4 skipped.** Compare failure *sets*, not
counts. Against that baseline this branch adds `live_product_smoke` and removes
`stealth_client_decodes_gzip_response`.

## How to measure, so the next session does not repeat mine

Reading commits missed things repeatedly. Six hypotheses were wrong this session
and every one was caught by measuring instead. Build the old fork and diff the
two engines:

```bash
git worktree add /c/tmp/obscura-old pre-rebuild
cd /c/tmp/obscura-old && CARGO_TARGET_DIR=/c/tmp/obscura-old-target \
  cargo build --release -p obscura-cli --bins --features stealth
```

Three tools are checked in:

- `tools/ab/surface-probe.js` - every own global with its descriptor, prototype
  shapes, navigator/screen/window values, `toString` of builtins, error stacks,
  WebGL identity. Run under `--eval` on both binaries and diff the JSON.
  **Compare sets and shapes, not values**: the profile rotates per run, so
  screen size, `deviceMemory` and DPR differ legitimately.
- `tools/ab/probe-chrome.mjs <file.js> [--headed] [--url U] [--window-size W,H]` -
  runs the same expression in the real Chrome over raw CDP, using
  `Page.navigate` and `Runtime.evaluate` only, never `Runtime.enable`, with
  `AutomationControlled` disabled. Always closes Chrome over CDP.
- `tools/ab/chrome-raw.mjs` and `tools/ab/journey.mjs` - the journey the project
  owner considers the true gate: home page, then click three product cards.

**The control that matters:**

```bash
node tools/ab/chrome-raw.mjs --cards 3 --headed          # 28 links, 3/3 cards
node tools/ab/journey.mjs --site wb --cards 3 --only obscura   # 0 links today
```

`tools/ab` needs `playwright-core` at `target/test-fixtures/playwright`; there
is no setup script:

```bash
mkdir -p target/test-fixtures/playwright && cd target/test-fixtures/playwright
npm install playwright-core@1.62.1
```

Live sites throttle, and a throttled run looks exactly like a fingerprinting
failure. Always read the returned body, never a link count alone, and always run
the Chrome control in the same minute.

## Next milestones, in priority order

### 1. Finish porting `764298d` "fix stealth challenge token generation"

This is the prime suspect for Wildberries. The name is literal and WB's cookie
is `x_wbaas_token`. It touches six files; **two of its three visible mechanisms
are already ported**:

- done - `Error.stack` is no longer read when a page logs an Error
  (`fork_console.js`). Upstream's `_consoleFn` did `a.stack || a.message`; a
  page can install a getter on `Error.prototype.stack` and detect it.
- done - Chrome's missing console methods (`dirxml`, `timeStamp`, `profile`,
  `profileEnd`, `context`, `createTask`).
- **not done** - `Deno` made non-enumerable rather than deleted.
- **not done** - `page.rs` (+102), `runtime.rs` (+101), `ops.rs` (+46).

Port the remaining pieces one at a time, measuring after each with
`node tools/ab/journey.mjs --site wb --cards 3 --only obscura` against the
`chrome-raw.mjs` control. Keep them in `fork_*` files wherever an inherent impl
or a spliced module can carry them.

### 2. If that does not do it, instrument the challenge

The WB challenge scripts all load and execute with no console errors, `__vmfp`
is present with `{bundle, getExported, run}`, and `x_wbaas_token` is set, yet the
body never advances past 773 bytes. Wrap `__vmfp.run` and log what it returns,
and watch the request the solver posts and the response it gets. That separates
"the fingerprint is computed and rejected" from "the submission never happens".
Inject via CDP `Page.addScriptToEvaluateOnNewDocument` or Playwright
`addInitScript` through `tools/ab/engines.mjs`.

### 3. Port the frame realms, for Turnstile

`bc1cd60`, `e43e651`, `f11e748`, `ba3d9d8`, `99426aa`, `ce18b78`, then land
`live_turnstile_smoke.rs` and `crates/obscura/tests/frame_*.rs`. Two invariants
that abort the process if broken: `Page::frames` must be declared **before**
`Page::js` (fields drop in declaration order and a realm holds a V8 handle), and
ops must resolve their realm with `scope.get_entered_or_microtask_context()`,
never `get_current_context()`.

### 4. Close the remaining measured gaps

`Element.prototype` privates, `window[0..49]`, ALPN advertising `h2`,
`Navigator.prototype` member count, `history` enumerability.

### 5. Write the sync rules into `CLAUDE.md`

Not yet done. `AGENTS.md` belongs to upstream and must be taken as-is on every
merge. The rules to write down are the ones this branch already follows: fork
code lives in files upstream does not have; `bootstrap.js` takes markers, not
inline edits; never reformat inside an upstream file; sync early and often; tag
every sync point; push to `xrip` only.

## Files to read first

| file | why |
|---|---|
| `crates/obscura-js/build.rs` | the splice mechanism the whole JS isolation rests on |
| `crates/obscura-js/js/fork_*.js` | every JS behaviour the fork adds |
| `crates/obscura-browser/src/profiles.rs` | the profile engine (~1,440 lines) |
| `crates/obscura-net/src/wreq_client.rs` | transport identity, `with_browser_identity` |
| `crates/obscura-net/src/transport_profile.rs` | Chrome major to wreq emulation |
| `crates/obscura-browser/tests/fork_*.rs` | what the fork guarantees, as tests |
| `PROFILES.md` | profile selection, capture and catalog workflow |
| `tools/ab/README.md` | the A/B harness and its hard-won warnings |
| `AGENTS.md` | upstream's build/test rules; do not edit |

## Constraints and risks

- **Proxy credentials must never reach git, logs, tests or docs.** They arrive
  at runtime only, via `--proxy` / `OBSCURA_PROXY`. `tools/ab/*` strips proxy
  variables from child environments on purpose.
- **Never push to `origin`.** Push to `xrip`.
- **Do not bulk-run `cargo fmt`** - the tree is not rustfmt-clean and a blanket
  format produces a huge unrelated diff.
- **Keep ops panic-safe**: `op_dom` is wrapped in `catch_unwind` so a DOM-op
  panic returns null rather than aborting inside V8's FFI frame.
- The `render` feature is opt-in (`default = []`), so stealth work does not have
  to fight the renderer. Both feature combinations build.
- Wildberries, Ozon and Avito results are heavily IP-dependent and throttle
  fast. Reproduce offline where possible; `tools/ab/clicklocal.mjs` runs the
  whole click path against a local fixture in seconds.

## Intentionally uncommitted

- `.idea/` - IDE settings. Not in `.gitignore`; left untracked deliberately
  rather than committing editor state or modifying ignore rules during a
  handoff.
- `polymorphic-hopping-eagle.md` - the plan document from the session that
  started this rebuild. Superseded by this file; left in place rather than
  deleted, since it is the owner's artifact.
