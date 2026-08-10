// Fork-only Chrome 151 Navigator surface. Measured in a fresh headed Chrome
// 151 on a secure loopback page. Keep this version-gated: API exposure changes
// between Chrome releases, and adding a future surface to an older profile is
// as inconsistent as leaving members out.
(function _forkInstallChrome151Navigator() {
  const memberNames = [
    'scheduling', 'userActivation', 'webkitTemporaryStorage',
    'webkitPersistentStorage', 'windowControlsOverlay', 'vibrate', 'bluetooth',
    'virtualKeyboard', 'login', 'ink', 'devicePosture', 'hid', 'mediaSession',
    'presentation', 'serial', 'usb', 'xr', 'storageBuckets',
    'adAuctionComponents', 'runAdAuction', 'canLoadAdAuctionFencedFrame',
    'clearAppBadge', 'getUserMedia', 'requestMIDIAccess',
    'requestMediaKeySystemAccess', 'setAppBadge', 'webkitGetUserMedia',
    'clearOriginJoinedAdInterestGroups', 'createAuctionNonce',
    'joinAdInterestGroup', 'leaveAdInterestGroup', 'updateAdInterestGroups',
    'deprecatedReplaceInURN', 'deprecatedURNToURL', 'getInstalledRelatedApps',
    'getInterestGroupAdAuctionData', 'registerProtocolHandler',
    'unregisterProtocolHandler',
  ];
  for (const name of memberNames) {
    try { delete Navigator.prototype[name]; } catch (_) {}
  }
  for (const name of [
    'Scheduling', 'UserActivation', 'WindowControlsOverlay', 'Bluetooth',
    'VirtualKeyboard', 'NavigatorLogin', 'Ink', 'DevicePosture', 'HID',
    'MediaSession', 'Presentation', 'Serial', 'USB', 'XRSystem',
    'StorageBucketManager',
  ]) {
    try { delete globalThis[name]; } catch (_) {}
  }

  const major = Number((_fingerprintProfile && _fingerprintProfile.browser
    && _fingerprintProfile.browser.major) || _chromeMajor());
  if (major !== 151 || !globalThis.isSecureContext) return;

  function nativeMethod(name, length, brand, implementation) {
    const fn = {
      [name](...args) {
        if (!brand(this)) throw new TypeError('Illegal invocation');
        return implementation.apply(this, args);
      },
    }[name];
    Object.defineProperty(fn, 'length', {value:length, configurable:true});
    return _markNative(fn);
  }

  function nativeGetter(name, brand, implementation) {
    const get = Object.getOwnPropertyDescriptor({
      get value() {
        if (!brand(this)) throw new TypeError('Illegal invocation');
        return implementation.call(this);
      },
    }, 'value').get;
    Object.defineProperty(get, 'name', {value:`get ${name}`, configurable:true});
    return _markNativeAs(get, `function get ${name}() { [native code] }`);
  }

  function nativeSetter(name, brand, implementation) {
    const set = Object.getOwnPropertyDescriptor({
      set value(value) {
        if (!brand(this)) throw new TypeError('Illegal invocation');
        implementation.call(this, value);
      },
    }, 'value').set;
    Object.defineProperty(set, 'name', {value:`set ${name}`, configurable:true});
    return _markNativeAs(set, `function set ${name}() { [native code] }`);
  }

  function interfaceInstance(name, parent, members, state = {}) {
    const instances = new WeakSet();
    const C = function () {
      throw new TypeError(`Failed to construct '${name}': Illegal constructor`);
    };
    Object.defineProperty(C, 'name', {value:name, configurable:true});
    _markNative(C);
    const proto = Object.create(parent || Object.prototype);
    const brand = value => instances.has(value);
    for (const member of members) {
      if (member.kind === 'method') {
        Object.defineProperty(proto, member.name, {
          value: nativeMethod(member.name, member.length, brand,
            function (...args) { return member.call(state, ...args); }),
          writable: true, enumerable: true, configurable: true,
        });
      } else {
        const descriptor = {
          get: nativeGetter(member.name, brand, function () { return member.get(state); }),
          enumerable: true,
          configurable: true,
        };
        if (member.set) {
          descriptor.set = nativeSetter(member.name, brand,
            function (value) { member.set(state, value); });
        }
        Object.defineProperty(proto, member.name, descriptor);
      }
    }
    Object.defineProperty(proto, 'constructor', {
      value: C, writable: true, configurable: true,
    });
    Object.defineProperty(proto, Symbol.toStringTag, {value:name, configurable:true});
    C.prototype = proto;
    Object.defineProperty(globalThis, name, {
      value: C, writable: true, enumerable: false, configurable: true,
    });
    const instance = Object.create(proto);
    instances.add(instance);
    return instance;
  }

  const method = (name, length, call) => ({kind:'method', name, length, call});
  const getter = (name, get, set) => ({kind:'getter', name, get, set});
  const eventParent = typeof EventTarget === 'function'
    ? EventTarget.prototype : Object.prototype;
  const denied = () => Promise.reject(new DOMException('Permission denied', 'NotAllowedError'));
  const unsupported = () => Promise.reject(new DOMException('Not supported', 'NotSupportedError'));
  const rect = () => typeof DOMRect === 'function'
    ? new DOMRect(0, 0, 0, 0)
    : {x:0, y:0, width:0, height:0, top:0, right:0, bottom:0, left:0};

  const values = {};
  values.scheduling = interfaceInstance('Scheduling', Object.prototype, [
    method('isInputPending', 0, () => false),
  ]);
  values.userActivation = interfaceInstance('UserActivation', Object.prototype, [
    getter('hasBeenActive', () => false), getter('isActive', () => false),
  ]);

  const quotaInstances = new WeakSet();
  const quotaProto = Object.create(Object.prototype);
  const quotaBrand = value => quotaInstances.has(value);
  Object.defineProperty(quotaProto, 'queryUsageAndQuota', {
    value: nativeMethod('queryUsageAndQuota', 1, quotaBrand, function (callback) {
      if (typeof callback === 'function') queueMicrotask(() => callback(0, 0));
    }), writable:true, enumerable:true, configurable:true,
  });
  Object.defineProperty(quotaProto, 'requestQuota', {
    value: nativeMethod('requestQuota', 1, quotaBrand, function (bytes, callback) {
      if (typeof callback === 'function') queueMicrotask(() => callback(0));
    }), writable:true, enumerable:true, configurable:true,
  });
  Object.defineProperty(quotaProto, Symbol.toStringTag, {
    value:'DeprecatedStorageQuota', configurable:true,
  });
  values.webkitTemporaryStorage = Object.create(quotaProto);
  values.webkitPersistentStorage = Object.create(quotaProto);
  quotaInstances.add(values.webkitTemporaryStorage);
  quotaInstances.add(values.webkitPersistentStorage);

  values.windowControlsOverlay = interfaceInstance(
    'WindowControlsOverlay', eventParent,
    [
      getter('visible', () => false),
      getter('ongeometrychange', state => state.ongeometrychange,
        (state, value) => { state.ongeometrychange = typeof value === 'function' ? value : null; }),
      method('getTitlebarAreaRect', 0, rect),
    ],
    {ongeometrychange:null},
  );
  values.bluetooth = interfaceInstance('Bluetooth', eventParent, [
    method('getAvailability', 0, () => Promise.resolve(false)),
    method('requestDevice', 0, denied),
  ]);
  values.virtualKeyboard = interfaceInstance(
    'VirtualKeyboard', eventParent,
    [
      getter('boundingRect', rect),
      getter('overlaysContent', state => state.overlaysContent,
        (state, value) => { state.overlaysContent = Boolean(value); }),
      getter('ongeometrychange', state => state.ongeometrychange,
        (state, value) => { state.ongeometrychange = typeof value === 'function' ? value : null; }),
      method('hide', 0, () => undefined),
      method('show', 0, () => undefined),
    ],
    {overlaysContent:false, ongeometrychange:null},
  );
  values.login = interfaceInstance('NavigatorLogin', Object.prototype, [
    method('setStatus', 1, () => Promise.resolve()),
  ]);
  values.ink = interfaceInstance('Ink', Object.prototype, [
    method('requestPresenter', 0, unsupported),
  ]);
  values.devicePosture = interfaceInstance(
    'DevicePosture', eventParent,
    [
      getter('type', () => 'continuous'),
      getter('onchange', state => state.onchange,
        (state, value) => { state.onchange = typeof value === 'function' ? value : null; }),
    ],
    {onchange:null},
  );

  function deviceInterface(name, collectionName, requestName) {
    return interfaceInstance(
      name, eventParent,
      [
        getter('onconnect', state => state.onconnect,
          (state, value) => { state.onconnect = typeof value === 'function' ? value : null; }),
        getter('ondisconnect', state => state.ondisconnect,
          (state, value) => { state.ondisconnect = typeof value === 'function' ? value : null; }),
        method(collectionName, 0, () => Promise.resolve([])),
        method(requestName, name === 'Serial' ? 0 : 1, denied),
      ],
      {onconnect:null, ondisconnect:null},
    );
  }
  values.hid = deviceInterface('HID', 'getDevices', 'requestDevice');
  values.serial = deviceInterface('Serial', 'getPorts', 'requestPort');
  values.usb = deviceInterface('USB', 'getDevices', 'requestDevice');
  _forkSetPrototypeOrder(globalThis.HID, [
    'onconnect', 'ondisconnect', 'getDevices', 'constructor', 'requestDevice',
  ]);
  _forkSetPrototypeOrder(globalThis.Serial, [
    'onconnect', 'ondisconnect', 'getPorts', 'constructor', 'requestPort',
  ]);
  _forkSetPrototypeOrder(globalThis.USB, [
    'onconnect', 'ondisconnect', 'getDevices', 'constructor', 'requestDevice',
  ]);
  values.mediaSession = interfaceInstance(
    'MediaSession', Object.prototype,
    [
      getter('metadata', state => state.metadata,
        (state, value) => { state.metadata = value == null ? null : value; }),
      getter('playbackState', state => state.playbackState,
        (state, value) => { state.playbackState = String(value); }),
      method('setActionHandler', 2, () => undefined),
      method('setCameraActive', 1, () => undefined),
      method('setMicrophoneActive', 1, () => undefined),
      method('setPositionState', 0, () => undefined),
    ],
    {metadata:null, playbackState:'none'},
  );
  values.presentation = interfaceInstance(
    'Presentation', Object.prototype,
    [
      getter('defaultRequest', state => state.defaultRequest,
        (state, value) => { state.defaultRequest = value == null ? null : value; }),
      getter('receiver', () => null),
    ],
    {defaultRequest:null},
  );
  values.xr = interfaceInstance(
    'XRSystem', eventParent,
    [
      getter('ondevicechange', state => state.ondevicechange,
        (state, value) => { state.ondevicechange = typeof value === 'function' ? value : null; }),
      method('isSessionSupported', 1, () => Promise.resolve(false)),
      method('requestSession', 1, unsupported),
    ],
    {ondevicechange:null},
  );
  values.storageBuckets = interfaceInstance('StorageBucketManager', Object.prototype, [
    method('delete', 1, () => Promise.resolve()),
    method('keys', 0, () => Promise.resolve([])),
    method('open', 1, unsupported),
  ]);

  const navigatorBrand = value => value === globalThis.navigator;
  for (const [name, value] of Object.entries(values)) {
    Object.defineProperty(Navigator.prototype, name, {
      get: nativeGetter(name, navigatorBrand, () => value),
      enumerable: true, configurable: true,
    });
  }

  const navigatorMethods = {
    vibrate: [1, () => false],
    adAuctionComponents: [1, () => []],
    runAdAuction: [1, () => Promise.resolve(null)],
    canLoadAdAuctionFencedFrame: [0, () => false],
    clearAppBadge: [0, () => Promise.resolve()],
    getUserMedia: [3, function (_constraints, _success, failure) {
      if (typeof failure === 'function') queueMicrotask(() => failure(
        new DOMException('Permission denied', 'NotAllowedError')));
    }],
    requestMIDIAccess: [0, unsupported],
    requestMediaKeySystemAccess: [2, unsupported],
    setAppBadge: [0, () => Promise.resolve()],
    webkitGetUserMedia: [3, function (_constraints, _success, failure) {
      if (typeof failure === 'function') queueMicrotask(() => failure(
        new DOMException('Permission denied', 'NotAllowedError')));
    }],
    clearOriginJoinedAdInterestGroups: [1, () => Promise.resolve()],
    createAuctionNonce: [0, unsupported],
    joinAdInterestGroup: [1, denied],
    leaveAdInterestGroup: [0, () => Promise.resolve()],
    updateAdInterestGroups: [0, () => Promise.resolve()],
    deprecatedReplaceInURN: [2, () => Promise.resolve()],
    deprecatedURNToURL: [1, () => Promise.resolve(null)],
    getInstalledRelatedApps: [0, () => Promise.resolve([])],
    getInterestGroupAdAuctionData: [1, unsupported],
    registerProtocolHandler: [2, () => undefined],
    unregisterProtocolHandler: [2, () => undefined],
  };
  for (const [name, [length, implementation]] of Object.entries(navigatorMethods)) {
    Object.defineProperty(Navigator.prototype, name, {
      value: nativeMethod(name, length, navigatorBrand, implementation),
      writable: true, enumerable: true, configurable: true,
    });
  }

  // Match Chrome's measured property order. The serializer walks this list, so
  // a set-equal but differently ordered prototype is still observable.
  const chromeOrder = [
    'vendorSub', 'productSub', 'vendor', 'maxTouchPoints', 'scheduling',
    'userActivation', 'geolocation', 'doNotTrack', 'webkitTemporaryStorage',
    'webkitPersistentStorage', 'windowControlsOverlay', 'hardwareConcurrency',
    'cookieEnabled', 'appCodeName', 'appName', 'appVersion', 'platform',
    'product', 'userAgent', 'language', 'languages', 'onLine', 'webdriver',
    'plugins', 'mimeTypes', 'pdfViewerEnabled', 'connection', 'getGamepads',
    'javaEnabled', 'sendBeacon', 'vibrate', 'constructor',
    'deprecatedRunAdAuctionEnforcesKAnonymity', 'protectedAudience', 'bluetooth',
    'clipboard', 'credentials', 'keyboard', 'managed', 'mediaDevices',
    'serviceWorker', 'virtualKeyboard', 'wakeLock', 'deviceMemory',
    'userAgentData', 'locks', 'storage', 'gpu', 'login', 'ink',
    'mediaCapabilities', 'permissions', 'devicePosture', 'hid', 'mediaSession',
    'presentation', 'serial', 'usb', 'xr', 'storageBuckets',
    'adAuctionComponents', 'runAdAuction', 'canLoadAdAuctionFencedFrame',
    'canShare', 'share', 'clearAppBadge', 'getBattery', 'getUserMedia',
    'requestMIDIAccess', 'requestMediaKeySystemAccess', 'setAppBadge',
    'webkitGetUserMedia', 'clearOriginJoinedAdInterestGroups',
    'createAuctionNonce', 'joinAdInterestGroup', 'leaveAdInterestGroup',
    'updateAdInterestGroups', 'deprecatedReplaceInURN', 'deprecatedURNToURL',
    'getInstalledRelatedApps', 'getInterestGroupAdAuctionData',
    'registerProtocolHandler', 'unregisterProtocolHandler',
  ];
  const descriptors = new Map(chromeOrder.map(name => [
    name, Object.getOwnPropertyDescriptor(Navigator.prototype, name),
  ]));
  for (const name of chromeOrder) {
    const descriptor = descriptors.get(name);
    if (descriptor && descriptor.configurable) delete Navigator.prototype[name];
  }
  for (const name of chromeOrder) {
    const descriptor = descriptors.get(name);
    if (descriptor) Object.defineProperty(Navigator.prototype, name, descriptor);
  }
})();
