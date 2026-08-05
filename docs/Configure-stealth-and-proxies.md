## Stealth mode

```bash
obscura fetch https://example.com --stealth
obscura serve --stealth
obscura scrape url1 url2 --stealth
obscura mcp --stealth
```

`--stealth` is a global flag, so it works before or after the subcommand and applies to `fetch`, `serve`, `scrape`, and `mcp`. In a `scrape` run each worker inherits it.

What `--stealth` changes:

- Uses the wreq HTTP client with browser-matching TLS fingerprints (ClientHello, ALPN, cipher order).
- Loads a tracker blocklist that drops requests to known analytics and fingerprinting endpoints.
- Bundles webpki roots instead of relying on the system store.

Requires a build that includes the stealth feature. The Releases page provides
explicitly named `-stealth` archives alongside the lean default archives, so
users who do not need wreq/BoringSSL do not pay its binary-size or RSS cost. To
build the stealth variant yourself:

```bash
cargo build --release --features stealth
```

## What stealth handles

- Basic bot detection that checks TLS fingerprint or User-Agent.
- Sites that rely on third-party analytics being reachable.

## What stealth does not handle

- Cloudflare interactive challenges.
- Datadome and Akamai bot manager active challenges.
- CAPTCHAs.
- IP-based rate limiting (use proxies).

## Proxies

HTTP proxy:

```bash
obscura fetch https://example.com --proxy http://proxy.example.com:8080
obscura serve --proxy http://proxy.example.com:8080
```

With auth:

```bash
obscura fetch https://example.com --proxy http://user:pass@proxy.example.com:8080
```

SOCKS5:

```bash
obscura fetch https://example.com --proxy socks5://proxy.example.com:1080
```

## Custom User-Agent

```bash
obscura fetch https://example.com --user-agent "Mozilla/5.0 (...) ..."
obscura serve --user-agent "Mozilla/5.0 (...) ..."
```

The default UA is the UA from the selected Chrome 145 Windows profile. A custom
UA changes the HTTP header and `navigator.userAgent` only. If it is not an exact
Chrome 145 Windows UA, Obscura gives one warning. The caller then owns the match
between the custom UA and all other profile data.

## Browser profile, timezone, and geolocation

The engine has a Chrome 145 Windows catalog. One profile joins the browser,
navigator, screen, window, WebGL, and WebGPU data for a `BrowserContext`. All
pages, navigation, iframe shims, and worker shims in that context use the same
profile. Graphics data always stays with its captured ANGLE/D3D11 adapter row.

A single stable profile is used by default. One IP cycling through different identities is itself a signal, so rotation is opt-in:

```bash
OBSCURA_PROFILE=0 obscura serve          # fixed catalog default
OBSCURA_PROFILE=42 obscura serve         # stable catalog seed
OBSCURA_PROFILE=c145w1:BASE:GRAPHICS:SCREEN obscura serve  # exact ID
OBSCURA_ROTATE_PROFILE=1 obscura serve   # weighted random parts per context
```

Timezone is driven by the process zone so `Date` (`getTimezoneOffset`, `toString`) and `Intl.DateTimeFormat` report the same region. Default is `Europe/Berlin`; set it to match the exit IP:

```bash
OBSCURA_TIMEZONE=America/New_York obscura serve
```

`navigator.geolocation` reports configurable coordinates. Set them as `lat,lon` and keep them consistent with the timezone and proxy region:

```bash
OBSCURA_GEOLOCATION="40.7128,-74.0060" obscura serve
```

Keep these aligned. A rotated or mismatched profile carries no matching TLS or timezone fingerprint, so when you pin a proxy region or TLS fingerprint, leave rotation off and set the timezone and geolocation to the same region. See [Environment variables](Environment-variables.md) for the full list.

## Combine

```bash
obscura serve \
  --stealth \
  --proxy http://user:pass@proxy.example.com:8080 \
  --user-agent "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 ... Chrome/145.0.0.0 Safari/537.36"
```
