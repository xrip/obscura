import { spawn } from 'node:child_process';
import net from 'node:net';
import { chromium } from './target/test-fixtures/playwright/node_modules/playwright-core/index.mjs';

const profileId = process.env.OBSCURA_PROFILE_ID;
const proxy = process.env.OBSCURA_PROXY;
if (!profileId || !proxy) {
  throw new Error('Set OBSCURA_PROFILE_ID and OBSCURA_PROXY');
}
const urls = [
  'https://www.wildberries.ru/catalog/797296322/detail.aspx',
];

async function freePort() {
  const server = net.createServer();
  await new Promise((done, fail) => {
    server.once('error', fail);
    server.listen(0, '127.0.0.1', done);
  });
  const port = server.address().port;
  await new Promise(done => server.close(done));
  return port;
}

async function waitForServer(child, port) {
  for (let attempt = 0; attempt < 100; attempt++) {
    if (child.exitCode !== null) throw new Error(`Obscura stopped with ${child.exitCode}`);
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/version`);
      if (response.ok) return;
    } catch {}
    await new Promise(done => setTimeout(done, 100));
  }
  throw new Error('Obscura did not become ready');
}

const port = await freePort();
const obscura = spawn(
  '.\\target\\release\\obscura.exe',
  ['--verbose', '--proxy', proxy, '--stealth', 'serve', '--port', String(port)],
  { cwd: process.cwd(), stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true },
);
  let serverLog = '';
  const interestingServerLog = [];
  for (const stream of [obscura.stdout, obscura.stderr]) {
    stream.on('data', chunk => {
    const text = chunk.toString();
    serverLog = (serverLog + text).slice(-200000);
    for (const line of text.split(/\r?\n/)) {
        if (/stealth (scripted )?(request|response)|document\.cookie write|JS navigation|JS-triggered navigation chain|create-token .*shape|create-token (request shape|response)|create-token ->|Dynamic script|report/i.test(line)) {
        interestingServerLog.push(line);
        if (interestingServerLog.length > 500) interestingServerLog.shift();
      }
    }
    });
  }

let browser;
try {
  await waitForServer(obscura, port);
  browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  const root = await browser.newBrowserCDPSession();
  const selected = await root.send('Obscura.setProfile', { profileId });
  const context = await browser.newContext();

  const products = [];
  for (const url of urls) {
    const page = await context.newPage();
    await page.addInitScript(() => {
      globalThis.__wb_errors = [];
      globalThis.__wb_rejections = [];
      globalThis.__wb_dynamic_scripts = [];
      globalThis.__wb_fetch_calls = [];
      globalThis.__wb_token_transcript = [];
      globalThis.__wb_console_errors = [];
      const consoleError = console.error;
      console.error = function (...args) {
        globalThis.__wb_console_errors.push(args.map(value => String(value)).join(' '));
        return consoleError.apply(this, args);
      };
      const fetchImpl = globalThis.fetch;
      globalThis.fetch = function (input, init) {
        const requestUrl = typeof input === 'string' ? input : input?.url || String(input);
        globalThis.__wb_fetch_calls.push({
          url: String(requestUrl),
          method: String(init?.method || 'GET'),
        });
        return fetchImpl.call(this, input, init).then(async response => {
          if (String(requestUrl).includes('/api/v1/create-token')) {
            const body = typeof init?.body === 'string' ? init.body : '';
            let requestShape = null;
            try {
              const value = JSON.parse(body);
              const shape = item => {
                if (item === null) return 'null';
                if (Array.isArray(item)) return `array(${item.length})`;
                if (typeof item === 'object') {
                  return Object.fromEntries(Object.entries(item).map(([key, value]) => [key, shape(value)]));
                }
                return typeof item;
              };
              requestShape = shape(value);
            } catch {}
            let responseShape = null;
            let responseLength = 0;
            try {
              const text = await response.clone().text();
              responseLength = text.length;
              responseShape = shape(JSON.parse(text));
            } catch {}
            globalThis.__wb_token_transcript.push({
              status: response.status,
              requestLength: body.length,
              requestShape,
              responseLength,
              responseShape,
            });
          }
          return response;
        });
      };
      const appendChild = Node.prototype.appendChild;
      Node.prototype.appendChild = function (child) {
        if (child?.tagName === 'SCRIPT' && child?.src) {
          child.addEventListener('load', () => {
            globalThis.__wb_dynamic_scripts.push({ event: 'load', src: child.src });
          });
          child.addEventListener('error', () => {
            globalThis.__wb_dynamic_scripts.push({ event: 'error', src: child.src });
          });
        }
        return appendChild.call(this, child);
      };
      addEventListener('error', event => {
        globalThis.__wb_errors.push({ message: String(event.message || event.error || '') });
      });
      addEventListener('unhandledrejection', event => {
        globalThis.__wb_rejections.push({ reason: String(event.reason || '') });
      });
    });
    const browserEvents = [];
    page.on('console', message => {
      browserEvents.push({ type: 'console', level: message.type(), text: message.text() });
    });
    page.on('pageerror', error => {
      browserEvents.push({ type: 'pageerror', text: String(error) });
    });
    page.on('requestfailed', request => {
      browserEvents.push({
        type: 'requestfailed',
        url: request.url(),
        method: request.method(),
        failure: request.failure(),
      });
    });
    page.on('response', response => {
      if (response.url() === url || response.url().includes('/__wbaas/')) {
        browserEvents.push({
          type: 'response',
          url: response.url(),
          status: response.status(),
          contentType: response.headers()['content-type'] || '',
        });
      }
    });
    let navigationError = null;
    try {
      await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60000 });
    } catch (error) {
      navigationError = String(error);
    }
    await page.waitForTimeout(15000);
    let pageData;
    try {
      pageData = await page.evaluate(async () => {
        globalThis.__cdp_probe = false;
        const cdpProbeObject = {};
        Object.defineProperty(cdpProbeObject, 'stack', {
          get() {
            globalThis.__cdp_probe = true;
            return 'probe';
          },
        });
        console.log(cdpProbeObject);
        const cdpProbeBeforeWait = globalThis.__cdp_probe;
        await new Promise(resolve => setTimeout(resolve, 250));
        const read = selector => {
          const node = document.querySelector(selector);
          return node ? (node.getAttribute('content') || node.textContent || '').trim() : '';
        };
        const body = document.body
          ? (document.body.innerText || document.body.textContent || '')
          : '';
        const functionSource = value => {
          try { return Function.prototype.toString.call(value); } catch { return ''; }
        };
        const webglSignals = () => {
          try {
            const canvas = document.createElement('canvas');
            const context = canvas.getContext('webgl');
            if (!context) return null;
            const debug = context.getExtension('WEBGL_debug_renderer_info');
            return {
              vendor: debug ? context.getParameter(debug.UNMASKED_VENDOR_WEBGL) : '',
              renderer: debug ? context.getParameter(debug.UNMASKED_RENDERER_WEBGL) : '',
              version: context.getParameter(context.VERSION),
              shadingLanguageVersion: context.getParameter(context.SHADING_LANGUAGE_VERSION),
              extensions: context.getSupportedExtensions(),
              antialias: context.getContextAttributes()?.antialias,
            };
          } catch (error) { return { error: String(error) }; }
        };
        const navigatorPrototype = Object.getPrototypeOf(navigator);
        const uaData = navigator.userAgentData;
        let highEntropy = null;
        try {
          highEntropy = await uaData?.getHighEntropyValues?.([
            'architecture', 'bitness', 'fullVersionList', 'model',
            'platformVersion', 'uaFullVersion', 'wow64',
          ]);
        } catch (error) { highEntropy = { error: String(error) }; }
        const fingerprint = {
          userAgent: navigator.userAgent,
          appVersion: navigator.appVersion,
          webdriver: navigator.webdriver,
          platform: navigator.platform,
          languages: navigator.languages,
          language: navigator.language,
          vendor: navigator.vendor,
          hardwareConcurrency: navigator.hardwareConcurrency,
          deviceMemory: navigator.deviceMemory,
          maxTouchPoints: navigator.maxTouchPoints,
          plugins: navigator.plugins?.length,
          mimeTypes: navigator.mimeTypes?.length,
          pdfViewerEnabled: navigator.pdfViewerEnabled,
          userAgentData: {
            brands: uaData?.brands,
            mobile: uaData?.mobile,
            platform: uaData?.platform,
            highEntropy,
          },
          screen: {
            width: screen.width, height: screen.height,
            availWidth: screen.availWidth, availHeight: screen.availHeight,
            colorDepth: screen.colorDepth, pixelDepth: screen.pixelDepth,
            devicePixelRatio: devicePixelRatio,
          },
          window: {
            innerWidth, innerHeight, outerWidth, outerHeight,
            screenX, screenY,
          },
          visibility: { hidden: document.hidden, state: document.visibilityState },
          chrome: {
            type: typeof globalThis.chrome,
            keys: globalThis.chrome ? Object.keys(globalThis.chrome).sort() : [],
          },
          apis: Object.fromEntries([
            'Audio', 'AudioContext', 'OfflineAudioContext', 'WebGLRenderingContext',
            'WebGL2RenderingContext', 'Worker', 'SharedWorker', 'Notification',
            'Permissions', 'WebAssembly', 'WebGPU', 'GPU', 'MessageChannel',
            'ResizeObserver', 'matchMedia',
          ].map(name => [name, typeof globalThis[name]])),
          webgl: webglSignals(),
          functionToString: functionSource(navigatorPrototype?.webdriver?.get),
          errorStackDescriptor: Object.getOwnPropertyDescriptor(Error.prototype, 'stack')
            ? functionSource(Object.getOwnPropertyDescriptor(Error.prototype, 'stack').get)
            : null,
        };
        const jsonLd = Array.from(document.querySelectorAll('script[type="application/ld+json"]'))
          .map(node => node.textContent || '')
          .join('\n')
          .slice(0, 50000);
        const priceText = Array.from(document.querySelectorAll(
          '[data-testid*="price"], [class*="price"], [class*="Price"]',
        ))
          .map(node => (node.innerText || node.textContent || '').trim())
          .filter(Boolean)
          .slice(0, 30);
        const scripts = Array.from(document.scripts).map(node => node.textContent || '');
        const targetScript = scripts.find(text => /797296322|1902651403/.test(text)) || '';
        const resources = performance.getEntriesByType('resource')
          .map(entry => entry.name)
          .filter(name => name.includes('/__wbaas/'));
        const summarizeToken = cookieString => {
          const token = cookieString
            .split(';')
            .map(item => item.trim())
            .find(item => item.startsWith('x_wbaas_token='))
            ?.slice('x_wbaas_token='.length) || '';
          if (!token) return null;
          const decode = value => {
            const normalized = value.replace(/-/g, '+').replace(/_/g, '/');
            return atob(normalized + '='.repeat((4 - normalized.length % 4) % 4));
          };
          const describe = value => {
            if (value === null) return 'null';
            if (Array.isArray(value)) return `array(${value.length})`;
            if (typeof value === 'object') {
              return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, describe(item)]));
            }
            if (typeof value === 'string') return `string(${value.length})`;
            return typeof value;
          };
          const parts = token.split('.');
          let fields = [];
          let inner = null;
          try {
            fields = decode(parts[3] || '').split('|');
            try { inner = JSON.parse(decode(fields[6] || '')); } catch {}
          } catch {}
          return {
            length: token.length,
            parts: parts.map(part => part.length),
            fields: fields.map(field => `${field.length}`),
            safeFields: Object.fromEntries([0, 3, 4, 5, 7, 8, 9, 10].map(index => [index, fields[index] || ''])),
            stringFieldLengths: Object.fromEntries([1, 2, 6].map(index => [index, fields[index]?.length || 0])),
            inner: inner ? describe(inner) : null,
          };
        };
        let storage = {};
        try {
          storage = {
            cookie: summarizeToken(document.cookie),
            localStorage: Object.keys(localStorage),
            localStorageThreshold: localStorage.getItem('x_wbaas_token_treshold'),
            sessionStorage: Object.keys(sessionStorage),
          };
        } catch (error) {
          storage = { error: String(error) };
        }
        const iframes = Array.from(document.querySelectorAll('iframe')).map(frame => {
          const frameDocument = frame.contentDocument;
          const frameWindow = frame.contentWindow;
          return {
            src: frame.src,
            sandbox: frame.getAttribute('sandbox') || '',
            hasContentDocument: !!frameDocument,
            documentUrl: frameDocument?.URL || '',
            bodyText: (frameDocument?.body?.innerText || frameDocument?.body?.textContent || '').slice(0, 2000),
            hasContentWindow: !!frameWindow,
            postMessage: typeof frameWindow?.postMessage,
            addEventListener: typeof frameWindow?.addEventListener,
          };
        });
        const sdkInstances = Object.keys(window)
          .filter(key => key.startsWith('ANTI_SDK_WB_'))
          .map(key => {
            const value = window[key];
            return {
              key,
              fields: value && typeof value === 'object' ? Object.keys(value) : [],
              challengeSolver: value?.challengeSolver ? Object.keys(value.challengeSolver) : null,
              httpService: value?.httpService ? Object.keys(value.httpService) : null,
              analyticsService: value?.analyticsService ? Object.keys(value.analyticsService) : null,
            };
          });
        const globalSymbols = Object.getOwnPropertySymbols(window).map(symbol => ({
          description: symbol.description || '',
          valueType: typeof window[symbol],
          name: typeof window[symbol] === 'function' ? window[symbol].name : '',
          prototypeKeys: typeof window[symbol] === 'function'
            ? Object.getOwnPropertyNames(window[symbol].prototype || {})
            : [],
        }));
        let solverTest = null;
        const solverConstructor = Object.getOwnPropertySymbols(window)
          .map(symbol => window[symbol])
          .find(value => typeof value === 'function' && value.prototype?.solve);
        if (solverConstructor) {
          try {
            const solver = new solverConstructor('/__wbaas/challenges/antibot', {
              lang: navigator.language,
              metricsEnabled: false,
            });
            solverTest = { ok: true, fields: Object.keys(solver) };
          } catch (error) {
            solverTest = { ok: false, error: String(error) };
          }
        }
        return {
          url: location.href,
          title: document.title,
          h1: document.querySelector('h1')?.textContent?.trim() || '',
          description: read('meta[name="description"]'),
          ogTitle: read('meta[property="og:title"]'),
          ogDescription: read('meta[property="og:description"]'),
          priceText,
          jsonLd,
          body: body.slice(0, 30000),
          targetScript: targetScript.slice(0, 50000),
          challenge: {
            cdpProbeBeforeWait,
            cdpProbeAfterWait: globalThis.__cdp_probe,
            readyState: document.readyState,
            waitMessage: document.querySelector('#wait_msg')?.innerText || '',
            contentText: document.querySelector('#c_cont')?.innerText || '',
            contentHtml: document.querySelector('#c_cont')?.innerHTML || '',
            outdatedBrowser: window.IS_OUTDATED_BROWSER,
            userAgent: navigator.userAgent,
            platform: navigator.platform,
            language: navigator.language,
            cookieEnabled: navigator.cookieEnabled,
            webdriver: navigator.webdriver,
            fingerprint,
            resources,
            storage,
            errors: window.__obscura_errors || [],
            initErrors: window.__wb_errors || [],
            rejections: window.__wb_rejections || [],
            consoleErrors: window.__wb_console_errors || [],
            dynamicScripts: window.__wb_dynamic_scripts || [],
            fetchCalls: window.__wb_fetch_calls || [],
            tokenTranscript: window.__wb_token_transcript || [],
            iframes,
            sdkInstances,
          globalSymbols,
          vmfp: {
            type: typeof globalThis.__vmfp,
            fields: globalThis.__vmfp && typeof globalThis.__vmfp === 'object'
              ? Object.keys(globalThis.__vmfp)
              : [],
          },
          solverTest,
          },
        };
      });
    } catch (error) {
      pageData = { evaluationError: String(error) };
    }
    products.push({ requestedUrl: url, navigationError, pageData, browserEvents });
    await page.close();
  }

  const serverLogSummary = interestingServerLog.slice(-400);
  console.log(JSON.stringify({ selected, products, serverLogSummary }, null, 2));
  await context.close();
  await browser.close();
  browser = undefined;
} finally {
  if (browser) await browser.close().catch(() => {});
  obscura.kill();
}
