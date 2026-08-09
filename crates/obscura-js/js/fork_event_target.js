// Fork-only. Spliced at /* __OBSCURA_FORK_LATE_MODULE__ */.
//
// Upstream aliases the global: `EventTarget === Node`. Measured against the
// pre-rebuild fork build, which is what a browser looks like:
//
//                                        here      fork / Chrome
//   EventTarget === Node                 true      false
//   EventTarget.name                     "Node"    "EventTarget"
//   EventTarget.prototype own props      51        4
//   Node.prototype inherits EventTarget  false     true
//
// `EventTarget.name === "Node"` is a one-line detection on its own, and any
// script that walks the prototype chain sees Node's 51 members, including every
// nodeType constant, sitting on what claims to be EventTarget.
//
// The three listener methods are moved onto a real EventTarget.prototype and
// Node.prototype is re-parented to it, which is the chain Chrome has:
// Element -> Node -> EventTarget -> Object. Nothing is deleted from the reachable
// surface, so `node.addEventListener` still resolves, by inheritance now.
(function _forkEventTarget() {
  const Node_ = globalThis.Node;
  if (typeof Node_ !== 'function' || globalThis.EventTarget !== Node_) return;

  // Constructible in Chrome: `new EventTarget()` is legal.
  const EventTarget_ = function EventTarget() {};
  Object.defineProperty(EventTarget_, 'name', { value: 'EventTarget', configurable: true });
  _markNative(EventTarget_);

  const proto = EventTarget_.prototype;
  Object.defineProperty(proto, 'constructor', {
    value: EventTarget_, writable: true, configurable: true,
  });
  for (const method of ['addEventListener', 'removeEventListener', 'dispatchEvent']) {
    const fn = Node_.prototype[method];
    if (typeof fn !== 'function') continue;
    Object.defineProperty(proto, method, {
      value: fn, writable: true, enumerable: true, configurable: true,
    });
    // Remove the own copy so it resolves through the chain, as in a browser.
    try { delete Node_.prototype[method]; } catch (_) { /* not configurable */ }
  }
  Object.defineProperty(proto, Symbol.toStringTag, {
    value: 'EventTarget', configurable: true,
  });

  // Element -> Node -> EventTarget -> Object.
  try { Object.setPrototypeOf(Node_.prototype, proto); } catch (_) { /* sealed */ }

  Object.defineProperty(globalThis, 'EventTarget', {
    value: EventTarget_, writable: true, enumerable: false, configurable: true,
  });
})();
