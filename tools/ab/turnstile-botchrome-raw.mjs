// Strict Turnstile check with the prepared Chrome build.
// Runtime.enable and Debugger.enable are never sent.

import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import net from 'node:net';
import { join, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..', '..');
const engine = process.env.TURNSTILE_RAW_ENGINE || 'chrome';
const chrome = process.env.BOT_CHROME_BIN || 'C:\\chrome-bin\\chrome.exe';
const profile = process.env.BOT_CHROME_PROFILE || 'C:\\Chrome-bin\\offlinekiana.json';
const userDataDir = process.env.BOT_CHROME_USER_DATA_DIR || 'C:\\Chrome-bin\\9';
const chromeBotInternal = process.env.TURNSTILE_RAW_CHROME_BOT_INTERNAL !== '0';
const chromeBotProfile = process.env.TURNSTILE_RAW_CHROME_BOT_PROFILE !== '0';
const obscuraBin = join(root, 'target', 'release', 'obscura.exe');
const profileId = process.env.TURNSTILE_RAW_PROFILE;
const profileWorkbenchDir = process.env.TURNSTILE_RAW_PROFILE_WORKBENCH_DIR;
const url = process.env.TURNSTILE_URL || 'https://turnstile-test.vercel.app/';
const rootViewport = process.env.TURNSTILE_RAW_ROOT_VIEWPORT?.match(/^(\d+)x(\d+)$/)?.slice(1).map(Number);
const initialUrl = engine === 'obscura' && rootViewport ? 'about:blank' : url;
const stamp = Math.floor(Date.now() / 1000);
const dumpDir = join(root, 'target', 'turnstile-dumps', `botchrome-raw-${stamp}`);
await mkdir(dumpDir, { recursive: true });

const port = await new Promise((done, fail) => {
  const server = net.createServer();
  server.once('error', fail);
  server.listen(0, '127.0.0.1', () => {
    const { port } = server.address();
    server.close(() => done(port));
  });
});

const env = { ...process.env };
for (const name of ['OBSCURA_PROXY', 'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY',
                    'http_proxy', 'https_proxy', 'all_proxy']) {
  delete env[name];
  delete process.env[name];
}

const args = engine === 'obscura'
  ? [
    '--stealth',
    'serve',
    '--port', String(port),
    ...(profileWorkbenchDir ? ['--profile-workbench-dir', resolve(profileWorkbenchDir)] : []),
  ]
  : [
    '--no-sandbox',
    ...(chromeBotProfile ? [`--bot-profile=${profile}`] : []),
    `--user-data-dir=${userDataDir}`,
    ...(chromeBotInternal ? ['--bot-internal'] : []),
    '--no-first-run',
    '--no-default-browser-check',
    '--enable-logging',
    '--v=1',
    `--remote-debugging-port=${port}`,
    '--no-proxy-server',
    url,
  ];
const executable = engine === 'obscura' ? obscuraBin : chrome;
const child = spawn(executable, args, { cwd: root, env, stdio: ['ignore', 'ignore', 'inherit'], windowsHide: true });
const sleep = ms => new Promise(done => setTimeout(done, ms));

async function browserWebSocket() {
  const deadline = Date.now() + 30000;
  for (;;) {
    try {
      const version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
      if (version.webSocketDebuggerUrl) return version.webSocketDebuggerUrl;
    } catch { /* Chrome is still starting. */ }
    if (Date.now() > deadline) throw new Error('Chrome did not open its browser CDP endpoint');
    await sleep(200);
  }
}

function connect(endpoint) {
  const socket = new WebSocket(endpoint);
  const pending = new Map();
  const listeners = new Set();
  let nextId = 0;
  socket.addEventListener('message', event => {
    const message = JSON.parse(event.data);
    const request = pending.get(message.id);
    if (request) {
      pending.delete(message.id);
      if (message.error) request.fail(new Error(`${request.method}: ${message.error.message}`));
      else request.done(message.result);
      return;
    }
    for (const listener of listeners) listener(message);
  });
  const ready = new Promise((done, fail) => {
    socket.addEventListener('open', done);
    socket.addEventListener('error', fail);
  });
  socket.addEventListener('close', () => {
    const error = new Error('CDP connection closed');
    for (const request of pending.values()) request.fail(error);
    pending.clear();
    listeners.clear();
  });
  const send = (method, params = {}, sessionId) => new Promise((done, fail) => {
    const id = ++nextId;
    pending.set(id, { done, fail, method });
    socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
  });
  return { ready, send, onMessage: listener => listeners.add(listener), close: () => socket.close() };
}

async function getDomNodes(client, sessionId) {
  try {
    const { nodes } = await client.send('DOM.getFlattenedDocument', { depth: -1, pierce: true }, sessionId);
    return nodes || [];
  } catch (error) {
    if (!String(error).includes('Unknown DOM method: getFlattenedDocument')) throw error;
    const { root } = await client.send('DOM.getDocument', { depth: -1, pierce: true }, sessionId);
    const nodes = [];
    const visit = (node, parentId = 0) => {
      if (!node) return;
      nodes.push({ ...node, parentId });
      for (const child of node.children || []) visit(child, node.nodeId);
    };
    visit(root);
    return nodes;
  }
}

const responses = new Map();
const requests = new Map();
const requestExtraHeaders = new Map();
const sessions = new Set();
const targetSessions = new Map();
const sessionTargets = new Map();
const bodyPromises = new Set();
const MAX_NETWORK_RECORDS = 4096;

function setBounded(map, key, value) {
  if (!map.has(key) && map.size >= MAX_NETWORK_RECORDS) {
    map.delete(map.keys().next().value);
  }
  map.set(key, value);
}
let cdp;
let frameCdp;
let frameSessionId;
let pageSession;
let finalState = {};
let frameCandidates = [];
let rawShadowTrace = {};
let rawClickNodes = [];

function findChallengeFrame(frameTree) {
  if (!frameTree) return undefined;
  if (frameTree.frame?.url?.includes('challenges.cloudflare.com/cdn-cgi/challenge-platform')) {
    return frameTree.frame;
  }
  for (const child of frameTree.childFrames || []) {
    const found = findChallengeFrame(child);
    if (found) return found;
  }
  return undefined;
}

async function enableNetwork(sessionId) {
  if (!sessionId || sessions.has(sessionId)) return;
  sessions.add(sessionId);
  await cdp.send('Network.enable', {}, sessionId);
}

function trackNetwork(message, client, channel, sessionId) {
  if (message.method === 'Network.requestWillBeSent') {
    const request = message.params.request;
    const key = `${channel}:${message.params.requestId}`;
    const extra = requestExtraHeaders.get(key) || {};
    const header = name => Object.entries(request.headers || {})
      .find(([key]) => key.toLowerCase() === name)?.[1];
    setBounded(requests, key, {
      method: request.method,
      url: request.url,
      postData: request.postData,
      allHeaders: { ...extra, ...Object.fromEntries(Object.entries(request.headers || {}).map(([name, value]) => [name.toLowerCase(), value])) },
      headers: {
        'content-type': header('content-type'),
        'cf-chl': header('cf-chl'),
        'cf-chl-ra': header('cf-chl-ra'),
        'user-agent': header('user-agent'),
        'sec-ch-ua': header('sec-ch-ua'),
        'sec-ch-ua-mobile': header('sec-ch-ua-mobile'),
        'sec-ch-ua-platform': header('sec-ch-ua-platform'),
        'accept': header('accept'),
        'accept-language': header('accept-language'),
        'origin': header('origin'),
        'referer': header('referer'),
        'sec-fetch-site': header('sec-fetch-site'),
        'sec-fetch-mode': header('sec-fetch-mode'),
        'sec-fetch-dest': header('sec-fetch-dest'),
      },
    });
    return true;
  }
  if (message.method === 'Network.requestWillBeSentExtraInfo') {
    const key = `${channel}:${message.params.requestId}`;
    const extra = Object.fromEntries(Object.entries(message.params.headers || {})
      .map(([name, value]) => [name.toLowerCase(), value]));
    setBounded(requestExtraHeaders, key, extra);
    const request = requests.get(key);
    if (request) request.allHeaders = { ...request.allHeaders, ...extra };
    return true;
  }
  if (message.method === 'Network.responseReceived') {
    const response = message.params.response;
    setBounded(responses, `${channel}:${message.params.requestId}`, {
      client,
      channel,
      sessionId,
      requestId: message.params.requestId,
      url: response.url,
      status: response.status,
      mimeType: response.mimeType,
      request: requests.get(`${channel}:${message.params.requestId}`),
    });
    return true;
  }
  if (message.method === 'Network.loadingFinished') {
    const key = `${channel}:${message.params.requestId}`;
    const response = responses.get(key);
    if (response && !response.bodyPromise) {
      response.bodyPromise = client.send('Network.getResponseBody', { requestId: response.requestId }, sessionId)
        .then(body => { response.body = body; })
        .catch(error => { response.error = error.message; })
        .finally(() => bodyPromises.delete(response.bodyPromise));
      bodyPromises.add(response.bodyPromise);
    }
    requests.delete(key);
    requestExtraHeaders.delete(key);
    return true;
  }
  if (message.method === 'Network.loadingFailed') {
    const key = `${channel}:${message.params.requestId}`;
    const response = responses.get(key);
    if (response) response.error = message.params.errorText || 'Network load failed';
    requests.delete(key);
    requestExtraHeaders.delete(key);
    return true;
  }
  return false;
}

try {
  cdp = connect(await browserWebSocket());
  await cdp.ready;
  if (engine === 'obscura' && profileId) {
    await cdp.send('Obscura.setProfile', { profileId });
  }
  cdp.onMessage(message => {
    if (message.method === 'Target.attachedToTarget') {
      const { sessionId, targetInfo } = message.params;
      targetSessions.set(targetInfo.targetId, sessionId);
      sessionTargets.set(sessionId, targetInfo);
      void enableNetwork(message.params.sessionId).catch(() => {});
      return;
    }
    if (message.method === 'Target.targetInfoChanged') {
      const targetInfo = message.params.targetInfo;
      const sessionId = targetSessions.get(targetInfo.targetId);
      if (sessionId) sessionTargets.set(sessionId, targetInfo);
      return;
    }
    if (message.method === 'Target.detachedFromTarget') {
      const targetInfo = sessionTargets.get(message.params.sessionId);
      if (targetInfo) targetSessions.delete(targetInfo.targetId);
      sessionTargets.delete(message.params.sessionId);
      return;
    }
    trackNetwork(message, cdp, `browser:${message.sessionId || ''}`, message.sessionId);
  });

  await cdp.send('Target.setDiscoverTargets', { discover: true });
  await cdp.send('Target.setAutoAttach', {
    autoAttach: true,
    waitForDebuggerOnStart: false,
    flatten: true,
  });

  let createdTargetId;
  if (engine === 'obscura') {
    const browserContext = await cdp.send('Target.createBrowserContext');
    const target = await cdp.send('Target.createTarget', {
      url: initialUrl,
      browserContextId: browserContext.browserContextId,
    });
    createdTargetId = target.targetId;
  }

  const deadline = Date.now() + 30000;
  let pageTarget;
  while (!pageTarget) {
    const { targetInfos } = await cdp.send('Target.getTargets');
    pageTarget = targetInfos.find(target => target.targetId === createdTargetId)
      || targetInfos.find(target => target.type === 'page' && target.url.startsWith(url));
    if (!pageTarget) pageTarget = targetInfos.find(target => target.type === 'page');
    if (!pageTarget && Date.now() > deadline) throw new Error('Chrome did not expose the page target');
    if (!pageTarget) await sleep(200);
  }
  pageSession = targetSessions.get(pageTarget.targetId);
  if (!pageSession || engine === 'obscura') {
    const attached = await cdp.send('Target.attachToTarget', { targetId: pageTarget.targetId, flatten: true });
    pageSession = attached.sessionId;
  }
  if (engine === 'obscura') {
    console.error(JSON.stringify({ rawTarget: pageTarget, pageSession, targetSessions: [...targetSessions] }));
  }
  await enableNetwork(pageSession);
  await cdp.send('Page.enable', {}, pageSession);
  await cdp.send('DOM.enable', {}, pageSession);
  await cdp.send('Emulation.setFocusEmulationEnabled', { enabled: true }, pageSession);
  if (rootViewport) {
    await cdp.send('Emulation.setDeviceMetricsOverride', {
      width: rootViewport[0],
      height: rootViewport[1],
      deviceScaleFactor: 1,
      mobile: false,
      screenWidth: 2560,
      screenHeight: 1440,
    }, pageSession);
  }
  if (engine === 'obscura' && rootViewport) {
    await cdp.send('Page.navigate', { url }, pageSession);
  }

  const report = `JSON.stringify((() => {
    const host = document.getElementById('tf') || document.querySelector('.cf-turnstile');
    const rect = host && host.getBoundingClientRect();
    const input = document.querySelector('input[name="cf-turnstile-response"]');
    return { href: location.href, ready: document.readyState, token: input && input.value || '',
      tokenInput: !!input, viewport:[innerWidth,innerHeight,outerWidth,outerHeight,devicePixelRatio],
      host: rect && { x:rect.x, y:rect.y, width:rect.width, height:rect.height } };
  })())`;

  let clicked = false;
  let challengeSession;
  let challengeFrameId;
  let challengeFrameBox;
  let challengeState = {};
  const challengeViewportHistory = [];
  const maxSeconds = Number(process.env.OBSCURA_RAW_MAX_SECONDS) > 0
    ? Number(process.env.OBSCURA_RAW_MAX_SECONDS) : 40;
  for (let second = 1; second <= maxSeconds; second++) {
    await sleep(1000);
    const result = await cdp.send('Runtime.evaluate', { expression: report, returnByValue: true }, pageSession);
    finalState = JSON.parse(result?.result?.value || '{}');
    challengeSession ||= [...sessionTargets].find(([, target]) =>
      target.type === 'iframe' && target.url.includes('challenges.cloudflare.com/cdn-cgi/challenge-platform'))?.[0];
    if (!frameCdp) {
      const nodes = await getDomNodes(cdp, pageSession);
      frameCandidates = nodes.filter(node => node.nodeName === 'IFRAME').map(node => ({
        backendNodeId: node.backendNodeId,
        frameId: node.frameId,
        attributes: node.attributes,
      }));
      const iframeNode = nodes.find(node => node.nodeName === 'IFRAME'
        && node.attributes?.some(value => String(value).includes('challenges.cloudflare.com')))
        || nodes.find(node => node.nodeName === 'IFRAME' && node.frameId);
      if (iframeNode?.backendNodeId) {
        try {
          const box = await cdp.send('DOM.getBoxModel', { backendNodeId: iframeNode.backendNodeId }, pageSession);
          challengeFrameBox = box.model?.border || box.model?.content || null;
        } catch (_) { /* the widget can replace the iframe during setup */ }
      }
      const { frameTree } = await cdp.send('Page.getFrameTree', {}, pageSession);
      const frame = findChallengeFrame(frameTree);
      challengeFrameId = iframeNode?.frameId || frame?.id;
      if (challengeFrameId) {
        const attachedChallengeSession = [...sessionTargets].find(([, target]) =>
          target.type === 'iframe' && target.url.includes('challenges.cloudflare.com/cdn-cgi/challenge-platform'))?.[0];
        if (engine === 'obscura' || attachedChallengeSession) {
          frameCdp = cdp;
          frameSessionId = attachedChallengeSession || pageSession;
        } else {
          frameCdp = connect(`ws://127.0.0.1:${port}/devtools/page/${challengeFrameId}`);
          await frameCdp.ready;
          frameCdp.onMessage(message => trackNetwork(message, frameCdp, `frame:${challengeFrameId}`));
          await frameCdp.send('Page.enable');
          await frameCdp.send('DOM.enable');
          await frameCdp.send('Emulation.setFocusEmulationEnabled', { enabled: true });
          await frameCdp.send('Network.enable');
        }
      }
    }
    if (frameCdp) {
      try {
        const sendFrame = (method, params = {}) => frameCdp.send(method, params, frameSessionId);
        const state = await sendFrame('Runtime.evaluate', {
          expression: `JSON.stringify({ href: location.href, ready: document.readyState,
            width: innerWidth, height: innerHeight,
            windowPosition: [screenX, screenY, screenLeft, screenTop],
            surface: (() => { const safe = fn => { try { return fn(); } catch (e) { return 'error:' + e.name; } };
              return { ua:navigator.userAgent, webdriver:navigator.webdriver, platform:navigator.platform,
                vendor:navigator.vendor, languages:Array.from(navigator.languages || []),
                plugins:Array.from(navigator.plugins || []).map(p => [p.name,p.filename,p.length]),
                hardwareConcurrency:navigator.hardwareConcurrency, deviceMemory:navigator.deviceMemory,
                maxTouchPoints:navigator.maxTouchPoints, cookieEnabled:navigator.cookieEnabled,
                screen:[screen.width,screen.height,screen.availWidth,screen.availHeight,screen.colorDepth,screen.pixelDepth],
                viewport:[innerWidth,innerHeight,outerWidth,outerHeight,devicePixelRatio],
                inputEvents:globalThis.__obscuraRawInputEvents || [],
                timezone:safe(() => Intl.DateTimeFormat().resolvedOptions().timeZone),
                chrome:safe(() => Object.keys(chrome || {})),
                  privateToken:[typeof document.hasPrivateToken,typeof document.hasRedemptionRecord],
                  storageAccess:[typeof document.hasStorageAccess,typeof document.requestStorageAccess,
                    typeof document.hasUnpartitionedCookieAccess],
                  navigatorApis:{
                    storage:[typeof navigator.storage,typeof navigator.storage?.estimate,
                      typeof navigator.storage?.persisted,typeof navigator.storage?.persist],
                    connection:[Object.prototype.toString.call(navigator.connection),
                      typeof navigator.connection?.effectiveType,typeof navigator.connection?.rtt,
                      typeof navigator.connection?.downlink],
                    locks:[typeof navigator.locks,typeof navigator.locks?.request],
                  },
                 webgpu:typeof navigator.gpu,
                 webgl:(() => { try { const c=document.createElement('canvas'); const g=c.getContext('webgl');
                   const dbg=g?.getExtension('WEBGL_debug_renderer_info');
                   return {vendor:g?.getParameter(dbg?.UNMASKED_VENDOR_WEBGL),renderer:g?.getParameter(dbg?.UNMASKED_RENDERER_WEBGL),version:g?.getParameter(g.VERSION),extensions:g?.getSupportedExtensions?.()||[]};
                 } catch (e) { return {error:String(e)}; } })(),
                apis:['Worker','SharedWorker','WebAssembly','OffscreenCanvas','WebGLRenderingContext','PointerEvent',
                  'TouchEvent','PerformanceObserver','speechSynthesis','Notification'].map(k => [k,typeof globalThis[k]]),
                 cryptoRandomUUID:typeof crypto?.randomUUID, permissions:typeof navigator.permissions,
                 permissionQuery:typeof navigator.permissions?.query,
                 trustedTypes:[typeof globalThis.trustedTypes, typeof globalThis.trustedTypes?.createPolicy,
                   typeof globalThis.trustedTypes?.isHTML],
                 featurePolicy:[typeof document.featurePolicy, typeof document.featurePolicy?.features,
                   document.featurePolicy?.features?.().length ?? null],
                 featurePolicyFeatures:document.featurePolicy?.features?.() || null,
                 mediaDevices:typeof navigator.mediaDevices, crossOriginIsolated:globalThis.crossOriginIsolated,
                evalShape:safe(() => ({text:String(eval),nativeText:Function.prototype.toString.call(eval),
                  name:eval.name,length:eval.length,keys:Reflect.ownKeys(eval),
                  proto:Object.getPrototypeOf(eval)===Function.prototype,
                  completion:(0,eval)('0,/.*honk.*/,123456789')})),
                dateNowShape:safe(() => ({text:String(Date.now),nativeText:Function.prototype.toString.call(Date.now),
                  name:Date.now.name,length:Date.now.length,integer:Number.isInteger(Date.now())})),
                evalErrorStack:safe(() => { try { (0,eval)(\"throw new Error('eval-probe')\") } catch(e) { return String(e.stack) } }),
                webdriverDescriptor:safe(() => { const d=Object.getOwnPropertyDescriptor(Navigator.prototype,'webdriver');
                  return d && {enumerable:d.enumerable,configurable:d.configurable,get:String(d.get)}; }) } })(),
            point: (() => { const e = document.elementFromPoint(30, 30); return e && { tag:e.tagName, id:e.id, cls:e.className }; })() })`,
          returnByValue: true,
        });
        challengeState = JSON.parse(state?.result?.value || '{}');
        challengeViewportHistory.push({ second, viewport: [challengeState.width, challengeState.height] });
        if (second === 1) {
          const workerProbe = await sendFrame('Runtime.evaluate', {
            expression: `new Promise(resolve => {
              const source = \`postMessage(JSON.stringify({
                ownKeys: Reflect.ownKeys(navigator).map(String),
                protoKeys: Reflect.ownKeys(Object.getPrototypeOf(navigator)).map(String),
                userAgentData: typeof navigator.userAgentData,
                connection: typeof navigator.connection,
                gpu: typeof navigator.gpu,
                gpuRequestAdapter: typeof navigator.gpu?.requestAdapter,
                locks: typeof navigator.locks,
                storage: typeof navigator.storage,
                permissions: typeof navigator.permissions,
                mediaDevices: typeof navigator.mediaDevices,
                credentials: typeof navigator.credentials,
                hardwareConcurrency: navigator.hardwareConcurrency,
                deviceMemory: navigator.deviceMemory,
                platform: navigator.platform,
                userAgent: navigator.userAgent,
              }))\`;
              const url = URL.createObjectURL(new Blob([source], {type:'application/javascript'}));
              const worker = new Worker(url);
              worker.onmessage = event => { resolve(event.data); worker.terminate(); URL.revokeObjectURL(url); };
              worker.onerror = event => { resolve(JSON.stringify({error:String(event.message || event)})); worker.terminate(); URL.revokeObjectURL(url); };
            })`,
            awaitPromise: true,
            returnByValue: true,
          });
          try { challengeState.workerProbe = JSON.parse(workerProbe?.result?.value || 'null'); } catch (_) { challengeState.workerProbe = { error: 'invalid probe result' }; }
        }
        const privateToken = await sendFrame('Runtime.evaluate', {
          expression: `Promise.all([
            'https://challenges.cloudflare.com',
            'https://hagen.challenges.cloudflare.com',
            location.origin,
          ].flatMap(issuer => [
            document.hasPrivateToken?.(issuer),
            document.hasRedemptionRecord?.(issuer),
          ])).then(values => JSON.stringify(values.map(value => ({
            type: typeof value,
            value: value === undefined ? null : value,
          }))))`,
          awaitPromise: true,
          returnByValue: true,
        });
        challengeState.privateTokenResult = JSON.parse(privateToken?.result?.value || 'null');
        const storageAccess = await sendFrame('Runtime.evaluate', {
          expression: `Promise.all([
            typeof document.hasStorageAccess === 'function'
              ? document.hasStorageAccess().catch(error => ({ error: error.name }))
              : null,
            typeof document.hasUnpartitionedCookieAccess === 'function'
              ? document.hasUnpartitionedCookieAccess().catch(error => ({ error: error.name }))
              : null,
          ]).then(values => JSON.stringify(values))`,
          awaitPromise: true,
          returnByValue: true,
        });
        challengeState.storageAccessResult = JSON.parse(storageAccess?.result?.value || 'null');
        const gpu = await sendFrame('Runtime.evaluate', {
          expression: `Promise.resolve(navigator.gpu?.requestAdapter?.()).then(adapter => JSON.stringify({
            adapter: !!adapter,
            features: adapter ? [...adapter.features].slice(0, 16) : [],
            info: adapter ? {
              vendor: adapter.info?.vendor,
              architecture: adapter.info?.architecture,
              device: adapter.info?.device,
              description: adapter.info?.description,
              isFallbackAdapter: adapter.info?.isFallbackAdapter,
            } : null,
            limits: adapter ? {
              maxTextureDimension2D: adapter.limits.maxTextureDimension2D,
              maxBindGroups: adapter.limits.maxBindGroups,
              maxBufferSize: adapter.limits.maxBufferSize,
            } : null,
          }))`,
          awaitPromise: true,
          returnByValue: true,
        });
        challengeState.gpu = JSON.parse(gpu?.result?.value || 'null');
      } catch (error) {
        challengeState = { error: error.message };
      }
    }
    if (process.env.OBSCURA_RAW_NO_CLICK !== '1'
        && !clicked && second >= 8 && finalState.host?.height >= 60 && frameCdp) {
      const sendFrame = (method, params = {}) => frameCdp.send(method, params, frameSessionId);
      await sendFrame('Runtime.evaluate', { expression: `(() => {
         globalThis.__obscuraRawInputEvents = [];
         const recordInput = value => {
           if (globalThis.__obscuraRawInputEvents.length >= 500) globalThis.__obscuraRawInputEvents.shift();
           globalThis.__obscuraRawInputEvents.push(value);
         };
         for (const type of ['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click']) {
           document.addEventListener(type, event => recordInput({
            type, trusted:event.isTrusted, target:event.target?.tagName,
            client:[event.clientX,event.clientY], screen:[event.screenX,event.screenY],
            page:[event.pageX,event.pageY], buttons:event.buttons, button:event.button,
            detail:event.detail, composed:event.composed, view:event.view === window,
          }), true);
        }
      })()` });
      try {
        const { root: documentRoot } = await sendFrame('DOM.getDocument', { depth: -1, pierce: true });
        const findShadow = node => {
          if (node.nodeName === '#document-fragment') return node;
          for (const shadowRoot of node.shadowRoots || []) {
            const found = findShadow(shadowRoot);
            if (found) return found;
          }
          for (const child of node.children || []) {
            const found = findShadow(child);
            if (found) return found;
          }
          return null;
        };
        const shadow = findShadow(documentRoot);
        rawShadowTrace.node = shadow && {
          nodeId: shadow.nodeId, backendNodeId: shadow.backendNodeId,
          childNodeCount: shadow.childNodeCount,
        };
        if (shadow?.backendNodeId) {
          const resolved = await sendFrame('DOM.resolveNode', { backendNodeId: shadow.backendNodeId });
          rawShadowTrace.resolved = { objectId: resolved.object?.objectId, type: resolved.object?.type };
          const shadowMarkup = await sendFrame('Runtime.callFunctionOn', {
            objectId: resolved.object.objectId,
            functionDeclaration: `function () { return String(this.innerHTML || '').slice(0, 12000); }`,
            returnByValue: true,
          });
          rawShadowTrace.html = shadowMarkup?.result?.value || '';
          const installed = await sendFrame('Runtime.callFunctionOn', {
            objectId: resolved.object.objectId,
            functionDeclaration: `function () {
               const report = phase => event => {
                 if (globalThis.__obscuraRawInputEvents.length >= 500) globalThis.__obscuraRawInputEvents.shift();
                 globalThis.__obscuraRawInputEvents.push({
                 type: 'shadow-' + phase, target: event.target?.tagName,
                 path: event.composedPath?.().slice(0, 10).map(node => node?.tagName || node?.nodeName || null),
                 screen: [event.screenX, event.screenY], page: [event.pageX, event.pageY],
                 });
               };
              this.addEventListener('click', report('capture'), true);
              this.addEventListener('click', report('bubble'));
            }`,
            returnByValue: true,
          });
          rawShadowTrace.installed = installed?.exceptionDetails ? 'error' : 'ok';
        }
      } catch (error) { rawShadowTrace.error = error.message; }
      let x = 30;
      let y = 30;
      try {
        const nodes = await getDomNodes(frameCdp, frameSessionId);
        rawClickNodes = nodes.filter(node => ['INPUT', 'SPAN', 'DIV', 'IFRAME', '#document-fragment'].includes(node.nodeName))
          .map(node => ({ nodeName: node.nodeName, nodeId: node.nodeId, backendNodeId: node.backendNodeId,
            parentId: node.parentId, attributes: node.attributes }));
        const input = nodes.find(node => node.nodeName === 'INPUT');
        if (input) {
          const { model } = await sendFrame('DOM.getBoxModel', { backendNodeId: input.backendNodeId });
          const quad = model.border;
          x = (quad[0] + quad[2] + quad[4] + quad[6]) / 4;
          y = (quad[1] + quad[3] + quad[5] + quad[7]) / 4;
        }
      } catch { /* Match BotBrowser's 30,30 fallback. */ }
      if (engine === 'obscura') {
        try {
          const parentFrame = await sendFrame('Runtime.evaluate', {
            expression: `(() => {
              const frames = Object.values(globalThis.__obscura_frameElements || {});
              const frame = frames.find(item => {
                const source = String(item?.src || item?.getAttribute?.('src') || '');
                return source.includes('challenges.cloudflare.com') && item?.getBoundingClientRect;
              }) || frames.find(item => item?.getBoundingClientRect);
              const rect = frame?.getBoundingClientRect?.();
              return rect && JSON.stringify({ x: rect.x, y: rect.y, width: rect.width, height: rect.height });
            })()`,
            returnByValue: true,
          });
          const rect = JSON.parse(parentFrame?.result?.value || 'null');
          if (rect && rect.width > 0 && rect.height > 0) {
            // Input.dispatchMouseEvent routes the page coordinate into the
            // deepest Obscura frame. The widget's checkbox is near its top
            // left, not at the page's first ordinary input.
            x = rect.x + 30;
            y = rect.y + 30;
          }
        } catch { /* Keep the generic fallback. */ }
      }
      if (engine !== 'obscura' && challengeFrameBox?.length >= 8) {
        x = (challengeFrameBox[0] + challengeFrameBox[2] + challengeFrameBox[4] + challengeFrameBox[6]) / 4;
        y = (challengeFrameBox[1] + challengeFrameBox[3] + challengeFrameBox[5] + challengeFrameBox[7]) / 4;
      }
      await sendFrame('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y });
      await sendFrame('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', clickCount: 1 });
      await sleep(100);
      await sendFrame('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', clickCount: 1 });
      clicked = true;
      console.log(`clicked iframe (${x},${y}), ${challengeState.width}x${challengeState.height}`);
    }
    if (finalState.token) break;
  }

  let fileNumber = 0;
  const saved = [];
  const resources = [];
  let largeEvalProbe;
  if (frameCdp) {
    try {
      const largeEvalSource = await readFile(
        join(root, 'target', 'turnstile-dumps', '1786128074', '0011-dynamic.js'), 'utf8');
      const sendFrame = (method, params = {}) => frameCdp.send(method, params, frameSessionId);
      const probe = await sendFrame('Runtime.evaluate', {
        expression: `(() => { const source=${JSON.stringify(largeEvalSource)};
          const before=Date.now(); const value=(0,eval)(source); const after=Date.now();
          return {duration:after-before,resultType:typeof value,result:String(value),sourceLength:source.length}; })()`,
        returnByValue: true,
      });
      largeEvalProbe = probe?.result?.value;
      const { frameTree } = await sendFrame('Page.getResourceTree');
      const trees = [frameTree];
      while (trees.length) {
        const tree = trees.shift();
        for (const resource of tree.resources || []) {
          try {
            const body = await sendFrame('Page.getResourceContent', {
              frameId: tree.frame.id,
              url: resource.url,
            });
            const bytes = body.base64Encoded ? Buffer.from(body.content, 'base64') : Buffer.from(body.content);
            const name = `resource-${String(resources.length + 1).padStart(4, '0')}-${resource.type}-${resource.url.replace(/[^a-z0-9._-]+/gi, '_').slice(0, 100)}.body`;
            await writeFile(join(dumpDir, name), bytes);
            resources.push({ frameId: tree.frame.id, url: resource.url, type: resource.type, file: name,
              bodyLength: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') });
          } catch (error) {
            resources.push({ frameId: tree.frame.id, url: resource.url, type: resource.type, error: error.message });
          }
        }
        trees.push(...(tree.childFrames || []));
      }
      const html = await sendFrame('Runtime.evaluate', {
        expression: 'document.documentElement.outerHTML',
        returnByValue: true,
      });
      await writeFile(join(dumpDir, 'frame-final.html'), html?.result?.value || '');
    } catch (error) {
      resources.push({ error: error.message });
    }
  }
  await Promise.allSettled(bodyPromises);
  for (const response of responses.values()) {
    const responseMeta = {
      sessionId: response.sessionId || '',
      requestId: response.requestId,
      url: response.url,
      status: response.status,
      mimeType: response.mimeType,
      channel: response.channel,
    };
      if (response.request) {
        responseMeta.method = response.request.method;
        responseMeta.requestHeaders = response.request.headers;
        responseMeta.requestAllHeaders = response.request.allHeaders;
      if (typeof response.request.postData === 'string') {
        const requestBytes = Buffer.from(response.request.postData);
        const requestName = `${String(fileNumber + 1).padStart(4, '0')}-${response.url.replace(/[^a-z0-9._-]+/gi, '_').slice(0, 100)}.request`;
        await writeFile(join(dumpDir, requestName), requestBytes);
        responseMeta.requestFile = requestName;
        responseMeta.requestBodyLength = requestBytes.length;
        responseMeta.requestSha256 = createHash('sha256').update(requestBytes).digest('hex');
      }
    }
    try {
      const body = response.body || await response.client.send('Network.getResponseBody',
        { requestId: response.requestId }, response.sessionId);
      const bytes = body.base64Encoded ? Buffer.from(body.body, 'base64') : Buffer.from(body.body);
      const name = `${String(++fileNumber).padStart(4, '0')}-${response.url.replace(/[^a-z0-9._-]+/gi, '_').slice(0, 120)}.body`;
      await writeFile(join(dumpDir, name), bytes);
      saved.push({ ...responseMeta, file: name, bodyLength: bytes.length,
        sha256: createHash('sha256').update(bytes).digest('hex') });
    } catch (error) {
      saved.push({ ...responseMeta, error: response.error || error.message });
    }
  }

  const screenshot = await cdp.send('Page.captureScreenshot', { format: 'png' }, pageSession);
  await writeFile(join(dumpDir, 'page.png'), Buffer.from(screenshot.data, 'base64'));
  const metadata = {
    stamp,
    engine,
    runtimeEnableSent: false,
    debuggerEnableSent: false,
    proxyVariablesCleared: true,
    args,
    sessions: [...sessions],
    targets: [...sessionTargets].map(([sessionId, target]) => ({ sessionId, ...target })),
    click: { clicked, challengeSession, challengeFrameId, challengeFrameBox, challengeState, challengeViewportHistory },
    rawShadowTrace,
    rawClickNodes,
    frameCandidates,
    resources,
    largeEvalProbe,
    responses: saved,
    final: { ...finalState, tokenLength: finalState.token?.length || 0, token: undefined },
  };
  await writeFile(join(dumpDir, 'metadata.json'), JSON.stringify(metadata, null, 2));
  console.log(JSON.stringify({ pass: !!finalState.token, tokenLength: finalState.token?.length || 0,
    responses: saved.length, dumpDir }, null, 2));
} finally {
  if (frameCdp && frameCdp !== cdp) frameCdp.close();
  if (cdp) {
    try { await Promise.race([cdp.send('Browser.close'), sleep(2000)]); } catch { /* best effort */ }
    cdp.close();
  }
  child.kill();
}
