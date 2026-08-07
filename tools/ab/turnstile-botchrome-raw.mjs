// Strict Turnstile check with the prepared Chrome build.
// Runtime.enable and Debugger.enable are never sent.

import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import net from 'node:net';
import { join, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..', '..');
const chrome = process.env.BOT_CHROME_BIN || 'C:\\chrome-bin\\chrome.exe';
const profile = process.env.BOT_CHROME_PROFILE || 'C:\\Chrome-bin\\offlinekiana.json';
const userDataDir = process.env.BOT_CHROME_USER_DATA_DIR || 'C:\\Chrome-bin\\9';
const url = process.env.TURNSTILE_URL || 'https://turnstile-test.vercel.app/';
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

const args = [
  '--no-sandbox',
  `--bot-profile=${profile}`,
  `--user-data-dir=${userDataDir}`,
  '--bot-internal',
  '--enable-logging',
  '--v=1',
  `--remote-debugging-port=${port}`,
  '--no-proxy-server',
  url,
];
const child = spawn(chrome, args, { cwd: root, env, stdio: ['ignore', 'ignore', 'inherit'], windowsHide: true });
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
    const callback = pending.get(message.id);
    if (callback) {
      pending.delete(message.id);
      callback(message);
      return;
    }
    for (const listener of listeners) listener(message);
  });
  const ready = new Promise((done, fail) => {
    socket.addEventListener('open', done);
    socket.addEventListener('error', fail);
  });
  const send = (method, params = {}, sessionId) => new Promise((done, fail) => {
    const id = ++nextId;
    pending.set(id, message => message.error
      ? fail(new Error(`${method}: ${message.error.message}`))
      : done(message.result));
    socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
  });
  return { ready, send, onMessage: listener => listeners.add(listener), close: () => socket.close() };
}

const responses = new Map();
const requests = new Map();
const sessions = new Set();
const targetSessions = new Map();
const sessionTargets = new Map();
const bodyPromises = [];
let cdp;
let frameCdp;
let pageSession;
let finalState = {};
let frameCandidates = [];

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
    const header = name => Object.entries(request.headers || {})
      .find(([key]) => key.toLowerCase() === name)?.[1];
    requests.set(`${channel}:${message.params.requestId}`, {
      method: request.method,
      url: request.url,
      postData: request.postData,
      headers: {
        'content-type': header('content-type'),
        'cf-chl': header('cf-chl'),
        'cf-chl-ra': header('cf-chl-ra'),
        'user-agent': header('user-agent'),
        'sec-ch-ua': header('sec-ch-ua'),
        'sec-ch-ua-platform': header('sec-ch-ua-platform'),
      },
    });
    return true;
  }
  if (message.method === 'Network.responseReceived') {
    const response = message.params.response;
    responses.set(`${channel}:${message.params.requestId}`, {
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
    const response = responses.get(`${channel}:${message.params.requestId}`);
    if (!response || response.bodyPromise) return true;
    response.bodyPromise = client.send('Network.getResponseBody', { requestId: response.requestId }, sessionId)
      .then(body => { response.body = body; })
      .catch(error => { response.error = error.message; });
    bodyPromises.push(response.bodyPromise);
    return true;
  }
  return false;
}

try {
  cdp = connect(await browserWebSocket());
  await cdp.ready;
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

  const deadline = Date.now() + 30000;
  let pageTarget;
  while (!pageTarget) {
    const { targetInfos } = await cdp.send('Target.getTargets');
    pageTarget = targetInfos.find(target => target.type === 'page' && target.url.startsWith(url));
    if (!pageTarget) pageTarget = targetInfos.find(target => target.type === 'page');
    if (!pageTarget && Date.now() > deadline) throw new Error('Chrome did not expose the page target');
    if (!pageTarget) await sleep(200);
  }
  pageSession = targetSessions.get(pageTarget.targetId);
  if (!pageSession) {
    const attached = await cdp.send('Target.attachToTarget', { targetId: pageTarget.targetId, flatten: true });
    pageSession = attached.sessionId;
  }
  await enableNetwork(pageSession);
  await cdp.send('Page.enable', {}, pageSession);
  await cdp.send('DOM.enable', {}, pageSession);
  await cdp.send('Emulation.setFocusEmulationEnabled', { enabled: true }, pageSession);

  const report = `JSON.stringify((() => {
    const host = document.getElementById('tf') || document.querySelector('.cf-turnstile');
    const rect = host && host.getBoundingClientRect();
    const input = document.querySelector('input[name="cf-turnstile-response"]');
    return { href: location.href, ready: document.readyState, token: input && input.value || '',
      tokenInput: !!input, host: rect && { x:rect.x, y:rect.y, width:rect.width, height:rect.height } };
  })())`;

  let clicked = false;
  let challengeSession;
  let challengeFrameId;
  let challengeState = {};
  for (let second = 1; second <= 40; second++) {
    await sleep(1000);
    const result = await cdp.send('Runtime.evaluate', { expression: report, returnByValue: true }, pageSession);
    finalState = JSON.parse(result?.result?.value || '{}');
    challengeSession ||= [...sessionTargets].find(([, target]) =>
      target.type === 'iframe' && target.url.includes('challenges.cloudflare.com/cdn-cgi/challenge-platform'))?.[0];
    if (!frameCdp) {
      const { nodes } = await cdp.send('DOM.getFlattenedDocument', { depth: -1, pierce: true }, pageSession);
      frameCandidates = nodes.filter(node => node.nodeName === 'IFRAME').map(node => ({
        backendNodeId: node.backendNodeId,
        frameId: node.frameId,
        attributes: node.attributes,
      }));
      const iframeNode = nodes.find(node => node.nodeName === 'IFRAME'
        && node.attributes?.some(value => String(value).includes('challenges.cloudflare.com')))
        || nodes.find(node => node.nodeName === 'IFRAME' && node.frameId);
      const { frameTree } = await cdp.send('Page.getFrameTree', {}, pageSession);
      const frame = findChallengeFrame(frameTree);
      challengeFrameId = iframeNode?.frameId || frame?.id;
      if (challengeFrameId) {
        frameCdp = connect(`ws://127.0.0.1:${port}/devtools/page/${challengeFrameId}`);
        await frameCdp.ready;
        frameCdp.onMessage(message => trackNetwork(message, frameCdp, `frame:${challengeFrameId}`));
        await frameCdp.send('Page.enable');
        await frameCdp.send('DOM.enable');
        await frameCdp.send('Emulation.setFocusEmulationEnabled', { enabled: true });
        await frameCdp.send('Network.enable');
      }
    }
    if (frameCdp) {
      try {
        const state = await frameCdp.send('Runtime.evaluate', {
          expression: `JSON.stringify({ href: location.href, ready: document.readyState,
            width: innerWidth, height: innerHeight,
            surface: (() => { const safe = fn => { try { return fn(); } catch (e) { return 'error:' + e.name; } };
              return { ua:navigator.userAgent, webdriver:navigator.webdriver, platform:navigator.platform,
                vendor:navigator.vendor, languages:Array.from(navigator.languages || []),
                plugins:Array.from(navigator.plugins || []).map(p => [p.name,p.filename,p.length]),
                hardwareConcurrency:navigator.hardwareConcurrency, deviceMemory:navigator.deviceMemory,
                maxTouchPoints:navigator.maxTouchPoints, cookieEnabled:navigator.cookieEnabled,
                screen:[screen.width,screen.height,screen.availWidth,screen.availHeight,screen.colorDepth,screen.pixelDepth],
                viewport:[innerWidth,innerHeight,outerWidth,outerHeight,devicePixelRatio],
                timezone:safe(() => Intl.DateTimeFormat().resolvedOptions().timeZone),
                chrome:safe(() => Object.keys(chrome || {})),
                privateToken:[typeof document.hasPrivateToken,typeof document.hasRedemptionRecord],
                apis:['Worker','SharedWorker','WebAssembly','OffscreenCanvas','WebGLRenderingContext','PointerEvent',
                  'TouchEvent','PerformanceObserver','speechSynthesis','Notification'].map(k => [k,typeof globalThis[k]]),
                cryptoRandomUUID:typeof crypto?.randomUUID, permissions:typeof navigator.permissions,
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
      } catch (error) {
        challengeState = { error: error.message };
      }
    }
    if (!clicked && second >= 8 && finalState.host?.height >= 60 && frameCdp) {
      await frameCdp.send('Runtime.evaluate', { expression: `(() => {
        window.dtp = 1;
        const sx = Math.floor(Math.random() * 401) + 800;
        const sy = Math.floor(Math.random() * 301) + 400;
        Object.defineProperty(MouseEvent.prototype, 'screenX', { value: sx });
        Object.defineProperty(MouseEvent.prototype, 'screenY', { value: sy });
      })()` });
      let x = 30;
      let y = 30;
      try {
        const { nodes } = await frameCdp.send('DOM.getFlattenedDocument', { depth: -1, pierce: true });
        const input = nodes.find(node => node.nodeName === 'INPUT');
        if (input) {
          const { model } = await frameCdp.send('DOM.getBoxModel', { backendNodeId: input.backendNodeId });
          const quad = model.border;
          x = (quad[0] + quad[2] + quad[4] + quad[6]) / 4;
          y = (quad[1] + quad[3] + quad[5] + quad[7]) / 4;
        }
      } catch { /* Match BotBrowser's 30,30 fallback. */ }
      await frameCdp.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y });
      await frameCdp.send('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', clickCount: 1 });
      await sleep(100);
      await frameCdp.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', clickCount: 1 });
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
      const probe = await frameCdp.send('Runtime.evaluate', {
        expression: `(() => { const source=${JSON.stringify(largeEvalSource)};
          const before=Date.now(); const value=(0,eval)(source); const after=Date.now();
          return {duration:after-before,resultType:typeof value,result:String(value),sourceLength:source.length}; })()`,
        returnByValue: true,
      });
      largeEvalProbe = probe?.result?.value;
      const { frameTree } = await frameCdp.send('Page.getResourceTree');
      const trees = [frameTree];
      while (trees.length) {
        const tree = trees.shift();
        for (const resource of tree.resources || []) {
          try {
            const body = await frameCdp.send('Page.getResourceContent', {
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
      const html = await frameCdp.send('Runtime.evaluate', {
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
    runtimeEnableSent: false,
    debuggerEnableSent: false,
    proxyVariablesCleared: true,
    args,
    sessions: [...sessions],
    targets: [...sessionTargets].map(([sessionId, target]) => ({ sessionId, ...target })),
    click: { clicked, challengeSession, challengeFrameId, challengeState },
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
  if (frameCdp) frameCdp.close();
  if (cdp) {
    try { await Promise.race([cdp.send('Browser.close'), sleep(2000)]); } catch { /* best effort */ }
    cdp.close();
  }
  child.kill();
}
