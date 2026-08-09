// Fork-only. Spliced into bootstrap.js at /* __OBSCURA_GRAPHICS_MODULE__ */,
// immediately before graphics_api_v145.js and graphics.js.
//
// graphics.js was written against the fork's own bootstrap. Rather than push
// those helpers back into upstream's bootstrap.js, they are defined here, on
// top of what upstream already provides. Upstream's bootstrap.js then carries
// only two marker comments for the whole graphics layer.

// The selected fingerprint profile, handed over per page by
// crates/obscura-js/src/graphics.rs and picked up in graphics_page_init.js.
let _fingerprintProfile = null;

function _freezeFingerprintProfile(value) {
  if (!value || typeof value !== 'object' || Object.isFrozen(value)) return value;
  const keys = Object.keys(value);
  for (let i = 0; i < keys.length; i++) _freezeFingerprintProfile(value[keys[i]]);
  return Object.freeze(value);
}

// Upstream has _markNative(fn) and _markNativeAs(fn, str), which control what
// Function.prototype.toString reports. graphics.js also needs the reported
// `name` and `length` to match Chrome's IDL, because a WebGL method whose
// length is wrong is as good a tell as one whose source is wrong. Built on
// upstream's two helpers rather than beside them.
function _makeNativeFunction(fn, name, length, source) {
  try {
    Object.defineProperty(fn, 'name', { value: name, configurable: true });
    Object.defineProperty(fn, 'length', { value: length, configurable: true });
  } catch (_) {}
  return source ? _markNativeAs(fn, source) : _markNative(fn);
}

// graphics.js brands the `gpu` accessor so `Navigator.prototype.gpu` grabbed off
// the prototype and called on a foreign object throws, as it does in Chrome.
// Upstream does not keep such a set, so track instances here. Membership is
// checked, never enumerated, so a WeakSet is enough.
const _navigatorInstances = new WeakSet();
if (typeof navigator !== 'undefined' && navigator) _navigatorInstances.add(navigator);

// Interface objects on the global are `enumerable: false` per WebIDL, so
// Object.keys(window) does not list them. Upstream gets this for the names in
// its _preHideInternals list, which pre-declares them non-enumerable so a later
// plain assignment only updates the value. Names outside that list, including
// WebGLRenderingContext and every WebGPU interface, would land enumerable and
// show up in Object.keys(window) - a one-line detection.
//
// Everything graphics.js puts on the global goes through here instead.
function _graphicsDefineGlobal(name, value) {
  try {
    Object.defineProperty(globalThis, name, {
      value,
      writable: true,
      enumerable: false,
      configurable: true,
    });
  } catch (_) {
    globalThis[name] = value;
  }
  return value;
}

// Upstream installs `navigator.gpu` as an own data property returning a stub
// whose requestAdapter always resolves null. graphics.js replaces it with a
// real accessor on Navigator.prototype; an own property on the instance would
// shadow that, so it goes first.
try { delete navigator.gpu; } catch (_) {}
