// Fork-only. Spliced into bootstrap.js at
// /* __OBSCURA_FORK_LATE_PAGE_INIT__ */, inside __obscura_init and after
// upstream has assigned performance.timeOrigin, .timing and .memory.
//
// Upstream builds `performance` as a plain object literal with every method as
// an own property. In Chrome it is a `Performance` instance: the methods live on
// Performance.prototype, `constructor.name` is "Performance",
// Object.prototype.toString reports "[object Performance]", and `toJSON` exists
// on Performance, PerformanceTiming and PerformanceNavigation.
//
// This is not cosmetic. Ozon's anti-bot challenge calls
// `performance[...].toJSON()` and dies with
// "TypeError: performance[b[127]].toJSON is not a function", so the challenge
// never completes and the page stays on "Please enable JavaScript".
//
// The object is reshaped in place, never replaced: bootstrap hands the same
// reference to workers and other realms, and __obscura_init reassigns
// timeOrigin/timing/memory on it every navigation.

(function _forkUpgradePerformance() {
  const perf = globalThis.performance;
  if (!perf || typeof perf !== 'object') return;

  // Classes are built once and reused across navigations, so `Performance`
  // keeps a stable identity for the life of the isolate, as it does in a
  // browser tab.
  if (!globalThis.Performance) {
    const _defineIface = (name, members) => {
      const C = function () { throw new TypeError('Illegal constructor'); };
      Object.defineProperty(C, 'name', { value: name, configurable: true });
      _markNative(C);
      C.prototype = Object.create(Object.prototype);
      Object.defineProperty(C.prototype, 'constructor', {
        value: C, writable: true, configurable: true,
      });
      // Backs Object.prototype.toString.call(x) === "[object <name>]".
      Object.defineProperty(C.prototype, Symbol.toStringTag, {
        value: name, configurable: true,
      });
      for (const key of Object.keys(members)) {
        Object.defineProperty(C.prototype, key, {
          value: _markNative(members[key]), writable: true, configurable: true,
        });
      }
      Object.defineProperty(globalThis, name, {
        value: C, writable: true, enumerable: false, configurable: true,
      });
      return C;
    };

    // PerformanceTiming and PerformanceNavigation serialize every own numeric
    // field, which is what toJSON does in Chrome.
    const _plainToJSON = function toJSON() {
      const out = {};
      for (const key of Object.keys(this)) {
        const value = this[key];
        if (typeof value !== 'function') out[key] = value;
      }
      return out;
    };
    _defineIface('PerformanceTiming', { toJSON: _plainToJSON });
    _defineIface('PerformanceNavigation', { toJSON: _plainToJSON });

    // Move the object literal's own methods onto the prototype, so a page sees
    // them inherited exactly as in Chrome rather than as own properties.
    const members = {};
    for (const key of Object.keys(perf)) {
      if (typeof perf[key] === 'function') members[key] = perf[key];
    }
    members.toJSON = function toJSON() {
      return {
        timeOrigin: this.timeOrigin,
        timing: this.timing && typeof this.timing.toJSON === 'function'
          ? this.timing.toJSON() : this.timing,
        navigation: this.navigation && typeof this.navigation.toJSON === 'function'
          ? this.navigation.toJSON() : this.navigation,
      };
    };
    _defineIface('Performance', members);
  }

  // Reshape, per navigation.
  for (const key of Object.keys(perf)) {
    if (typeof perf[key] === 'function') delete perf[key];
  }
  Object.setPrototypeOf(perf, globalThis.Performance.prototype);

  // Chrome always exposes performance.navigation; upstream never sets it.
  if (!perf.navigation || typeof perf.navigation !== 'object') {
    perf.navigation = { type: 0, redirectCount: 0 };
  }
  if (perf.timing && typeof perf.timing === 'object') {
    Object.setPrototypeOf(perf.timing, globalThis.PerformanceTiming.prototype);
  }
  Object.setPrototypeOf(perf.navigation, globalThis.PerformanceNavigation.prototype);
})();
