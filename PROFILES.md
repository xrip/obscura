# Chrome 145 fingerprint profiles

Obscura has one profile system for browser, screen, WebGL, and WebGPU data.
The target is fixed:

- Chrome 145, revision `145.0.7632.75`.
- Windows.
- ANGLE with D3D11.

The profile system is on in normal and stealth mode. `--stealth` does not pick
a profile. It changes the network client and other stealth behavior. The same
Chrome 145 profile data is used in both modes.

## What a profile contains

A profile has three selected parts.

| Part | Main data |
|---|---|
| Base | Chrome version, User-Agent, User-Agent Client Hints, Windows version, CPU architecture, languages, CPU count, memory, and touch points |
| Graphics | Masked and unmasked GPU data, WebGL 1, WebGL 2, WebGPU adapters, WebGPU limits and features, and preferred canvas format |
| Screen | Screen size, available area, color depth, DPR, inner and outer window size, and screen position |

The profile ID has this form:

```text
c145w1:<base-id>:<graphics-id>:<screen-id>
```

Every part ID is 32 lower-case hex characters. The current catalog permits any
base part, graphics part, and screen part to be used together. A graphics row
always keeps its WebGL 1, WebGL 2, and WebGPU data together.

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
| `OBSCURA_PROFILE=c145w1:...` | Exact base, graphics, and screen IDs |
| `OBSCURA_ROTATE_PROFILE=1` | New weighted random selection for each new context |

### Fixed default

With no selector, every new context gets the same default composition. Use
this mode for tests and work that must have the same result on every run.

The current default is:

```text
c145w1:d2e85f68f4092704b75e2a9fe7145fd7:f9b781363030180eb52d391c03167488:012a7166bca451ee154cd22665977ee4
```

`OBSCURA_PROFILE=0` is an explicit form of the same rule.

### Stable decimal seed

Any decimal value from `1` through the maximum `u64` value makes a stable
weighted selection. Base, graphics, and screen draws are separate.

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
OBSCURA_PROFILE=c145w1:<base-id>:<graphics-id>:<screen-id>
```

All three IDs must exist in the embedded catalog. Use this form for a test
case, a saved scraping job, or a known graphics setup.

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
$env:OBSCURA_PROFILE = 'c145w1:d2e85f68f4092704b75e2a9fe7145fd7:f9b781363030180eb52d391c03167488:012a7166bca451ee154cd22665977ee4'
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
OBSCURA_PROFILE='c145w1:d2e85f68f4092704b75e2a9fe7145fd7:f9b781363030180eb52d391c03167488:012a7166bca451ee154cd22665977ee4' \
  ./target/release/obscura fetch https://example.com --dump text
```

The selector works with `fetch`, `serve`, `scrape`, and `mcp`, because all of
them make browser contexts through the same profile code.

For a CDP server, use a fixed ID or seed when all new contexts must be
repeatable. With rotation, separate contexts in one server process may have
different profiles.

## Find and inspect profile IDs

The tracked runtime catalog is:

```text
crates/obscura-browser/data/chrome-145-windows-v1.json
```

The CLI does not print the selected profile ID. Exact pins can be read from
the catalog. Rust code can get the ID from `BrowserContext::profile_id()`.

### Read the default ID with PowerShell

```powershell
$catalogPath = 'crates/obscura-browser/data/chrome-145-windows-v1.json'
$catalog = Get-Content -LiteralPath $catalogPath -Raw | ConvertFrom-Json
$baseId = $catalog.defaultComposition.baseId
$graphicsId = $catalog.defaultComposition.graphicsId
$screenId = $catalog.defaultComposition.screenId
$profileId = "c145w1:${baseId}:${graphicsId}:${screenId}"
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

Make a composed ID from any valid three rows:

```powershell
$baseId = $catalog.baseProfiles[0].id
$graphicsId = $catalog.graphicsProfiles[0].id
$screenId = $catalog.screenProfiles[0].id
$profileId = "c145w1:${baseId}:${graphicsId}:${screenId}"
$profileId
```

### Read the default ID with `jq`

```bash
jq -r '"c145w1:\(.defaultComposition.baseId):\(.defaultComposition.graphicsId):\(.defaultComposition.screenId)"' \
  crates/obscura-browser/data/chrome-145-windows-v1.json
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

If the value is not the exact User-Agent from the selected Chrome 145 Windows
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

## Catalog files

Tracked files:

| File | Purpose |
|---|---|
| `crates/obscura-browser/data/chrome-145-windows-v1.json` | Compact runtime catalog |
| `webgl/catalog/chrome-145-windows-v1.schema.json` | JSON schema |
| `webgl/catalog/chrome-145-windows-v1.report.json` | Counts, size, rejects, and checks |
| `webgl/catalog/chrome-145-windows-v1.sources.json` | Source hashes and byte counts |
| `webgl/catalog/chrome-145-graphics-api-v1.json` | Chrome 145 graphics API manifest |
| `webgl/catalog/chrome-145-graphics-api-v1.sources.json` | API source revision and hashes |

Local source files:

```text
webgl/adapters.json
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
cargo run --manifest-path tools/fingerprint-catalog/Cargo.toml -- generate --profiles webgl/profiles --adapters webgl/adapters.json --windows webgl/window.json --out crates/obscura-browser/data/chrome-145-windows-v1.json --schema webgl/catalog/chrome-145-windows-v1.schema.json --report webgl/catalog/chrome-145-windows-v1.report.json --sources webgl/catalog/chrome-145-windows-v1.sources.json
```

The tool:

- Accepts only Chrome 145 Windows base rows.
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
Get-Content webgl/catalog/chrome-145-windows-v1.report.json
Get-Content webgl/catalog/chrome-145-windows-v1.sources.json
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

### A custom User-Agent gives a warning

The custom value is not equal to the catalog User-Agent. Remove the override,
or accept that the caller must keep all browser surfaces consistent.

### A catalog build fails

Read the full error. Common causes are a missing core base field, a non-Chrome
145 row, a missing graphics component, an ID collision, bad JSON, a schema
error, or a catalog over 2 MiB. The generator report records rejected rows and
their reasons when generation can finish.
