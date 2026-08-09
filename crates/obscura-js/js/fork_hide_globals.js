// Fork-only. Spliced into bootstrap.js at /* __OBSCURA_FORK_PAGE_INIT_END__ */,
// the last statement of __obscura_init, after upstream's own hide-list loop and
// before any page script runs.
//
// WebIDL says an interface object on the global is `enumerable: false`, so
// Object.keys(window) never lists Node, Element, Event and friends. Assigning
// with `globalThis.X = X` defaults to enumerable: true, which is detectable in
// one line:
//
//   Object.keys(window).includes('Node')
//
// Upstream handles this with a hardcoded name list in _preHideInternals, which
// pre-declares those names non-enumerable so a later plain assignment updates
// only the value. That works, but the list cannot keep pace: measured against
// the pre-rebuild fork build, 138 interfaces were enumerable here that were not
// there, including Event, EventTarget, Blob, FormData and most of the HTML*
// element interfaces.
//
// A sweep is used instead of extending the list, because it also covers
// whatever upstream adds next and needs no maintenance.
(function _forkHideGlobals() {
  for (const name of Object.getOwnPropertyNames(globalThis)) {
    // Uppercase names are interface objects (Node, Event) and namespace objects
    // (CSS). Chrome exposes every one of them non-enumerable. Lowercase globals
    // are left alone: `window`, `self`, `document`, `location` and `name` are
    // legitimately enumerable, and guessing at those would break real pages.
    const internal = name.startsWith('_');
    if (!internal && !/^[A-Z]/.test(name)) continue;
    const descriptor = Object.getOwnPropertyDescriptor(globalThis, name);
    // Data properties only. Accessors are left as upstream defined them.
    if (!descriptor || !descriptor.enumerable || !('value' in descriptor)) continue;
    if (!descriptor.configurable) continue;
    try {
      Object.defineProperty(globalThis, name, {
        value: descriptor.value,
        writable: descriptor.writable,
        enumerable: false,
        configurable: true,
      });
    } catch (_) { /* a frozen global is not worth failing init over */ }
  }
})();
