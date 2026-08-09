// Fork-only. Spliced at /* __OBSCURA_FORK_LATE_MODULE__ */.
//
// Two pieces of fork commit 764298d "fix stealth challenge token generation".
//
// 1. Do not read Error.stack when the page logs an Error.
//
// Upstream's _consoleFn does `a.stack || a.message` for any Error argument.
// Chrome's console transport does not read `stack` at that point, so a page can
// install a getter on Error.prototype.stack and learn that something automated
// is watching, before any inspector is involved. It is a cheap and widely used
// probe.
//
// _consoleFn is a module-local const and cannot be replaced from here, so the
// console methods are wrapped instead and Error arguments are converted to the
// string Chrome would show *before* they reach it. _consoleFn then sees a
// string and never touches the getter.
//
// 2. Chrome's console has more methods than upstream defines. A short console
// object is trivially checkable.
(function _forkConsole() {
  const console_ = globalThis.console;
  if (!console_ || typeof console_ !== 'object') return;

  const describe = value => {
    // Same shape Chrome prints for a logged Error, without reading .stack.
    const name = value.name || 'Error';
    const message = value.message || '';
    return message ? `${name}: ${message}` : name;
  };

  for (const method of ['log', 'info', 'warn', 'error', 'debug', 'trace']) {
    const original = console_[method];
    if (typeof original !== 'function') continue;
    const wrapped = function (...args) {
      for (let i = 0; i < args.length; i++) {
        if (args[i] instanceof Error) args[i] = describe(args[i]);
      }
      return original.apply(this, args);
    };
    Object.defineProperty(wrapped, 'name', { value: method, configurable: true });
    _markNative(wrapped);
    console_[method] = wrapped;
  }

  // Present in Chrome, absent upstream.
  const additions = {
    dirxml() {}, timeStamp() {}, profile() {}, profileEnd() {},
    context() { return globalThis.console; },
    createTask() { return { run(fn) { return typeof fn === 'function' ? fn() : undefined; } }; },
  };
  for (const name of Object.keys(additions)) {
    if (typeof console_[name] === 'function') continue;
    Object.defineProperty(additions[name], 'name', { value: name, configurable: true });
    _markNative(additions[name]);
    console_[name] = additions[name];
  }
})();
