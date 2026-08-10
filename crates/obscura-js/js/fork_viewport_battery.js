// Fork-only. Spliced after fork_event_target.js at startup-snapshot time.
// Ozon walks both interfaces, so plain object shims are not enough: Chrome has
// no own fields here, uses WebIDL accessors, and inherits from EventTarget.

const _forkVisualViewportState = new WeakMap();
const _forkVisualViewportToken = {};
class VisualViewport {
  constructor(token, width, height) {
    if (token !== _forkVisualViewportToken) {
      throw new TypeError("Failed to construct 'VisualViewport': Illegal constructor");
    }
    _forkVisualViewportState.set(this, {
      offsetLeft: 0, offsetTop: 0, pageLeft: 0, pageTop: 0,
      width, height, scale: 1,
      onresize: null, onscroll: null, onscrollend: null,
    });
  }
  get offsetLeft() { return _forkVisualViewportState.get(this).offsetLeft; }
  get offsetTop() { return _forkVisualViewportState.get(this).offsetTop; }
  get pageLeft() { return _forkVisualViewportState.get(this).pageLeft; }
  get pageTop() { return _forkVisualViewportState.get(this).pageTop; }
  get width() { return _forkVisualViewportState.get(this).width; }
  get height() { return _forkVisualViewportState.get(this).height; }
  get scale() { return _forkVisualViewportState.get(this).scale; }
  get onresize() { return _forkVisualViewportState.get(this).onresize; }
  set onresize(value) {
    _forkVisualViewportState.get(this).onresize = typeof value === 'function' ? value : null;
  }
  get onscroll() { return _forkVisualViewportState.get(this).onscroll; }
  set onscroll(value) {
    _forkVisualViewportState.get(this).onscroll = typeof value === 'function' ? value : null;
  }
  get onscrollend() { return _forkVisualViewportState.get(this).onscrollend; }
  set onscrollend(value) {
    _forkVisualViewportState.get(this).onscrollend = typeof value === 'function' ? value : null;
  }
}
const _forkVisualViewportNames = [
  'offsetLeft', 'offsetTop', 'pageLeft', 'pageTop', 'width', 'height', 'scale',
  'onresize', 'onscroll', 'onscrollend',
];
for (const name of _forkVisualViewportNames) {
  const descriptor = Object.getOwnPropertyDescriptor(VisualViewport.prototype, name);
  descriptor.enumerable = true;
  if (typeof descriptor.get === 'function') {
    _markNativeAs(descriptor.get, `function get ${name}() { [native code] }`);
  }
  if (typeof descriptor.set === 'function') {
    _markNativeAs(descriptor.set, `function set ${name}() { [native code] }`);
  }
  Object.defineProperty(VisualViewport.prototype, name, descriptor);
}
const _forkVisualViewportConstructor = Object.getOwnPropertyDescriptor(
  VisualViewport.prototype, 'constructor');
delete VisualViewport.prototype.constructor;
Object.defineProperty(VisualViewport.prototype, 'constructor', _forkVisualViewportConstructor);
Object.setPrototypeOf(VisualViewport.prototype, EventTarget.prototype);
Object.defineProperty(VisualViewport.prototype, Symbol.toStringTag, {
  value: 'VisualViewport', configurable: true,
});
_markNative(VisualViewport);
Object.defineProperty(globalThis, 'VisualViewport', {
  value: VisualViewport, writable: true, enumerable: false, configurable: true,
});

const _forkInitialVisualViewport = globalThis.visualViewport;
let _forkVisualViewportInstance = new VisualViewport(
  _forkVisualViewportToken,
  Number(_forkInitialVisualViewport && _forkInitialVisualViewport.width) || 1920,
  Number(_forkInitialVisualViewport && _forkInitialVisualViewport.height) || 1000,
);
function _forkSetVisualViewportSize(width, height) {
  const state = _forkVisualViewportState.get(_forkVisualViewportInstance);
  if (!state) return;
  if (Number.isFinite(width) && width > 0) state.width = width;
  if (Number.isFinite(height) && height > 0) state.height = height;
}
const _forkWindowVisualViewport = {
  get visualViewport() { return _forkVisualViewportInstance; },
};
const _forkWindowVisualViewportDescriptor = Object.getOwnPropertyDescriptor(
  _forkWindowVisualViewport, 'visualViewport');
_markNativeAs(
  _forkWindowVisualViewportDescriptor.get,
  'function get visualViewport() { [native code] }',
);
Object.defineProperty(globalThis, 'visualViewport', {
  get: _forkWindowVisualViewportDescriptor.get,
  enumerable: true,
  configurable: true,
});
Object.defineProperty(globalThis, '__obscura_set_visual_viewport_size', {
  value: _forkSetVisualViewportSize,
  writable: false,
  enumerable: false,
  configurable: false,
});

const _forkBatteryState = new WeakMap();
const _forkBatteryToken = {};
class BatteryManager {
  constructor(token, values) {
    if (token !== _forkBatteryToken) {
      throw new TypeError("Failed to construct 'BatteryManager': Illegal constructor");
    }
    _forkBatteryState.set(this, {
      charging: values.charging,
      chargingTime: values.chargingTime,
      dischargingTime: values.dischargingTime,
      level: values.level,
      onchargingchange: null,
      onchargingtimechange: null,
      ondischargingtimechange: null,
      onlevelchange: null,
    });
  }
  get charging() { return _forkBatteryState.get(this).charging; }
  get chargingTime() { return _forkBatteryState.get(this).chargingTime; }
  get dischargingTime() { return _forkBatteryState.get(this).dischargingTime; }
  get level() { return _forkBatteryState.get(this).level; }
  get onchargingchange() { return _forkBatteryState.get(this).onchargingchange; }
  set onchargingchange(value) {
    _forkBatteryState.get(this).onchargingchange = typeof value === 'function' ? value : null;
  }
  get onchargingtimechange() { return _forkBatteryState.get(this).onchargingtimechange; }
  set onchargingtimechange(value) {
    _forkBatteryState.get(this).onchargingtimechange = typeof value === 'function' ? value : null;
  }
  get ondischargingtimechange() { return _forkBatteryState.get(this).ondischargingtimechange; }
  set ondischargingtimechange(value) {
    _forkBatteryState.get(this).ondischargingtimechange = typeof value === 'function' ? value : null;
  }
  get onlevelchange() { return _forkBatteryState.get(this).onlevelchange; }
  set onlevelchange(value) {
    _forkBatteryState.get(this).onlevelchange = typeof value === 'function' ? value : null;
  }
}
const _forkBatteryNames = [
  'charging', 'chargingTime', 'dischargingTime', 'level',
  'onchargingchange', 'onchargingtimechange', 'ondischargingtimechange', 'onlevelchange',
];
for (const name of _forkBatteryNames) {
  const descriptor = Object.getOwnPropertyDescriptor(BatteryManager.prototype, name);
  descriptor.enumerable = true;
  if (typeof descriptor.get === 'function') {
    _markNativeAs(descriptor.get, `function get ${name}() { [native code] }`);
  }
  if (typeof descriptor.set === 'function') {
    _markNativeAs(descriptor.set, `function set ${name}() { [native code] }`);
  }
  Object.defineProperty(BatteryManager.prototype, name, descriptor);
}
const _forkBatteryConstructor = Object.getOwnPropertyDescriptor(
  BatteryManager.prototype, 'constructor');
delete BatteryManager.prototype.constructor;
Object.defineProperty(BatteryManager.prototype, 'constructor', _forkBatteryConstructor);
Object.setPrototypeOf(BatteryManager.prototype, EventTarget.prototype);
Object.defineProperty(BatteryManager.prototype, Symbol.toStringTag, {
  value: 'BatteryManager', configurable: true,
});
_markNative(BatteryManager);
Object.defineProperty(globalThis, 'BatteryManager', {
  value: BatteryManager, writable: true, enumerable: false, configurable: true,
});

let _forkBatteryInstance = new BatteryManager(_forkBatteryToken, {
  charging: true,
  chargingTime: 0,
  dischargingTime: Infinity,
  level: 1,
});
function _forkResetBattery() {
  const charging = Boolean(_fp('batteryCharging'));
  _forkBatteryInstance = new BatteryManager(_forkBatteryToken, {
    charging,
    chargingTime: charging ? 0 : Infinity,
    dischargingTime: charging ? Infinity : Math.floor(3600 + _fpRand(250) * 7200),
    level: _fp('batteryLevel'),
  });
}
const _forkNavigatorBattery = {
  getBattery() { return Promise.resolve(_forkBatteryInstance); },
};
const _forkGetBattery = _forkNavigatorBattery.getBattery;
_markNative(_forkGetBattery);
Object.defineProperty(globalThis.navigator, 'getBattery', {
  value: _forkGetBattery, writable: true, enumerable: true, configurable: true,
});
