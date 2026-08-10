// Fork-only. Spliced into bootstrap.js at /* __OBSCURA_FORK_LATE_MODULE__ */,
// late enough that navigator, screen and the DOM classes all exist.
//
// Upstream builds working *instances* - navigator.permissions,
// navigator.mediaDevices, screen.orientation, navigator.userAgentData - but
// never exposes their constructors, and never exposes `Navigator` itself even
// though it declares the class. In Chrome every one of these is on window, so
// `typeof Navigator === 'undefined'` is a one-line detection in a browser
// claiming to be Chrome.
//
// The instances are branded in place rather than rebuilt, so upstream keeps
// owning how they are constructed and what they return. Recovered from fork
// commit c59cd68 "Harden browser compatibility for challenge flows".

// Interface object: not callable, throws on construction, correct name and
// toStringTag, reports as native, and non-enumerable on the global per WebIDL.
function _forkInterface(name) {
  const C = function () {
    throw new TypeError(`Failed to construct '${name}': Illegal constructor`);
  };
  Object.defineProperty(C, 'name', { value: name, configurable: true });
  Object.defineProperty(C.prototype, 'constructor', {
    value: C, writable: true, configurable: true,
  });
  Object.defineProperty(C.prototype, Symbol.toStringTag, {
    value: name, configurable: true,
  });
  _markNative(C);
  Object.defineProperty(globalThis, name, {
    value: C, writable: true, enumerable: false, configurable: true,
  });
  return C;
}

// Move an instance's own methods onto its interface prototype and adopt it, so
// a page sees them inherited as in Chrome instead of as own properties.
function _forkBrandInstance(name, instance) {
  if (!instance || typeof instance !== 'object') return;
  const C = _forkInterface(name);
  for (const key of Object.keys(instance)) {
    const value = instance[key];
    if (typeof value !== 'function') continue;
    Object.defineProperty(C.prototype, key, {
      value: _markNative(value), writable: true, configurable: true,
    });
    delete instance[key];
  }
  Object.setPrototypeOf(instance, C.prototype);
}

// Upstream declares `class Navigator` but never puts it on the global.
if (typeof Navigator === 'function') {
  Object.defineProperty(globalThis, 'Navigator', {
    value: Navigator, writable: true, enumerable: false, configurable: true,
  });
  Object.defineProperty(Navigator.prototype, Symbol.toStringTag, {
    value: 'Navigator', configurable: true,
  });
}

_forkBrandInstance('Permissions', globalThis.navigator && navigator.permissions);
_forkBrandInstance('MediaDevices', globalThis.navigator && navigator.mediaDevices);
_forkBrandInstance('NavigatorUAData', globalThis.navigator && navigator.userAgentData);
_forkBrandInstance('ScreenOrientation', globalThis.screen && screen.orientation);
_forkBrandInstance('Clipboard', globalThis.navigator && navigator.clipboard);
_forkBrandInstance('CredentialsContainer', globalThis.navigator && navigator.credentials);
_forkBrandInstance('Geolocation', globalThis.navigator && navigator.geolocation);
_forkBrandInstance('Keyboard', globalThis.navigator && navigator.keyboard);
_forkBrandInstance('LockManager', globalThis.navigator && navigator.locks);
_forkBrandInstance('MediaCapabilities', globalThis.navigator && navigator.mediaCapabilities);
_forkBrandInstance('ServiceWorkerContainer', globalThis.navigator && navigator.serviceWorker);
_forkBrandInstance('StorageManager', globalThis.navigator && navigator.storage);
_forkBrandInstance('WakeLock', globalThis.navigator && navigator.wakeLock);

// Both interfaces inherit EventTarget in Chrome. The upstream instances are
// plain objects, so branding alone would leave the prototype chain too short.
if (typeof EventTarget === 'function') {
  if (typeof MediaDevices === 'function') {
    Object.setPrototypeOf(MediaDevices.prototype, EventTarget.prototype);
  }
  if (typeof ScreenOrientation === 'function') {
    Object.setPrototypeOf(ScreenOrientation.prototype, EventTarget.prototype);
  }
  if (typeof Clipboard === 'function') {
    Object.setPrototypeOf(Clipboard.prototype, EventTarget.prototype);
  }
  if (typeof ServiceWorkerContainer === 'function') {
    for (const name of ['addEventListener', 'removeEventListener', 'dispatchEvent']) {
      delete ServiceWorkerContainer.prototype[name];
    }
    Object.setPrototypeOf(ServiceWorkerContainer.prototype, EventTarget.prototype);
  }
}

// Chromium exposes live navigator instances for these two APIs. Ozon walks
// both object graphs, so constructor-only shells are observably incomplete.
const _protectedAudienceConstructionToken = {};
const _protectedAudienceInstances = new WeakSet();
class ProtectedAudience {
  constructor(token) {
    if (token !== _protectedAudienceConstructionToken) throw new TypeError('Illegal constructor');
    _protectedAudienceInstances.add(this);
  }
  queryFeatureSupport(feature) {
    if (!_protectedAudienceInstances.has(this)) throw new TypeError('Illegal invocation');
    if (arguments.length < 1) {
      throw new TypeError("Failed to execute 'queryFeatureSupport' on 'ProtectedAudience': 1 argument required, but only 0 present.");
    }
    return String(feature) === 'adComponentsLimit' ? 40 : undefined;
  }
}
_markNative(ProtectedAudience);
_markNative(ProtectedAudience.prototype.queryFeatureSupport);
Object.defineProperty(ProtectedAudience.prototype, 'queryFeatureSupport', {
  ...Object.getOwnPropertyDescriptor(ProtectedAudience.prototype, 'queryFeatureSupport'),
  enumerable: true,
});
Object.defineProperty(ProtectedAudience.prototype, Symbol.toStringTag, {
  value: 'ProtectedAudience', configurable: true,
});
Object.defineProperty(globalThis, 'ProtectedAudience', {
  value: ProtectedAudience, writable: true, enumerable: false, configurable: true,
});
const _protectedAudience = new ProtectedAudience(_protectedAudienceConstructionToken);

const _navigatorManagedDataConstructionToken = {};
const _navigatorManagedDataInstances = new WeakSet();
const _navigatorManagedDataHandlers = new WeakMap();
class NavigatorManagedData extends EventTarget {
  constructor(token) {
    if (token !== _navigatorManagedDataConstructionToken) throw new TypeError('Illegal constructor');
    super();
    _navigatorManagedDataInstances.add(this);
    _navigatorManagedDataHandlers.set(this, null);
  }
  getManagedConfiguration(keys) {
    if (!_navigatorManagedDataInstances.has(this)) throw new TypeError('Illegal invocation');
    if (arguments.length < 1) {
      throw new TypeError("Failed to execute 'getManagedConfiguration' on 'NavigatorManagedData': 1 argument required, but only 0 present.");
    }
    if (keys === null || typeof keys !== 'object' || typeof keys[Symbol.iterator] !== 'function') {
      throw new TypeError("Failed to execute 'getManagedConfiguration' on 'NavigatorManagedData': The provided value cannot be converted to a sequence.");
    }
    Array.from(keys, String);
    return Promise.reject(new DOMException(
      'Managed configuration is empty. This API is available only for managed apps.',
      'NotAllowedError'
    ));
  }
  get onmanagedconfigurationchange() {
    if (!_navigatorManagedDataInstances.has(this)) throw new TypeError('Illegal invocation');
    return _navigatorManagedDataHandlers.get(this);
  }
  set onmanagedconfigurationchange(value) {
    if (!_navigatorManagedDataInstances.has(this)) throw new TypeError('Illegal invocation');
    _navigatorManagedDataHandlers.set(this, typeof value === 'function' ? value : null);
  }
}
_markNative(NavigatorManagedData);
for (const name of ['getManagedConfiguration', 'onmanagedconfigurationchange']) {
  const descriptor = Object.getOwnPropertyDescriptor(NavigatorManagedData.prototype, name);
  if (descriptor.value) _markNative(descriptor.value);
  if (descriptor.get) _markNativeAs(descriptor.get, `function get ${name}() { [native code] }`);
  if (descriptor.set) _markNativeAs(descriptor.set, `function set ${name}() { [native code] }`);
  Object.defineProperty(NavigatorManagedData.prototype, name, { ...descriptor, enumerable: true });
}
Object.defineProperty(NavigatorManagedData.prototype, Symbol.toStringTag, {
  value: 'NavigatorManagedData', configurable: true,
});
Object.defineProperty(globalThis, 'NavigatorManagedData', {
  value: NavigatorManagedData, writable: true, enumerable: false, configurable: true,
});
const _navigatorManagedData = new NavigatorManagedData(_navigatorManagedDataConstructionToken);

if (globalThis.navigator) {
  navigator.protectedAudience = _protectedAudience;
  navigator.deprecatedRunAdAuctionEnforcesKAnonymity = false;
  navigator.managed = _navigatorManagedData;
}

// Element interfaces Chrome exposes that upstream has no class for. They are
// real constructors in a browser, so `Element` alone is not enough.
if (typeof Element === 'function') {
  Object.defineProperty(globalThis, 'HTMLIFrameElement', {
    value: class HTMLIFrameElement extends Element {},
    writable: true, enumerable: false, configurable: true,
  });
  Object.defineProperty(globalThis, 'HTMLEmbedElement', {
    value: class HTMLEmbedElement extends Element {},
    writable: true, enumerable: false, configurable: true,
  });
  Object.defineProperty(globalThis, 'HTMLSourceElement', {
    value: class HTMLSourceElement extends Element {},
    writable: true, enumerable: false, configurable: true,
  });
  _markNative(globalThis.HTMLIFrameElement);
  _markNative(globalThis.HTMLEmbedElement);
  _markNative(globalThis.HTMLSourceElement);
}

// HTMLDocument is a legacy alias of Document, and document instanceof
// HTMLDocument must hold.
if (typeof Document === 'function') {
  Object.defineProperty(globalThis, 'HTMLDocument', {
    value: Document, writable: true, enumerable: false, configurable: true,
  });
}
