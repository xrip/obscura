'use strict';

(() => {
  const captureButton = document.getElementById('capture');
  const state = document.getElementById('state');
  const checksBody = document.getElementById('checks');
  const downloads = document.getElementById('downloads');
  const summary = document.getElementById('summary');
  const errorBox = document.getElementById('error');
  const warningBox = document.getElementById('warning');
  const saveButton = document.getElementById('save-capture');
  const profileButton = document.getElementById('download-profile');
  const windowsButton = document.getElementById('download-windows');
  const catalogState = document.getElementById('catalog-state');
  const baseSelect = document.getElementById('base-select');
  const graphicsSelect = document.getElementById('graphics-select');
  const screenSelect = document.getElementById('screen-select');
  const composedId = document.getElementById('composed-id');
  const copyIdButton = document.getElementById('copy-id');
  const selectDefaultButton = document.getElementById('select-default');
  const selectCaptureButton = document.getElementById('select-capture');
  const selectedSummary = document.getElementById('selected-summary');

  let result = null;
  let catalog = null;

  const CATALOG_URL = '/obscura/profiles/catalog';
  const CAPTURE_URL = '/obscura/profiles/capture';
  const DEFAULT_GRAPHICS_API_BROWSER_MAJOR = 145;
  const DEFAULT_TRANSPORT_BROWSER_MAJORS = [
    100, 101, 104, 105, 106, 107, 108, 109, 110, 114, 116, 117, 118, 119,
    120, 123, 124, 126, 127, 128, 129, 130, 131, 132, 133, 134, 135, 136,
    137, 138, 139, 140, 141, 142, 143, 144, 145, 146, 147, 148,
  ];

  const own = (object, key) => Object.prototype.hasOwnProperty.call(object, key);

  function plainBrandList(items) {
    return Array.from(items || [], item => ({
      brand: String(item.brand),
      version: String(item.version),
    }));
  }

  function finiteNumber(value, name) {
    const number = Number(value);
    if (!Number.isFinite(number)) throw new Error(`${name} is not a finite number`);
    return number;
  }

  function unsignedInteger(value, name) {
    const number = finiteNumber(value, name);
    if (!Number.isInteger(number) || number < 0) throw new Error(`${name} is not an unsigned integer`);
    return number;
  }

  function signedInteger(value, name) {
    const number = finiteNumber(value, name);
    if (!Number.isInteger(number)) throw new Error(`${name} is not an integer`);
    return number;
  }

  function collectScreen() {
    return {
      width: unsignedInteger(screen.width, 'screen.width'),
      height: unsignedInteger(screen.height, 'screen.height'),
      availWidth: unsignedInteger(screen.availWidth, 'screen.availWidth'),
      availHeight: unsignedInteger(screen.availHeight, 'screen.availHeight'),
      availLeft: signedInteger(screen.availLeft, 'screen.availLeft'),
      availTop: signedInteger(screen.availTop, 'screen.availTop'),
      colorDepth: unsignedInteger(screen.colorDepth, 'screen.colorDepth'),
      pixelDepth: unsignedInteger(screen.pixelDepth, 'screen.pixelDepth'),
    };
  }

  function collectWindow() {
    return {
      devicePixelRatio: finiteNumber(devicePixelRatio, 'devicePixelRatio'),
      innerHeight: unsignedInteger(innerHeight, 'innerHeight'),
      innerWidth: unsignedInteger(innerWidth, 'innerWidth'),
      outerHeight: unsignedInteger(outerHeight, 'outerHeight'),
      outerWidth: unsignedInteger(outerWidth, 'outerWidth'),
      screenX: signedInteger(screenX, 'screenX'),
      screenY: signedInteger(screenY, 'screenY'),
    };
  }

  function numericConstants(object) {
    const constants = new Map();
    const seen = new Set();
    for (let current = object; current && current !== Object.prototype; current = Object.getPrototypeOf(current)) {
      for (const name of Object.getOwnPropertyNames(current).sort()) {
        if (seen.has(name)) continue;
        seen.add(name);
        const descriptor = Object.getOwnPropertyDescriptor(current, name);
        if (!descriptor || typeof descriptor.value !== 'number') continue;
        const value = descriptor.value;
        if (!Number.isSafeInteger(value) || value < 0) continue;
        if (!constants.has(value)) constants.set(value, name);
      }
    }
    return constants;
  }

  function parameterValue(value) {
    if (Array.isArray(value)) return { type: 'Array', value: value.slice() };
    if (typeof value === 'boolean') return { type: 'Boolean', value };
    if (typeof value === 'number') return { type: 'Number', value };
    if (typeof value === 'string') return { type: 'String', value };
    for (const type of ['Float32Array', 'Int32Array', 'Uint32Array']) {
      if (globalThis[type] && value instanceof globalThis[type]) {
        return { type, value: Array.from(value) };
      }
    }
    return { type: '', value: null };
  }

  function drainErrors(gl) {
    for (let count = 0; count < 32 && gl.getError() !== gl.NO_ERROR; count += 1) {}
  }

  function collectWebGl(kind) {
    const canvas = document.createElement('canvas');
    const gl = canvas.getContext(kind);
    if (!gl) throw new Error(`${kind} context creation failed`);

    const supportedExtensions = Array.from(gl.getSupportedExtensions() || []);
    const enabledExtensions = [];
    for (const name of supportedExtensions.slice().sort()) {
      let extension = null;
      try { extension = gl.getExtension(name); } catch (_) {}
      if (extension) enabledExtensions.push({ name, object: extension });
    }

    const contextConstants = numericConstants(gl);
    const parameterEnums = new Set(contextConstants.keys());
    const extensions = {};
    for (const extension of enabledExtensions) {
      for (const [value, constantName] of numericConstants(extension.object)) {
        parameterEnums.add(value);
        const key = String(value);
        const candidate = { name: extension.name, constantName };
        if (!own(extensions, key)
            || `${candidate.name}:${candidate.constantName}` < `${extensions[key].name}:${extensions[key].constantName}`) {
          extensions[key] = candidate;
        }
      }
    }

    const parameters = {};
    for (const pname of Array.from(parameterEnums).sort((left, right) => left - right)) {
      drainErrors(gl);
      let value = null;
      let failed = false;
      try { value = gl.getParameter(pname); } catch (_) { failed = true; }
      const error = gl.getError();
      parameters[String(pname)] = failed || error !== gl.NO_ERROR
        ? { type: '', value: null }
        : parameterValue(value);
      drainErrors(gl);
    }

    const shaderPrecisionFormats = [];
    for (const shaderType of [gl.VERTEX_SHADER, gl.FRAGMENT_SHADER]) {
      for (const precisionType of [
        gl.LOW_FLOAT, gl.MEDIUM_FLOAT, gl.HIGH_FLOAT,
        gl.LOW_INT, gl.MEDIUM_INT, gl.HIGH_INT,
      ]) {
        const format = gl.getShaderPrecisionFormat(shaderType, precisionType);
        if (!format) throw new Error(`${kind} precision format ${shaderType}/${precisionType} is missing`);
        shaderPrecisionFormats.push({
          shaderType,
          precisionType,
          shaderPrecisionFormat: {
            rangeMin: signedInteger(format.rangeMin, 'rangeMin'),
            rangeMax: signedInteger(format.rangeMax, 'rangeMax'),
            precision: signedInteger(format.precision, 'precision'),
          },
        });
      }
    }

    const anisotropy = enabledExtensions.find(item => item.name.toLowerCase() === 'ext_texture_filter_anisotropic');
    const drawBuffers = enabledExtensions.find(item => item.name.toLowerCase() === 'webgl_draw_buffers');
    const maxAnisotropy = anisotropy
      ? finiteNumber(gl.getParameter(anisotropy.object.MAX_TEXTURE_MAX_ANISOTROPY_EXT), `${kind}.maxAnisotropy`)
      : 1;
    const maxDrawBuffersWebgl = kind === 'webgl2'
      ? unsignedInteger(gl.getParameter(gl.MAX_DRAW_BUFFERS), `${kind}.maxDrawBuffers`)
      : drawBuffers
        ? unsignedInteger(gl.getParameter(drawBuffers.object.MAX_DRAW_BUFFERS_WEBGL), `${kind}.maxDrawBuffers`)
        : undefined;

    const out = {
      contextAttributes: Object.assign({}, gl.getContextAttributes()),
      parameters,
      extensions,
      supportedExtensions,
      shaderPrecisionFormats,
      version: String(gl.getParameter(gl.VERSION)),
      shadingLanguageVersion: String(gl.getParameter(gl.SHADING_LANGUAGE_VERSION)),
      maxAnisotropy,
    };
    if (maxDrawBuffersWebgl !== undefined) out.maxDrawBuffersWebgl = maxDrawBuffersWebgl;

    const debug = enabledExtensions.find(item => item.name.toLowerCase() === 'webgl_debug_renderer_info');
    return {
      data: out,
      validParameterCount: Object.values(parameters).filter(item => item.type !== '').length,
      unmaskedVendor: debug ? String(gl.getParameter(debug.object.UNMASKED_VENDOR_WEBGL)) : '',
      unmaskedRenderer: debug ? String(gl.getParameter(debug.object.UNMASKED_RENDERER_WEBGL)) : '',
    };
  }

  function numericInterface(object) {
    const result = {};
    if (!object) return result;
    for (const key in object) {
      let value;
      try { value = object[key]; } catch (_) { continue; }
      if (typeof value === 'number' && Number.isFinite(value) && value >= 0) result[key] = value;
    }
    return result;
  }

  async function adapterInfo(adapter) {
    let source = adapter.info;
    if (!source && typeof adapter.requestAdapterInfo === 'function') source = await adapter.requestAdapterInfo();
    source = source || {};
    const info = {
      vendor: String(source.vendor || ''),
      architecture: String(source.architecture || ''),
      device: String(source.device || ''),
      description: String(source.description || ''),
      isFallbackAdapter: Boolean(adapter.isFallbackAdapter),
    };
    if (Number.isInteger(source.subgroupMinSize)) info.subgroupMinSize = source.subgroupMinSize;
    if (Number.isInteger(source.subgroupMaxSize)) info.subgroupMaxSize = source.subgroupMaxSize;
    return info;
  }

  async function collectAdapter(options) {
    const adapter = await navigator.gpu.requestAdapter(options);
    if (!adapter) return null;
    const device = await adapter.requestDevice();
    const entry = {
      info: await adapterInfo(adapter),
      isFallbackAdapter: Boolean(adapter.isFallbackAdapter),
      features: Array.from(adapter.features || [], String),
      limits: numericInterface(adapter.limits),
      deviceLimits: numericInterface(device.limits),
    };
    device.destroy();
    return entry;
  }

  async function collectWebGpu() {
    if (!navigator.gpu) throw new Error('navigator.gpu is missing; use the loopback HTTP page and enable hardware acceleration');
    const entries = {};
    const requests = [
      ['default', undefined],
      ['low-power', { powerPreference: 'low-power' }],
      ['high-performance', { powerPreference: 'high-performance' }],
    ];
    for (const [name, options] of requests) {
      const adapter = await collectAdapter(options);
      if (adapter) entries[name] = adapter;
    }
    if (!entries.default) throw new Error('the default WebGPU adapter is missing');
    return entries;
  }

  function fullVersionFrom(ua) {
    if (ua.uaFullVersion) return String(ua.uaFullVersion);
    const item = plainBrandList(ua.fullVersionList).find(entry =>
      entry.brand === 'Google Chrome' || entry.brand === 'Chromium');
    return item ? item.version : '';
  }

  async function collectUa() {
    if (!navigator.userAgentData) throw new Error('navigator.userAgentData is missing');
    const high = await navigator.userAgentData.getHighEntropyValues([
      'architecture', 'bitness', 'fullVersionList', 'platformVersion', 'uaFullVersion', 'wow64',
    ]);
    const fullVersion = fullVersionFrom(high);
    return {
      brands: plainBrandList(navigator.userAgentData.brands || high.brands),
      fullVersionList: plainBrandList(high.fullVersionList),
      uaFullVersion: fullVersion,
      platform: String(navigator.userAgentData.platform || high.platform || ''),
      platformVersion: String(high.platformVersion || ''),
      architecture: String(high.architecture || ''),
      bitness: String(high.bitness || ''),
    };
  }

  function check(name, ok, detail) {
    return { name, ok: Boolean(ok), detail: String(detail) };
  }

  function browserWarnings(browserVersion) {
    const browserMajor = Number(String(browserVersion).split('.')[0]);
    if (!Number.isInteger(browserMajor)) return [];
    const catalogApiMajor = Number(catalog && catalog.graphicsApiBrowserMajor);
    const apiMajor = Number.isInteger(catalogApiMajor) && catalogApiMajor > 0
      ? catalogApiMajor
      : DEFAULT_GRAPHICS_API_BROWSER_MAJOR;
    const catalogTransportMajors = catalog && Array.isArray(catalog.transportBrowserMajors)
      ? catalog.transportBrowserMajors.map(Number).filter(Number.isInteger)
      : DEFAULT_TRANSPORT_BROWSER_MAJORS;
    const warnings = [];
    if (browserMajor !== apiMajor) {
      warnings.push(`Chrome ${browserMajor} is accepted, but the current JS graphics API shape is Chrome ${apiMajor}. Cross-surface inconsistencies are possible.`);
    }
    if (!catalogTransportMajors.includes(browserMajor) && catalogTransportMajors.length) {
      const transportMajor = catalogTransportMajors.reduce((best, value) => (
        Math.abs(value - browserMajor) < Math.abs(best - browserMajor) ? value : best
      ));
      warnings.push(`No exact wreq transport exists for Chrome ${browserMajor}. Obscura will use the nearest transport, Chrome ${transportMajor}, and will give a runtime warning.`);
    }
    return warnings;
  }

  function buildChecks(data) {
    const base = data.profile.fingerprints;
    const ua = base.browser.userAgentData;
    const nav = base.browser.navigator;
    const gpu = base.hardware.gpu;
    const defaultAdapter = gpu.adapter.default;
    const chromeMajor = ua.uaFullVersion.split('.')[0];
    return [
      check('Chrome version', /^\d+\.\d+\.\d+\.\d+$/.test(ua.uaFullVersion), ua.uaFullVersion),
      check('Windows platform', ua.platform === 'Windows', `${ua.platform} ${ua.platformVersion}`),
      check('x86-64 architecture', ua.architecture === 'x86' && ua.bitness === '64', `${ua.architecture}-${ua.bitness}`),
      check('Reduced Chrome UA', nav.userAgent.includes('(Windows NT 10.0; Win64; x64)') && nav.userAgent.includes(`Chrome/${chromeMajor}.0.0.0`), nav.userAgent),
      check('WebGL debug renderer', Boolean(gpu.unmaskedVendor && gpu.unmaskedRenderer), `${gpu.unmaskedVendor} / ${gpu.unmaskedRenderer}`),
      check('ANGLE D3D11 renderer', /Direct3D11|D3D11/.test(gpu.unmaskedRenderer), gpu.unmaskedRenderer),
      check('WebGL 1 parameters', data.webgl1.validParameterCount >= 82, `${data.webgl1.validParameterCount} valid`),
      check('WebGL 2 parameters', data.webgl2.validParameterCount >= 132, `${data.webgl2.validParameterCount} valid`),
      check('WebGL precision records', data.webgl1.data.shaderPrecisionFormats.length === 12 && data.webgl2.data.shaderPrecisionFormats.length === 12, '12 + 12'),
      check('WebGPU default adapter', Boolean(defaultAdapter), defaultAdapter ? defaultAdapter.info.vendor : 'missing'),
      check('WebGPU features', Boolean(defaultAdapter && defaultAdapter.features.length), defaultAdapter ? `${defaultAdapter.features.length} features` : 'missing'),
      check('WebGPU limits', Boolean(defaultAdapter && Object.keys(defaultAdapter.limits).length && Object.keys(defaultAdapter.deviceLimits).length), defaultAdapter ? `${Object.keys(defaultAdapter.limits).length} adapter / ${Object.keys(defaultAdapter.deviceLimits).length} device` : 'missing'),
      check('Screen and window', base.hardware.screen.width > 0 && base.hardware.screen.height > 0 && base.browser.window.innerWidth > 0 && base.browser.window.innerHeight > 0, `${base.hardware.screen.width}x${base.hardware.screen.height}, DPR ${base.browser.window.devicePixelRatio}`),
    ];
  }

  async function makeCapture() {
    const ua = await collectUa();
    const screenData = collectScreen();
    const windowData = collectWindow();
    const webgl1 = collectWebGl('webgl');
    const webgl2 = collectWebGl('webgl2');
    const webgpu = await collectWebGpu();
    const preferredCanvasFormat = String(navigator.gpu.getPreferredCanvasFormat());
    const wgslLanguageFeatures = Array.from(navigator.gpu.wgslLanguageFeatures || [], String);
    const navigatorData = {
      userAgent: String(navigator.userAgent),
      languages: Array.from(navigator.languages || [], String),
      hardwareConcurrency: unsignedInteger(navigator.hardwareConcurrency, 'navigator.hardwareConcurrency'),
      deviceMemory: finiteNumber(navigator.deviceMemory, 'navigator.deviceMemory'),
      maxTouchPoints: unsignedInteger(navigator.maxTouchPoints, 'navigator.maxTouchPoints'),
    };
    const profile = {
      profileVersion: 'obscura-capture-v1',
      fingerprints: {
        system: { osType: 'win', osVersion: ua.platformVersion },
        browser: {
          version: ua.uaFullVersion,
          userAgent: navigatorData.userAgent,
          navigator: navigatorData,
          userAgentData: ua,
          window: windowData,
          webglContext: webgl1.data,
          webgl2Context: webgl2.data,
        },
        hardware: {
          cpu: { arch: ua.architecture, bitness: ua.bitness },
          screen: screenData,
          gpu: {
            unmaskedVendor: webgl2.unmaskedVendor || webgl1.unmaskedVendor,
            unmaskedRenderer: webgl2.unmaskedRenderer || webgl1.unmaskedRenderer,
            preferredCanvasFormat,
            wgslLanguageFeatures,
            adapter: webgpu,
          },
        },
      },
    };
    const windows = [{ total: 1, window: [windowData], screen: screenData }];
    const data = { profile, windows, webgl1, webgl2 };
    data.ids = await ObscuraProfileIds.idsFromProfile(profile);
    data.checks = buildChecks(data);
    return data;
  }

  function option(value, label) {
    const item = document.createElement('option');
    item.setAttribute('value', value);
    item.value = value;
    item.textContent = label;
    return item;
  }

  function optionValue(item) {
    return item ? item.getAttribute('value') || item.value : '';
  }

  function selectedValue(select) {
    return optionValue(select.options[select.selectedIndex]);
  }

  function chooseValue(select, value) {
    for (let index = 0; index < select.options.length; index += 1) {
      if (optionValue(select.options[index]) === value) {
        select.selectedIndex = index;
        return true;
      }
    }
    return false;
  }

  function baseLabel(row) {
    return `${row.platform} ${row.platformVersion} | Chrome ${row.browserVersion} | ${row.architecture}-${row.bitness} | CPU ${row.hardwareConcurrency} | RAM ${row.deviceMemory} GiB | ${row.id}`;
  }

  function graphicsLabel(row) {
    const majors = Array.from(new Set(
      Object.keys(row.observationsByBrowserVersion || {}).map(version => version.split('.')[0]),
    ));
    const versions = majors.length ? `Chrome ${majors.join(',')} | ` : '';
    return `${versions}${row.unmaskedRenderer} | ${row.id}`;
  }

  function screenLabel(row) {
    return `${row.width}x${row.height} DPR ${row.devicePixelRatio} | inner ${row.innerWidth}x${row.innerHeight} | ${row.id}`;
  }

  function findRow(name, id) {
    return catalog && catalog[name].find(row => row.id === id);
  }

  function selectedBaseVersion(baseId) {
    const base = findRow('baseProfiles', baseId);
    if (base) return base.browserVersion;
    if (result && result.ids.baseId === baseId) {
      return result.profile.fingerprints.browser.version;
    }
    return '';
  }

  function graphicsSupportsMajor(row, major) {
    return Boolean(row && Object.keys(row.observationsByBrowserVersion || {})
      .some(version => version.split('.')[0] === major));
  }

  function selectedGraphicsRow(graphicsId) {
    const row = findRow('graphicsProfiles', graphicsId);
    if (row) return row;
    if (result && result.ids.graphicsId === graphicsId) {
      const version = result.profile.fingerprints.browser.version;
      return { observationsByBrowserVersion: { [version]: 1 } };
    }
    return null;
  }

  function updateComposedId() {
    const baseId = selectedValue(baseSelect);
    const browserVersion = selectedBaseVersion(baseId);
    const browserMajor = browserVersion.split('.')[0];
    let graphicsId = selectedValue(graphicsSelect);
    if (!graphicsSupportsMajor(selectedGraphicsRow(graphicsId), browserMajor)) {
      const compatible = catalog && catalog.graphicsProfiles.find(row => (
        graphicsSupportsMajor(row, browserMajor)
      ));
      if (compatible) {
        chooseValue(graphicsSelect, compatible.id);
        graphicsId = compatible.id;
      }
    }
    const screenId = selectedValue(screenSelect);
    if (!/^\d+$/.test(browserMajor)
        || !graphicsSupportsMajor(selectedGraphicsRow(graphicsId), browserMajor)) {
      composedId.value = 'No graphics row is available for the selected Chrome major.';
      copyIdButton.disabled = true;
      return;
    }
    const id = `c${browserMajor}w1:${baseId}:${graphicsId}:${screenId}`;
    composedId.value = id;
    copyIdButton.disabled = !catalog;
    const base = findRow('baseProfiles', baseId);
    const graphics = findRow('graphicsProfiles', graphicsId);
    const screenRow = findRow('screenProfiles', screenId);
    const isNew = !base || !graphics || !screenRow;
    selectedSummary.textContent = JSON.stringify({
      status: isNew ? 'new capture; regenerate the catalog before use' : 'present in the current catalog',
      base: base ? {
        browserVersion: base.browserVersion,
        platform: base.platform,
        platformVersion: base.platformVersion,
        architecture: `${base.architecture}-${base.bitness}`,
        hardwareConcurrency: base.hardwareConcurrency,
        deviceMemory: base.deviceMemory,
      } : { id: baseId },
      graphics: graphics ? {
        vendor: graphics.unmaskedVendor,
        renderer: graphics.unmaskedRenderer,
        observationsByBrowserVersion: graphics.observationsByBrowserVersion,
        webgl1Id: graphics.webgl1Id,
        webgl2Id: graphics.webgl2Id,
        webgpuId: graphics.webgpuId,
      } : { id: graphicsId },
      screen: screenRow ? {
        size: `${screenRow.width}x${screenRow.height}`,
        devicePixelRatio: screenRow.devicePixelRatio,
        innerSize: `${screenRow.innerWidth}x${screenRow.innerHeight}`,
      } : { id: screenId },
    }, null, 2);
  }

  function defaultIds() {
    if (catalog.defaultComposition) return catalog.defaultComposition;
    const parts = String(catalog.defaultProfileId || '').split(':');
    if (parts.length !== 4 || !/^c\d+w1$/.test(parts[0])) throw new Error('catalog default profile ID is invalid');
    return { baseId: parts[1], graphicsId: parts[2], screenId: parts[3] };
  }

  function selectDefault() {
    if (!catalog) return;
    const ids = defaultIds();
    chooseValue(baseSelect, ids.baseId);
    chooseValue(graphicsSelect, ids.graphicsId);
    chooseValue(screenSelect, ids.screenId);
    updateComposedId();
  }

  function addCaptureOption(select, id, label) {
    if (!Array.from(select.options).some(item => optionValue(item) === id)) {
      select.append(option(id, `[new capture] ${label} | ${id}`));
    }
    chooseValue(select, id);
  }

  function selectCapture() {
    if (!catalog || !result) return;
    const profile = result.profile.fingerprints;
    addCaptureOption(
      baseSelect,
      result.ids.baseId,
      `${profile.browser.userAgentData.platform} ${profile.browser.userAgentData.platformVersion} | Chrome ${profile.browser.version}`,
    );
    addCaptureOption(graphicsSelect, result.ids.graphicsId, profile.hardware.gpu.unmaskedRenderer);
    addCaptureOption(
      screenSelect,
      result.ids.screenId,
      `${profile.hardware.screen.width}x${profile.hardware.screen.height} DPR ${profile.browser.window.devicePixelRatio}`,
    );
    updateComposedId();
  }

  async function loadCatalog() {
    try {
      const response = await fetch(CATALOG_URL, { cache: 'no-store' });
      if (!response.ok) throw new Error(`catalog request returned HTTP ${response.status}`);
      catalog = await response.json();
      for (const row of catalog.baseProfiles) baseSelect.append(option(row.id, baseLabel(row)));
      for (const row of catalog.graphicsProfiles) graphicsSelect.append(option(row.id, graphicsLabel(row)));
      for (const row of catalog.screenProfiles) screenSelect.append(option(row.id, screenLabel(row)));
      for (const select of [baseSelect, graphicsSelect, screenSelect]) select.disabled = false;
      selectDefaultButton.disabled = false;
      selectCaptureButton.disabled = !result;
      catalogState.textContent = `${catalog.catalogId}: ${catalog.baseProfiles.length} base, ${catalog.graphicsProfiles.length} graphics, ${catalog.screenProfiles.length} screen rows.`;
      selectDefault();
    } catch (error) {
      catalogState.textContent = `Catalog load failed: ${error.message}. Serve the repository root, not only webgl/capture.`;
      catalogState.className = 'bad';
    }
  }

  function renderChecks(items) {
    checksBody.textContent = '';
    for (const item of items) {
      const row = document.createElement('tr');
      const name = document.createElement('td');
      const value = document.createElement('td');
      name.textContent = item.name;
      value.textContent = `${item.ok ? 'PASS' : 'FAIL'}: ${item.detail}`;
      value.className = item.ok ? 'ok' : 'bad';
      row.append(name, value);
      checksBody.append(row);
    }
  }

  function downloadJson(name, value) {
    const blob = new Blob([`${JSON.stringify(value, null, 2)}\n`], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = name;
    document.body.append(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  function setDownloadState(enabled) {
    downloads.hidden = false;
    saveButton.disabled = !enabled;
    profileButton.disabled = !enabled;
    windowsButton.disabled = !enabled;
  }

  captureButton.addEventListener('click', async () => {
    captureButton.disabled = true;
    state.textContent = 'Capturing WebGL and WebGPU data...';
    errorBox.textContent = '';
    warningBox.textContent = '';
    warningBox.hidden = true;
    summary.value = 'Capture in progress...';
    result = null;
    setDownloadState(false);
    try {
      const data = await makeCapture();
      renderChecks(data.checks);
      const failed = data.checks.filter(item => !item.ok);
      const profile = data.profile.fingerprints;
      summary.value = JSON.stringify({
        browserVersion: profile.browser.version,
        platform: profile.browser.userAgentData.platform,
        platformVersion: profile.browser.userAgentData.platformVersion,
        architecture: `${profile.browser.userAgentData.architecture}-${profile.browser.userAgentData.bitness}`,
        hardwareConcurrency: profile.browser.navigator.hardwareConcurrency,
        deviceMemory: profile.browser.navigator.deviceMemory,
        renderer: profile.hardware.gpu.unmaskedRenderer,
        screen: profile.hardware.screen,
        window: profile.browser.window,
        adapters: Object.keys(profile.hardware.gpu.adapter),
        webgl1ValidParameters: data.webgl1.validParameterCount,
        webgl2ValidParameters: data.webgl2.validParameterCount,
        ids: data.ids,
      }, null, 2);
      if (failed.length) {
        state.textContent = `${failed.length} check(s) failed.`;
        errorBox.textContent = 'Downloads are off because this capture is not a consistent Chrome Windows ANGLE/D3D11 profile.';
        return;
      }
      result = data;
      setDownloadState(true);
      selectCaptureButton.disabled = !catalog;
      if (catalog) selectCapture();
      const warnings = browserWarnings(profile.browser.version);
      if (warnings.length) {
        warningBox.textContent = `Warning: ${warnings.join(' ')}`;
        warningBox.hidden = false;
      }
      state.textContent = 'Capture is valid. Save it or use the two download buttons.';
    } catch (error) {
      checksBody.innerHTML = '<tr><td colspan="2" class="bad">Capture stopped.</td></tr>';
      state.textContent = 'Capture failed.';
      errorBox.textContent = error && error.stack ? error.stack : String(error);
      summary.value = 'No valid capture.';
    } finally {
      captureButton.disabled = false;
    }
  });

  profileButton.addEventListener('click', () => result && downloadJson('obscura-profile.json', result.profile));
  windowsButton.addEventListener('click', () => result && downloadJson('obscura-windows.json', result.windows));
  saveButton.addEventListener('click', async () => {
    if (!result) return;
    saveButton.disabled = true;
    errorBox.textContent = '';
    state.textContent = 'Saving capture...';
    try {
      const response = await fetch(CAPTURE_URL, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          profile: result.profile,
          windows: result.windows,
        }),
      });
      const answer = await response.json();
      if (!response.ok || !answer.ok) throw new Error(answer.error || `save returned HTTP ${response.status}`);
      state.textContent = `Saved ${answer.saved.profile}; window rows ${answer.saved.windowRows}.`;
    } catch (error) {
      state.textContent = 'Save failed.';
      errorBox.textContent = error && error.stack ? error.stack : String(error);
    } finally {
      saveButton.disabled = false;
    }
  });
  for (const select of [baseSelect, graphicsSelect, screenSelect]) select.addEventListener('change', updateComposedId);
  selectDefaultButton.addEventListener('click', selectDefault);
  selectCaptureButton.addEventListener('click', selectCapture);
  copyIdButton.addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText(composedId.value);
      copyIdButton.textContent = 'Copied';
      setTimeout(() => { copyIdButton.textContent = 'Copy profile ID'; }, 1000);
    } catch (_) {
      composedId.select();
      document.execCommand('copy');
    }
  });

  loadCatalog();
})();
