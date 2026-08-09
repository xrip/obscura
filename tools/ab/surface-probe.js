// Behavioural surface probe. Run against two obscura builds and diff the JSON.
// Everything here must be deterministic: no timings, no randomness, no values
// that legitimately differ between two runs of the same binary.
(function () {
  const out = {};
  const safe = fn => { try { const v = fn(); return v === undefined ? '<undefined>' : v; } catch (e) { return 'throw:' + (e && e.name); } };
  const typeOf = v => (v === null ? 'null' : typeof v);

  // 1. Every own global, with its kind and enumerability. This is the big one:
  //    a missing interface, an extra leak, or a wrong descriptor all show here.
  const globals = {};
  for (const name of Object.getOwnPropertyNames(globalThis).sort()) {
    const d = Object.getOwnPropertyDescriptor(globalThis, name);
    let kind = 'accessor';
    if (d && 'value' in d) kind = typeOf(d.value) === 'function'
      ? (/^class\s/.test(String(d.value)) ? 'class' : 'function')
      : typeOf(d.value);
    globals[name] = `${kind}${d && d.enumerable ? ' ENUM' : ''}`;
  }
  out.globals = globals;

  // 2. Prototype shape of the interfaces anti-bot code reads most.
  const protoOf = name => {
    const C = globalThis[name];
    if (typeof C !== 'function' || !C.prototype) return null;
    return Object.getOwnPropertyNames(C.prototype).sort().join(',');
  };
  out.prototypes = {};
  for (const n of ['Navigator', 'Performance', 'PerformanceTiming', 'Screen', 'Document',
                   'Element', 'HTMLElement', 'HTMLCanvasElement', 'WebGLRenderingContext',
                   'WebGL2RenderingContext', 'Permissions', 'MediaDevices', 'NavigatorUAData',
                   'ScreenOrientation', 'EventTarget', 'Node', 'Window', 'Worker', 'Notification']) {
    out.prototypes[n] = protoOf(n);
  }

  // 3. navigator, screen and their brands.
  out.navigator = {};
  for (const k of ['userAgent', 'appVersion', 'appName', 'appCodeName', 'platform', 'vendor',
                   'vendorSub', 'product', 'productSub', 'language', 'languages', 'onLine',
                   'cookieEnabled', 'doNotTrack', 'hardwareConcurrency', 'deviceMemory',
                   'maxTouchPoints', 'webdriver', 'pdfViewerEnabled', 'userActivation',
                   'buildID', 'oscpu', 'globalPrivacyControl']) {
    out.navigator[k] = safe(() => { const v = navigator[k]; return Array.isArray(v) ? v.join(',') : typeOf(v) === 'object' ? '[object]' : v; });
  }
  out.navigator['#plugins'] = safe(() => navigator.plugins.length);
  out.navigator['#mimeTypes'] = safe(() => navigator.mimeTypes.length);
  out.navigator['plugins[0]'] = safe(() => navigator.plugins[0] && navigator.plugins[0].name);
  out.navigator['#brand'] = safe(() => Object.prototype.toString.call(navigator));
  out.navigator['uaData.brands'] = safe(() => navigator.userAgentData.brands.map(b => b.brand + ':' + b.version).join(' | '));
  out.navigator['uaData.platform'] = safe(() => navigator.userAgentData.platform);

  out.screen = {};
  for (const k of ['width', 'height', 'availWidth', 'availHeight', 'colorDepth', 'pixelDepth',
                   'availTop', 'availLeft', 'isExtended']) {
    out.screen[k] = safe(() => screen[k]);
  }
  out.screen['#brand'] = safe(() => Object.prototype.toString.call(screen));
  out.screen['orientation.type'] = safe(() => screen.orientation.type);

  // 4. window-level surfaces that separate a browser from an engine.
  out.window = {
    devicePixelRatio: safe(() => devicePixelRatio),
    isSecureContext: safe(() => typeOf(globalThis.isSecureContext)),
    crossOriginIsolated: safe(() => typeOf(globalThis.crossOriginIsolated)),
    chrome: safe(() => typeOf(globalThis.chrome)),
    'chrome.runtime': safe(() => typeOf(globalThis.chrome && globalThis.chrome.runtime)),
    'chrome.csi': safe(() => typeOf(globalThis.chrome && globalThis.chrome.csi)),
    'chrome.loadTimes': safe(() => typeOf(globalThis.chrome && globalThis.chrome.loadTimes)),
    Notification_permission: safe(() => Notification.permission),
    outerWidth: safe(() => typeOf(globalThis.outerWidth)),
    outerHeight: safe(() => typeOf(globalThis.outerHeight)),
    matchMedia: safe(() => typeOf(globalThis.matchMedia)),
    requestAnimationFrame: safe(() => typeOf(globalThis.requestAnimationFrame)),
    requestIdleCallback: safe(() => typeOf(globalThis.requestIdleCallback)),
    reportError: safe(() => typeOf(globalThis.reportError)),
    structuredClone: safe(() => typeOf(globalThis.structuredClone)),
    queueMicrotask: safe(() => typeOf(globalThis.queueMicrotask)),
  };

  // 5. toString of things a detector reads. A shim that forgets to look native
  //    is the classic tell.
  out.toStrings = {};
  for (const [label, fn] of [
    ['Function.toString', () => Function.prototype.toString.toString()],
    ['navigator.permissions.query', () => navigator.permissions.query.toString()],
    ['performance.now', () => performance.now.toString()],
    ['HTMLCanvasElement.getContext', () => HTMLCanvasElement.prototype.getContext.toString()],
    ['Navigator', () => Navigator.toString()],
    ['Performance', () => Performance.toString()],
    ['setTimeout', () => setTimeout.toString()],
    ['fetch', () => fetch.toString()],
    ['Error.captureStackTrace', () => Error.captureStackTrace.toString()],
  ]) out.toStrings[label] = safe(fn);

  // 6. Error stacks: a leaked internal frame names the engine outright.
  out.errors = {
    stackHasObscura: safe(() => { try { null.x; } catch (e) { return /obscura|bootstrap|deno/i.test(e.stack || ''); } }),
    stackFirstLine: safe(() => { try { null.x; } catch (e) { return String(e.stack || '').split('\n')[0]; } }),
    typeErrorMessage: safe(() => { try { null.x; } catch (e) { return e.message; } }),
    illegalCanvas: safe(() => { try { new HTMLCanvasElement(); } catch (e) { return e.message; } }),
  };

  // 7. Graphics identity, the fork's whole point.
  out.graphics = safe(() => {
    const c = document.createElement('canvas');
    const gl = c.getContext('webgl');
    if (!gl) return 'no webgl';
    const d = gl.getExtension('WEBGL_debug_renderer_info');
    return {
      vendor: gl.getParameter(d.UNMASKED_VENDOR_WEBGL),
      renderer: gl.getParameter(d.UNMASKED_RENDERER_WEBGL),
      version: gl.getParameter(gl.VERSION),
      shading: gl.getParameter(gl.SHADING_LANGUAGE_VERSION),
      maxTexture: gl.getParameter(gl.MAX_TEXTURE_SIZE),
      extensions: (gl.getSupportedExtensions() || []).length,
      params: [0x0d33, 0x8869, 0x8dfb, 0x84e8].map(p => gl.getParameter(p)).join('/'),
    };
  });

  return JSON.stringify(out, null, 1);
})()
