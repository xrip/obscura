// Fork-only. Spliced at /* __OBSCURA_FORK_PAGE_INIT_END__ */, after
// fork_browser_shape.js so _forkLiftToPrototype is available.
//
// Two differences measured against the real Chrome over raw CDP:
//
//   new AudioContext().state    Chrome "suspended"   here "running"
//   performance.memory          Chrome [object MemoryInfo], own []
//                               here  [object Object],     own [3 fields]

// Chrome creates an AudioContext suspended and only starts it after a user
// gesture. An audio context that is already running on a freshly loaded page is
// a long-standing headless check.
//
// Upstream's constructor does `this.state = 'running'` as an own property, and
// its resume/suspend/close then assign the same property. Rather than edit that
// file, `state` becomes a prototype accessor: the setter swallows the
// constructor's initial write, and the three transition methods drive the real
// value. Assigning through a getter-only property would throw under "use
// strict" and break construction, which is why the setter has to exist.
(function _forkAudioContextStartsSuspended() {
  const Ctx = globalThis.AudioContext;
  if (typeof Ctx !== 'function' || !Ctx.prototype) return;
  if (Object.getOwnPropertyDescriptor(Ctx.prototype, 'state')) return;

  const stateOf = new WeakMap();
  const get = function () { return stateOf.get(this) || 'suspended'; };
  _markNativeAs(get, 'function get state() { [native code] }');
  Object.defineProperty(Ctx.prototype, 'state', {
    get,
    set(_value) { /* transitions go through resume/suspend/close */ },
    enumerable: true,
    configurable: true,
  });

  // Windows Chrome reports 48000; upstream derives 44100 from its own random
  // fingerprint. Every profile in the catalog is Chrome on Windows, so the rate
  // has to agree with the identity the rest of the surface claims. Same
  // swallow-the-constructor-write shape as `state` above.
  const rateGet = function () { return _fingerprintProfile ? 48000 : 44100; };
  _markNativeAs(rateGet, 'function get sampleRate() { [native code] }');
  Object.defineProperty(Ctx.prototype, 'sampleRate', {
    get: rateGet, set(_v) {}, enumerable: true, configurable: true,
  });

  for (const [method, value] of [['resume', 'running'], ['suspend', 'suspended'], ['close', 'closed']]) {
    const fn = function () { stateOf.set(this, value); return Promise.resolve(); };
    Object.defineProperty(fn, 'name', { value: method, configurable: true });
    _markNative(fn);
    Object.defineProperty(Ctx.prototype, method, {
      value: fn, writable: true, enumerable: true, configurable: true,
    });
  }
})();

// performance.memory is a MemoryInfo in Chrome, with its three fields on the
// prototype and no own properties. Chrome does not expose the MemoryInfo
// constructor on window, so this one is deliberately not published.
(function _forkMemoryInfo() {
  const memory = globalThis.performance && performance.memory;
  if (!memory || typeof memory !== 'object') return;
  if (Object.prototype.toString.call(memory) === '[object MemoryInfo]') return;

  const MemoryInfo = function () {
    throw new TypeError("Failed to construct 'MemoryInfo': Illegal constructor");
  };
  Object.defineProperty(MemoryInfo, 'name', { value: 'MemoryInfo', configurable: true });
  Object.defineProperty(MemoryInfo.prototype, 'constructor', {
    value: MemoryInfo, writable: true, configurable: true,
  });
  _markNative(MemoryInfo);
  _forkLiftToPrototype(MemoryInfo, memory, 'MemoryInfo');
})();
