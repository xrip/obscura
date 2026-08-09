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

// Element interfaces Chrome exposes that upstream has no class for. They are
// real constructors in a browser, so `Element` alone is not enough.
if (typeof Element === 'function') {
  Object.defineProperty(globalThis, 'HTMLEmbedElement', {
    value: class HTMLEmbedElement extends Element {},
    writable: true, enumerable: false, configurable: true,
  });
  Object.defineProperty(globalThis, 'HTMLSourceElement', {
    value: class HTMLSourceElement extends Element {},
    writable: true, enumerable: false, configurable: true,
  });
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
