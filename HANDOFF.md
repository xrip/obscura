# Obscura handoff

Date: 2026-08-06

## Project and current goal

Obscura is a Rust headless browser for web scraping and AI agents. It runs
JavaScript in V8, owns a DOM, and serves Chrome DevTools Protocol endpoints for
Playwright and Puppeteer. The active branch adds a versioned Chrome Windows
identity catalog, synthetic WebGL and WebGPU surfaces, per-CDP-connection
profile selection, a local profile workbench, and Playwright cookie/localStorage
state.

The current profile work makes every valid captured Chrome major selectable.
The fixed API shape stays Chrome 145, and unsupported transport majors use the
nearest pinned `wreq` profile. Both differences give warnings instead of
blocking a profile. The earlier real-site tests used Wildberries and Ozon; both
still blocked Obscura and need separate network diagnostics.

## Git state

- Branch: `webgl+webgpu`
- HEAD before this handoff: `1c1b6ef add Playwright storage state support`
- Remotes:
  - `origin`: upstream `h4ckf0r0day/obscura`
  - `xrip`: user fork `xrip/obscura`
- `xrip/webgl+webgpu` points at `89f0e81`; the three newest local commits have
  not been pushed at handoff time.

Important commits, oldest first:

- `9c01d15 feat(graphics): add Chrome 145 profile facade`
- `9d3e88b test(graphics): load smoke libraries from CDN`
- `89f0e81 fix graphics fingerprint object shape`
- `30891e2 add profile workbench and capture flow`
- `b1cef86 add per-connection CDP profile selection`
- `1c1b6ef add Playwright storage state support`

## Implemented work

### Versioned Chrome profile catalog and graphics facade

- The embedded catalog is
  `crates/obscura-browser/data/chrome-windows-v1.json`.
- `crates/obscura-browser/src/profiles.rs` loads and resolves catalog profiles.
- A composed ID has the form
  `c<chrome-major>w1:<base-id>:<graphics-id>:<screen-id>`.
- Captured Chrome majors 143, 144, 145, 147, 148, and 150 are selectable.
  Chrome 145 remains the fixed default and graphics API shape. Other majors
  warn about this difference but still run.
- Stealth mode uses an exact pinned `wreq` transport where available. It uses
  the nearest one and warns otherwise; Chrome 150 currently uses Chrome 148.
- One `BrowserContext` owns one frozen resolved identity. Pages, navigation,
  iframe shims, workers, WebGL, and WebGPU use that identity.
- `crates/obscura-js/js/graphics.js` holds the graphics implementation.
- `crates/obscura-js/js/graphics_api_v145.js` holds the Chrome 145 API tables.
- `crates/obscura-js/build.rs` adds the graphics sources to the bootstrap.
- WebGL output is synthetic and deterministic. No native GPU is started.
- Three.js r184 and PixiJS 8.18.1 are not stored in the repository. Smoke tests
  get them from their pinned CDN URLs. License files remain tracked.

The graphics work is a facade, not a claim of full Khronos conformance. Re-run
the focused graphics tests before changing its API shape or native-looking
function handling.

### Profile capture and selection workbench

- `webgl/capture/index.html` combines browser capture and a three-part profile
  picker.
- `collector.js`, `profile-id.js`, and `import-capture.js` produce the capture
  data and stable IDs.
- `crates/obscura-cdp/src/profile_workbench.rs` serves the catalog and saves
  accepted capture data through the same local `serve` process. A saved capture
  is registered at once and kept in a private `.obscura-runtime/` sidecar for
  the next workbench server.
- Start it with:

  ```powershell
  .\target\release\obscura.exe serve --port 9222 --profile-workbench-dir webgl
  ```

- Open `http://127.0.0.1:9222/obscura/profiles/`.
- Full instructions are in `PROFILES.md`.
- Raw input files under `webgl/profiles/` and `webgl/window.json` are local and
  ignored. The compact catalog, schema,
  report, and source digests are tracked.

### Per-root-CDP profile selection

- `Obscura.setProfile` is a non-standard CDP command implemented in
  `crates/obscura-cdp/src/domains/obscura.rs`.
- Call it through a root browser CDP session before making any page or browser
  context.
- The selected identity belongs to one root WebSocket connection. Another root
  connection may select another identity in the same server process.
- The command is rejected after the first page or browser context is made.
- Profile selection changes browser identity only. It does not own or change
  cookies or Web Storage.
- Browser version and screen bounds returned by CDP follow the connection
  profile.

Playwright example:

```javascript
const browser = await chromium.connectOverCDP('http://127.0.0.1:9222');
const root = await browser.newBrowserCDPSession();
await root.send('Obscura.setProfile', { profileId });
const context = await browser.newContext();
```

### Playwright cookies and localStorage state

Commit `1c1b6ef` completed the standard Playwright flow:

- `context.cookies()` and URL-filtered reads.
- `context.addCookies()`.
- `context.clearCookies()`, including filtered clear.
- `browser.newContext({ storageState })` with an object or client-side file.
- `context.storageState()` and `context.storageState({ path })`.
- `context.setStorageState()` with an object or client-side file.
- BrowserContext-scoped cookies and localStorage.
- Origin-scoped localStorage shared by pages in the same context.
- Page-local sessionStorage.
- Correct CDP session-cookie handling for `expires: -1` and removal for
  `expires: 0` or an old positive time.
- Expired response and JavaScript cookies now remove an existing matching
  cookie, which is needed for logout and correct saved state.

Main files:

- `crates/obscura-net/src/cookies.rs`
- `crates/obscura-js/src/ops.rs`
- `crates/obscura-js/js/bootstrap.js`
- `crates/obscura-browser/src/context.rs`
- `crates/obscura-browser/src/page.rs`
- `crates/obscura-cdp/src/domains/storage.rs`
- `crates/obscura-cdp/src/domains/network.rs`
- `crates/obscura-cdp/tests/playwright_storage_state.mjs`

`Network.clearBrowserCache` is a successful no-op because Playwright calls it
while replacing storage state. IndexedDB and Cache Storage remain out of this
work. Playwright does not put sessionStorage in its normal storage-state file.

The Playwright test gets pinned `playwright-core` 1.62.1 only when the test is
run and puts it under ignored `target/test-fixtures/`. Do not add Playwright,
Three.js, or PixiJS packages to Git.

## Last real-site test

The last manual test used this profile from the old catalog:

```text
c145w1:673aa76b117fad13f52aa7cbf7d534c3:e3d3a2bc9ffee855f993c3e1c6588a7e:02d2105e93d6c86e29d80eb82952159d
```

The graphics ID changed when captured WebGPU feature order became part of the
content. This old composed ID is no longer valid. Repeat the real-site test
with an ID from the current catalog before using it as a release gate.

Observed identity:

- Chrome `145.0.7632.75`, reduced UA Chrome `145.0.0.0`
- Windows, `Win32`, platform version `19.0.0`
- 16 CPU threads and 8 GB device memory
- 2560 by 1440 screen, DPR 1
- Intel UHD Graphics through ANGLE D3D11

The release binary was built with `--features stealth`, started with
`--stealth`, and the exact profile was set through `Obscura.setProfile` before
the BrowserContext was made.

### Without a proxy

- Wildberries product `797296322` stayed on its `Проверяем браузер` support
  page after 15 seconds.
- The direct exit was IPv4 `185.93.70.106`.
- Ozon product `1902651403` entered a `__rr=2` redirect loop and hit Obscura's
  redirect limit before page JavaScript ran.
- Wildberries card API returned `403 Forbidden` from Angie.
- Ozon composer product API had the same redirect loop.

### With the user-supplied SOCKS5 proxy

Credentials are intentionally not recorded here. Do not copy them into Git,
logs, tests, or documentation.

- The proxy worked and Wildberries saw IPv6 `2001:470:1f06:a3::2`.
- Wildberries still returned its support page, now without the visible
  `Проверяем браузер` text.
- Obscura logged failures when loading these page subresources:
  - `/__wbaas/challenges/antibot/__static/v2/browser-check.js`
  - `/__wbaas/challenges/antibot/__static/v2/index-Bob5L-dt.js`
- A separate top-level Obscura fetch of `browser-check.js` through the same
  proxy succeeded. This is the strongest current clue: top-level fetch works,
  while the page subresource and module paths fail.
- Both `card.wb.ru/cards/v2/detail` and `u-card.wb.ru/cards/v4/detail` returned
  `403 Forbidden` from Angie through the proxy.
- Ozon still entered the same redirect loop.

No reliable price, seller, stock, rating, or Wildberries product name was
obtained. The Ozon URL names a SINTEC AdBlue exhaust-system fluid, 10 L, but
the site response did not confirm its product data.

## Build and test

Use scoped builds while working. A full workspace run creates heavy disk,
memory, CPU, and Microsoft linker load on this Windows machine.

Default release:

```powershell
cargo build --release -p obscura-cli
```

Stealth release:

```powershell
cargo build --release -p obscura-cli --features stealth
```

The last stealth release build passed on 2026-08-05 in 24.2 seconds.

Focused storage checks run during handoff:

```powershell
cargo nextest run -p obscura-cdp playwright_cookie_methods_are_scoped_to_browser_context
node crates/obscura-cdp/tests/playwright_storage_state.mjs
```

Results:

- CDP cookie-context test: 1/1 passed.
- Real Playwright 1.62.1 storage flow: passed. It covered object and file state,
  add/read/clear/filter cookies, localStorage by origin, sessionStorage
  separation, and BrowserContext separation.

Other recent focused results from the same working state:

- `cargo nextest run -p obscura-net --no-fail-fast`: 63/63 passed.
- Local/session storage V8 test: 1/1 passed.
- A combined `obscura-net`, `obscura-browser`, and `obscura-cdp` run passed
  181/182 tests.
- The one failure was the existing Windows
  `max_connections_refuses_then_recovers` test. The raw over-limit socket was
  reset with Windows error 10054 instead of returning the expected HTTP 503.
  It failed again when run alone. None of the storage changes touch that path.
- The companion `obscura-benchmark` repository was not present, so the 33/33
  obstacle course was not run.

Do not use `cargo test` for V8 tests. Use `cargo nextest`. Do not run the full
workspace gate on this machine unless it is necessary and the user approves
the disk and linker cost.

## Known issues and risks

1. Wildberries page subresource/module requests fail through the supplied
   SOCKS5 proxy even though the same JavaScript file works as a top-level
   Obscura fetch.
2. Ozon has a server redirect loop. The present error hides each response's
   status, `Location`, and `Set-Cookie`, so its cause is not yet known.
3. The real-site blocks may include IP reputation, request-header differences,
   cookie/redirect state, JavaScript gaps, or more than one cause. Do not call
   them only fingerprint failures without evidence.
4. `Network.clearBrowserCache` is a no-op. This is enough for current
   Playwright storage state but is not a full browser cache implementation.
5. localStorage has safety limits: 5 MiB per origin, 32 MiB per context, and
   256 origins. It is memory-backed, not server-side disk storage.
6. IndexedDB and Cache Storage are not part of saved state.
7. The graphics API is synthetic. Exact known-probe GPU replay, native shader
   work, and WebGPU draw/compute output are not present.
8. CDN smoke tests need network access. They do not keep large library files in
   the repository.
9. The Windows max-connection test needs a separate decision. Do not mix that
   socket behavior change into the stealth or storage work.

## Next tasks, in order

1. Add narrow diagnostics for navigation and subresource errors without
   printing proxy credentials. Record response status, redirect target,
   `Set-Cookie`, request kind, and which HTTP client path was used.
2. Reproduce the Wildberries failure with one page and one static challenge
   script. Compare top-level navigation, classic-script loading, and module
   loading through the stealth SOCKS5 client. Inspect
   `crates/obscura-browser/src/page.rs`, `crates/obscura-js/src/module_loader.rs`,
   and `crates/obscura-net/src/wreq_client.rs`.
3. Reproduce the Ozon redirect chain with bounded logging. Check whether
   cookies from every redirect response enter the BrowserContext cookie jar
   before the next request.
4. Make the smallest fix supported by the new diagnostics. Keep SSRF checks,
   V8 panic safety, and the watchdog unchanged.
5. Re-run the exact real-site script with the same profile, first without and
   then with an approved proxy. Wait at least 15 seconds and save final URL,
   title, body text, JSON-LD, and server warnings.
6. Re-run the focused storage tests and the affected network/browser/CDP tests.
   Run the 33/33 obstacle course when the companion repository is available.
7. Push local commits to `xrip/webgl+webgpu` only when the user asks.

## Important entry points

- `AGENTS.md`: build, test, safety, and project rules.
- `PROFILES.md`: complete profile, workbench, CDP, and storage-state guide.
- `crates/obscura-cli/src/main.rs`: CLI flags and `serve` entry.
- `crates/obscura-cdp/src/server.rs`: CDP connection server.
- `crates/obscura-cdp/src/dispatch.rs`: command routing and per-connection
  state.
- `crates/obscura-cdp/src/domains/obscura.rs`: `Obscura.setProfile`.
- `crates/obscura-cdp/src/profile_workbench.rs`: workbench HTTP routes and
  capture saves.
- `crates/obscura-browser/src/context.rs`: profile, cookie, and localStorage
  ownership.
- `crates/obscura-browser/src/page.rs`: navigation, subresources, runtime
  creation, and network events.
- `crates/obscura-net/src/client.rs`: normal HTTP client.
- `crates/obscura-net/src/wreq_client.rs`: stealth transport path.
- `crates/obscura-net/src/cookies.rs`: shared BrowserContext cookie jar.
- `crates/obscura-js/src/module_loader.rs`: JavaScript module requests.
- `crates/obscura-js/src/runtime.rs`: V8 runtime and profile injection.
- `crates/obscura-js/src/ops.rs`: V8 operations and bounded localStorage.
- `crates/obscura-js/js/bootstrap.js`: browser, DOM, Storage, and native-looking
  JavaScript surfaces.
- `crates/obscura-js/js/graphics.js`: WebGL and WebGPU facade.
- `tools/fingerprint-catalog/`: isolated catalog generator and fixtures.

## Intentionally uncommitted local files

These files belong to the user. Do not delete, edit, inspect, or commit them
without direct approval:

- `1`
- `2.html`
- `test1.ps1`
- `webgl/webgl.md`
- `obscura-proxy-product-check.mjs`

The last file is the user's saved real-site test and contains proxy access
data. Keep it out of Git and avoid printing its contents.
