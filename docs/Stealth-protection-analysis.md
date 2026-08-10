# Wildberries and Ozon protection analysis

This document records the evidence collected while testing Obscura against
Wildberries and Ozon. It is an investigation snapshot, not a promise that a
site will keep the same protection code or policy.

The work is limited to normal browser compatibility and a consistent browser
identity. It does not include challenge tokens, proxy credentials, or a recipe
for bypassing access controls.

## Evidence labels

- **Proved** means the result came from a saved response, a raw request trace,
  a source-level test, or an interleaved Chrome and Obscura run.
- **Inferred** means the evidence supports the explanation, but the server does
  not expose the decision rule.
- **Open** means a controlled test is still needed.

## Upstream audit

The current branch already contains the fetched upstream head used for this
investigation. The profile header, UAData, Canvas2D, WebGL reflection, and
Chrome 151 object-shape fixes described below are not duplicate upstream code.
The older fork still contains separate frame-realm and Turnstile work that has
not yet been rebuilt here; that is a different plan stage and must not be mixed
with the Ozon fingerprint diagnosis.

## Test controls

Live anti-bot results are easy to invalidate. Use all of these controls:

1. Start each engine with a new browser profile.
2. Use headed Chrome as the control. Do not use Playwright or Puppeteer for the
   control, because they enable CDP Runtime domains and add automation launch
   signals.
3. Drive Chrome through raw CDP without `Runtime.enable`.
4. Clear upper- and lower-case `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, and
   `NO_PROXY`, plus `OBSCURA_PROXY` and `OBSCURA_PROFILE`, before applying an
   explicit route or profile. Inherited proxy variables caused several invalid
   early comparisons.
5. Load `https://ipv6.one/` in the tested engine before the site. Record the
   address family, exit address, ASN, country, and city. Reject the run if it
   does not use the expected route.
6. Pin one Obscura profile. Do not rotate identity during a comparison.
7. Compare the same route, time window, page, and settle policy.
8. Close every Chrome control through `Browser.close` and remove its temporary
   profile directory. Reusing a profile changes both storage and server history.

Avito is not part of the current gate. A fresh real Chrome profile is rejected
on the available route, so an Avito failure cannot separate an engine defect
from an IP policy decision.

### Route controls

The current direct route leaves through the expected `2001:470:...` IPv6 in
Moscow. Wildberries accepts that route. Two tested explicit proxy endpoints
left through datacenter addresses in the United States. Both headed Chrome and
Obscura received the same initial Ozon rejection on those endpoints. They are
useful controls for proxy wiring, but not for comparing browser engines.

**Proved:** HTTP and SOCKS forms of the same tested endpoint produced the same
exit address. The explicit proxy was visible only after inherited proxy
variables were cleared in the child process.

## Ozon request flow

The observed flow has two protection stages.

### Stage 1: edge classification

On the last valid direct comparison route, headed Chrome 151 received the real
Ozon page with HTTP status 200. Obscura received an HTTP 403 challenge document
for the same top-level URL. This result must not be combined with the later
proxy controls, where both engines received the initial rejection.

This decision happens before page JavaScript runs. It can use the network route,
TLS ClientHello, ALPN, HTTP version, HTTP/2 settings and order, request headers,
cookies, and server history. The server does not expose the weight of each
signal.

**Proved for the recorded direct run:** the initial status differs before the
challenge VM runs. This is a challenge-routing difference, not a final access
failure. After the compatibility fix below, Obscura completes that challenge
and reaches the real HTTP 200 product page.

**Inferred:** a transport or request-identity signal makes Obscura enter the
challenge path. This is not yet proof that one exact TLS field is responsible.

### Stage 2: JavaScript challenge and server result

The 403 document contains an obfuscated inline program of about 112 KiB. The
program builds a small VM, gathers browser data, and sends a JSON result to
`/abt/result`. The observed body has five top-level fields:

- `error`
- `fp`
- `info`
- `timings`
- `token`

The fingerprint field is an opaque encrypted envelope. Tests record only its
length and digest. Tokens and full payloads must not be committed.

The server response, not a local JavaScript branch, selects the final page. The
failing Obscura sequence was:

```text
VM completes -> result POST accepted -> Location.replace(URL) throws
             -> Ozon catch handler sends an error report -> generic failure page
```

The value-safe live trace proved that the first request was a normal result: it
had a 27,100-character fingerprint, a 3,402-character token, an empty `error`
field, and received HTTP 200. The second request was a smaller error report. Its
recorded exception was `url.startsWith is not a function`, from `_resolveUrl`
inside `Location.replace()`.

Ozon passes a `URL` object to `Location.replace()`. Chrome applies Web IDL
string conversion. Obscura passed the object to `_resolveUrl`, which called
`.startsWith()` directly. `location.href`, `location.assign()`, and
`location.replace()` now convert their input to a string before URL resolution.
A source-level regression test covers `Location.replace(new URL(...))`.

The final direct run used the expected `2001:470:...` IPv6, ASN 6939 in Moscow.
The challenge result received HTTP 200, the following document received HTTP
200, five product links loaded, and one product card opened. No second error
report was sent.

The final page says that there is a connection problem and asks the user to
enable JavaScript. That text is a generic failure view. It is not evidence that
JavaScript is disabled: the VM already ran and submitted a result before this
view appeared.

## What the Ozon program measures

The names below describe observed behavior, not stable public API names. The
program is obfuscated and can change.

### Function and accessor shape

The serializer checks more than values. For accessors it observes the source
returned by `Function.prototype.toString` and whether a bare getter call throws.
A normal browser accessor looks native and rejects a missing or wrong receiver
with an illegal-invocation error.

Early Obscura getters looked native but returned values when called without a
receiver. That made the serializer emit special getter records. Branded,
non-constructable accessors reduced those records to zero for Navigator and
Performance.

**Proved:** this changes the submitted fingerprint while leaving the challenge
`error` field empty.

### Browser object graphs

The serializer walks own properties and prototypes with a depth limit. It sees
the difference between a plain object and an interface such as `Screen`,
`VisualViewport`, `BatteryManager`, `PerformanceTiming`, or
`PerformanceNavigation`.

Measured fixes added browser-like prototype chains, illegal constructors,
native-shaped methods, branded receivers, and expected event-target parents.
The exact Chrome 151 secure-context probe reports 83 own members on
`Navigator.prototype`. Obscura now reports the same names in the same order.
The complete measured Navigator graph is 13,531 JSON bytes in each engine with
zero structural differences. The added members are version-gated to Chrome 151
and secure-context exposure follows the headed control.

Nine existing child objects were also plain objects with own methods. Clipboard,
CredentialsContainer, Geolocation, Keyboard, LockManager, MediaCapabilities,
ServiceWorkerContainer, StorageManager, and WakeLock now use branded interface
prototypes. PluginArray and MimeTypeArray now have Chrome-style indexed and
named own properties, prototype `length`, and an Object parent.

Unversioned APIs must not be added only to match a newer trace. The current work
uses a headed Chrome 151 secure loopback control because the selected profile is
also Chrome 151. A `data:` probe is not an equivalent control: Chrome hides many
powerful APIs there because it is not a secure context.

### DOM behavior

The program calls `document.createEvent("TouchEvent")` through a try/catch
helper. Chrome rejects this unsupported event name in the tested desktop
context. Obscura used to create it successfully. Obscura now throws
`NotSupportedError`, matching the control result.

### Canvas and WebGL

The program renders a known 2D scene and a small WebGL shader, serializes each
canvas as PNG, and hashes the data URL. This detects empty canvases, fake
constant output, missing drawing operations, uncompressed PNG output, and
different WebGL pixels.

Several separate defects were proved:

- PNG image data was stored without useful compression, producing a much larger
  data URL than Chrome.
- WebGL `drawArrays` produced per-pixel hash noise instead of the simple shader
  gradient used by the challenge.
- Canvas2D text used pseudo-random 5x7 blocks and a fixed character-width
  formula instead of font glyphs and real metrics.
- Canvas path `rect()` painted immediately, while Chrome records it until
  `fill()`, and repeated `fill()` keeps the current path.
- DOM canvas wrappers exposed engine fields such as `_style` and `_nid` through
  reflection. The WebGL facade also called page-hooked `JSON.stringify` while
  computing internal digests.

Obscura now uses zlib compression and a narrow software raster path for the
observed simple gradient program. Unsupported shaders keep the existing
fallback. The WebGL data URL moved from about 206 KiB to about 3.6 KiB; the
Chrome control was about 4.5 KiB.

Canvas2D now rasterizes bundled deterministic font glyphs through the render
crate, supports point-to-pixel font canonicalization, and uses measured text
widths. The Ozon 2D scene moved from about 5.2 KiB to about 9.0 KiB; Chrome is
about 35.8 KiB. The remaining gap is mainly the bundled font set and incomplete
Canvas2D paint behavior, not a stopped script. The WebGL context graph now has
the same 83-byte serialized canvas back-reference shape as Chrome, and internal
WebGL work no longer appears in a hook on `JSON.stringify`.

### Other observed groups

The payload also includes navigation timing, RTC codec capabilities, fonts,
plugins and MIME types, screen and window geometry, hardware values, client
hints, WebGL renderer data, and several small browser-presence checks.

The selected workbench profile now supplies JavaScript UA brands, full-version
client hints, screen values, hardware values, GPU values, and the outbound
`Accept-Language` header. Captured WebGL extension order is preserved because
Chrome returns an ordered sequence; sorting it changed the profile identity.
Performance entries now include Chrome 151's `navigationId` shape, and
`chrome.app`, `chrome.csi`, and `chrome.loadTimes` match direct headed Chrome
measurements.

The first saved Chrome 151 capture was created before extension-order
preservation was fixed, so its old profile id still names the sorted sequence.
Do not silently rewrite that runtime sidecar. A new capture/save produces the
correct sequence and a new id; comparisons must state which saved row they use.

Not every difference is a defect. Values must be filtered by Chrome version,
feature state, locale, hardware, and profile before code is changed.

## Transport findings

Obscura builds the stealth client from the selected fingerprint profile. The
same profile supplies JavaScript values, User-Agent, client hints, and the
transport selector. Scripted fetch and XHR requests also use the stealth client
when stealth mode is active.

The lock currently uses `wreq 6.0.0-rc.29` with `wreq-util 3.0.0-rc.12`.
`wreq-util` has browser profiles through Chrome 148, while the selected runtime
profile is Chrome 151. Updating only a profile label is not expected to change
the network fingerprint when the underlying TLS and HTTP/2 builder is shared.
Obscura already replaces the default UA and client-hint headers with selected
profile values.

**Rejected fix:** changing the dependency only to make the transport profile
name one version newer. It does not supply new TLS or HTTP/2 data.

The successful Chrome control used HTTP/3. The current wreq path uses HTTP/2
over TLS and does not provide a Chrome QUIC transport. The final Obscura run
still passed Ozon after completing the challenge, so QUIC is not required for
access on the recorded route. It may still be one input to the initial
challenge-routing decision.

One general consistency gap remains outside the observed Ozon POST: a
cross-origin scripted request that needs a CORS preflight sends that OPTIONS
request through the regular client before the stealth request path is selected.
The Ozon result POST is same-origin and does not use this path, so this is a
separate transport issue, not part of the recorded Ozon result flow.

## CDP network visibility

The JavaScript fetch path did make both observed `/abt/result` requests, but an
early raw-CDP listener saw neither. Engine tracing proved the calls before the
CDP reporting bug was isolated.

Completed scripted requests were drained once and emitted to the first session
found for the page. A managed page session and a later explicit flattened
attachment can both be valid, so a listener on the other session missed the
events. Runtime network events are now emitted to every session attached to the
page. A focused two-session test checks all three events:
`Network.requestWillBeSent`, `Network.responseReceived`, and
`Network.loadingFinished`.

The final live Ozon trace saw the same accepted result request on both
`page-1-session` and `page-1-session-1`. Events are still completion-backed:
`requestWillBeSent` is synthesized after the response is available. True
request-start streaming remains separate work.

## Wildberries request and render flow

Wildberries now passes the exact live product-page gate with the pinned Chrome
151 profile on the valid direct route. This is stronger than the earlier home
page result: the product API returns data, the product Module Federation bundle
runs, React mounts the product tree, and the final title and body contain the
requested product id.

### Protection and server result

The final Wildberries document served to headed Chrome and Obscura was byte for
byte equal in the controlled run: 24,435 bytes with SHA-256
`9C6AD0356FFD8E6889F2F1FD049DBB5FDEE18BCF334010AA3C38AC74F403EE11`.
Wildberries returned HTTP 200 for the document, all watched site and product
chunks, and `__internal/u-card/cards/v4/detail`.

The product remote loaded 18 chunks in both engines. Its `get()` calls resolved
`./I18nResources`, `./ProductPage`, and `./ProductPageViewModel`; all three
factories ran, and the first `ProductPage` render returned a valid React
element. This proves the protection and server path had already accepted the
page. The remaining failure was in local DOM compatibility.

An earlier Wildberries defect was transport decoding: compressed script bodies
could reach the parser without gzip, Brotli, zstd, or deflate decoding. The
stealth client now keeps advertised encodings and response decoding in sync.

### Exact product mount failure

Before the slot fix, headed Chrome put about 400 KiB of product markup under
`#appReactRoot`. Obscura left that root empty and kept the visible page on its
generic loading or connection view. The view can ask the user to enable
JavaScript, but JavaScript was running: the product remote and API had already
finished.

A value-safe startup probe found the first real render error:

```text
TypeError: e.assignedElements is not a function
```

The saved current product chunk contains this call:

```javascript
e instanceof HTMLSlotElement && i.push(...e.assignedElements())
```

Obscura had set `HTMLSlotElement = Element`. As a result, every HTML element
passed the `instanceof HTMLSlotElement` check, but no element had
`assignedElements()`. This was both a wrong interface identity and a missing
Web Components API. It was not a server block and not disabled JavaScript.

The fix adds a real `HTMLSlotElement` class and maps parsed and created `slot`
elements to it. It also adds:

- reflected `Element.slot` and `HTMLSlotElement.name` properties;
- named, default, flattened fallback, and manual slot assignment;
- `assignedNodes()`, `assignedElements()`, and `assign()`;
- `Element.assignedSlot` and `Text.assignedSlot`, which the saved WB product
  chunk also reads.

The current saved chunk does not contain a `slotchange` listener. That event is
still a wider Web Components item and was not added as a guessed WB fix.

### Wildberries proof

Three fresh page-level runs passed after the main fix. Each run used the pinned
Chrome 151 profile and the accepted direct exit
`2001:470:df80:0:b9f4:5900:a29e:6f6b`, ASN 6939 in Moscow. The route control
reported bot score 99. Each result had the exact RTX 5060 Ti product title,
product id `1193913221`, one mounted React-root child, and about 62.8 KiB of
root HTML. A fresh run after adding `assignedSlot` passed the same gate.

Several other attempts ended with Windows socket error `10054` or a connect
timeout before the main document arrived. These runs are transport failures,
not protection results, because no Wildberries page code ran. They are not part
of the three page-level results.

The release-mode Rust live product smoke also passes with the Chrome 151 runtime
profile. Its helper now follows the CDP server rule and recovers from up to
three bounded heavy browser tasks instead of failing on the first task-budget
guard.

Wildberries passing does not prove Ozon must pass. It proves that the selected
identity, transport, JavaScript, product data, Module Federation, React, and the
slot APIs used by this product page work together on the recorded route.

## Current result table

| Gate | Result | Meaning |
| --- | --- | --- |
| Wildberries product, valid route, pinned Chrome 151 profile | Pass | Exact product title/id and mounted React root; 3/3 page-level runs |
| Ozon challenge offline replay | Pass | VM completes and submits with empty local error |
| Ozon live, pinned Chrome 151 profile | Pass | Challenge accepted, products load, product card opens |
| Headed Chrome, recorded valid direct route | Pass | Real Ozon page and products load |
| Headed Chrome and Obscura, tested proxy routes | Same initial block | Route is not an engine oracle |
| Avito | Excluded | Fresh Chrome is also rejected on the available route |

## Next controlled tests

Run these in order:

1. Keep the expected route check in both engines with `https://ipv6.one/`.
2. Keep the exact Wildberries product id, title, and mounted React root as the
   WB gate. Do not count a pre-document socket reset as a protection result.
3. Keep the pinned Chrome 151 profile and the `Location.replace(URL)` regression
   in the live Ozon gate.
4. Repeat the accepted Ozon flow over several fresh contexts to separate a
   stable compatibility result from server-history noise.
5. Add a headed Chrome `--disable-quic` control only if the initial challenge
   routing difference needs to be removed.
6. Keep offline challenge replay as the JavaScript regression gate.
7. Track the remaining Canvas2D and bundled-font pixel gap separately; it did
   not stop the recorded Ozon pass.

The raw harness supports these controls:

```bash
node tools/ab/chrome-raw.mjs --site wb --url https://www.wildberries.ru/catalog/1193913221/detail.aspx?targetUrl=MI --cards 0 --only obscura --clean-host --profile PROFILE_ID --profile-workbench-dir webgl --wait 35
node tools/ab/chrome-raw.mjs --site ozon --only chrome --headed --clean-host --trace-network
node tools/ab/chrome-raw.mjs --site ozon --only chrome --headed --clean-host --disable-quic --trace-network
node tools/ab/chrome-raw.mjs --site ozon --only obscura --clean-host --profile PROFILE_ID --trace-challenge --trace-network
node tools/ab/chrome-raw.mjs --site ozon --only obscura --profile PROFILE_ID --replay SAVED_RESPONSE --trace-replay-helpers
```

Do not accept a live result if the IP check is wrong, if Chrome was headless, if
the Chrome profile was reused, or if an automation library enabled extra CDP
domains.

## Main conclusion

The Ozon failure was not disabled JavaScript and was not rejection of the main
fingerprint result. The server accepted that result. Obscura then failed a
normal browser API operation because `Location.replace()` did not apply string
conversion to a `URL` object. Ozon correctly caught that exception and sent the
error report which led to its generic failure page.

With the location conversion fixed, Ozon passes on the valid direct route. WB
also passes its exact product gate after replacing the false
`HTMLSlotElement = Element` alias with the slot APIs used by its product code.
The CDP trace now reports scripted requests to every attached page session.
Route, fresh-context, pinned-profile, and headed-control rules still apply to
future live claims.
