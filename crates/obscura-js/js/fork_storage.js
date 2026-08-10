// Fork-only BrowserContext storage bridge. The Rust backend lives in
// origin_storage.rs; this module keeps the JavaScript side out of upstream's
// Storage implementation so future bootstrap merges need only retain one
// marker.

const __obscuraStorageSlots = new WeakMap();
const __obscuraStorageSlot = value => {
  const slot = __obscuraStorageSlots.get(value);
  if (!slot) throw new TypeError('Illegal invocation');
  return slot;
};
const __obscuraStorageSnapshot = slot => {
  if (slot.local) {
    try { return JSON.parse(Deno.core.ops.op_local_storage('snapshot', '', '')); }
    catch (_) { return []; }
  }
  return Object.keys(slot.data).map(key => [key, slot.data[key]]);
};

Storage.prototype.getItem = function(k) {
  const slot = __obscuraStorageSlot(this);
  k = String(k);
  if (slot.local) {
    try { return JSON.parse(Deno.core.ops.op_local_storage('get', k, '')); }
    catch (_) { return null; }
  }
  return Object.prototype.hasOwnProperty.call(slot.data, k) ? slot.data[k] : null;
};
Storage.prototype.setItem = function(k, v) {
  const slot = __obscuraStorageSlot(this);
  k = String(k);
  v = String(v);
  if (slot.local) {
    let stored = false;
    try { stored = JSON.parse(Deno.core.ops.op_local_storage('set', k, v)); }
    catch (_) {}
    if (!stored) {
      throw new DOMException('Setting the value exceeded the quota.', 'QuotaExceededError');
    }
    return;
  }
  slot.data[k] = v;
};
Storage.prototype.removeItem = function(k) {
  const slot = __obscuraStorageSlot(this);
  k = String(k);
  if (slot.local) Deno.core.ops.op_local_storage('remove', k, '');
  else delete slot.data[k];
};
Storage.prototype.clear = function() {
  const slot = __obscuraStorageSlot(this);
  if (slot.local) Deno.core.ops.op_local_storage('clear', '', '');
  else for (const key in slot.data) delete slot.data[key];
};
Storage.prototype.key = function(i) {
  const entries = __obscuraStorageSnapshot(__obscuraStorageSlot(this));
  i = i >>> 0;
  return i < entries.length ? entries[i][0] : null;
};
Object.defineProperty(Storage.prototype, 'length', {
  get: function() { return __obscuraStorageSnapshot(__obscuraStorageSlot(this)).length; },
  configurable: true,
});

const __obscuraMakeStorage = local => {
  const target = Object.create(Storage.prototype);
  const slot = { local: !!local, data: Object.create(null) };
  const isReal = property => property === 'constructor' || property in Storage.prototype;
  const proxy = new Proxy(target, {
    get(t, property, receiver) {
      if (typeof property === 'symbol' || isReal(property)) {
        return Reflect.get(t, property, receiver);
      }
      const value = t.getItem(property);
      return value === null ? undefined : value;
    },
    set(t, property, value, receiver) {
      if (typeof property === 'symbol' || isReal(property)) {
        return Reflect.set(t, property, value, receiver);
      }
      t.setItem(property, value);
      return true;
    },
    has(t, property) {
      if (typeof property === 'symbol' || isReal(property)) return true;
      return t.getItem(property) !== null;
    },
    deleteProperty(t, property) {
      if (typeof property === 'symbol' || isReal(property)) {
        return Reflect.deleteProperty(t, property);
      }
      t.removeItem(property);
      return true;
    },
    ownKeys() { return __obscuraStorageSnapshot(slot).map(entry => entry[0]); },
    getOwnPropertyDescriptor(t, property) {
      if (typeof property !== 'symbol') {
        const value = t.getItem(property);
        if (value !== null) {
          return { value, writable: true, enumerable: true, configurable: true };
        }
      }
      return Reflect.getOwnPropertyDescriptor(t, property);
    },
  });
  __obscuraStorageSlots.set(target, slot);
  __obscuraStorageSlots.set(proxy, slot);
  return proxy;
};

globalThis.localStorage = __obscuraMakeStorage(true);
globalThis.sessionStorage = __obscuraMakeStorage(false);
