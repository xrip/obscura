// Fork-only. Spliced at /* __OBSCURA_FORK_PAGE_INIT_END__ */, per page, before
// the enumerability sweep in fork_hide_globals.js.
//
// Measured against the pre-rebuild fork build, which is the target shape:
//
//   navigator own props    ours 25   fork 0    Chrome 0
//   Navigator.prototype    ours 2    fork 45   Chrome ~60
//   screen own props       ours 9    fork 0    Chrome 0
//   toString.call(screen)  ours [object Object]  fork [object Screen]
//   chrome.runtime         ours object          fork undefined
//
// In a browser every navigator and screen member lives on the interface
// prototype as an accessor, so `Object.getOwnPropertyNames(navigator)` is empty.
// Upstream builds both as instances carrying their own data properties, which
// is a one-line detection and also makes toString report [object Object].
//
// The values are re-read from the instance on every page init, because upstream
// reassigns them per navigation and the prototype is shared for the life of the
// isolate.

// Move `instance`'s own data properties onto `Ctor.prototype` as accessors that
// read a per-instance store, then delete the own properties. Methods move as
// plain values. Accessors already on the instance are moved as-is.
function _forkLiftToPrototype(Ctor, instance, brand) {
  if (typeof Ctor !== 'function' || !instance) return;
  const store = new Map();
  for (const key of Object.getOwnPropertyNames(instance)) {
    const d = Object.getOwnPropertyDescriptor(instance, key);
    if (!d || !d.configurable) continue;
    if (typeof d.value === 'function') {
      Object.defineProperty(Ctor.prototype, key, {
        value: _markNative(d.value), writable: true, enumerable: true, configurable: true,
      });
    } else if ('value' in d) {
      store.set(key, d.value);
      const get = function () { return store.get(key); };
      _markNativeAs(get, `function get ${key}() { [native code] }`);
      // A setter is required, not optional. CDP's Emulation.setDeviceMetrics
      // Override writes `screen.width = n` and friends, and a getter-only
      // accessor silently swallows that in sloppy mode, which broke three
      // emulation tests. Writable properties stay writable.
      const descriptor = { get, enumerable: true, configurable: true };
      if (d.writable) {
        const set = function (value) { store.set(key, value); };
        _markNativeAs(set, `function set ${key}() { [native code] }`);
        descriptor.set = set;
      }
      Object.defineProperty(Ctor.prototype, key, descriptor);
    } else {
      Object.defineProperty(Ctor.prototype, key, d);
    }
    delete instance[key];
  }
  Object.defineProperty(Ctor.prototype, Symbol.toStringTag, {
    value: brand, configurable: true,
  });
  // Only re-point the prototype when the instance is not already an instance of
  // the interface. Upstream installs the spoofed navigator properties on a thin
  // prototype *between* navigator and Navigator.prototype (see _navProto in
  // bootstrap.js); re-pointing straight at Ctor.prototype drops that link and
  // takes userAgent, platform and the rest with it.
  if (!(instance instanceof Ctor)) {
    Object.setPrototypeOf(instance, Ctor.prototype);
  }
}

// Chrome reports these three and upstream has none of them. They are among the
// first things a fingerprint script reads.
if (globalThis.navigator) {
  if (navigator.appCodeName === undefined) navigator.appCodeName = 'Mozilla';
  if (navigator.appName === undefined) navigator.appName = 'Netscape';
  if (navigator.vendorSub === undefined) navigator.vendorSub = '';
}

if (typeof Navigator === 'function') {
  _forkLiftToPrototype(Navigator, globalThis.navigator, 'Navigator');
}

// Upstream has no Screen constructor at all, so `screen` reports
// [object Object]. Build the interface, then lift onto it.
if (globalThis.screen) {
  if (typeof globalThis.Screen !== 'function') {
    const Screen = function () {
      throw new TypeError("Failed to construct 'Screen': Illegal constructor");
    };
    Object.defineProperty(Screen, 'name', { value: 'Screen', configurable: true });
    Object.defineProperty(Screen.prototype, 'constructor', {
      value: Screen, writable: true, configurable: true,
    });
    _markNative(Screen);
    Object.defineProperty(globalThis, 'Screen', {
      value: Screen, writable: true, enumerable: false, configurable: true,
    });
  }
  _forkLiftToPrototype(globalThis.Screen, globalThis.screen, 'Screen');
}

// Every page in Chrome has isSecureContext. Derived from the document's own
// origin, matching the rule WebGPU already uses in graphics.js.
if (globalThis.isSecureContext === undefined) {
  let secure = false;
  try {
    const u = new URL(__currentUrl());
    secure = u.protocol === 'https:' || u.protocol === 'wss:' || u.protocol === 'file:'
      || u.protocol === 'about:' || u.protocol === 'data:'
      || (u.protocol === 'http:' && (u.hostname === 'localhost' || u.hostname === '127.0.0.1' || u.hostname === '[::1]'));
  } catch (_) { /* opaque origin: not secure */ }
  Object.defineProperty(globalThis, 'isSecureContext', {
    value: secure, writable: false, enumerable: true, configurable: true,
  });
}

// window.chrome.runtime only exists when an extension is present. Reporting it
// on an ordinary page says "automation harness" rather than "Chrome".
if (globalThis.chrome && globalThis.chrome.runtime !== undefined) {
  try { delete globalThis.chrome.runtime; } catch (_) { /* not configurable */ }
}

// Every builtin on window reports its own name in Chrome:
//
//   setTimeout.toString() === 'function setTimeout() { [native code] }'
//
// Upstream assigns several as anonymous arrows (`globalThis.setTimeout = (...)
// => {}`), and assigning to a member expression does not name a function, so
// they printed as `function () { [native code] }`. Named here rather than at
// each definition, so the fix survives upstream adding more.
for (const _name of Object.getOwnPropertyNames(globalThis)) {
  const _d = Object.getOwnPropertyDescriptor(globalThis, _name);
  if (!_d || typeof _d.value !== 'function' || _d.value.name !== '') continue;
  try { Object.defineProperty(_d.value, 'name', { value: _name, configurable: true }); }
  catch (_) { /* frozen builtin */ }
}
