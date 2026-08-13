// How far a Turnstile widget gets, in real Chrome and in Obscura.
//
//   node tools/ab/turnstile.mjs [url|local] [--only chrome|obscura]
//                              [--wait seconds] [--proxy url] [--headed]
//                              [--outbound] [--child-trace] [--click]
//                              [--verbose <RUST_LOG>]
//                              [--profile-workbench-dir path] [--profile id]
//
// "local" serves a fixture using Turnstile's dummy sitekey, which always issues
// a token without interaction. That separates the two questions a live site
// answers at once: whether the engine can mechanically complete the flow, and
// whether Cloudflare's risk engine trusts this browser and IP. A live failure
// cannot tell them apart, and guessing which one happened is how the previous
// round of this work lost a day.
//
// Body text says nothing here: the widget is an iframe and renders no text, so
// a page that fully solved and a page that never started look identical from
// document.innerText. What separates them is the token, which Turnstile writes
// into <input name="cf-turnstile-response">. That input being non-empty is the
// only real definition of passing, and it is what this reports.
//
// The stages in between are reported too, because they say *where* it stopped:
// the api.js script loading, window.turnstile appearing, the challenge iframe
// being created, and that iframe having a document with something in it.

import http from 'node:http';
import { runIn, tryEvaluate, evaluated } from './engines.mjs';

// Turnstile's "always passes" test sitekey. Documented as a dummy: it still
// loads the real challenge iframe from Cloudflare, so the whole pipeline runs,
// but it issues a token instead of scoring the visitor.
const DUMMY_SITEKEY = '1x00000000000000000000AA';

function pushBounded(items, value, limit = 500) {
  if (items.length >= limit) items.shift();
  items.push(value);
}

const FIXTURE = `<!doctype html>
<html><head><meta charset="utf-8"><title>turnstile fixture</title>
<script src="https://challenges.cloudflare.com/turnstile/v0/api.js" async defer></script>
</head><body>
<form><div class="cf-turnstile" data-sitekey="${DUMMY_SITEKEY}"></div></form>
</body></html>`;

/// Serves the fixture on a free port. Returns its URL and a stop function.
function serveFixture() {
  return new Promise(done => {
    const server = http.createServer((_request, response) => {
      response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
      response.end(FIXTURE);
    });
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      done({ url: `http://127.0.0.1:${port}/`, stop: () => server.close() });
    });
  });
}

function parseArgs(argv) {
  const opts = { wait: 30, headed: false };
  const rest = [];
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--headed') opts.headed = true;
    else if (arg === '--click') opts.click = true;
    // Obscura's own log is the only view into what a frame realm did; nothing
    // about a frame that failed to run reaches CDP.
    else if (arg === '--verbose') {
      opts.env = { RUST_LOG: argv[++i] || 'obscura_browser=debug,obscura_js=debug' };
      opts.onStderr = chunk => process.stderr.write(chunk);
    }
    else if (arg === '--outbound') opts.outbound = true;
    else if (arg === '--child-trace') opts.childTrace = true;
    else if (arg === '--only') opts.only = argv[++i];
    else if (arg === '--wait') opts.wait = Number(argv[++i]);
    else if (arg === '--proxy') opts.proxy = argv[++i];
    else if (arg === '--profile-workbench-dir') opts.profileWorkbenchDir = argv[++i];
    else if (arg === '--profile') opts.profile = argv[++i];
    else rest.push(arg);
  }
  opts.url = rest[0] || 'https://turnstile-test.vercel.app/';
  return opts;
}
const opts = parseArgs(process.argv.slice(2));
if (process.env.OBSCURA_AB_STDERR === '1') {
  opts.onStderr = data => process.stderr.write(data);
}

// Runs in the page. Everything is read defensively: a missing shim must read as
// "absent", not throw and lose the whole report.
function probe() {
  const out = { iframes: [] };
  out.hasApiScript = [...document.querySelectorAll('script[src]')]
    .some(s => s.src.includes('turnstile') && s.src.includes('api.js'));
  out.hasTurnstileGlobal = typeof globalThis.turnstile;
  out.widgets = document.querySelectorAll('.cf-turnstile,[data-sitekey]').length;

  const input = document.querySelector('input[name="cf-turnstile-response"]');
  out.tokenInput = input ? 'present' : 'absent';
  out.token = input && input.value ? input.value : '';
  out.viewport = [innerWidth, innerHeight, outerWidth, outerHeight, devicePixelRatio];
  out.screen = [screen.width, screen.height, screen.availWidth, screen.availHeight];

  // Turnstile puts its iframe inside a *closed* shadow root, which
  // querySelectorAll correctly cannot pierce — so searching the light DOM alone
  // reports zero iframes whether the widget worked or never started. The init
  // script recorded every root as it was attached; look there too.
  const roots = globalThis.__abShadowRoots || [];
  out.shadowRoots = roots.length;
  out.messages = (globalThis.__abMessages || []).slice(0, 60);
  out.messageRecords = (globalThis.__abMessageRecords || []).slice(-100);
  const diagnosticEntries = (globalThis.__abChildDiagnostics || [])
    .filter(entry => /^(diag|gpu|ua-high|fetch|fetch-response|response-read|message-in|worker(?:-state|-construct|-post|-message|-error)?|worker-create-error|console-error|child-error|child-unhandled-rejection|error|unhandledrejection|privacy-api|iframe-|image-|element-innerhtml|document-(open|write|writeln|close)|(?:input|dispatch|prepare)-|shadow-(?:pointer|mouse|click|input|change)|interactiveBegin|overrunBegin|late-diag)/.test(entry.event || ''));
  // Keep early child failures visible even when the challenge emits many
  // repetitive worker-state records afterwards.
  const earlyErrors = diagnosticEntries.filter(entry => /^(console-error|child-error|child-unhandled-rejection|worker-error|worker-create-error|error|unhandledrejection)$/.test(entry.event || ''));
  const gpuEntries = diagnosticEntries.filter(entry => /^(gpu|gpu-error)$/.test(entry.event || ''));
  out.childDiagnostics = [...new Map([...earlyErrors, ...gpuEntries, ...diagnosticEntries.slice(-80)]
    .map((entry, index) => [index + ':' + JSON.stringify(entry), entry])).values()];
  const late = [...(globalThis.__abChildDiagnostics || [])].reverse().find(entry => entry.event === 'late-diag');
  out.iframeEvents = (globalThis.__abChildDiagnostics || [])
    .filter(entry => /^iframe-/.test(entry.event || ''))
    .slice(-24);
  out.imageEvents = (globalThis.__abChildDiagnostics || [])
    .filter(entry => /^image-/.test(entry.event || ''))
    .slice(-24);
  out.fetchShapes = (globalThis.__abChildDiagnostics || [])
    .filter(entry => entry.event === 'fetch' && entry.bodyLength)
    .map(entry => ({
      url: String(entry.url || '').split('/').slice(-3).join('/'),
      method: entry.method, bodyLength: entry.bodyLength, bodyHash: entry.bodyHash,
      dotParts: entry.bodyShape?.dotParts,
      dollarParts: entry.bodyShape?.dollarParts,
      inputType: entry.inputType,
      initHeaderNames: entry.initHeaderNames,
      viewport: entry.viewport,
    }));
  const diagEntry = [...(globalThis.__abChildDiagnostics || [])]
    .reverse().find(entry => entry.event === 'diag');
  const uaHigh = [...(globalThis.__abChildDiagnostics || [])]
    .reverse().find(entry => entry.event === 'ua-high');
  out.childSurface = diagEntry ? {
    obscuraFrameId: diagEntry.obscuraFrameId, origin: diagEntry.origin,
    readyState: diagEntry.readyState, viewport: diagEntry.viewport,
    windowPosition: diagEntry.windowPosition, timezone: diagEntry.timezone,
    hasCrypto: diagEntry.hasCrypto, cryptoSubtle: diagEntry.cryptoSubtle,
    hasTextEncoder: diagEntry.hasTextEncoder, hasURL: diagEntry.hasURL,
    hasPerformance: diagEntry.hasPerformance, hasVisualViewport: diagEntry.hasVisualViewport,
    hasFonts: diagEntry.hasFonts, hasOffscreenCanvas: diagEntry.hasOffscreenCanvas,
    hasWebGL: diagEntry.hasWebGL, hasWebGPU: diagEntry.hasWebGPU,
    isSecureContext: diagEntry.isSecureContext, crossOriginIsolated: diagEntry.crossOriginIsolated,
    visibility: diagEntry.visibility, webdriver: diagEntry.webdriver,
    vendor: diagEntry.vendor, hardwareConcurrency: diagEntry.hardwareConcurrency,
    deviceMemory: diagEntry.deviceMemory, maxTouchPoints: diagEntry.maxTouchPoints,
    cookieEnabled: diagEntry.cookieEnabled, screen: diagEntry.screen,
    plugins: diagEntry.plugins, mimeTypes: diagEntry.mimeTypes,
     navigatorPermissions: diagEntry.navigatorPermissions,
     navigatorMediaDevices: diagEntry.navigatorMediaDevices,
     cryptoRandomUUID: diagEntry.cryptoRandomUUID,
     storageAccess: diagEntry.storageAccess,
     navigatorApis: diagEntry.navigatorApis,
    chromeKeys: diagEntry.chromeKeys, privateToken: diagEntry.privateToken,
     challengeApis: diagEntry.challengeApis, notification: diagEntry.notification,
     permissions: diagEntry.permissions, permissionQuery: diagEntry.permissionQuery,
    trustedTypes: diagEntry.trustedTypes,
    featurePolicy: diagEntry.featurePolicy,
    featurePolicyFeatures: diagEntry.featurePolicyFeatures,
    workers: diagEntry.workers, observers: diagEntry.observers,
    userAgent: diagEntry.userAgent, platform: diagEntry.platform,
    languages: diagEntry.languages, evalShape: diagEntry.evalShape,
    dateNowShape: diagEntry.dateNowShape, webdriverDescriptor: diagEntry.webdriverDescriptor,
    uaData: diagEntry.uaDataShape,
    uaHigh: uaHigh?.value || uaHigh?.error || null,
  } : null;
  out.challengeUi = late?.bodyShadow ? {
    children: late.bodyShadow.children,
    elements: (late.bodyShadow.elements || []).map(element => ({ tag: element[0], id: element[1], rect: element[4] })),
    images: late.bodyShadow.images || [],
    iframes: late.bodyShadow.iframes || [],
  } : null;
  out.contentWindowDescriptor = globalThis.__abContentWindowDescriptor || 'unknown';
  out.outboundInstalled = globalThis.__abOutboundInstalled === true;
  out.frameTable = Object.entries(globalThis.__obscura_frameElements || {}).map(([id, frame]) => {
    try {
      const rect = frame?.getBoundingClientRect?.();
      const child = globalThis.__obscura_frameWindows?.[id];
      return { id, nid: frame?._nid, rect: rect && [rect.x, rect.y, rect.width, rect.height],
        child: child && { inner: [child.innerWidth, child.innerHeight], outer: [child.outerWidth, child.outerHeight] } };
    } catch (error) { return { id, error: String(error).slice(0, 200) }; }
  });
  const frames = [...document.querySelectorAll('iframe')];
  for (const root of roots) {
    try { frames.push(...root.querySelectorAll('iframe')); } catch { /* gone */ }
  }

  for (const frame of frames) {
    const entry = { src: (frame.getAttribute('src') || '').slice(0, 120) };
    entry.connected = frame.isConnected;
    // Cross-origin in a real browser, so this throws there and must not abort
    // the rest of the report.
    try {
      const doc = frame.contentDocument;
      entry.doc = doc ? 'reachable' : 'null';
      if (doc && doc.body) entry.docBodyChars = doc.body.innerHTML.length;
    } catch { entry.doc = 'cross-origin (correct)'; }
    try { entry.win = frame.contentWindow ? typeof frame.contentWindow.postMessage : 'null'; }
    catch { entry.win = 'cross-origin (correct)'; }
    // Obscura-only bookkeeping: whether the frame document was fetched, and
    // whether it was given a realm. Absent in Chrome, which is the point — it
    // says where an Obscura-side frame stopped.
    if (frame._iframeLoadInfo) entry.loadInfo = JSON.stringify({ ...frame._iframeLoadInfo, url: undefined });
    if (frame._frameId !== undefined) entry.frameId = frame._frameId;
    out.iframes.push(entry);
  }
  return out;
}

async function scenario(page) {
  // Must be installed before any page script runs, so it catches the root
  // Turnstile attaches. A closed root cannot be reached any other way.
  await page.addInitScript(captureOutbound => {
    const pushBounded = (items, value, limit = 500) => {
      if (items.length >= limit) items.shift();
      items.push(value);
    };
    globalThis.__abShadowRoots = [];
    const attach = Element.prototype.attachShadow;
    Element.prototype.attachShadow = function (init) {
      const root = attach.call(this, init);
      pushBounded(globalThis.__abShadowRoots, root, 256);
      return root;
    };

    // The widget's whole conversation with its frame, both directions. Which
    // message an engine fails to send or answer is the actual difference
    // between passing and not; the DOM afterwards only shows that it did not.
    globalThis.__abMessages = [];
    globalThis.__abMessageRecords = [];
    globalThis.__abChildDiagnostics = [];
    globalThis.__abInputEvents = [];
    for (const type of ['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click']) {
      document.addEventListener(type, event => pushBounded(globalThis.__abInputEvents, {
        type, trusted: event.isTrusted, target: event.target && event.target.tagName,
        clientX: event.clientX, clientY: event.clientY, screenX: event.screenX, screenY: event.screenY,
        pageX: event.pageX, pageY: event.pageY, offsetX: event.offsetX, offsetY: event.offsetY,
        movementX: event.movementX, movementY: event.movementY, button: event.button,
        buttons: event.buttons, detail: event.detail, composed: event.composed,
        viewIsWindow: event.view === window,
      }), true);
    }
    const label = data => {
      try {
        if (typeof data === 'string') return `"${data.slice(0, 60)}"`;
        const { event, source, widgetId, ...rest } = data || {};
        const extra = Object.keys(rest).slice(0, 4).join(',');
        return `${event || '?'}${extra ? ` {${extra}}` : ''}`;
      } catch { return '<unreadable>'; }
    };
    addEventListener('message', e => {
      pushBounded(globalThis.__abMessages, `in  ${label(e.data)}  src=${e.source ? 'yes' : 'NONE'}`);
      const frames = [];
      try {
        for (const root of globalThis.__abShadowRoots || []) frames.push(...root.querySelectorAll('iframe'));
        frames.push(...document.querySelectorAll('iframe'));
      } catch (_) {}
      pushBounded(globalThis.__abMessageRecords, {
        event: e.data?.event || null,
        keys: e.data && typeof e.data === 'object' ? Object.keys(e.data).sort() : [],
        origin: e.origin,
        sourcePresent: !!e.source,
        sourceMatchesContentWindow: frames.some(frame => {
          try { return e.source === frame.contentWindow; } catch (_) { return false; }
        }),
      });
      if (e.data && e.data.source === 'ab-child-diag') {
        pushBounded(globalThis.__abChildDiagnostics, e.data);
        if (/^(prepare|dispatch|shadow-dispatch|input-)/.test(e.data.event || '')) {
          console.log('CHILD_DIAG ' + JSON.stringify(e.data));
        }
      }
    });

    // Outbound needs the iframe's window intercepted. A cross-origin
    // contentWindow cannot be read from, but postMessage on it can be wrapped.
    //
    // Off by default, and deliberately: wrapping contentWindow in a Proxy stops
    // real Chrome from ever issuing a token, so a run with this on cannot be
    // read as a pass or a fail — only as a record of the sequence.
    let descriptor;
    let descriptorOwner;
    for (let proto = HTMLIFrameElement.prototype; proto && !descriptor; proto = Object.getPrototypeOf(proto)) {
      descriptor = Object.getOwnPropertyDescriptor(proto, 'contentWindow');
      if (descriptor) descriptorOwner = proto;
    }
    globalThis.__abContentWindowDescriptor = descriptor
      ? `${typeof descriptor.get}/${typeof descriptor.set}` : 'absent';
    if (captureOutbound && descriptor && descriptor.get) {
      globalThis.__abOutboundInstalled = true;
      Object.defineProperty(descriptorOwner, 'contentWindow', {
        configurable: true,
        get() {
          const win = descriptor.get.call(this);
          if (!win) return win;
          if (this.localName !== 'iframe') return win;
          return new Proxy(win, {
            get(target, prop) {
              if (prop === 'postMessage') {
                return function (data, ...rest) {
                  pushBounded(globalThis.__abMessages, `out ${label(data)}`);
                  return target.postMessage(data, ...rest);
                };
              }
              const value = target[prop];
              return typeof value === 'function' ? value.bind(target) : value;
            },
          });
        },
      });
    }
  }, opts.outbound || false);

  if (opts.childTrace) {
    await page.addInitScript(() => {
      if (globalThis.parent === globalThis) return;
      const report = data => {
        try { globalThis.parent.postMessage({ source: 'ab-child-diag', ...data }, '*'); } catch (_) {}
      };
      addEventListener('error', event => report({
        event: 'child-error', message: String(event.message || event.error || '').slice(0, 300),
      }));
      addEventListener('unhandledrejection', event => report({
        event: 'child-unhandled-rejection', reason: String(event.reason || '').slice(0, 300),
      }));
      addEventListener('message', event => {
        const data = event.data;
        report({ event: 'message-in', messageEvent: data?.event || null,
          source: data?.source || null, keys: data && typeof data === 'object' ? Object.keys(data).sort() : [],
          wPr: data?.wPr || null, origin: event.origin,
          sourcePresent: !!event.source });
      });
      const originalFetch = globalThis.fetch;
      if (typeof originalFetch === 'function') {
        globalThis.fetch = function (input, ...args) {
          try {
            const url = typeof input === 'string' ? input : input?.url;
            const init = args[0] && typeof args[0] === 'object' ? args[0] : {};
            const body = init.body;
            let bodyHash = null;
            if (typeof body === 'string') {
              bodyHash = 2166136261;
              for (let i = 0; i < body.length; i++) {
                bodyHash ^= body.charCodeAt(i);
                bodyHash = Math.imul(bodyHash, 16777619) >>> 0;
              }
              bodyHash = bodyHash.toString(16);
            }
            const headerNames = value => {
              try {
                if (value && typeof value.entries === 'function') {
                  return Array.from(value.entries()).map(([name]) => name);
                }
                return value && typeof value === 'object' ? Object.keys(value) : [];
              } catch (_) { return []; }
            };
            report({ event: 'fetch', url: String(url || '').slice(0, 240),
              method: init.method || 'GET', bodyType: typeof body,
              bodyLength: typeof body === 'string' ? body.length : null,
              viewport: (() => { try { return [innerWidth, innerHeight, outerWidth, outerHeight]; } catch (_) { return null; } })(),
              bodyShape: typeof body === 'string' ? {
                prefix: body.slice(0, 48), suffix: body.slice(-48),
                dollarParts: body.split('$').map(part => part.length),
                dotParts: body.split('.').map(part => part.length),
              } : null,
              bodyHash, bodyBytes: body instanceof Uint8Array ? body.byteLength : null,
              inputType: input?.constructor?.name || typeof input,
              inputHeaderNames: headerNames(input?.headers),
              initHeaderNames: headerNames(init.headers),
              headers: (() => { try {
                return init.headers && typeof init.headers.entries === 'function'
                  ? Object.fromEntries(init.headers.entries()) : init.headers || {};
              } catch (_) { return {}; } })() });
          } catch (_) {}
          const pending = originalFetch.call(this, input, ...args);
          return Promise.resolve(pending).then(response => {
            report({ event: 'fetch-response', status: response?.status,
              body: typeof response?.body, reader: typeof response?.body?.getReader,
              arrayBuffer: typeof response?.arrayBuffer, text: typeof response?.text });
            return response;
          });
        };
      }
      const originalCreateElement = Document.prototype.createElement;
      Document.prototype.createElement = function (name, ...args) {
        const element = originalCreateElement.call(this, name, ...args);
        if (String(name).toLowerCase() === 'iframe') {
          report({ event: 'iframe-create', src: element.getAttribute?.('src') || '' });
        }
        if (String(name).toLowerCase() === 'img') {
          report({ event: 'image-create' });
        }
        return element;
      };
      for (const method of ['open', 'write', 'writeln', 'close']) {
        const original = Document.prototype[method];
        if (typeof original !== 'function') continue;
        Document.prototype[method] = function (...args) {
          report({ event: `document-${method}`, args: args.map(value => String(value).slice(0, 180)),
            frameId: globalThis.__obscura_frameId ?? null });
          return original.apply(this, args);
        };
      }
      const iframeProto = globalThis.HTMLIFrameElement?.prototype;
      const originalLoadIframeSrc = iframeProto?._loadIframeSrc;
      if (typeof originalLoadIframeSrc === 'function') {
        iframeProto._loadIframeSrc = function (url, ...args) {
          report({ event: 'iframe-load-start', url: String(url || '') });
          return originalLoadIframeSrc.call(this, url, ...args);
        };
      }
      if (iframeProto) {
        const srcdocDescriptor = Object.getOwnPropertyDescriptor(iframeProto, 'srcdoc');
        if (srcdocDescriptor?.set) Object.defineProperty(iframeProto, 'srcdoc', {
          configurable: srcdocDescriptor.configurable, enumerable: srcdocDescriptor.enumerable,
          get: srcdocDescriptor.get,
          set(value) {
            const html = String(value ?? '');
            report({ event: 'iframe-srcdoc-set', length: html.length,
              html, frameId: globalThis.__obscura_frameId ?? 0 });
            return srcdocDescriptor.set.call(this, value);
          },
        });
      }
      const originalAppendChild = Node.prototype.appendChild;
      Node.prototype.appendChild = function (node, ...args) {
        const result = originalAppendChild.call(this, node, ...args);
        if (node?.localName === 'iframe') {
          report({ event: 'iframe-append', src: node.getAttribute?.('src') || '',
            connected: !!node.isConnected, frameId: node._frameId ?? 0 });
        }
        return result;
      };
      const srcDescriptor = Object.getOwnPropertyDescriptor(Element.prototype, 'src');
      if (srcDescriptor?.set) Object.defineProperty(Element.prototype, 'src', {
        configurable: srcDescriptor.configurable, enumerable: srcDescriptor.enumerable,
        get: srcDescriptor.get,
        set(value) {
          if (this?.localName === 'iframe') report({ event: 'iframe-src-set', value: String(value || '') });
          return srcDescriptor.set.call(this, value);
        },
      });
      const originalSetAttribute = Element.prototype.setAttribute;
      Element.prototype.setAttribute = function (name, value, ...args) {
        const result = originalSetAttribute.call(this, name, value, ...args);
        if (this?.localName === 'iframe' && ['src', 'srcdoc', 'sandbox'].includes(String(name).toLowerCase())) {
          report({ event: 'iframe-attribute', name: String(name).toLowerCase(),
            value: String(value).slice(0, 240), frameId: this._frameId ?? 0 });
        }
        return result;
      };
      const innerHtmlDescriptor = Object.getOwnPropertyDescriptor(Element.prototype, 'innerHTML');
      if (innerHtmlDescriptor?.set) Object.defineProperty(Element.prototype, 'innerHTML', {
        configurable: innerHtmlDescriptor.configurable, enumerable: innerHtmlDescriptor.enumerable,
        get: innerHtmlDescriptor.get,
        set(value) {
          if (this?.localName === 'body' || this?.localName === 'html') {
            report({ event: 'element-innerhtml', tag: this.localName,
              length: String(value || '').length, frameId: globalThis.__obscura_frameId ?? 0 });
          }
          return innerHtmlDescriptor.set.call(this, value);
        },
      });
      const imageSrc = Object.getOwnPropertyDescriptor(globalThis.HTMLImageElement?.prototype || {}, 'src');
      if (imageSrc?.set) Object.defineProperty(HTMLImageElement.prototype, 'src', {
        configurable: imageSrc.configurable, enumerable: imageSrc.enumerable,
        get: imageSrc.get,
        set(value) {
          report({ event: 'image-src', value: String(value).slice(0, 240) });
          return imageSrc.set.call(this, value);
        },
      });
      const imageSetAttribute = globalThis.HTMLImageElement?.prototype?.setAttribute;
      if (typeof imageSetAttribute === 'function') {
        globalThis.HTMLImageElement.prototype.setAttribute = function (name, value, ...args) {
          const result = imageSetAttribute.call(this, name, value, ...args);
          if (String(name).toLowerCase() === 'src') {
            if (!this.__abImageObserved) {
              this.__abImageObserved = true;
              this.addEventListener('load', () => report({ event: 'image-load',
                complete: this.complete, width: this.naturalWidth, height: this.naturalHeight,
                currentSrc: String(this.currentSrc || '').slice(0, 300) }));
              this.addEventListener('error', () => report({ event: 'image-error',
                complete: this.complete, currentSrc: String(this.currentSrc || '').slice(0, 300) }));
            }
            report({ event: 'image-attribute', value: String(value).slice(0, 240),
              resolved: (() => { try { return String(this.src).slice(0, 300); } catch (_) { return 'error'; } })(),
              base: (() => { try { return String(this.baseURI).slice(0, 300); } catch (_) { return 'error'; } })(),
              nid: this._nid ?? null, frameId: globalThis.__obscura_frameId ?? 0,
              onload: typeof this.onload, onerror: typeof this.onerror });
            setTimeout(() => report({ event: 'image-state', complete: this.complete,
              width: this.naturalWidth, height: this.naturalHeight,
              currentSrc: String(this.currentSrc || '').slice(0, 300) }), 1000);
          }
          return result;
        };
      }
      for (const name of ['hasPrivateToken', 'hasRedemptionRecord']) {
        const original = Document.prototype[name];
        if (typeof original !== 'function') continue;
        Document.prototype[name] = function (issuer, ...args) {
          report({ event: 'privacy-api-call', name, issuer: String(issuer || '').slice(0, 240) });
          let result;
          try { result = original.call(this, issuer, ...args); }
          catch (error) {
            report({ event: 'privacy-api-reject', name, reason: String(error).slice(0, 240) });
            throw error;
          }
          return Promise.resolve(result).then(value => {
            report({ event: 'privacy-api-result', name, value });
            return value;
          }, error => {
            report({ event: 'privacy-api-reject', name, reason: String(error).slice(0, 240) });
            throw error;
          });
        };
      }
      const responseProto = globalThis.Response?.prototype;
      for (const name of ['text', 'arrayBuffer', 'json', 'blob']) {
        const original = responseProto?.[name];
        if (typeof original !== 'function') continue;
        responseProto[name] = async function (...args) {
          const result = await original.apply(this, args);
          report({ event: `response-read-${name}`,
            status: this.status, length: typeof result === 'string'
              ? result.length : (result?.byteLength ?? result?.size ?? null) });
          return result;
        };
      }
      for (const type of ['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click']) {
        document.addEventListener(type, event => report({
          event: `input-${type}`, target: event.target?.tagName || null,
          path: event.composedPath?.().slice(0, 10).map(node => node?.tagName || node?.nodeName || null),
          trusted: event.isTrusted,
          client: [event.clientX, event.clientY], screen: [event.screenX, event.screenY],
          page: [event.pageX, event.pageY], buttons: event.buttons, button: event.button,
          detail: event.detail, composed: event.composed, view: event.view === window,
        }), true);
      }
      if (typeof globalThis.Worker === 'function') {
        const WorkerClass = globalThis.Worker;
        globalThis.Worker = new Proxy(WorkerClass, {
          construct(target, args, newTarget) {
            const worker = Reflect.construct(target, args, newTarget);
            report({ event: 'worker-construct', url: String(args[0] || '').slice(0, 180) });
            setTimeout(() => {
              try {
                const scope = worker._scope;
                const workerNavigator = scope?.navigator;
                const workerNavigatorProto = workerNavigator ? Object.getPrototypeOf(workerNavigator) : null;
                report({ event: 'worker-surface', navigatorKeys: workerNavigator ? Reflect.ownKeys(workerNavigator) : null,
                  navigatorProtoKeys: workerNavigatorProto ? Reflect.ownKeys(workerNavigatorProto) : null,
                  userAgentData: typeof workerNavigator?.userAgentData,
                  connection: typeof workerNavigator?.connection,
                  gpu: typeof workerNavigator?.gpu,
                  gpuRequestAdapter: typeof workerNavigator?.gpu?.requestAdapter,
                  locks: typeof workerNavigator?.locks,
                  storage: typeof workerNavigator?.storage,
                  permissions: typeof workerNavigator?.permissions,
                  mediaDevices: typeof workerNavigator?.mediaDevices,
                  credentials: typeof workerNavigator?.credentials,
                  hardwareConcurrency: workerNavigator?.hardwareConcurrency,
                  deviceMemory: workerNavigator?.deviceMemory,
                  platform: workerNavigator?.platform,
                  userAgent: workerNavigator?.userAgent });
              } catch (error) { report({ event: 'worker-surface-error', error: String(error) }); }
            }, 25);
            const post = worker.postMessage;
            worker.postMessage = function (...postArgs) {
              const data = postArgs[0];
              const source = typeof data === 'string' ? data : '';
              const codeHints = ['navigator.gpu', 'navigator.userAgentData', 'location.',
                'structuredClone', 'OffscreenCanvas', 'WebGL', 'WorkerLocation',
                'crypto.subtle', 'SharedArrayBuffer'].filter(hint => source.includes(hint));
              report({ event: 'worker-post', kind: typeof data,
                length: source ? source.length : null, codeHints,
                keys: data && typeof data === 'object' ? Object.keys(data).slice(0, 12) : null,
                transfer: Array.isArray(postArgs[1]) ? postArgs[1].map(item => ({
                  type: Object.prototype.toString.call(item),
                  byteLength: typeof item?.byteLength === 'number' ? item.byteLength : null,
                })) : null });
              return post.apply(this, postArgs);
            };
            const observe = () => report({ event: 'worker-state',
              codeLength: typeof worker._code === 'string' ? worker._code.length : null,
              codeHead: typeof worker._code === 'string' ? worker._code.slice(0, 900) : null,
              terminated: worker._terminated === true,
              scopeReady: !!worker._scope,
              scopeMessageListeners: worker._scope?._ev?.message?.length || 0,
              scopeOnMessage: typeof worker._scope?.onmessage,
            });
            const observeMessages = () => {
              const scope = worker._scope;
              if (!scope || scope.__abPostWrapped) return;
              const originalPost = scope.postMessage;
               scope.postMessage = function (data, transfer) {
                report({ event: 'worker-message', kind: typeof data,
                  length: typeof data === 'string' ? data.length : null,
                  keys: data && typeof data === 'object' ? Object.keys(data).slice(0, 12) : null,
                  transfer: Array.isArray(transfer) ? transfer.map(item => ({
                    type: Object.prototype.toString.call(item),
                    byteLength: typeof item?.byteLength === 'number' ? item.byteLength : null,
                  })) : null });
                return originalPost.call(this, data, transfer);
              };
              scope.__abPostWrapped = true;
            };
            setTimeout(observe, 0);
            setTimeout(observeMessages, 0);
            setTimeout(observe, 100);
            setTimeout(observeMessages, 100);
            setTimeout(observe, 1000);
            setTimeout(observeMessages, 1000);
            return worker;
          },
        });
      }
      if (globalThis.__obscura_frameElements && typeof globalThis.SharedWorker === 'function') {
        const SharedWorkerClass = globalThis.SharedWorker;
        globalThis.SharedWorker = new Proxy(SharedWorkerClass, {
          construct(target, args, newTarget) {
            const worker = Reflect.construct(target, args, newTarget);
            report({ event: 'shared-worker-construct', url: String(args[0] || '').slice(0, 180) });
            return worker;
          },
        });
      }
      addEventListener('error', event => report({
        event: 'error', message: String(event.message || '').slice(0, 300),
      }));
      addEventListener('unhandledrejection', event => report({
        event: 'unhandledrejection', reason: String(event.reason || '').slice(0, 300),
      }));
      for (const name of ['error', 'warn']) {
        const original = console[name];
        console[name] = function (...args) {
          report({ event: `console-${name}`, text: args.map(value => String(value)).join(' ').slice(0, 400) });
          return original.apply(this, args);
        };
      }
      const originalAttachShadow = Element.prototype.attachShadow;
      Element.prototype.attachShadow = function (init) {
        const isBody = this?.tagName === 'BODY';
        const beforeOpen = isBody && (() => {
          try { return !!this.shadowRoot; } catch { return 'error'; }
        })();
        const stack = isBody ? (() => {
          try { return String(new Error().stack || '').slice(0, 900); } catch { return ''; }
        })() : '';
        try {
          const root = originalAttachShadow.call(this, init);
          if (isBody) globalThis.__abBodyShadow = root;
          if (isBody) {
            const shadowClick = phase => event => report({
              event: `shadow-click-${phase}`,
              target: event.target?.tagName || null,
              path: event.composedPath?.().slice(0, 10).map(node => node?.tagName || node?.nodeName || null),
              defaultPrevented: event.defaultPrevented,
              propagationStopped: !!event._propagationStopped,
            });
            root.addEventListener('click', shadowClick('capture'), true);
            root.addEventListener('click', shadowClick('bubble'));
            for (const type of ['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click', 'input', 'change']) {
              for (const capture of [true, false]) {
                root.addEventListener(type, event => report({
                  event: `shadow-${type}-${capture ? 'capture' : 'bubble'}`,
                  target: event.target?.tagName || null,
                  path: event.composedPath?.().slice(0, 10).map(node => node?.tagName || node?.nodeName || null),
                  trusted: event.isTrusted,
                  composed: event.composed,
                  defaultPrevented: event.defaultPrevented,
                }), capture);
              }
            }
            report({
              event: 'attachShadow-body', mode: init?.mode,
              beforeOpen, afterOpen: (() => {
                try { return !!this.shadowRoot; } catch { return 'error'; }
              })(), children: root?.childNodes?.length ?? null, stack,
            });
          }
          return root;
        }
        catch (error) {
          report({
            event: 'attachShadow-error',
            tag: this?.tagName, id: this?.id, className: this?.className,
            beforeOpen, stack, reason: String(error).slice(0, 300),
          });
          throw error;
        }
      };
      const typeOf = name => {
        try { return typeof globalThis[name]; } catch { return 'error'; }
      };
      const has = name => {
        try { return name in globalThis; } catch { return false; }
      };
      const diag = {
        source: 'ab-child-diag', event: 'diag',
        obscuraFrameId: globalThis.__obscura_frameId ?? null,
        parentIsSelf: globalThis.parent === globalThis,
        topIsSelf: globalThis.top === globalThis,
        selfIsWindow: globalThis.self === globalThis,
        origin: (() => { try { return location.origin; } catch { return 'error'; } })(),
        readyState: (() => { try { return document.readyState; } catch { return 'error'; } })(),
        bodyShadow: (() => { try { return !!document.body?.shadowRoot; } catch { return 'error'; } })(),
        bodyShadowInfo: (() => {
          try { return Deno.core.ops.op_shadow_root_info(document.body._nid); }
          catch (error) { return String(error).slice(0, 180); }
        })(),
        bodyChildren: (() => { try { return document.body?.childNodes?.length ?? null; } catch { return 'error'; } })(),
        bodyHtml: (() => { try { return String(document.body?.innerHTML || '').slice(0, 240); } catch { return 'error'; } })(),
        capturedShadowHosts: (() => {
          try { return (globalThis.__abShadowRoots || []).map(root => root.host?.tagName || '?'); }
          catch { return 'error'; }
        })(),
        hasCrypto: has('crypto'), cryptoSubtle: (() => { try { return typeof crypto.subtle; } catch { return 'error'; } })(),
        hasTextEncoder: has('TextEncoder'), hasURL: has('URL'), hasURLSearchParams: has('URLSearchParams'),
        hasPerformance: has('performance'), hasVisualViewport: has('visualViewport'),
        hasFonts: (() => { try { return !!document.fonts; } catch { return false; } })(),
        hasOffscreenCanvas: has('OffscreenCanvas'), hasWebGL: typeOf('WebGLRenderingContext'),
        hasWebGPU: (() => { try { return typeof navigator.gpu; } catch { return 'error'; } })(),
        isSecureContext: (() => { try { return [typeof isSecureContext, isSecureContext]; } catch { return 'error'; } })(),
        crossOriginIsolated: (() => { try { return [typeof crossOriginIsolated, crossOriginIsolated]; } catch { return 'error'; } })(),
        visibility: (() => { try { return [document.visibilityState, document.hidden]; } catch { return 'error'; } })(),
        webdriver: (() => { try { return [typeof navigator.webdriver, navigator.webdriver]; } catch { return 'error'; } })(),
        vendor: (() => { try { return navigator.vendor; } catch { return 'error'; } })(),
        hardwareConcurrency: (() => { try { return navigator.hardwareConcurrency; } catch { return 'error'; } })(),
        deviceMemory: (() => { try { return navigator.deviceMemory; } catch { return 'error'; } })(),
        maxTouchPoints: (() => { try { return navigator.maxTouchPoints; } catch { return 'error'; } })(),
        cookieEnabled: (() => { try { return navigator.cookieEnabled; } catch { return 'error'; } })(),
        screen: (() => { try { return [screen.width, screen.height, screen.availWidth, screen.availHeight, screen.colorDepth, screen.pixelDepth]; } catch { return 'error'; } })(),
        viewport: (() => { try { return [innerWidth, innerHeight, outerWidth, outerHeight, devicePixelRatio]; } catch { return 'error'; } })(),
        windowPosition: (() => { try { return [screenX, screenY, screenLeft, screenTop]; } catch { return 'error'; } })(),
        timezone: (() => { try { return Intl.DateTimeFormat().resolvedOptions().timeZone; } catch { return 'error'; } })(),
        mediaDevices: typeOf('MediaDevices'),
        navigatorPermissions: (() => { try { return typeof navigator.permissions; } catch { return 'error'; } })(),
        navigatorMediaDevices: (() => { try { return typeof navigator.mediaDevices; } catch { return 'error'; } })(),
        cryptoRandomUUID: (() => { try { return typeof crypto?.randomUUID; } catch { return 'error'; } })(),
        uaData: (() => { try { return [Object.prototype.toString.call(navigator.userAgentData), navigator.userAgentData.brands?.map(b => `${b.brand}:${b.version}`)]; } catch { return 'error'; } })(),
        plugins: (() => { try { return Array.from(navigator.plugins || []).map(plugin => [plugin.name, plugin.filename, plugin.length]); } catch { return 'error'; } })(),
        mimeTypes: (() => { try { return Array.from(navigator.mimeTypes || []).map(type => [type.type, type.description, type.suffixes]); } catch { return 'error'; } })(),
        chromeKeys: (() => { try { return Object.keys(globalThis.chrome || {}); } catch { return 'error'; } })(),
         privateToken: (() => {
           try { return [typeof document.hasPrivateToken, typeof document.hasRedemptionRecord]; }
           catch { return 'error'; }
         })(),
         storageAccess: (() => { try { return [typeof document.hasStorageAccess, typeof document.requestStorageAccess, typeof document.hasUnpartitionedCookieAccess]; } catch { return 'error'; } })(),
         navigatorApis: (() => { try { return {
           storage: [typeof navigator.storage, typeof navigator.storage?.estimate,
             typeof navigator.storage?.persisted, typeof navigator.storage?.persist],
           connection: [Object.prototype.toString.call(navigator.connection),
             typeof navigator.connection?.effectiveType, typeof navigator.connection?.rtt,
             typeof navigator.connection?.downlink],
           locks: [typeof navigator.locks, typeof navigator.locks?.request],
         }; } catch { return 'error'; } })(),
        challengeApis: ['Worker', 'SharedWorker', 'WebAssembly', 'OffscreenCanvas', 'WebGLRenderingContext',
          'PointerEvent', 'TouchEvent', 'PerformanceObserver', 'speechSynthesis', 'Notification']
          .map(name => [name, typeOf(name)]),
        notification: (() => { try { return [typeof Notification, Notification?.permission]; } catch { return 'error'; } })(),
        permissions: typeOf('Permissions'), permissionQuery: (() => { try { return typeof navigator.permissions?.query; } catch { return 'error'; } })(),
        trustedTypes: (() => { try { return [typeof globalThis.trustedTypes, typeof globalThis.trustedTypes?.createPolicy, typeof globalThis.trustedTypes?.isHTML]; } catch { return 'error'; } })(),
        featurePolicy: (() => { try { return [typeof document.featurePolicy, typeof document.featurePolicy?.features, document.featurePolicy?.features?.().length ?? null]; } catch { return 'error'; } })(),
        featurePolicyFeatures: (() => { try { return document.featurePolicy?.features?.() || null; } catch { return 'error'; } })(),
        workers: [typeOf('Worker'), typeOf('Blob'), typeOf('URL'), typeOf('WebAssembly')],
        observers: [typeOf('MutationObserver'), typeOf('ResizeObserver'), typeOf('IntersectionObserver')],
        userAgent: (() => { try { return navigator.userAgent; } catch { return 'error'; } })(),
        platform: (() => { try { return navigator.platform; } catch { return 'error'; } })(),
        languages: (() => { try { return [...navigator.languages]; } catch { return []; } })(),
        evalShape: (() => { try { return {
          text: String(eval), nativeText: Function.prototype.toString.call(eval),
          name: eval.name, length: eval.length, keys: Reflect.ownKeys(eval),
          proto: Object.getPrototypeOf(eval) === Function.prototype,
          completion: (0, eval)('0,/.*honk.*/,123456789'),
        }; } catch (error) { return String(error); } })(),
        dateNowShape: (() => { try { return {
          text: String(Date.now), nativeText: Function.prototype.toString.call(Date.now),
          name: Date.now.name, length: Date.now.length, integer: Number.isInteger(Date.now()),
        }; } catch (error) { return String(error); } })(),
        webdriverDescriptor: (() => { try {
          const descriptor = Object.getOwnPropertyDescriptor(Navigator.prototype, 'webdriver');
          return descriptor && {
            enumerable: descriptor.enumerable,
            configurable: descriptor.configurable,
            get: String(descriptor.get),
          };
        } catch (error) { return String(error); } })(),
        userAgentDataShape: (() => { try {
          const data = navigator.userAgentData;
          const proto = Object.getPrototypeOf(data);
          return {
            ownKeys: Reflect.ownKeys(data),
            prototype: proto && proto.constructor && proto.constructor.name,
            prototypeKeys: Reflect.ownKeys(proto),
            brands: data.brands,
            platform: data.platform,
            mobile: data.mobile,
          };
        } catch (error) { return String(error); } })(),
      };
      report(diag);
      Promise.resolve().then(() => navigator.userAgentData?.getHighEntropyValues?.([
        'architecture', 'bitness', 'brands', 'fullVersionList', 'mobile', 'model',
        'platform', 'platformVersion', 'uaFullVersion', 'wow64',
      ])).then(value => report({ event: 'ua-high', value })).catch(error =>
        report({ event: 'ua-high', error: String(error) }));
      setTimeout(() => report({
        event: 'late-diag',
        readyState: (() => { try { return document.readyState; } catch { return 'error'; } })(),
        viewport: (() => { try { return [innerWidth, innerHeight, outerWidth, outerHeight]; } catch { return 'error'; } })(),
        windowPosition: (() => { try { return [screenX, screenY, screenLeft, screenTop]; } catch { return 'error'; } })(),
        timezone: (() => { try { return Intl.DateTimeFormat().resolvedOptions().timeZone; } catch { return 'error'; } })(),
        bodyChildren: (() => { try { return document.body?.childNodes?.length ?? null; } catch { return 'error'; } })(),
        bodyShadow: (() => { try {
          const root = globalThis.__abBodyShadow;
          const describe = element => [
            element.tagName, element.id, String(element.className || '').slice(0, 80),
            String(element.outerHTML || '').slice(0, 1000),
            (() => { try { const rect = element.getBoundingClientRect(); return [rect.x, rect.y, rect.width, rect.height]; } catch { return 'error'; } })(),
          ];
          return root ? {
            children: [...root.childNodes].map(node => node.nodeName),
            html: String(root.innerHTML || '').slice(0, 5000),
            images: [...root.querySelectorAll('img')].map(image => String(image.src || '').slice(0, 240)),
            styleUrls: [...root.querySelectorAll('style')].flatMap(style =>
              [...String(style.textContent || '').matchAll(/url\(([^)]+)\)/g)]
                .map(match => match[1].slice(0, 240))),
            elements: [...root.querySelectorAll('*')].filter(element =>
              element.tagName === 'INPUT' || element.tagName === 'LABEL' || element.id === 'WmdT3' || element.id === 'iRAM5')
              .map(describe),
            controls: [...new Set([root, ...(globalThis.__abShadowRoots || [])])].flatMap(shadow =>
              [...shadow.querySelectorAll('input,button,label')].map(describe)),
            iframes: [...root.querySelectorAll('iframe')].map(frame => ({
              src: String(frame.src || frame.getAttribute('src') || '').slice(0, 240),
              frameId: frame._frameId || 0,
              rect: (() => { try { const box = frame.getBoundingClientRect(); return [box.x, box.y, box.width, box.height]; } catch { return 'error'; } })(),
              loadInfo: frame._iframeLoadInfo || null,
              loadingUrl: frame._iframeLoadingUrl || null,
              attributes: (() => { try { return [...frame.attributes].map(attribute => [attribute.name, attribute.value]); } catch { return 'error'; } })(),
              shimBody: (() => { try { return String(frame._iframeDoc?.body?.innerHTML || '').slice(0, 1200); } catch { return 'error'; } })(),
              docReady: (() => { try { return frame.contentDocument?.readyState || null; } catch { return 'error'; } })(),
              docBody: (() => { try { return String(frame.contentDocument?.body?.innerHTML || '').slice(0, 500); } catch { return 'error'; } })(),
            })),
          } : null;
        } catch { return 'error'; } })(),
        stylesheets: (() => { try { return [...document.styleSheets || []].length; } catch { return 'error'; } })(),
        hit: (() => { try {
          const element = document.elementFromPoint(30, 30);
          return element && [element.tagName, element.id, element.className];
        } catch { return 'error'; } })(),
      }), 8000);
      if (typeof navigator.gpu === 'object' && navigator.gpu) {
        Promise.resolve().then(() => navigator.gpu.requestAdapter())
          .then(adapter => report({
            event: 'gpu', adapter: !!adapter,
            features: adapter ? [...adapter.features] : [],
            info: adapter?.info ? {
              vendor: adapter.info.vendor, architecture: adapter.info.architecture,
              device: adapter.info.device, description: adapter.info.description,
              fallback: adapter.info.isFallbackAdapter,
            } : null,
            limits: adapter ? {
              maxTextureDimension2D: adapter.limits.maxTextureDimension2D,
              maxBufferSize: adapter.limits.maxBufferSize,
              maxBindGroups: adapter.limits.maxBindGroups,
            } : null,
            webgl: (() => { try {
              const canvas = document.createElement('canvas');
              const gl = canvas.getContext('webgl');
              const info = gl?.getExtension('WEBGL_debug_renderer_info');
              return { vendor: gl?.getParameter(info?.UNMASKED_VENDOR_WEBGL),
                renderer: gl?.getParameter(info?.UNMASKED_RENDERER_WEBGL),
                version: gl?.getParameter(gl.VERSION), extensions: gl?.getSupportedExtensions?.() || [] };
            } catch (error) { return { error: String(error) }; } })(),
          }))
          .catch(error => report({ event: 'gpu-error', reason: String(error).slice(0, 300) }));
      }
      try {
      const source = `self.postMessage({kind:'boot',document:typeof document,navigator:typeof navigator,crypto:typeof crypto,wasm:typeof WebAssembly,selfTag:Object.prototype.toString.call(self),selfConstructor:self.constructor?.name,workerGlobalScope:typeof WorkerGlobalScope,dedicatedWorkerGlobalScope:typeof DedicatedWorkerGlobalScope,selfWorkerGlobalScope:self instanceof WorkerGlobalScope,selfDedicatedWorkerGlobalScope:self instanceof DedicatedWorkerGlobalScope,messageEvent:typeof MessageEvent,messageEventTag:Object.prototype.toString.call(new MessageEvent('message')),navigatorTag:Object.prototype.toString.call(navigator),navigatorConstructor:navigator?.constructor?.name});self.onmessage=e=>self.postMessage({kind:'echo',data:e.data});`;
        const workerUrl = URL.createObjectURL(new Blob([source], { type: 'application/javascript' }));
        const worker = new Worker(workerUrl);
        URL.revokeObjectURL(workerUrl);
        worker.onmessage = event => {
          report({ event: 'worker', data: event.data });
          if (event.data?.kind === 'boot') worker.postMessage('ping');
          else worker.terminate();
        };
        worker.onerror = event => report({ event: 'worker-error', reason: String(event.message || event).slice(0, 300) });
      } catch (error) {
        report({ event: 'worker-create-error', reason: String(error).slice(0, 300) });
      }
    });
  }
  const errors = [];
  const challengeRequests = [];
  const challengeBodies = [];
  page.on('pageerror', e => pushBounded(errors, 'pageerror: ' + String(e).slice(0, 160)));
  page.on('console', m => {
    if (m.type() === 'error') pushBounded(errors, 'console: ' + m.text().slice(0, 160));
  });
  page.on('request', request => {
    if (process.env.OBSCURA_AB_HEADERS !== '1' || !request.url().includes('challenges.cloudflare.com')) return;
    void request.allHeaders().then(headers => {
      const selected = Object.fromEntries(Object.entries(headers).filter(([name]) => [
        'accept', 'accept-language', 'content-type', 'cf-chl', 'cf-chl-ra',
        'origin', 'referer', 'sec-ch-ua', 'sec-ch-ua-platform', 'sec-fetch-dest',
        'sec-fetch-mode', 'sec-fetch-site', 'user-agent',
      ].includes(name.toLowerCase())));
      console.log(`REQUEST_HEADERS ${request.method()} ${request.url().slice(0, 160)} ${JSON.stringify(selected)}`);
    }).catch(() => {});
  });
  page.on('response', r => {
    const url = r.url();
    if (url.includes('challenges.cloudflare.com') || url.includes('challenge-platform')) {
      pushBounded(challengeRequests, `${r.status()} ${url.slice(0, 110)}`);
      void (async () => {
        let bodyLength = 'unknown';
        let requestLength = 'unknown';
        try { bodyLength = (await r.body()).length; } catch (_) {}
        try { requestLength = r.request().postDataBuffer()?.length ?? 0; } catch (_) {}
        pushBounded(challengeBodies, `${r.status()} ${r.request().method()} post=${requestLength} body=${bodyLength} ${url.slice(0, 110)}`);
      })();
    }
  });

  await page.goto(opts.url, { waitUntil: 'load', timeout: 90000 });

  let clickTarget = null;
  if (opts.click) {
    const forceClick = process.env.OBSCURA_AB_FORCE_CLICK === '1';
    let clickState = null;
    // The checkbox may appear well after the first challenge request. Give the
    // child frame enough time to finish before using a coordinate fallback.
    for (let second = 0; second < 60; second++) {
      await new Promise(done => setTimeout(done, 1000));
      clickState = await page.evaluate(() => {
        const frames = [];
        for (const root of globalThis.__abShadowRoots || []) {
          try { frames.push(...root.querySelectorAll('iframe')); } catch { /* gone */ }
        }
        frames.push(...document.querySelectorAll('iframe'));
        const frame = frames[0];
        const rect = frame?.getBoundingClientRect();
        const late = [...globalThis.__abChildDiagnostics || []].reverse()
          .find(entry => entry.event === 'late-diag');
        const elements = [
          ...(late?.bodyShadow?.elements || []),
          ...(late?.bodyShadow?.controls || []),
        ];
        const control = elements.find(element =>
          ['INPUT', 'BUTTON', 'LABEL'].includes(element[0])
          && Array.isArray(element[4]) && element[4][2] > 0 && element[4][3] > 0);
        const root = globalThis.__abBodyShadow;
        const controls = root ? [...root.querySelectorAll('input,button,label')].map(element => [
          element.tagName, element.id, element.getAttribute('type'),
          (() => { try { const box = element.getBoundingClientRect(); return [box.x, box.y, box.width, box.height]; } catch { return 'error'; } })(),
        ]) : [];
        return {
          engine: typeof globalThis.__obscura_frameElements === 'object' ? 'obscura' : 'chrome',
          rect: rect && { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
          controls, childControl: control ? { tag: control[0], id: control[1], rect: control[4] } : null,
        };
      });
      if (process.env.OBSCURA_AB_CLICK_TRACE === '1') {
        console.log(`CLICK_STATE ${JSON.stringify(clickState)}`);
      }
      // Both engines need time to finish the challenge setup. A real Chrome
      // iframe can expose its input only after the first network round trip.
      if (clickState?.childControl) break;
    }
    const frameRect = clickState?.rect;
    const childControl = clickState?.childControl;
    if (frameRect && (clickState.engine !== 'obscura' || childControl || forceClick)) {
      const localRect = childControl?.rect;
      const x = localRect
        ? (childControl.absolute ? localRect[0] : frameRect.x + localRect[0]) + localRect[2] / 2
        : frameRect.x + frameRect.width / 2;
      const y = localRect
        ? (childControl.absolute ? localRect[1] : frameRect.y + localRect[1]) + localRect[3] / 2
        // Match the real Chrome control's known fallback point. The closed
        // Turnstile shadow root retargets this to BODY, but the widget handles
        // the point at its checkbox location.
        : frameRect.y + 30;
      const clickX = localRect ? x : frameRect.x + 30;
      clickTarget = { engine: clickState.engine, frameRect, control: childControl, point: [clickX, y] };
      await page.mouse.move(clickX, y);
      await page.mouse.down();
      await new Promise(done => setTimeout(done, 100));
      await page.mouse.up();
    } else if (frameRect) {
      clickTarget = { engine: clickState.engine, frameRect,
        error: 'no visible interactive control was exposed by the child frame' };
    }
  }

  // Poll rather than wait once: the token can arrive many seconds after load,
  // and stopping early would report a failure that had not happened yet.
  let report = null;
  let tokenAfter = null;
  for (let second = 1; second <= opts.wait; second++) {
    await new Promise(done => setTimeout(done, 1000));
    report = evaluated(await tryEvaluate(page, probe)) || report;
    if (report && report.token) { tokenAfter = second; break; }
  }
  // Do not call child.evaluate here. If a CDP client has a frame-attached
  // event but no execution context, a timeout would leave its request pending.
  await new Promise(done => setTimeout(done, 250));
  const clickEvents = evaluated(await tryEvaluate(page, () => globalThis.__abInputEvents)) || [];
  const childEventCounts = evaluated(await tryEvaluate(page, () =>
    (globalThis.__abChildDiagnostics || []).reduce((counts, entry) => {
      counts[entry.event] = (counts[entry.event] || 0) + 1;
      return counts;
    }, {}))) || {};
  const childDiagStates = evaluated(await tryEvaluate(page, () =>
    (globalThis.__abChildDiagnostics || [])
      .filter(entry => entry.event === 'diag' || entry.event === 'late-diag')
      .map(entry => ({ event: entry.event, frameId: entry.obscuraFrameId, readyState: entry.readyState, viewport: entry.viewport })))) || [];
  const childInputEvents = evaluated(await tryEvaluate(page, () =>
    (globalThis.__abChildDiagnostics || [])
      .filter(entry => /^(input-|shadow-(pointerdown|mousedown|pointerup|mouseup|click|input|change))/.test(entry.event || ''))
      .map(entry => ({ event: entry.event, target: entry.target, path: entry.path, trusted: entry.trusted,
        composed: entry.composed, client: entry.client, screen: entry.screen,
        defaultPrevented: entry.defaultPrevented })))) || [];
  return { report, tokenAfter, errors, challengeRequests: [...challengeRequests, ...challengeBodies], clickTarget, clickEvents, childEventCounts, childDiagStates, childInputEvents };
}

const fixture = opts.url === 'local' ? await serveFixture() : null;
if (fixture) {
  opts.url = fixture.url;
  console.log(`fixture on ${fixture.url} with the dummy sitekey`);
}

for (const engine of opts.only ? [opts.only] : ['chrome', 'obscura']) {
  console.log(`\n=== ${engine}`);
  try {
    const out = await runIn(engine, opts, scenario);
    const r = out.report || {};
    console.log(`   api.js script tag : ${r.hasApiScript}`);
    console.log(`   window.turnstile  : ${r.hasTurnstileGlobal}`);
    console.log(`   widget elements   : ${r.widgets}`);
    console.log(`   token input       : ${r.tokenInput}`);
    console.log(`   TOKEN             : ${r.token
      ? `${r.token.slice(0, 24)}... (after ${out.tokenAfter}s)  PASS` : 'empty  FAIL'}`);
    console.log(`   main viewport     : ${JSON.stringify(r.viewport)}`);
    console.log(`   main screen       : ${JSON.stringify(r.screen)}`);
    console.log(`   contentWindow API : ${r.contentWindowDescriptor} outbound=${r.outboundInstalled}`);
    if (r.frameTable?.length) console.log(`   frame table       : ${JSON.stringify(r.frameTable)}`);
    if (r.childDiagnostics?.length) {
      const gpu = r.childDiagnostics.filter(entry => entry.event === 'gpu' || entry.event === 'gpu-error');
      if (gpu.length) console.log(`   gpu diagnostics    : ${JSON.stringify(gpu)}`);
      if (process.env.OBSCURA_AB_COMPACT !== '1') {
        console.log(`   child diagnostics  : ${JSON.stringify(r.childDiagnostics)}`);
      }
    }
    if (r.challengeUi) console.log(`   challenge UI       : ${JSON.stringify(r.challengeUi)}`);
    if (r.iframeEvents?.length) console.log(`   child iframe trace : ${JSON.stringify(r.iframeEvents)}`);
    if (r.imageEvents?.length) console.log(`   child image trace  : ${JSON.stringify(r.imageEvents)}`);
    if (r.fetchShapes?.length) console.log(`   fetch shapes       : ${JSON.stringify(r.fetchShapes)}`);
    if (r.childSurface) console.log(`   child surface      : ${JSON.stringify(r.childSurface)}`);
    console.log(`   child event counts : ${JSON.stringify(out.childEventCounts)}`);
    if (out.childInputEvents?.length) console.log(`   child input events : ${JSON.stringify(out.childInputEvents)}`);
    console.log(`   child diag states  : ${JSON.stringify(out.childDiagStates)}`);
    const childErrors = (r.childDiagnostics || []).filter(entry => /^(console-|worker-|child-|error|unhandled)/.test(entry.event || ''));
    if (childErrors.length) console.log(`   child errors       : ${JSON.stringify(childErrors)}`);
    console.log(`   shadow roots      : ${r.shadowRoots}`);
    console.log(`   iframes           : ${(r.iframes || []).length}`);
    for (const f of r.iframes || []) {
      console.log(`     - src=${f.src || '(none)'}`);
      console.log(`       connected=${f.connected} doc=${f.doc}` +
                  `${f.docBodyChars !== undefined ? ` bodyChars=${f.docBodyChars}` : ''}` +
                  ` contentWindow.postMessage=${f.win}`);
      if (f.frameId !== undefined) console.log(`       frameId=${f.frameId}`);
      if (f.loadInfo) console.log(`       loadInfo=${f.loadInfo}`);
    }
    console.log(`   message exchange  : ${(r.messages || []).length}`);
    if (out.clickTarget) console.log(`   click target      : ${JSON.stringify(out.clickTarget)}`);
    if (out.clickEvents?.length) console.log(`   click events      : ${JSON.stringify(out.clickEvents)}`);
    for (const line of r.messages || []) console.log('     ' + line);
    const seen = [...new Set(out.challengeRequests)];
    console.log(`   challenge requests: ${seen.length}`);
    for (const line of seen.slice(0, 10)) console.log('     ' + line);
    const unique = [...new Set(out.errors)];
    for (const line of unique.slice(0, 8)) console.log('   ' + line);
  } catch (error) {
    console.log('   THREW ' + String(error).split('\n')[0].slice(0, 300));
  }
}
fixture?.stop();
