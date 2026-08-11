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
function _forkBrandedAccessor(kind, key, instance, implementation) {
  let accessor;
  if (kind === 'get') {
    accessor = Object.getOwnPropertyDescriptor({
      get value() {
        if (this !== instance) throw new TypeError('Illegal invocation');
        return implementation.call(this);
      },
    }, 'value').get;
  } else {
    accessor = Object.getOwnPropertyDescriptor({
      set value(value) {
        if (this !== instance) throw new TypeError('Illegal invocation');
        implementation.call(this, value);
      },
    }, 'value').set;
  }
  try {
    Object.defineProperty(accessor, 'name', {
      value: `${kind} ${String(key)}`, configurable: true,
    });
  } catch (_) {}
  return _markNativeAs(accessor, `function ${kind} ${String(key)}() { [native code] }`);
}

function _forkLiftToPrototype(Ctor, instance, brand) {
  if (typeof Ctor !== 'function' || !instance) return;
  const store = new Map();
  for (const key of Object.getOwnPropertyNames(instance)) {
    // Engine privates stay on the instance. Lifting `_w`/`_availH` onto
    // Screen.prototype would publish upstream's internals under a name no
    // browser has, which is worse than the own-property shape being fixed.
    if (key.startsWith('_')) continue;
    const d = Object.getOwnPropertyDescriptor(instance, key);
    if (!d || !d.configurable) continue;
    if (typeof d.value === 'function') {
      Object.defineProperty(Ctor.prototype, key, {
        value: _markNative(d.value), writable: true, enumerable: true, configurable: true,
      });
    } else if ('value' in d) {
      store.set(key, d.value);
      const get = _forkBrandedAccessor('get', key, instance, function () {
        return store.get(key);
      });
      // A setter is required, not optional. CDP's Emulation.setDeviceMetrics
      // Override writes `screen.width = n` and friends, and a getter-only
      // accessor silently swallows that in sloppy mode, which broke three
      // emulation tests. Writable properties stay writable.
      const descriptor = { get, enumerable: true, configurable: true };
      if (d.writable) {
        const set = _forkBrandedAccessor('set', key, instance, function (value) {
          store.set(key, value);
        });
        descriptor.set = set;
      }
      Object.defineProperty(Ctor.prototype, key, descriptor);
    } else {
      const descriptor = { ...d };
      if (typeof d.get === 'function') {
        descriptor.get = _forkBrandedAccessor('get', key, instance, d.get);
      }
      if (typeof d.set === 'function') {
        descriptor.set = _forkBrandedAccessor('set', key, instance, d.set);
      }
      Object.defineProperty(Ctor.prototype, key, descriptor);
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

function _forkSetPrototypeOrder(Ctor, names) {
  if (typeof Ctor !== 'function') return;
  const proto = Ctor.prototype;
  const current = Object.getOwnPropertyNames(proto);
  const order = [...names, ...current.filter(name => !names.includes(name))];
  const descriptors = new Map(current.map(name => [
    name, Object.getOwnPropertyDescriptor(proto, name),
  ]));
  for (const name of current) {
    const descriptor = descriptors.get(name);
    if (descriptor && descriptor.configurable) delete proto[name];
  }
  for (const name of order) {
    const descriptor = descriptors.get(name);
    if (descriptor) Object.defineProperty(proto, name, descriptor);
  }
}

// The profile was consumed near the start of __obscura_init. Upstream then
// replaces several of its values with random defaults. Restore the selected
// row before any page script can observe it. Keep this in the fork module so
// upstream's general browser shim stays easy to merge.
const _forkNavigatorProfile = _fingerprintProfile && _fingerprintProfile.navigator;
if (globalThis.navigator && _forkNavigatorProfile) {
  if (Number.isFinite(_forkNavigatorProfile.hardwareConcurrency)) {
    globalThis.__obscura_hw = _forkNavigatorProfile.hardwareConcurrency;
  }
  if (Number.isFinite(_forkNavigatorProfile.deviceMemory)) {
    globalThis.__obscura_mem = _forkNavigatorProfile.deviceMemory;
  }
  if (Number.isFinite(_forkNavigatorProfile.maxTouchPoints)) {
    navigator.maxTouchPoints = _forkNavigatorProfile.maxTouchPoints;
  }

  if (Array.isArray(_forkNavigatorProfile.languages) && _forkNavigatorProfile.languages.length) {
    const languages = Object.freeze(_forkNavigatorProfile.languages.map(String));
    const language = languages[0];
    const navProto = Object.getPrototypeOf(navigator);
    const getLanguage = _forkBrandedAccessor('get', 'language', navigator, function () {
      return language;
    });
    const getLanguages = _forkBrandedAccessor('get', 'languages', navigator, function () {
      return languages;
    });
    Object.defineProperty(navProto, 'language', {
      get: getLanguage, enumerable: true, configurable: true,
    });
    Object.defineProperty(navProto, 'languages', {
      get: getLanguages, enumerable: true, configurable: true,
    });
  }
}

// NetworkInformation is part of the same captured identity. Upstream exposes
// fixed 10 Mbps / 50 ms values, publishes non-desktop `type` fields, and keeps
// listener state as own properties. Chrome has no own fields here and its
// desktop prototype contains only the standard four values plus `onchange`.
const _forkNetworkProfile = _fingerprintProfile && _fingerprintProfile.network;
if (globalThis.navigator && _forkNetworkProfile && typeof EventTarget === 'function') {
  const _networkToken = {};
  const _networkOnChange = new WeakMap();
  const NetworkInformation_ = function NetworkInformation(token) {
    if (token !== _networkToken) throw new TypeError('Illegal constructor');
  };
  _markNative(NetworkInformation_);
  const proto = Object.create(EventTarget.prototype);
  const defineGetter = (name, read, write) => {
    _markNativeAs(read, `function get ${name}() { [native code] }`);
    const descriptor = { get: read, enumerable: true, configurable: true };
    if (write) {
      _markNativeAs(write, `function set ${name}() { [native code] }`);
      descriptor.set = write;
    }
    Object.defineProperty(proto, name, descriptor);
  };
  defineGetter('onchange',
    function () { return _networkOnChange.get(this) || null; },
    function (value) { _networkOnChange.set(this, typeof value === 'function' ? value : null); });
  defineGetter('effectiveType', function () { return String(_forkNetworkProfile.effectiveType); });
  defineGetter('rtt', function () { return Number(_forkNetworkProfile.rtt); });
  defineGetter('downlink', function () { return Number(_forkNetworkProfile.downlink); });
  defineGetter('saveData', function () { return Boolean(_forkNetworkProfile.saveData); });
  Object.defineProperty(proto, 'constructor', {
    value: NetworkInformation_, writable: true, configurable: true,
  });
  Object.defineProperty(proto, Symbol.toStringTag, {
    value: 'NetworkInformation', configurable: true,
  });
  NetworkInformation_.prototype = proto;
  Object.defineProperty(globalThis, 'NetworkInformation', {
    value: NetworkInformation_, writable: true, enumerable: false, configurable: true,
  });
  navigator.connection = new NetworkInformation_(_networkToken);
}

_forkResetBattery();

// Upstream installs spoofed navigator getters on a thin object between the
// instance and Navigator.prototype. Chrome has no such extra prototype layer.
// Move those descriptors onto the interface prototype before lifting the
// instance values, otherwise a depth-limited fingerprint dump sees only the
// thin layer and reduces the real Navigator.prototype to `object(N)`.
if (typeof Navigator === 'function' && globalThis.navigator) {
  const directProto = Object.getPrototypeOf(navigator);
  if (directProto && directProto !== Navigator.prototype &&
      Navigator.prototype.isPrototypeOf(directProto)) {
    for (const key of Reflect.ownKeys(directProto)) {
      const descriptor = Object.getOwnPropertyDescriptor(directProto, key);
      if (!descriptor) continue;
      if (typeof descriptor.get === 'function') {
        descriptor.get = _forkBrandedAccessor('get', key, navigator, descriptor.get);
      }
      if (typeof descriptor.set === 'function') {
        descriptor.set = _forkBrandedAccessor('set', key, navigator, descriptor.set);
      }
      Object.defineProperty(Navigator.prototype, key, descriptor);
    }
    Object.setPrototypeOf(navigator, Navigator.prototype);
  }
}

const _forkScreenProfile = _fingerprintProfile && _fingerprintProfile.screen;
function _forkSyncScreenOrientation(width, height) {
  const orientation = globalThis.screen && screen.orientation;
  if (!orientation) return;
  orientation.type = width >= height ? 'landscape-primary' : 'portrait-primary';
  orientation.angle = 0;
  if (orientation.onchange === undefined) orientation.onchange = null;
  if (typeof orientation.lock !== 'function') {
    orientation.lock = function lock() { return Promise.resolve(); };
  }
  if (typeof orientation.unlock !== 'function') {
    orientation.unlock = function unlock() {};
  }
}

if (globalThis.screen && _forkScreenProfile) {
  const hasExplicitScreenSize =
    Number.isFinite(globalThis.__obscura_screen_w) && globalThis.__obscura_screen_w > 0 &&
    Number.isFinite(globalThis.__obscura_screen_h) && globalThis.__obscura_screen_h > 0;

  // An explicit CDP screen size wins. With no such override, expose the exact
  // captured screen row, including available work area and window position.
  if (!hasExplicitScreenSize) {
    _setScreenValues(screen, {
      width: _forkScreenProfile.width,
      height: _forkScreenProfile.height,
      availWidth: _forkScreenProfile.availWidth,
      availHeight: _forkScreenProfile.availHeight,
      availLeft: _forkScreenProfile.availLeft,
      availTop: _forkScreenProfile.availTop,
    });
    globalThis.outerWidth = _forkScreenProfile.outerWidth;
    globalThis.outerHeight = _forkScreenProfile.outerHeight;
  }
  _setScreenValues(screen, {
    colorDepth: _forkScreenProfile.colorDepth,
    pixelDepth: _forkScreenProfile.pixelDepth,
  });
  globalThis.screenX = _forkScreenProfile.screenX;
  globalThis.screenY = _forkScreenProfile.screenY;
  globalThis.screenLeft = _forkScreenProfile.screenX;
  globalThis.screenTop = _forkScreenProfile.screenY;
  globalThis.devicePixelRatio = _forkScreenProfile.devicePixelRatio;
}
if (globalThis.screen) {
  _forkSyncScreenOrientation(screen.width, screen.height);
}

// Upstream restores its generic fingerprint pool when CDP clears a physical
// screen override. Keep the hook, but make its no-override state return to the
// selected profile row instead. Explicit CDP sizes still win unchanged.
if (_forkScreenProfile && typeof globalThis.__obscura_set_screen_override === 'function') {
  const _forkSetScreenOverride = globalThis.__obscura_set_screen_override;
  globalThis.__obscura_set_screen_override = function(w, h, emulated) {
    _forkSetScreenOverride.call(globalThis, w, h, emulated);
    const hasExplicitSize =
      Number.isFinite(w) && w > 0 && Number.isFinite(h) && h > 0;
    if (!globalThis.screen) return;
    if (!hasExplicitSize) {
      _setScreenValues(globalThis.screen, {
        width: _forkScreenProfile.width,
        height: _forkScreenProfile.height,
        availWidth: _forkScreenProfile.availWidth,
        availHeight: _forkScreenProfile.availHeight,
        availLeft: _forkScreenProfile.availLeft,
        availTop: _forkScreenProfile.availTop,
        colorDepth: _forkScreenProfile.colorDepth,
        pixelDepth: _forkScreenProfile.pixelDepth,
      });
    }
    _forkSyncScreenOrientation(screen.width, screen.height);
  };
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
  if (typeof EventTarget === 'function' &&
      Object.getPrototypeOf(globalThis.Screen.prototype) !== EventTarget.prototype) {
    Object.setPrototypeOf(globalThis.Screen.prototype, EventTarget.prototype);
  }
  _forkLiftToPrototype(globalThis.Screen, globalThis.screen, 'Screen');
}

// These three still carried their members as own properties of the instance;
// fork_interfaces.js only moved methods. Same treatment as navigator and screen:
// in Chrome every one of them is an accessor on the interface prototype.
_forkLiftToPrototype(globalThis.NavigatorUAData, globalThis.navigator && navigator.userAgentData, 'NavigatorUAData');
_forkLiftToPrototype(globalThis.ScreenOrientation, globalThis.screen && screen.orientation, 'ScreenOrientation');
_forkLiftToPrototype(globalThis.MediaDevices, globalThis.navigator && navigator.mediaDevices, 'MediaDevices');
for (const [name, instance] of [
  ['Clipboard', globalThis.navigator && navigator.clipboard],
  ['CredentialsContainer', globalThis.navigator && navigator.credentials],
  ['Geolocation', globalThis.navigator && navigator.geolocation],
  ['Keyboard', globalThis.navigator && navigator.keyboard],
  ['LockManager', globalThis.navigator && navigator.locks],
  ['MediaCapabilities', globalThis.navigator && navigator.mediaCapabilities],
  ['ServiceWorkerContainer', globalThis.navigator && navigator.serviceWorker],
  ['StorageManager', globalThis.navigator && navigator.storage],
  ['WakeLock', globalThis.navigator && navigator.wakeLock],
]) {
  _forkLiftToPrototype(globalThis[name], instance, name);
}
for (const [name, order] of [
  ['Clipboard', ['onclipboardchange', 'read', 'readText', 'write', 'writeText', 'constructor']],
  ['CredentialsContainer', ['create', 'get', 'preventSilentAccess', 'store', 'constructor']],
  ['Geolocation', ['clearWatch', 'getCurrentPosition', 'watchPosition', 'constructor']],
  ['Keyboard', ['getLayoutMap', 'lock', 'unlock', 'constructor']],
  ['LockManager', ['query', 'request', 'constructor']],
  ['MediaCapabilities', ['decodingInfo', 'encodingInfo', 'constructor']],
  ['ServiceWorkerContainer', [
    'controller', 'ready', 'oncontrollerchange', 'onmessage', 'onmessageerror',
    'getRegistration', 'getRegistrations', 'register', 'startMessages', 'constructor',
  ]],
  ['StorageManager', ['estimate', 'persisted', 'constructor', 'getDirectory', 'persist']],
  ['WakeLock', ['request', 'constructor']],
]) {
  _forkSetPrototypeOrder(globalThis[name], order);
}
if (typeof DOMRect === 'function') {
  Object.defineProperty(DOMRect.prototype, Symbol.toStringTag, {
    value: 'DOMRect', configurable: true,
  });
}

// Recompute for every document. Obscura keeps one global across navigations,
// while Chrome creates a new realm and derives this value from the new origin.
// Top-level data: and about:blank documents are opaque and are not trustworthy.
let _forkSecureContext = false;
try {
  const u = new URL(__currentUrl());
  const loopback = u.hostname === 'localhost' || u.hostname.endsWith('.localhost')
    || u.hostname === '[::1]' || /^127(?:\.\d{1,3}){3}$/.test(u.hostname);
  _forkSecureContext = u.protocol === 'https:' || u.protocol === 'wss:'
    || u.protocol === 'file:' || (u.protocol === 'http:' && loopback);
} catch (_) { /* opaque origin: not secure */ }
Object.defineProperty(globalThis, 'isSecureContext', {
  value: _forkSecureContext, writable: false, enumerable: true, configurable: true,
});
// Ordinary pages are not cross-origin isolated unless the response opts into
// COOP/COEP. Obscura does not implement those response policies yet, so expose
// the browser's honest default instead of leaving a direct read undefined.
Object.defineProperty(globalThis, 'crossOriginIsolated', {
  value: false, writable: false, enumerable: true, configurable: true,
});

// Chrome exposes the multi-screen fields only in secure contexts. Their
// functions are created with the Screen interface in the startup snapshot;
// page init only toggles the saved descriptors, so it does not publish new JS
// getter identities after navigation.
if (typeof Screen === 'function' && globalThis.screen) {
  for (const name of ['isExtended', 'onchange']) {
    if (globalThis.isSecureContext) {
      const descriptor = _screenSecureDescriptors.get(name);
      if (descriptor && !Object.prototype.hasOwnProperty.call(Screen.prototype, name)) {
        Object.defineProperty(Screen.prototype, name, descriptor);
      }
    } else {
      try { delete Screen.prototype[name]; } catch (_) { /* configurable in Chrome */ }
    }
  }
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
