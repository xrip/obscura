# Chrome Windows fingerprint profiles

Obscura has one profile system for browser, screen, WebGL, and WebGPU data.
The current catalog target is:

- Captured Chrome browser identities from every valid source profile.
- Windows.
- ANGLE with D3D11.
- A Chrome 145 JavaScript graphics API shape, revision `145.0.7632.75`.

The profile system is on in normal and stealth mode. `--stealth` does not pick
a profile. It changes the network client and other stealth behavior. The same
selected Chrome profile data is used in both modes.

Stealth mode selects the matching pinned `wreq` Chrome transport when it is
available. If the selected browser major has no exact `wreq` transport,
Obscura uses the nearest available Chrome transport and gives a warning. A
profile with a browser major other than 145 also gives a warning because some
JavaScript API shapes still follow Chrome 145. The profile remains usable.

The current catalog has selectable Chrome 143, 144, 145, 147, 148, and 150
rows. Chrome 143 through 148 have exact pinned `wreq` transports. Chrome 150
uses the nearest available transport, Chrome 148, when stealth mode is on.

## What a profile contains

A profile has three selected parts.

| Part | Main data |
|---|---|
| Base | Chrome version, User-Agent, User-Agent Client Hints, Windows version, CPU architecture, languages, CPU count, memory, and touch points |
| Graphics | Masked and unmasked GPU data, WebGL 1, WebGL 2, WebGPU adapters, WebGPU limits and features, and preferred canvas format |
| Screen | Screen size, available area, color depth, DPR, inner and outer window size, and screen position |

The profile ID has this form:

```text
c<chrome-major>w1:<base-id>:<graphics-id>:<screen-id>
```

Every part ID is 32 lower-case hex characters. The prefix major must equal the
base-row Chrome major. The graphics row must have an observation for that same
major. Any screen row may be used. A graphics row always keeps its WebGL 1,
WebGL 2, and WebGPU data together.

The profile also gives WebGL a stable render seed. The same profile and the
same WebGL command record give the same synthetic pixels after a restart.

The profile does not select these values:

- Proxy address and exit IP.
- Timezone.
- Geolocation.
- A custom User-Agent.
- The old audio, battery, and 2D canvas fingerprint data.

Set the first three values as one group when region consistency is important.

## Profile life

Obscura selects a profile when it makes a new `BrowserContext`.

- All pages in that context use the same profile.
- Navigation keeps the same profile.
- Iframes and worker shims use the same profile.
- An isolated context copy keeps the same profile.
- Canvas contexts and graphics resources have separate mutable state.
- A second `BrowserContext` makes a new selection under the active rule.

Environment variables are read when the context is made. A change to an
environment variable does not change a context that already exists. Restart a
long-running `serve` or `mcp` process after a selector change.

The raw runtime profile is removed from the JavaScript global object during
page setup. Page code can read normal browser surfaces, but it cannot read or
change the internal profile object.

## Selector rules

Obscura uses `OBSCURA_PROFILE` and `OBSCURA_ROTATE_PROFILE`.

| Setting | Result |
|---|---|
| No setting | Fixed catalog default |
| `OBSCURA_PROFILE=0` | Fixed catalog default |
| `OBSCURA_PROFILE=<positive decimal>` | Stable weighted selection for catalog version 1 |
| `OBSCURA_PROFILE=c<major>w1:...` | Exact compatible base, graphics, and screen IDs |
| `OBSCURA_ROTATE_PROFILE=1` | New weighted random selection for each new context |

### Fixed default

With no selector, every new context gets the same default composition. Use
this mode for tests and work that must have the same result on every run.

The current default is:

```text
c145w1:d2e85f68f4092704b75e2a9fe7145fd7:8546ea21d519f56b82c1099e4893e49b:be830914f3fb0f9eb2577321d1d4a9fa
```

`OBSCURA_PROFILE=0` is an explicit form of the same rule.

### Stable decimal seed

Any decimal value from `1` through the maximum `u64` value makes a stable
weighted selection. Obscura selects a base first, then a graphics row observed
in that base major, then a screen row.

```text
OBSCURA_PROFILE=42
```

Seed `42` gives the same profile after each restart while catalog version 1 and
its rows stay the same. Two different seeds may still select the same row.
Rows with more observation weight have a greater chance of selection.

Use a decimal seed when you need repeatable profile variety but do not need to
store a long composed ID.

### Exact composed ID

An exact ID pins all three parts:

```text
OBSCURA_PROFILE=c<chrome-major>w1:<base-id>:<graphics-id>:<screen-id>
```

All three IDs must exist in the embedded catalog. The prefix and graphics row
must match the base Chrome major. Use this form for a test case, a saved
scraping job, or a known graphics setup.

### Weighted rotation

`OBSCURA_ROTATE_PROFILE` accepts `1`, `true`, `yes`, or `on`, without regard
to letter case. Obscura uses OS random bytes and picks the three parts by their
weights. The result is then fixed for that context.

`OBSCURA_ROTATE_PROFILE` is used only when `OBSCURA_PROFILE` is absent. If
`OBSCURA_PROFILE` exists, even as an empty value, it has priority. Remove it
before using rotation.

### Bad selectors

A bad decimal, a negative number, an unknown ID, an ID with the wrong form, or
an empty `OBSCURA_PROFILE` gives one warning per process. Obscura then uses the
fixed default. If OS random data is not available, rotation also uses the
fixed default and gives a warning.

## Build and run

Build the normal binary:

```powershell
cargo build --release
```

Build with the stealth network path:

```powershell
cargo build --release --features stealth
```

The binary is at `target/release/obscura.exe` on Windows.

### PowerShell

Use the fixed default:

```powershell
Remove-Item Env:OBSCURA_ROTATE_PROFILE -ErrorAction SilentlyContinue
$env:OBSCURA_PROFILE = '0'
.\target\release\obscura.exe fetch https://example.com --dump text
```

Use a stable seed:

```powershell
$env:OBSCURA_PROFILE = '42'
.\target\release\obscura.exe serve --port 9222
```

Pin an exact profile:

```powershell
$env:OBSCURA_PROFILE = 'c145w1:d2e85f68f4092704b75e2a9fe7145fd7:8546ea21d519f56b82c1099e4893e49b:be830914f3fb0f9eb2577321d1d4a9fa'
.\target\release\obscura.exe --stealth fetch https://example.com --dump text
```

Use weighted rotation:

```powershell
Remove-Item Env:OBSCURA_PROFILE -ErrorAction SilentlyContinue
$env:OBSCURA_ROTATE_PROFILE = '1'
.\target\release\obscura.exe serve --port 9222
```

Clear both settings:

```powershell
Remove-Item Env:OBSCURA_PROFILE -ErrorAction SilentlyContinue
Remove-Item Env:OBSCURA_ROTATE_PROFILE -ErrorAction SilentlyContinue
```

The values stay in the current PowerShell process until they are removed or
the shell is closed.

### Bash

Use a stable seed for one command:

```bash
OBSCURA_PROFILE=42 ./target/release/obscura fetch https://example.com --dump text
```

Use rotation and make certain that no exact selector is present:

```bash
env -u OBSCURA_PROFILE OBSCURA_ROTATE_PROFILE=1 \
  ./target/release/obscura serve --port 9222
```

Pin an exact profile:

```bash
OBSCURA_PROFILE='c145w1:d2e85f68f4092704b75e2a9fe7145fd7:8546ea21d519f56b82c1099e4893e49b:be830914f3fb0f9eb2577321d1d4a9fa' \
  ./target/release/obscura fetch https://example.com --dump text
```

The selector works with `fetch`, `serve`, `scrape`, and `mcp`, because all of
them make browser contexts through the same profile code.

For a CDP server, use a fixed ID or seed when all new contexts must be
repeatable. The process selector is the first profile for every new root CDP
connection. A connection may replace it with the exact-ID command below.

### Select a profile before CDP work

Start one server. It may have many root CDP connections running in parallel:

```powershell
.\target\release\obscura.exe serve --port 9222
```

Keep that terminal open. In another terminal, connect a CDP client to Obscura.
This Puppeteer example uses `connect`; it does not start Chrome. It sends the
non-standard `Obscura.setProfile` command on the browser session before it
makes a page or browser context:

```javascript
import puppeteer from 'puppeteer-core';

const browser = await puppeteer.connect({
  browserWSEndpoint: 'ws://127.0.0.1:9222/devtools/browser',
});

const profileId = 'c145w1:d2e85f68f4092704b75e2a9fe7145fd7:8546ea21d519f56b82c1099e4893e49b:be830914f3fb0f9eb2577321d1d4a9fa';
const root = await browser.target().createCDPSession();
const selected = await root.send('Obscura.setProfile', { profileId });
console.log(selected.profileId);

const context = await browser.createBrowserContext();
const page = await context.newPage();

const identity = await page.evaluate(async () => {
  const high = await navigator.userAgentData.getHighEntropyValues([
    'platformVersion',
  ]);
  const canvas = document.createElement('canvas');
  const gl = canvas.getContext('webgl2');
  const debug = gl.getExtension('WEBGL_debug_renderer_info');
  return {
    userAgent: navigator.userAgent,
    platform: navigator.userAgentData.platform,
    platformVersion: high.platformVersion,
    renderer: debug
      ? gl.getParameter(debug.UNMASKED_RENDERER_WEBGL)
      : null,
  };
});

console.log(identity);
await context.close();
await root.detach();
await browser.disconnect();
```

`profileId` must be one exact versioned `c<major>w1:...` ID from the embedded
catalog. The result gives the canonical selected ID. An unknown ID gives an
error and keeps the old connection profile.

The call is connection-scoped:

- Every later `newPage()` and `createBrowserContext()` on that CDP connection
  inherits the selected profile.
- Another root WebSocket connection to the same `serve` process may select a
  different profile.
- A page CDP session cannot set the connection profile.
- The call is rejected after the connection has made its first page or browser
  context, even if that object was later closed.
- A second call is allowed only while the connection still has no page or
  browser context.

If the connection does not call `Obscura.setProfile`, it keeps the profile
selected by `OBSCURA_PROFILE`, `OBSCURA_ROTATE_PROFILE`, or the fixed default
when `serve` started.

The same command works through Playwright's browser CDP session:

```javascript
import { chromium } from 'playwright';

const browser = await chromium.connectOverCDP(
  'http://127.0.0.1:9222',
);
const root = await browser.newBrowserCDPSession();
await root.send('Obscura.setProfile', { profileId });

const context = await browser.newContext();
const page = await context.newPage();
```

This command changes fingerprint identity only. It does not select, load, or
save cookies, `localStorage`, or other account state. Stock Playwright does not
send a custom `profileId` option from `browser.newContext()` in this release,
so use `Obscura.setProfile` before the first context or page.

### Use a profile with a saved Playwright login

The fingerprint profile and account state are separate. Select the profile on
the root CDP connection. Then use Playwright's normal `storageState` option and
methods for each browser context:

```javascript
import { existsSync } from 'node:fs';
import { chromium } from 'playwright';

const statePath = './account-state.json';
const profileId = 'c145w1:d2e85f68f4092704b75e2a9fe7145fd7:8546ea21d519f56b82c1099e4893e49b:be830914f3fb0f9eb2577321d1d4a9fa';

const browser = await chromium.connectOverCDP(
  'http://127.0.0.1:9222',
);
const root = await browser.newBrowserCDPSession();
await root.send('Obscura.setProfile', { profileId });

const context = await browser.newContext(
  existsSync(statePath) ? { storageState: statePath } : {},
);
const page = await context.newPage();
await page.goto('https://example.com/account');

// Log in or do other work here. Save the new account state on the client.
await context.storageState({ path: statePath });

await context.close();
await browser.close();
```

The JSON file is read and written by Playwright, not by the Obscura server.
This keeps local and remote CDP use the same: the file path is always a path on
the Playwright machine.

Obscura supports the standard Playwright calls used by this flow:

- `browser.newContext({ storageState })`, with a file path or state object.
- `context.storageState()` and `context.storageState({ path })`.
- `context.setStorageState()`, with a file path or state object.
- `context.cookies()` and `context.cookies(urls)`.
- `context.addCookies()`.
- `context.clearCookies()`, including name, domain, and path filters.

Cookies and `localStorage` belong to one Playwright `BrowserContext`. Pages in
that context share them. Another context has separate data, even when both
contexts use the same fingerprint profile. Changing the connection profile
does not clear or copy account state.

`localStorage` is kept by origin. `sessionStorage` stays page-local and is not
part of Playwright storage state. IndexedDB and Cache Storage are not included
in this release.

To run the real Playwright test, first build the release binary, then run:

```powershell
cargo build --release -p obscura-cli
node crates/obscura-cdp/tests/playwright_storage_state.mjs
```

The test gets pinned `playwright-core` 1.62.1 with npm only when it is missing.
It puts the package under ignored `target/test-fixtures/`; Playwright is not
kept in the repository.

## Find and inspect profile IDs

The tracked runtime catalog is:

```text
crates/obscura-browser/data/chrome-windows-v1.json
```

The CLI can list the selectable rows, show an exact composed profile, and
resolve the selector that a new context would use.

List the base, graphics, and screen rows as JSON:

```powershell
.\target\release\obscura.exe profiles list
```

The list has `defaultProfileId`, `baseProfiles`, `graphicsProfiles`, and
`screenProfiles`. It does not copy the large WebGL and WebGPU component data
into every row.

Show an exact profile, including its resolved WebGL and WebGPU components:

```powershell
.\target\release\obscura.exe profiles show 'c145w1:d2e85f68f4092704b75e2a9fe7145fd7:8546ea21d519f56b82c1099e4893e49b:be830914f3fb0f9eb2577321d1d4a9fa'
```

Show the selection made from the current environment:

```powershell
.\target\release\obscura.exe profiles current
```

For a short Windows, GPU, and screen view in PowerShell:

```powershell
$profile = .\target\release\obscura.exe profiles current | ConvertFrom-Json
[pscustomobject]@{
  Id = $profile.id
  Chrome = $profile.browser.version
  Platform = $profile.navigator.uaPlatform
  PlatformVersion = $profile.navigator.uaPlatformVersion
  Architecture = $profile.navigator.architecture
  Bitness = $profile.navigator.bitness
  CpuThreads = $profile.navigator.hardwareConcurrency
  DeviceMemory = $profile.navigator.deviceMemory
  GpuVendor = $profile.graphics.unmaskedVendor
  GpuRenderer = $profile.graphics.unmaskedRenderer
  Screen = "$($profile.screen.width)x$($profile.screen.height)"
  Dpr = $profile.screen.devicePixelRatio
} | Format-List
```

`profiles current` resolves one profile in its own process. With a fixed ID,
the default, or a decimal seed, it reports the same selection that a new
browser context gets under the same environment. With rotation, it is one new
random draw and cannot report the profile already held by another running
`serve`, `scrape`, or `mcp` process. Rust code can read an existing context ID
with `BrowserContext::profile_id()`.

### Read the default ID with PowerShell

```powershell
$catalogPath = 'crates/obscura-browser/data/chrome-windows-v1.json'
$catalog = Get-Content -LiteralPath $catalogPath -Raw | ConvertFrom-Json
$baseId = $catalog.defaultComposition.baseId
$graphicsId = $catalog.defaultComposition.graphicsId
$screenId = $catalog.defaultComposition.screenId
$base = $catalog.baseProfiles | Where-Object id -eq $baseId
$major = $base.browserVersion.Split('.')[0]
$profileId = "c${major}w1:${baseId}:${graphicsId}:${screenId}"
$profileId
```

List short base data:

```powershell
$catalog.baseProfiles |
  Select-Object id, browserVersion, platformVersion, architecture, bitness, hardwareConcurrency, deviceMemory, weight
```

List graphics rows:

```powershell
$catalog.graphicsProfiles |
  Select-Object id, unmaskedVendor, unmaskedRenderer, webgl1Id, webgl2Id, webgpuId, weight
```

List screen rows:

```powershell
$catalog.screenProfiles |
  Select-Object id, width, height, devicePixelRatio, innerWidth, innerHeight, weight
```

Make a composed ID from a base, a graphics row seen in its major, and any
screen row:

```powershell
$base = $catalog.baseProfiles[0]
$major = $base.browserVersion.Split('.')[0]
$graphics = $catalog.graphicsProfiles | Where-Object {
  $_.observationsByBrowserVersion.psobject.Properties.Name |
    Where-Object { $_.Split('.')[0] -eq $major }
} | Select-Object -First 1
$baseId = $base.id
$graphicsId = $graphics.id
$screenId = $catalog.screenProfiles[0].id
$profileId = "c${major}w1:${baseId}:${graphicsId}:${screenId}"
$profileId
```

### Read the default ID with `jq`

```bash
jq -r '. as $catalog | .defaultComposition as $d | (.baseProfiles[] | select(.id == $d.baseId) | .browserVersion | split(".")[0]) as $major | "c\($major)w1:\($d.baseId):\($d.graphicsId):\($d.screenId)"' \
  crates/obscura-browser/data/chrome-windows-v1.json
```

Do not make a graphics row by joining WebGL and WebGPU component IDs from
different graphics rows. Select a `graphicsProfiles` row and keep its three
component references together.

## Check browser-visible values

This PowerShell command prints key browser and graphics surfaces:

```powershell
$env:OBSCURA_PROFILE = '42'
.\target\release\obscura.exe fetch https://example.com --eval '(function(){const c=document.createElement("canvas");const gl=c.getContext("webgl2");const ext=gl.getExtension("WEBGL_debug_renderer_info");return JSON.stringify({ua:navigator.userAgent,platform:navigator.platform,languages:navigator.languages,hardwareConcurrency:navigator.hardwareConcurrency,deviceMemory:navigator.deviceMemory,screen:[screen.width,screen.height,devicePixelRatio],renderer:ext?gl.getParameter(ext.UNMASKED_RENDERER_WEBGL):null,webgpu:navigator.gpu?navigator.gpu.getPreferredCanvasFormat():null,rawProfile:typeof globalThis.__obscura_fingerprint_profile});})()'
```

`rawProfile` must be `"undefined"`. The other values come through normal web
interfaces.

WebGPU is visible only in a potentially trustworthy context. Use HTTPS,
`file:`, inherited `about:blank`, or HTTP loopback. A normal remote HTTP page
does not get `navigator.gpu`.

## Use profiles from Rust

Set the environment before making the browser or context. Do not change these
process-wide values after worker threads have started.

The high-level `obscura` crate uses the same selectors:

```rust
use obscura::Browser;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set OBSCURA_PROFILE in the parent shell before this process starts.
    let _browser = Browser::builder()
        .stealth(true)
        .build()?;
    Ok(())
}
```

The lower-level browser crate gives direct access to the selected ID:

```rust
use obscura_browser::BrowserContext;

fn main() {
    let context = BrowserContext::with_options(
        "profile-example".to_string(),
        None,
        false,
    );

    println!("{}", context.profile_id());
}
```

`obscura_browser::profiles::resolve_profile()` also returns a
`ResolvedFingerprintProfile` with browser, navigator, screen, graphics,
WebGL, WebGPU, and render-seed data. Do not call it only to inspect a different
rotated context: a separate call is a separate random selection. Read
`profile_id()` from the context that owns the pages.

## Custom User-Agent behavior

`--user-agent` and the Rust `.user_agent(...)` setting override the selected
User-Agent on the network and at `navigator.userAgent`. They do not replace
the other catalog data.

If the value is not the exact User-Agent from the selected Chrome Windows
base row, Obscura gives one warning. The caller then owns consistency between
the custom value and these surfaces:

- User-Agent Client Hints.
- Chrome full version.
- Windows platform version.
- WebGL and WebGPU data.
- Screen and hardware data.

Use a catalog profile without a custom User-Agent when possible.

## Proxy, timezone, and geolocation

A profile does not set network location. Keep the proxy exit, timezone, and
geolocation in agreement.

PowerShell example:

```powershell
$env:OBSCURA_PROFILE = '42'
$env:OBSCURA_TIMEZONE = 'America/New_York'
$env:OBSCURA_GEOLOCATION = '40.7128,-74.0060'
.\target\release\obscura.exe --proxy http://USER:PASS@HOST:PORT --stealth fetch https://example.com --dump text
```

The screen part is a device shape, not a location signal. Changing the screen
part does not make a timezone or geolocation change.

## Local browser profile workbench

The local workbench is at:

```text
webgl/capture/index.html
```

It has two jobs on one page:

1. Three select boxes let you join any existing base, graphics, and screen row
   and copy the final versioned profile ID.
2. The capture tool reads the current real browser, checks it against the
   Windows ANGLE/D3D11 source rules, makes the same content IDs as the Rust
   generator, and saves one source observation through the local Obscura
   server.

The workbench has no external script, font, image, CDN, or service. It reads
only the catalog built into Obscura. A save request goes back to the same local
Obscura process. It does not send the capture to another server.

### Capture target

A new graphics capture is accepted only for this target:

- 64-bit Google Chrome or Chromium with a numeric full version.
- Windows.
- `x86` architecture and `64` bitness in User-Agent Client Hints.
- A reduced Windows User-Agent with the same Chrome major.
- WebGL 1 and WebGL 2.
- ANGLE with a D3D11 renderer.
- A working default WebGPU adapter and device.
- At least 82 valid WebGL 1 parameters and 132 valid WebGL 2 parameters.
- All 12 shader precision records for each WebGL generation.

Every valid capture can become a base row, a graphics-version observation, and
a versioned composed ID. If the browser major differs from the Chrome 145 API
shape, the workbench shows a non-blocking consistency warning. If there is no
exact pinned `wreq` transport, it also shows the nearest transport major that
stealth mode will use. The capture can still be saved and selected.

Use a normal browser state for a useful observation:

- Turn hardware acceleration on.
- Do not force SwiftShader, OpenGL, Vulkan, or another ANGLE backend.
- Set page zoom to 100%.
- Put the browser window on the display to record.
- Use the wanted Windows display scale before opening the page.
- Set the wanted window size and state before capture.
- Close docked DevTools if it changes `innerWidth` or `innerHeight`.
- Use a clean browser profile if extensions or policy may change graphics.

The page checks the reported renderer. `chrome://gpu` is also useful for a
manual check that hardware acceleration and D3D11 are active.

### Start the workbench

Build Obscura, then start its CDP server with the workbench source directory.
Run this from the repository root:

```powershell
cargo build --release
.\target\release\obscura.exe serve --port 9222 --profile-workbench-dir webgl
```

Open this address in the real browser:

```text
http://127.0.0.1:9222/obscura/profiles/
```

The flag is off by default. Without `--profile-workbench-dir`, this route gives
a not-found response. The path may be absolute, but `webgl` is the right value
when `serve` starts from this repository root.

The save route accepts only a client with all of these properties:

- Its network address is loopback.
- Its `Host` is `localhost`, `127.0.0.1`, or another loopback address on the
  active `serve` port.
- Its `Origin` is that same local HTTP address.
- Its body is JSON.

The read-only page and catalog may still be read if `serve` uses another bind
address, but save stays loopback-only. Use the normal loopback bind:

```powershell
.\target\release\obscura.exe serve --host 127.0.0.1 --port 9222 --profile-workbench-dir webgl
```

The workbench needs one server worker because there must be one writer for the
raw profile files and screen array. Obscura rejects this combination:

```powershell
.\target\release\obscura.exe serve --workers 2 --profile-workbench-dir webgl
```

Loopback HTTP is a potentially trustworthy context, so Chrome can expose
WebGPU. Stop `serve` with `Ctrl+C` after the work is complete.

### Make an ID from existing rows

The page loads the compact catalog built into the running binary plus any
profiles already saved by this workbench, then gives three select boxes:

- Base: Chrome, Windows platform version, CPU, memory, and languages.
- Graphics: one whole GPU, WebGL 1, WebGL 2, and WebGPU row.
- Screen: physical screen, available area, window, position, and DPR.

Each graphics label shows the Chrome majors recorded for that exact row. The
selected-row summary gives its exact version observation counts.

Changing any select box updates the final value:

```text
c<chrome-major>w1:<base-id>:<graphics-id>:<screen-id>
```

Use **Copy profile ID**, then set it before Obscura starts:

```powershell
$env:OBSCURA_PROFILE = 'c<chrome-major>w1:<base-id>:<graphics-id>:<screen-id>'
.\target\release\obscura.exe profiles current
.\target\release\obscura.exe --stealth fetch https://example.com --dump text
```

**Select catalog default** restores the catalog default composition.

### Capture the current real browser

Press **Capture and check**. The page does this work locally:

1. Reads full User-Agent Client Hints.
2. Reads navigator CPU, memory, languages, and touch data.
3. Reads screen, available area, DPR, inner and outer sizes, and position.
4. Creates separate WebGL 1 and WebGL 2 canvases.
5. Enables every reported WebGL extension.
6. Reads numeric context and extension constants.
7. Tests each numeric value with `getParameter` and records its exact return
   type. Invalid enum probes are kept with an empty type so the Rust generator
   can drop them by its normal rule.
8. Reads context attributes, supported extensions, precision formats,
   anisotropy, draw-buffer limits, version strings, and the unmasked renderer.
9. Requests default, low-power, and high-performance WebGPU adapters.
10. Reads adapter information, features, adapter limits, and the limits of a
    default device from each available adapter.
11. Normalizes the data with the same field order and rules as the Rust tool.
12. Makes the first 16 bytes of each SHA-256 content ID and the final composed
    profile ID.

When every check passes, the page selects the captured base, graphics, and
screen IDs in the three boxes. The final versioned ID is visible and can be
copied. This is true even when the browser needs an API or transport warning.

If an ID is already in the tracked catalog, the select option is the existing
row. If it is new, the option begins with `[new capture]`. The new ID is still
valid: the workbench registers it when the save request succeeds.

### Save the source files

After all checks pass, press **Save capture to source files**. The built-in
server checks that the profile, graphics, and screen blocks belong to the same
capture. It then makes these changes under the directory from
`--profile-workbench-dir`:

- Appends one screen observation to `window.json`.
- Makes a new non-overwriting file such as
  `profiles/capture-<digest>-001.json`.
- Uses `.obscura-new` and `.obscura-backup` files while it replaces the screen
  source array.
- Registers the normalized runtime profile in the running `serve` process.
- Writes a private `.obscura-runtime/` sidecar so the next workbench server can
  load the same profile without another capture.
- Makes the new base, graphics, and screen rows visible from the workbench
  `/catalog` endpoint.

The page gives the new profile path, composed profile ID, and window row count.
If an old `.obscura-backup` file is present, save stops so that the old data can
be checked and recovered by hand. It does not remove an old backup silently.

Saving the same observation again is allowed. It adds another observation, so
the generator gives equal content more weight. Use one save per real
observation. Do not press save twice by mistake.

For a large existing source set, save needs enough free memory and disk space
for the old array, the new array, and one short backup. The raw source files
are local and ignored, but a separate private backup is still wise before a
large import.

### Manual downloads

The two download buttons are a manual backup and import path:

| Download | Input role | Main content |
|---|---|---|
| `obscura-profile.json` | One file under `webgl/profiles/` | Base identity plus matching screen, WebGL, and WebGPU data |
| `obscura-windows.json` | One row for `webgl/window.json` | One whole screen and window observation |

These two files are one observation and must stay together. Do not join the
profile from one machine with the window download from another machine while
importing a capture.

The capture includes detailed fingerprint data. It does not read cookies,
passwords, local storage, browsing history, proxy address, public IP,
geolocation, timezone, audio data, or 2D canvas pixels. Keep the raw files
local. The source paths are ignored by Git.

### Manually import downloaded files

Node.js 18 or newer can safely check that the two files belong together and
add them to the ignored source files. Run this from the repository root and
replace the two download paths:

```powershell
node webgl/capture/import-capture.js `
  'C:\Users\YOU\Downloads\obscura-profile.json' `
  'C:\Users\YOU\Downloads\obscura-windows.json'
```

The import helper:

- Rejects a file with the wrong capture shape.
- Rejects two files that do not have equal screen and window blocks.
- Creates `webgl/window.json` if it does not exist.
- Appends one observation to the screen array.
- Creates a new non-overwriting file such as
  `webgl/profiles/capture-<digest>-001.json`.
- Uses temporary and backup names while replacing the screen source array.
- Never changes the tracked compact catalog. Generation is a separate step.

Importing the same observation again is allowed. It adds another observation,
so the generator gives equal content more weight. Use one import per real
observation. Do not import a file twice by mistake.

The helper rewrites the local screen JSON array in a stable pretty form. The
built-in save button and this helper have the same source-file result.

### Use the captured ID in the running workbench

No catalog generation is needed for the running workbench. After the save
request returns, use the printed ID through `Obscura.setProfile` on a new root
CDP connection. The profile must still be selected before the first page or
browser context is created.

The sidecar is local to the workbench source directory and is ignored by Git.
It does not change the fixed catalog default or weighted rotation.

### Promote the capture into the embedded catalog

To make the profile available to a binary that is not started with the
workbench directory, stop `serve`, run the normal generator command from the
next section, and rebuild Obscura. Start the new binary with the workbench flag
again. Then refresh the workbench. The `[new capture]` mark must be gone and
the same three IDs must now be normal embedded catalog options.

Build Obscura and check the exact ID printed by the capture page:

```powershell
cargo build --release
$env:OBSCURA_PROFILE = 'c<captured-major>w1:<captured-base-id>:<captured-graphics-id>:<captured-screen-id>'
.\target\release\obscura.exe profiles current
```

The returned `id`, Windows fields, renderer, component IDs, screen, WebGL, and
WebGPU data must match the workbench capture and the generated catalog.

### Observation weights

Weights come from repeated equal observations:

- Every accepted base profile file adds one base observation.
- Every accepted full profile file adds one graphics observation.
- Every window entry in a screen row adds one screen observation.
- Equal normalized content is grouped and its weights are added.

The graphics row also has `observationsByBrowserVersion`. Its keys are exact
Chrome versions and its values are observation counts. If the same normalized
row was seen in more than one Chrome version, all version counts are recorded
on that one row instead of copying the graphics data.

The fixed default remains Chrome 145. It uses the highest-weight Chrome 145
base and compatible graphics row, plus the highest-weight screen row.
An import may change the default only when it changes these weight rankings.
Ties use the lowest content ID.

## Catalog files

Tracked files:

| File | Purpose |
|---|---|
| `crates/obscura-browser/data/chrome-windows-v1.json` | Compact runtime catalog |
| `webgl/catalog/chrome-windows-v1.schema.json` | JSON schema |
| `webgl/catalog/chrome-windows-v1.report.json` | Counts, size, rejects, and checks |
| `webgl/catalog/chrome-windows-v1.sources.json` | Source hashes and byte counts |
| `webgl/catalog/chrome-145-graphics-api-v1.json` | Chrome 145 graphics API manifest |
| `webgl/catalog/chrome-145-graphics-api-v1.sources.json` | API source revision and hashes |
| `webgl/capture/index.html` | Local capture and three-part profile picker |
| `webgl/capture/collector.js` | Browser, WebGL, WebGPU, screen, and adapter capture |
| `webgl/capture/profile-id.js` | Generator-compatible normalization and content IDs |
| `webgl/capture/import-capture.js` | Safe local source import helper |

Local source files:

```text
webgl/window.json
webgl/profiles/
```

These local files are ignored by Git. They are not needed to run Obscura.
They are needed only to make a new catalog. Do not add them to Git.

The build puts a small fixed-default fragment and a compressed full catalog in
the binary. The fixed default does not require the full catalog to be parsed.
A seed, exact ID, or rotation loads and checks the full embedded catalog once
per process.

## Generate a new catalog

Put the local source files in the paths shown above. Then run this command from
the repository root:

```powershell
cargo run --manifest-path tools/fingerprint-catalog/Cargo.toml -- generate --profiles webgl/profiles --windows webgl/window.json --out crates/obscura-browser/data/chrome-windows-v1.json --schema webgl/catalog/chrome-windows-v1.schema.json --report webgl/catalog/chrome-windows-v1.report.json --sources webgl/catalog/chrome-windows-v1.sources.json
```

The tool:

- Accepts every internally consistent Chrome Windows base row with a numeric
  four-part browser version.
- Reads graphics observations from every Windows profile under
  `webgl/profiles/`, including version subdirectories.
- Records exact browser-version counts on equal graphics rows.
- Keeps every valid base and graphics row selectable. A graphics row may be
  joined only to a base with the same captured Chrome major.
- Keeps only the approved base fields.
- Keeps graphics rows and screen-window pairs whole.
- Adds equal-row observation weights.
- Makes stable content IDs.
- Checks ID collisions.
- Drops invalid WebGL parameter probes.
- Removes private, commercial, timing, and benchmark fields.
- Checks the JSON schema.
- Stops if the compact catalog is over 2 MiB.
- Writes only hashes and counts to the source digest file.

The tool is separate from the main Cargo workspace. Test it with:

```powershell
cargo nextest run --manifest-path tools/fingerprint-catalog/Cargo.toml
```

After generation, inspect the report and source digest:

```powershell
Get-Content webgl/catalog/chrome-windows-v1.report.json
Get-Content webgl/catalog/chrome-windows-v1.sources.json
```

Then make a release build. The browser build script checks the catalog size,
parses the default composition, and checks all default component references.

```powershell
cargo build --release
cargo nextest run -p obscura-browser
cargo nextest run -p obscura-js
```

For a catalog or graphics change, also run the full project gates from
`AGENTS.md`, including the workspace tests, the obstacle course, and the
stealth build and tests.

## Common problems

### Rotation always gives the same profile

Check whether `OBSCURA_PROFILE` exists. It has priority over rotation. Remove
it, then make a new context or restart the process.

### A seed does not change an open page

Selection takes place at context creation. Restart the process or make a new
context.

### Two seeds give the same profile

This is possible with weighted selection. Use exact composed IDs when every
test case must use a different row.

### An exact ID gives the default

One or more parts are not in the embedded catalog, or the ID form is wrong.
Read all three IDs from the same catalog version. Look for the one-time warning
in process logs.

### WebGPU is missing

First check the page URL. Remote plain HTTP is not a trustworthy context. Then
check that the selected graphics row has the requested adapter preference and
features.

### The workbench says that the catalog load failed

Start the HTTP server from the repository root and open
`http://127.0.0.1:8765/webgl/capture/`. Serving only the capture directory does
not make the tracked catalog URL available.

### The workbench warns about the browser version

The catalog accepts captured Chrome Windows majors that pass the identity and
graphics checks. A major other than 145 gets a warning because the JavaScript
graphics API shape is pinned to Chrome 145. A missing exact transport profile
gets a second warning and uses the nearest supported transport profile.

### The workbench reports no WebGPU adapter

Use the loopback HTTP address, turn hardware acceleration on, restart Chrome,
and check `chrome://gpu`. Remote plain HTTP and some virtual machines do not
give the page a WebGPU adapter.

### The workbench reports a non-D3D11 renderer

Remove browser flags that force SwiftShader, Vulkan, OpenGL, or another ANGLE
backend. Restart Chrome and check the renderer again. Do not import that
capture into this D3D11 catalog.

### A captured ID is marked as new

The page has made the final stable content ID, but the embedded catalog does
not have that row yet. Press **Save capture to source files**. The running
workbench then registers the ID and returns it in the save result. Only a
binary that is not using `--profile-workbench-dir` needs catalog generation and
a rebuild before it can use that ID.

### Chrome blocks more than one download

Use the two separate download buttons. If Chrome asks for approval, allow
multiple downloads only for the local `127.0.0.1` page.

### A custom User-Agent gives a warning

The custom value is not equal to the catalog User-Agent. Remove the override,
or accept that the caller must keep all browser surfaces consistent.

### A catalog build fails

Read the full error. Common causes are a missing core base field, inconsistent
browser versions, a missing graphics component, an ID collision, bad JSON, a schema
error, or a catalog over 2 MiB. The generator report records rejected rows and
their reasons when generation can finish.
