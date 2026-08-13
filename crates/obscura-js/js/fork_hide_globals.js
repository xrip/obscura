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

  // The sweep above, __obscura_hide_list and _preHideInternals share one blind
  // spot: all three can only hide a name that already exists. These globals are
  // created later -- by a host evaluate (set_screen_size_override, the
  // geolocation override, evaluate-with-await, the injected stylesheet) or by a
  // DOM method call (click, focus) -- so the assignment creates a fresh
  // enumerable:true property that no pass ever looked at. Measured reaching a
  // page script this way: __obscura_screen_w, __obscura_screen_h,
  // __obscura_geo_lat.
  //
  // Both steps below are needed, for two different enumeration paths:
  //
  // 1. Declaring the name keeps it out of `for (k in window)`, which reads the
  //    enumerable flag directly and cannot be intercepted. Assigning to an
  //    existing writable+configurable property only updates the value, so the
  //    descriptor survives however late the assignment lands.
  // 2. The hide list keeps it out of Object.keys, getOwnPropertyNames,
  //    Reflect.ownKeys and getOwnPropertyDescriptors, which
  //    _hideInternalsFromReflection wraps and filters by list membership, not by
  //    the enumerable flag. Step 1 alone would make this surface worse: it turns
  //    names that only appeared once a feature was used into names present on
  //    every page.
  //
  // Only absent names are declared, so this never wipes a value the host has
  // already set (__obscura_viewport_w is live by the time we get here).
  const lateAssigned = [
    '__obscura_screen_w', '__obscura_screen_h', '__obscura_screen_emulated',
    '__obscura_viewport_w', '__obscura_viewport_h',
    '__obscura_click_target', '__obscura_mouse_down', '__obscura_focused',
    '__obscura_forget_frame',
    '__obscura_inputScreenX', '__obscura_inputScreenY',
    '__obscura_geo_lat', '__obscura_geo_lon',
    '__obscura_await_meta', '__obscura_await_rejected',
    '__obscura_css', '__obscura_clone_hooks', '__obscura_fingerprint_profile',
  ];
  const hideList = Array.isArray(globalThis.__obscura_hide_list)
    ? globalThis.__obscura_hide_list
    : null;
  for (const name of lateAssigned) {
    if (!Object.getOwnPropertyDescriptor(globalThis, name)) {
      try {
        Object.defineProperty(globalThis, name, {
          value: undefined,
          writable: true,
          enumerable: false,
          configurable: true,
        });
      } catch (_) { /* as above */ }
    }
    // Runs on every navigation against one snapshot-lived array, so this has to
    // stay idempotent or the list grows without bound.
    if (hideList && !hideList.includes(name)) hideList.push(name);
  }
})();
