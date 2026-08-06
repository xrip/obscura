'use strict';

((root, factory) => {
  const api = factory();
  root.ObscuraProfileIds = api;
  if (typeof module === 'object' && module.exports) module.exports = api;
})(globalThis, () => {
  const FLOAT64 = Symbol('float64');
  const SORTED_KEYS = Symbol('sortedKeys');

  const float64 = value => ({ [FLOAT64]: Number(value) });

  function canonicalJson(value) {
    if (value && typeof value === 'object' && value[FLOAT64] !== undefined) {
      const number = value[FLOAT64];
      if (!Number.isFinite(number)) throw new Error('non-finite f64 in profile content');
      return Number.isInteger(number) ? `${number}.0` : JSON.stringify(number);
    }
    if (value === null || typeof value === 'boolean' || typeof value === 'number' || typeof value === 'string') {
      return JSON.stringify(value);
    }
    if (Array.isArray(value)) return `[${value.map(canonicalJson).join(',')}]`;
    if (value && typeof value === 'object') {
      const keys = Object.keys(value);
      if (value[SORTED_KEYS]) keys.sort();
      return `{${keys.map(key => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(',')}}`;
    }
    throw new Error(`unsupported canonical JSON value: ${typeof value}`);
  }

  function cryptoApi() {
    if (globalThis.crypto && globalThis.crypto.subtle) return globalThis.crypto;
    if (typeof require === 'function') return require('node:crypto').webcrypto;
    throw new Error('Web Crypto is not available');
  }

  async function contentId(value) {
    const bytes = new TextEncoder().encode(canonicalJson(value));
    const digest = new Uint8Array(await cryptoApi().subtle.digest('SHA-256', bytes));
    return Array.from(digest.slice(0, 16), byte => byte.toString(16).padStart(2, '0')).join('');
  }

  const sortedObject = (source, convert = value => value) => {
    const out = {};
    Object.defineProperty(out, SORTED_KEYS, { value: true });
    for (const key of Object.keys(source || {}).sort()) out[key] = convert(source[key], key);
    return out;
  };

  const uniqueSortedStrings = values => Array.from(new Set(Array.from(values || [], String))).sort();
  const uniqueStrings = values => Array.from(new Set(Array.from(values || [], String)));

  function numericObjectToArray(value) {
    if (!value || Array.isArray(value) || typeof value !== 'object') return value;
    const keys = Object.keys(value);
    if (!keys.length || keys.some(key => !/^\d+$/.test(key))) return value;
    const numbers = keys.map(Number).sort((left, right) => left - right);
    if (numbers.some((number, index) => number !== index)) return value;
    return numbers.map(number => value[String(number)]);
  }

  function mutableWebGlParameter(pname) {
    return pname === 2849
      || (pname >= 2884 && pname <= 2886)
      || (pname >= 2928 && pname <= 2932)
      || (pname >= 2960 && pname <= 2968)
      || pname === 2978
      || pname === 3024
      || pname === 3042
      || pname === 3074
      || (pname >= 3088 && pname <= 3089)
      || (pname >= 3106 && pname <= 3107)
      || (pname >= 3314 && pname <= 3317)
      || (pname >= 3330 && pname <= 3333)
      || pname === 10752
      || pname === 32773
      || pname === 32777
      || (pname >= 32823 && pname <= 32824)
      || (pname >= 32877 && pname <= 32878)
      || pname === 32926
      || pname === 32928
      || (pname >= 32938 && pname <= 32939)
      || (pname >= 32968 && pname <= 32971)
      || pname === 33170
      || pname === 34016
      || (pname >= 34816 && pname <= 34819)
      || (pname >= 34853 && pname <= 34860)
      || pname === 34877
      || pname === 35723
      || pname === 35977
      || (pname >= 36003 && pname <= 36005)
      || (pname >= 36387 && pname <= 36388)
      || (pname >= 37440 && pname <= 37441)
      || pname === 37443;
  }

  function normalizeBase(profile) {
    const fingerprints = profile.fingerprints;
    const browser = fingerprints.browser;
    const navigator = browser.navigator;
    const ua = browser.userAgentData;
    return {
      browserVersion: String(browser.version),
      userAgent: String(browser.userAgent),
      brands: Array.from(ua.brands, item => ({ brand: String(item.brand), version: String(item.version) })),
      fullVersionList: Array.from(ua.fullVersionList, item => ({ brand: String(item.brand), version: String(item.version) })),
      platform: String(ua.platform),
      platformVersion: String(ua.platformVersion),
      architecture: String(ua.architecture),
      bitness: String(ua.bitness),
      languages: Array.from(navigator.languages, String),
      hardwareConcurrency: Number(navigator.hardwareConcurrency),
      deviceMemory: float64(navigator.deviceMemory),
      maxTouchPoints: Number(navigator.maxTouchPoints),
    };
  }

  function normalizeScreen(screen, windowData) {
    return {
      width: Number(screen.width),
      height: Number(screen.height),
      availWidth: Number(screen.availWidth),
      availHeight: Number(screen.availHeight),
      availLeft: Number(screen.availLeft),
      availTop: Number(screen.availTop),
      colorDepth: Number(screen.colorDepth),
      pixelDepth: Number(screen.pixelDepth),
      devicePixelRatio: float64(windowData.devicePixelRatio),
      innerWidth: Number(windowData.innerWidth),
      innerHeight: Number(windowData.innerHeight),
      outerWidth: Number(windowData.outerWidth),
      outerHeight: Number(windowData.outerHeight),
      screenX: Number(windowData.screenX),
      screenY: Number(windowData.screenY),
    };
  }

  function normalizeWebGl(raw, webgl2) {
    const contextAttributes = sortedObject({});
    for (const key of [
      'alpha', 'antialias', 'depth', 'desynchronized', 'failIfMajorPerformanceCaveat',
      'powerPreference', 'premultipliedAlpha', 'preserveDrawingBuffer', 'stencil', 'xrCompatible',
    ]) {
      if (Object.prototype.hasOwnProperty.call(raw.contextAttributes, key)) contextAttributes[key] = raw.contextAttributes[key];
    }

    const parameters = sortedObject({});
    const initialState = sortedObject({});
    for (const key of Object.keys(raw.parameters || {}).sort()) {
      const item = raw.parameters[key];
      if (!item || !item.type) continue;
      const entry = { type: String(item.type), value: numericObjectToArray(item.value) };
      (mutableWebGlParameter(Number(key)) ? initialState : parameters)[key] = entry;
    }

    const extensions = sortedObject(raw.extensions, item => ({
      name: String(item.name),
      constantName: String(item.constantName),
    }));
    const shaderPrecisionFormats = Array.from(raw.shaderPrecisionFormats || [], item => ({
      shaderType: Number(item.shaderType),
      precisionType: Number(item.precisionType),
      rangeMin: Number(item.shaderPrecisionFormat.rangeMin),
      rangeMax: Number(item.shaderPrecisionFormat.rangeMax),
      precision: Number(item.shaderPrecisionFormat.precision),
    })).sort((left, right) => left.shaderType - right.shaderType || left.precisionType - right.precisionType);

    const out = {
      contextAttributes,
      parameters,
      initialState,
      extensions,
      supportedExtensions: uniqueSortedStrings(raw.supportedExtensions),
      shaderPrecisionFormats,
      version: String(raw.version),
      shadingLanguageVersion: String(raw.shadingLanguageVersion),
      maxAnisotropy: float64(raw.maxAnisotropy),
    };
    if (raw.maxDrawBuffersWebgl !== undefined && raw.maxDrawBuffersWebgl !== null) {
      out.maxDrawBuffersWebgl = Number(raw.maxDrawBuffersWebgl);
    }
    const expected = webgl2 ? 132 : 82;
    if (Object.keys(parameters).length + Object.keys(initialState).length < expected) {
      throw new Error(`WebGL ${webgl2 ? 2 : 1} has too few valid parameters`);
    }
    return out;
  }

  function normalizeNumericMap(source) {
    return sortedObject(source, value => Number(value));
  }

  function normalizeAdapter(raw) {
    const infoSource = raw.info && Object.keys(raw.info).length ? raw.info : raw.adapterInfo;
    const info = {
      vendor: String(infoSource.vendor),
      architecture: String(infoSource.architecture || ''),
      device: String(infoSource.device || ''),
      description: String(infoSource.description || ''),
    };
    if (infoSource.subgroupMinSize !== undefined) info.subgroupMinSize = Number(infoSource.subgroupMinSize);
    if (infoSource.subgroupMaxSize !== undefined) info.subgroupMaxSize = Number(infoSource.subgroupMaxSize);
    info.isFallbackAdapter = Boolean(
      infoSource.isFallbackAdapter !== undefined ? infoSource.isFallbackAdapter : raw.isFallbackAdapter,
    );
    return {
      info,
      features: uniqueStrings(raw.features),
      limits: normalizeNumericMap(raw.limits),
      defaultDeviceLimits: normalizeNumericMap(raw.deviceLimits),
    };
  }

  function normalizeWebGpu(raw) {
    const names = [
      ['default', 'default'],
      ['high-performance', 'highPerformance'],
      ['low-power', 'lowPower'],
    ];
    const adapters = sortedObject({});
    for (const [sourceName, outputName] of names) {
      if (raw[sourceName] !== undefined && raw[sourceName] !== null) adapters[outputName] = normalizeAdapter(raw[sourceName]);
    }
    return { adapters };
  }

  async function idsFromProfile(profile) {
    const fingerprints = profile.fingerprints;
    const browser = fingerprints.browser;
    const hardware = fingerprints.hardware;
    const base = normalizeBase(profile);
    const screen = normalizeScreen(hardware.screen, browser.window);
    const webgl1 = normalizeWebGl(browser.webglContext, false);
    const webgl2 = normalizeWebGl(browser.webgl2Context, true);
    const webgpu = normalizeWebGpu(hardware.gpu.adapter);
    const [baseId, screenId, webgl1Id, webgl2Id, webgpuId] = await Promise.all([
      contentId(base), contentId(screen), contentId(webgl1), contentId(webgl2), contentId(webgpu),
    ]);
    const graphics = {
      maskedVendor: 'WebKit',
      maskedRenderer: 'WebKit WebGL',
      unmaskedVendor: String(hardware.gpu.unmaskedVendor),
      unmaskedRenderer: String(hardware.gpu.unmaskedRenderer),
      webgl1Id,
      webgl2Id,
      webgpuId,
      preferredCanvasFormat: String(hardware.gpu.preferredCanvasFormat),
      wgslLanguageFeatures: uniqueStrings(hardware.gpu.wgslLanguageFeatures),
    };
    const graphicsId = await contentId(graphics);
    const browserMajor = base.browserVersion.split('.')[0];
    return {
      baseId,
      graphicsId,
      screenId,
      webgl1Id,
      webgl2Id,
      webgpuId,
      composedId: `c${browserMajor}w1:${baseId}:${graphicsId}:${screenId}`,
    };
  }

  function runtimeValue(value) {
    if (value && typeof value === 'object' && value[FLOAT64] !== undefined) {
      return value[FLOAT64];
    }
    if (Array.isArray(value)) return value.map(runtimeValue);
    if (value && typeof value === 'object') {
      const out = {};
      for (const key of Object.keys(value)) out[key] = runtimeValue(value[key]);
      return out;
    }
    return value;
  }

  async function digestText(text) {
    const bytes = new TextEncoder().encode(text);
    const digest = new Uint8Array(await cryptoApi().subtle.digest('SHA-256', bytes));
    return Array.from(digest, byte => byte.toString(16).padStart(2, '0')).join('');
  }

  async function runtimeFromProfile(profile, ids) {
    const fingerprints = profile.fingerprints;
    const browser = fingerprints.browser;
    const navigator = browser.navigator;
    const ua = browser.userAgentData;
    const hardware = fingerprints.hardware;
    const webgl1 = runtimeValue(normalizeWebGl(browser.webglContext, false));
    const webgl2 = runtimeValue(normalizeWebGl(browser.webgl2Context, true));
    const webgpu = runtimeValue(normalizeWebGpu(hardware.gpu.adapter));
    const browserMajor = Number(browser.version.split('.')[0]);
    const screen = hardware.screen;
    const windowData = browser.window;
    const screenProfile = {
      id: ids.screenId,
      width: Number(screen.width),
      height: Number(screen.height),
      availWidth: Number(screen.availWidth),
      availHeight: Number(screen.availHeight),
      availLeft: Number(screen.availLeft),
      availTop: Number(screen.availTop),
      colorDepth: Number(screen.colorDepth),
      pixelDepth: Number(screen.pixelDepth),
      devicePixelRatio: Number(windowData.devicePixelRatio),
      innerWidth: Number(windowData.innerWidth),
      innerHeight: Number(windowData.innerHeight),
      outerWidth: Number(windowData.outerWidth),
      outerHeight: Number(windowData.outerHeight),
      screenX: Number(windowData.screenX),
      screenY: Number(windowData.screenY),
      weight: 1,
    };
    const graphics = {
      id: ids.graphicsId,
      maskedVendor: 'WebKit',
      maskedRenderer: 'WebKit WebGL',
      unmaskedVendor: String(hardware.gpu.unmaskedVendor),
      unmaskedRenderer: String(hardware.gpu.unmaskedRenderer),
      webgl1Id: ids.webgl1Id,
      webgl2Id: ids.webgl2Id,
      webgpuId: ids.webgpuId,
      preferredCanvasFormat: String(hardware.gpu.preferredCanvasFormat),
      wgslLanguageFeatures: uniqueStrings(hardware.gpu.wgslLanguageFeatures),
      observationsByBrowserVersion: { [String(browser.version)]: 1 },
      weight: 1,
      webgl1,
      webgl2,
      webgpu,
    };
    const networkDigest = await digestText(`network-profile-v1${ids.baseId}`);
    const renderSeed = await digestText(`graphics-render-v1${ids.composedId}`);
    return {
      id: ids.composedId,
      catalogId: 'chrome-windows-v1',
      renderSeed,
      browser: {
        major: browserMajor,
        version: String(browser.version),
        userAgent: String(browser.userAgent),
      },
      navigator: {
        platform: 'Win32',
        uaPlatform: String(ua.platform),
        uaPlatformVersion: String(ua.platformVersion),
        architecture: String(ua.architecture),
        bitness: String(ua.bitness),
        brands: Array.from(ua.brands, item => ({ brand: String(item.brand), version: String(item.version) })),
        fullVersionList: Array.from(ua.fullVersionList, item => ({ brand: String(item.brand), version: String(item.version) })),
        languages: Array.from(navigator.languages, String),
        hardwareConcurrency: Number(navigator.hardwareConcurrency),
        deviceMemory: Number(navigator.deviceMemory),
        maxTouchPoints: Number(navigator.maxTouchPoints),
      },
      network: {
        downlink: (26 + parseInt(networkDigest.slice(0, 2), 16) % 9) / 20,
        rtt: 50 + (parseInt(networkDigest.slice(2, 4), 16) % 5) * 25,
        effectiveType: '4g',
        saveData: false,
      },
      screen: screenProfile,
      graphics,
    };
  }

  return {
    canonicalJson,
    contentId,
    idsFromProfile,
    runtimeFromProfile,
    normalizeBase,
    normalizeScreen,
    normalizeWebGl,
    normalizeWebGpu,
  };
});
