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
const _performanceCore = globalThis.Deno.core;
const _performanceSlots = new WeakMap();
const _performanceToken = {};
const _performanceTimingToken = {};
const _performanceTimingSlots = new WeakMap();
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
    const slot = {};
    for (const field of _performanceTimingFields) {
      const value = Number(values[field]);
      slot[field] = Number.isFinite(value) ? value : 0;
    }
    _performanceTimingSlots.set(this, slot);
  }
  toJSON() {
    const slot = _performanceTimingSlots.get(this);
    if (!slot) throw new TypeError('Illegal invocation');
    const result = {};
    for (const field of _performanceTimingFields) result[field] = slot[field];
    return result;
  }
}
_markNative(PerformanceTiming);
_markNative(PerformanceTiming.prototype.toJSON);
for (const field of _performanceTimingFields) {
  const getter = Object.getOwnPropertyDescriptor({
    get value() {
      const slot = _performanceTimingSlots.get(this);
      if (!slot) throw new TypeError('Illegal invocation');
      return slot[field];
    },
  }, 'value').get;
  try { Object.defineProperty(getter, 'name', { value: `get ${field}`, configurable: true }); } catch (_) {}
  _markNativeAs(getter, `function get ${field}() { [native code] }`);
  Object.defineProperty(PerformanceTiming.prototype, field, {
    get: getter, enumerable: true, configurable: true,
  });
}
Object.defineProperty(PerformanceTiming.prototype, Symbol.toStringTag, {
  value: 'PerformanceTiming', configurable: true,
});
// Non-enumerable, as WebIDL requires of an interface object: anything that
// shows up in Object.keys(window) is a one-line detection.
Object.defineProperty(globalThis, 'PerformanceTiming', {
  value: PerformanceTiming, writable: true, enumerable: false, configurable: true,
});
const _newPerformanceTiming = values => new PerformanceTiming(_performanceTimingToken, values);

const _performanceNavigationToken = {};
const _performanceNavigationSlots = new WeakMap();
class PerformanceNavigation {
  constructor(token, values={}) {
    if (token !== _performanceNavigationToken) throw new TypeError('Illegal constructor');
    _performanceNavigationSlots.set(this, {
      type: Number(values.type) || 0,
      redirectCount: Number(values.redirectCount) || 0,
    });
  }
  toJSON() {
    const slot = _performanceNavigationSlots.get(this);
    if (!slot) throw new TypeError('Illegal invocation');
    return { type: slot.type, redirectCount: slot.redirectCount };
  }
}
_markNative(PerformanceNavigation);
_markNative(PerformanceNavigation.prototype.toJSON);
for (const field of ['type', 'redirectCount']) {
  const getter = Object.getOwnPropertyDescriptor({
    get value() {
      const slot = _performanceNavigationSlots.get(this);
      if (!slot) throw new TypeError('Illegal invocation');
      return slot[field];
    },
  }, 'value').get;
  try { Object.defineProperty(getter, 'name', { value: `get ${field}`, configurable: true }); } catch (_) {}
  _markNativeAs(getter, `function get ${field}() { [native code] }`);
  Object.defineProperty(PerformanceNavigation.prototype, field, {
    get: getter, enumerable: true, configurable: true,
  });
}
for (const [name, value] of [
  ['TYPE_NAVIGATE', 0], ['TYPE_RELOAD', 1], ['TYPE_BACK_FORWARD', 2], ['TYPE_RESERVED', 255],
]) {
  const descriptor = { value, writable: false, enumerable: true, configurable: false };
  Object.defineProperty(PerformanceNavigation, name, descriptor);
  Object.defineProperty(PerformanceNavigation.prototype, name, descriptor);
}
Object.defineProperty(PerformanceNavigation.prototype, Symbol.toStringTag, {
  value: 'PerformanceNavigation', configurable: true,
});
Object.defineProperty(globalThis, 'PerformanceNavigation', {
  value: PerformanceNavigation, writable: true, enumerable: false, configurable: true,
});
const _newPerformanceNavigation = values => new PerformanceNavigation(_performanceNavigationToken, values);
const _performanceEntryToken = {};
const _performanceEntrySlots = new WeakMap();
let _performanceNavigationId = null;
const _currentPerformanceNavigationId = () => {
  if (_performanceNavigationId === null) {
    _performanceNavigationId = Math.floor(_fpRand(642) * 10000);
  }
  return _performanceNavigationId;
};
const _performanceEntryBrand = value => {
  const slot = _performanceEntrySlots.get(value);
  if (!slot) throw new TypeError('Illegal invocation');
  return slot;
};
class PerformanceEntry {
  constructor(token, values) {
    if (token !== _performanceEntryToken) throw new TypeError('Illegal constructor');
    _performanceEntrySlots.set(this, {
      ...values, navigationId: _currentPerformanceNavigationId(),
    });
  }
  get name() { return _performanceEntryBrand(this).name; }
  get entryType() { return _performanceEntryBrand(this).entryType; }
  get startTime() { return _performanceEntryBrand(this).startTime; }
  get duration() { return _performanceEntryBrand(this).duration; }
  toJSON() {
    const slot = _performanceEntryBrand(this);
    return {
      name: slot.name,
      entryType: slot.entryType,
      startTime: slot.startTime,
      duration: slot.duration,
      navigationId: slot.navigationId,
    };
  }
}
_markNative(PerformanceEntry);
_markNative(PerformanceEntry.prototype.toJSON);
for (const name of ['name', 'entryType', 'startTime', 'duration']) {
  const descriptor = Object.getOwnPropertyDescriptor(PerformanceEntry.prototype, name);
  _markNativeAs(descriptor.get, `function get ${name}() { [native code] }`);
  Object.defineProperty(PerformanceEntry.prototype, name, { ...descriptor, enumerable: true });
}
const _performanceEntryConstructor = Object.getOwnPropertyDescriptor(
  PerformanceEntry.prototype, 'constructor');
delete PerformanceEntry.prototype.constructor;
Object.defineProperty(PerformanceEntry.prototype, 'constructor', _performanceEntryConstructor);
const _performanceNavigationIdGetter = function() {
  return _performanceEntryBrand(this).navigationId;
};
try {
  Object.defineProperty(_performanceNavigationIdGetter, 'name', {
    value: 'get navigationId', configurable: true,
  });
} catch (_) {}
_markNativeAs(
  _performanceNavigationIdGetter,
  'function get navigationId() { [native code] }',
);
Object.defineProperty(PerformanceEntry.prototype, 'navigationId', {
  get: _performanceNavigationIdGetter,
  enumerable: true,
  configurable: true,
});
Object.defineProperty(PerformanceEntry.prototype, Symbol.toStringTag, {
  value: 'PerformanceEntry', configurable: true,
});
Object.defineProperty(globalThis, 'PerformanceEntry', {
  value: PerformanceEntry, writable: true, enumerable: false, configurable: true,
});

const _performanceMarkSlots = new WeakMap();
class PerformanceMark extends PerformanceEntry {
  constructor(name, options={}) {
    if (arguments.length < 1) {
      throw new TypeError("Failed to construct 'PerformanceMark': 1 argument required, but only 0 present.");
    }
    const dictionary = options && typeof options === 'object' ? options : {};
    const startTime = dictionary.startTime === undefined
      ? globalThis.performance.now()
      : Number(dictionary.startTime);
    if (!Number.isFinite(startTime) || startTime < 0) {
      throw new TypeError("Failed to construct 'PerformanceMark': startTime must be a finite non-negative number.");
    }
    super(_performanceEntryToken, {
      name: String(name), entryType: 'mark', startTime, duration: 0,
    });
    _performanceMarkSlots.set(this, dictionary.detail === undefined ? null : dictionary.detail);
  }
  get detail() {
    if (!_performanceMarkSlots.has(this)) throw new TypeError('Illegal invocation');
    return _performanceMarkSlots.get(this);
  }
}
_markNative(PerformanceMark);
const _performanceMarkDetail = Object.getOwnPropertyDescriptor(PerformanceMark.prototype, 'detail');
_markNativeAs(_performanceMarkDetail.get, 'function get detail() { [native code] }');
Object.defineProperty(PerformanceMark.prototype, 'detail', {
  ..._performanceMarkDetail, enumerable: true,
});
const _performanceMarkConstructor = Object.getOwnPropertyDescriptor(
  PerformanceMark.prototype, 'constructor');
delete PerformanceMark.prototype.constructor;
Object.defineProperty(PerformanceMark.prototype, 'constructor', _performanceMarkConstructor);
Object.defineProperty(PerformanceMark.prototype, Symbol.toStringTag, {
  value: 'PerformanceMark', configurable: true,
});
Object.defineProperty(globalThis, 'PerformanceMark', {
  value: PerformanceMark, writable: true, enumerable: false, configurable: true,
});

const _performanceBrand = value => {
  const slot = _performanceSlots.get(value);
  if (!slot) throw new TypeError('Illegal invocation');
  return slot;
};

class Performance {
  constructor(token) {
    if (token !== _performanceToken) throw new TypeError('Illegal constructor');
    _performanceSlots.set(this, {
      timeOrigin: 0,
      timing: _newPerformanceTiming({}),
      navigation: _newPerformanceNavigation({}),
      memory: {
        jsHeapSizeLimit: 4294705152,
        totalJSHeapSize: 19321856,
        usedJSHeapSize: 16781520,
      },
      lastNow: -Infinity,
      monotonicOrigin: null,
      wallOffset: 0,
      entries: [],
    });
  }
  get timeOrigin() { return _performanceBrand(this).timeOrigin; }
  set timeOrigin(value) {
    const slot = _performanceBrand(this);
    slot.timeOrigin = Number(value) || 0;
    slot.lastNow = -Infinity;
    slot.monotonicOrigin = null;
  }
  get timing() { return _performanceBrand(this).timing; }
  set timing(value) {
    _performanceBrand(this).timing = value instanceof PerformanceTiming
      ? value
      : _newPerformanceTiming(value || {});
  }
  get navigation() { return _performanceBrand(this).navigation; }
  set navigation(value) {
    _performanceBrand(this).navigation = value instanceof PerformanceNavigation
      ? value
      : _newPerformanceNavigation(value || {});
  }
  get memory() { return _performanceBrand(this).memory; }
  set memory(value) { _performanceBrand(this).memory = value; }
  get eventCounts() { _performanceBrand(this); return new Map(); }
  get interactionCount() { _performanceBrand(this); return 0; }
  get onresourcetimingbufferfull() { _performanceBrand(this); return null; }
  set onresourcetimingbufferfull(_) { _performanceBrand(this); }
  now() {
    const slot = _performanceBrand(this);
    const clock = typeof _performanceCore.ops.op_monotonic_time_ms === 'function'
      ? _performanceCore.ops.op_monotonic_time_ms()
      : Date.now();
    if (slot.monotonicOrigin === null) {
      slot.monotonicOrigin = clock;
      slot.wallOffset = Date.now() - slot.timeOrigin;
    }
    const ms = slot.wallOffset + (clock - slot.monotonicOrigin);
    if (ms < slot.lastNow) return slot.lastNow;
    slot.lastNow = ms;
    return slot.lastNow;
  }
  mark(name, options) {
    if (arguments.length < 1) {
      throw new TypeError("Failed to execute 'mark' on 'Performance': 1 argument required, but only 0 present.");
    }
    const slot = _performanceBrand(this);
    const entry = new PerformanceMark(name, options);
    slot.entries.push(entry);
    return entry;
  }
  measure() {}
  clearMarks(name) {
    const slot = _performanceBrand(this);
    if (name === undefined) {
      slot.entries = slot.entries.filter(entry => entry.entryType !== 'mark');
      return;
    }
    const key = String(name);
    slot.entries = slot.entries.filter(entry =>
      entry.entryType !== 'mark' || entry.name !== key);
  }
  clearMeasures() {}
  clearResourceTimings() {}
  getEntries() {
    return _performanceBrand(this).entries.slice().sort((left, right) =>
      left.startTime - right.startTime);
  }
  getEntriesByName(name, type) {
    if (arguments.length < 1) {
      throw new TypeError("Failed to execute 'getEntriesByName' on 'Performance': 1 argument required, but only 0 present.");
    }
    const key = String(name);
    const entryType = type === undefined ? null : String(type);
    return this.getEntries().filter(entry =>
      entry.name === key && (entryType === null || entry.entryType === entryType));
  }
  getEntriesByType(type) {
    if (arguments.length < 1) {
      throw new TypeError("Failed to execute 'getEntriesByType' on 'Performance': 1 argument required, but only 0 present.");
    }
    const entryType = String(type);
    return this.getEntries().filter(entry => entry.entryType === entryType);
  }
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
globalThis.performance = new Performance(_performanceToken);
