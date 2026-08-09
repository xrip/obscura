// Fork-only. Spliced into bootstrap.js at /* __OBSCURA_FORK_EARLY_MODULE__ */,
// immediately before upstream's
//   globalThis.performance = globalThis.performance || { ... }
// so that assignment short-circuits and upstream's object literal never runs.
//
// Upstream's `performance` is a plain object with every method as an own
// property: `constructor.name` is "Object", there is no `toJSON` anywhere, and
// `timing` carries three fields instead of the twenty-one a browser reports.
// Ozon's anti-bot challenge calls `performance[...].toJSON()` and dies with
// "TypeError: performance[b[127]].toJSON is not a function".
//
// Ported from fork commit c59cd68 "Harden browser compatibility for challenge
// flows". The `timing` setter is the load-bearing part: upstream's
// __obscura_init assigns a three-field object literal every navigation, and the
// setter widens it to a full PerformanceTiming, so that line needs no edit.
const _performanceSlots = new WeakMap();
const _performanceTimingToken = {};
const _performanceTimingFields = [
  'navigationStart', 'unloadEventStart', 'unloadEventEnd',
  'redirectStart', 'redirectEnd', 'fetchStart',
  'domainLookupStart', 'domainLookupEnd', 'connectStart', 'connectEnd',
  'secureConnectionStart', 'requestStart', 'responseStart', 'responseEnd',
  'domLoading', 'domInteractive', 'domContentLoadedEventStart',
  'domContentLoadedEventEnd', 'domComplete', 'loadEventStart', 'loadEventEnd',
];
class PerformanceTiming {
  constructor(token, values={}) {
    if (token !== _performanceTimingToken) throw new TypeError('Illegal constructor');
    for (const field of _performanceTimingFields) {
      const value = Number(values[field]);
      this[field] = Number.isFinite(value) ? value : 0;
    }
  }
  toJSON() {
    const result = {};
    for (const field of _performanceTimingFields) result[field] = this[field];
    return result;
  }
}
_markNative(PerformanceTiming);
_markNative(PerformanceTiming.prototype.toJSON);
Object.defineProperty(PerformanceTiming.prototype, Symbol.toStringTag, {
  value: 'PerformanceTiming', configurable: true,
});
// Non-enumerable, as WebIDL requires of an interface object: anything that
// shows up in Object.keys(window) is a one-line detection.
Object.defineProperty(globalThis, 'PerformanceTiming', {
  value: PerformanceTiming, writable: true, enumerable: false, configurable: true,
});
const _newPerformanceTiming = values => new PerformanceTiming(_performanceTimingToken, values);

class Performance {
  constructor() {
    _performanceSlots.set(this, {
      timeOrigin: 0,
      timing: _newPerformanceTiming({}),
      navigation: { type: 0, redirectCount: 0 },
      memory: {
        jsHeapSizeLimit: 4294705152,
        totalJSHeapSize: 19321856,
        usedJSHeapSize: 16781520,
      },
      lastNow: -Infinity,
    });
  }
  get timeOrigin() { return _performanceSlots.get(this).timeOrigin; }
  set timeOrigin(value) { _performanceSlots.get(this).timeOrigin = Number(value) || 0; }
  get timing() { return _performanceSlots.get(this).timing; }
  set timing(value) {
    _performanceSlots.get(this).timing = value instanceof PerformanceTiming
      ? value
      : _newPerformanceTiming(value || {});
  }
  get navigation() { return _performanceSlots.get(this).navigation; }
  set navigation(value) { _performanceSlots.get(this).navigation = value; }
  get memory() { return _performanceSlots.get(this).memory; }
  set memory(value) { _performanceSlots.get(this).memory = value; }
  get eventCounts() { return new Map(); }
  get interactionCount() { return 0; }
  get onresourcetimingbufferfull() { return null; }
  set onresourcetimingbufferfull(_) {}
  now() {
    // Monotonically non-decreasing: return the wall-clock offset, but never a
    // value below the last one. Equal readings are allowed, and avoiding a
    // synthetic per-call increment keeps tight loops from advancing the clock
    // faster than real elapsed time.
    const slot = _performanceSlots.get(this);
    const ms = Date.now() - slot.timeOrigin;
    if (ms < slot.lastNow) return slot.lastNow;
    slot.lastNow = ms;
    return slot.lastNow;
  }
  mark() {}
  measure() {}
  clearMarks() {}
  clearMeasures() {}
  clearResourceTimings() {}
  getEntries() { return []; }
  getEntriesByName() { return []; }
  getEntriesByType() { return []; }
  setResourceTimingBufferSize() {}
  toJSON() { return {}; }
}
_markNative(Performance);
for (const _performanceMethod of [
  'now', 'mark', 'measure', 'clearMarks', 'clearMeasures',
  'clearResourceTimings', 'getEntries', 'getEntriesByName',
  'getEntriesByType', 'setResourceTimingBufferSize', 'toJSON',
]) _markNative(Performance.prototype[_performanceMethod]);
for (const _performanceGetter of [
  'timeOrigin', 'timing', 'navigation', 'memory',
  'eventCounts', 'interactionCount', 'onresourcetimingbufferfull',
]) {
  const _descriptor = Object.getOwnPropertyDescriptor(Performance.prototype, _performanceGetter);
  if (_descriptor?.get) _markNativeAs(_descriptor.get, `function get ${_performanceGetter}() { [native code] }`);
  if (_descriptor?.set) _markNativeAs(_descriptor.set, `function set ${_performanceGetter}() { [native code] }`);
}
Object.defineProperty(Performance.prototype, Symbol.toStringTag, {
  value: 'Performance', configurable: true,
});
Object.defineProperty(globalThis, 'Performance', {
  value: Performance, writable: true, enumerable: false, configurable: true,
});
globalThis.performance = new Performance();
