// Chrome or Obscura, driven over raw CDP without Playwright's automation tells.
//
//   node tools/ab/chrome-raw.mjs [--site wb|ozon] [--cards 3] [--headed]
//                                [--only chrome|obscura] [--clean-host]
//                                [--url url] [--trace-network] [--dump-dir path]
//                                [--profile-workbench-dir path]
//                                [--trace-replay-helpers]
//
// Playwright is convenient but it announces itself: it sends Runtime.enable on
// every page, and Chrome launched for automation carries
// --enable-automation and the AutomationControlled blink feature. Any of those
// is enough for a site to treat the session differently, which makes "real
// Chrome" a poor control exactly when the control matters.
//
// So this launches Chrome itself and speaks CDP over the websocket by hand:
// Page.navigate and Runtime.evaluate only, never Runtime.enable. Authenticated
// HTTP proxies briefly use Fetch only to answer the proxy login challenge.

import { execFileSync, spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { closeSync, existsSync, mkdirSync, mkdtempSync, openSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import net from 'node:net';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const SITES = {
  wb: {
    home: 'https://www.wildberries.ru/',
    link: 'a[href*="/catalog/"][href*="/detail.aspx"]',
    idFrom: url => (url.match(/\/catalog\/(\d+)\/detail/) || [])[1],
  },
  ozon: {
    home: 'https://www.ozon.ru/',
    link: 'a[href*="/product/"]',
    idFrom: url => (url.match(/\/product\/[^/?#]*?-(\d+)(?:[/?#]|$)/) || [])[1],
  },
};

function parseArgs(argv) {
  const opts = { cards: 3, headed: false, wait: 20, site: 'wb', engine: 'chrome' };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--cards') opts.cards = Number(argv[++i]);
    else if (argv[i] === '--headed') opts.headed = true;
    else if (argv[i] === '--proxy') opts.proxy = argv[++i];
    else if (argv[i] === '--wait') opts.wait = Number(argv[++i]);
    else if (argv[i] === '--url') opts.url = argv[++i];
    else if (argv[i] === '--hold') opts.hold = Number(argv[++i]);
    else if (argv[i] === '--site') opts.site = argv[++i];
    else if (argv[i] === '--clean-host') opts.cleanHost = true;
    else if (argv[i] === '--only') opts.engine = argv[++i];
    else if (argv[i] === '--trace-network') opts.traceNetwork = true;
    else if (argv[i] === '--dump-dir') opts.dumpDir = argv[++i];
    else if (argv[i] === '--engine-log') opts.engineLog = argv[++i];
    else if (argv[i] === '--disable-quic') opts.disableQuic = true;
    else if (argv[i] === '--emulate-major') opts.emulateMajor = Number(argv[++i]);
    else if (argv[i] === '--profile') opts.profile = argv[++i];
    else if (argv[i] === '--profile-workbench-dir') opts.profileWorkbenchDir = argv[++i];
    else if (argv[i] === '--chrome-bin') opts.chromeBin = argv[++i];
    else if (argv[i] === '--trace-challenge') opts.traceChallenge = true;
    else if (argv[i] === '--probe-wb-startup') opts.probeWbStartup = true;
    else if (argv[i] === '--replay') opts.replay = argv[++i];
    else if (argv[i] === '--trace-replay-helpers') opts.traceReplayHelpers = true;
  }
  return opts;
}
const opts = parseArgs(process.argv.slice(2));
const site = SITES[opts.site];
if (!site) throw new Error(`unknown --site ${opts.site}; expected wb or ozon`);
if (opts.cleanHost && opts.proxy) throw new Error('--clean-host cannot be combined with --proxy');
if (!['chrome', 'obscura'].includes(opts.engine)) {
  throw new Error(`unknown --only ${opts.engine}; expected chrome or obscura`);
}
if (opts.engine === 'obscura' && opts.chromeBin) {
  throw new Error('--chrome-bin applies only to the Chrome control');
}
if (opts.engine === 'chrome' && opts.profileWorkbenchDir) {
  throw new Error('--profile-workbench-dir applies only to Obscura');
}
if (opts.engine === 'chrome' && opts.engineLog) {
  throw new Error('--engine-log applies only to Obscura');
}
if (opts.replay && opts.proxy) {
  throw new Error('--replay is local and cannot be combined with --proxy');
}
if (opts.traceReplayHelpers && !opts.replay) {
  throw new Error('--trace-replay-helpers requires --replay');
}

const SAFE_REQUEST_HEADERS = [
  'accept', 'accept-encoding', 'accept-language', 'connection', 'content-length',
  'content-type', 'host', 'origin', 'priority', 'referer', 'sec-ch-ua',
  'sec-ch-ua-mobile', 'sec-ch-ua-platform', 'sec-fetch-dest', 'sec-fetch-mode',
  'sec-fetch-site', 'user-agent',
];

function safeRequestHeaders(headers) {
  return Object.fromEntries(SAFE_REQUEST_HEADERS.flatMap(name =>
    headers[name] === undefined ? [] : [[name, headers[name]]]));
}

function challengeFetchTraceScript() {
  return `(() => {
    const calls = [];
    Object.defineProperty(globalThis, '__abChallengeFetchCalls', {
      value: calls, writable: false, enumerable: false, configurable: true,
    });
    const pageErrors = [];
    Object.defineProperty(globalThis, '__abPageErrors', {
      value: pageErrors, writable: false, enumerable: false, configurable: true,
    });
    addEventListener('error', event => pageErrors.push({
      type: 'error',
      message: String(event?.message || '').slice(0, 1000),
      filename: String(event?.filename || '').slice(0, 1000),
      line: Number(event?.lineno || 0),
      column: Number(event?.colno || 0),
      stack: String(event?.error?.stack || '').slice(0, 6000),
    }));
    addEventListener('unhandledrejection', event => pageErrors.push({
      type: 'unhandledrejection',
      message: String(event?.reason?.message || event?.reason || '').slice(0, 1000),
      stack: String(event?.reason?.stack || '').slice(0, 6000),
    }));
    const safeStack = () => String(new Error().stack || '')
      .split('\\n')
      .slice(1, 7)
      .map(line => line.replace(/https?:\\/\\/[^\\s)]+/g, value => {
        try {
          const url = new URL(value);
          return url.origin + url.pathname;
        } catch { return '<url>'; }
      }));
    const hashText = value => {
      let hash = 2166136261;
      for (let i = 0; i < value.length; i++) {
        hash ^= value.charCodeAt(i);
        hash = Math.imul(hash, 16777619);
      }
      return (hash >>> 0).toString(16).padStart(8, '0');
    };
    const originalFetchOp = globalThis.Deno?.core?.ops?.op_fetch_url;
    if (typeof originalFetchOp === 'function') {
      globalThis.Deno.core.ops.op_fetch_url = new Proxy(originalFetchOp, {
        apply(target, thisArg, args) {
          const requestUrl = String(args[0] || '');
          let path = '';
          try { path = new URL(requestUrl, location.href).pathname; } catch {}
          if (path === '/webapi/logging/jserror') {
            const text = typeof args[3] === 'string' ? args[3] : '';
            calls.push({
              kind: 'op-page-error',
              method: String(args[1] || ''),
              body: text.slice(0, 16_384),
              truncated: text.length > 16_384,
              stack: safeStack(),
            });
          }
          return Reflect.apply(target, thisArg, args);
        },
      });
    }
    const originalFetch = globalThis.fetch;
    if (typeof originalFetch !== 'function') return;
    globalThis.fetch = new Proxy(originalFetch, {
      apply(target, thisArg, args) {
        const input = args[0];
        const init = args[1];
        const requestUrl = String(typeof input === 'string' ? input : input?.url || input);
        let path = '';
        try { path = new URL(requestUrl, location.href).pathname; } catch {}
        if (path === '/abt/result') {
          const body = init?.body;
          const text = typeof body === 'string' ? body : '';
          let bodyShape = null;
          let errorDetails = [];
          try {
            const parsed = JSON.parse(text);
            bodyShape = {
              keys: Object.keys(parsed).sort(),
              fpLength: typeof parsed.fp === 'string' ? parsed.fp.length : 0,
              tokenLength: typeof parsed.token === 'string' ? parsed.token.length : 0,
              errorLength: typeof parsed.error === 'string' ? parsed.error.length : 0,
              infoLength: typeof parsed.info === 'string' ? parsed.info.length : 0,
              hasTimings: Boolean(parsed.timings && typeof parsed.timings === 'object'),
            };
            const errors = JSON.parse(parsed.error || '[]');
            if (Array.isArray(errors)) {
              errorDetails = errors.slice(0, 3).map(item => ({
                level: String(item?.level || '').slice(0, 32),
                message: String(item?.message || '').slice(0, 300),
                stack: String(item?.stack_trace || '').slice(0, 1200),
                bytecode: String(item?.bytecode || '').slice(0, 160),
              }));
            }
          } catch {}
          calls.push({
            kind: 'challenge-result',
            bodyKind: body == null ? 'none' : Object.prototype.toString.call(body),
            bodyLength: text ? text.length : Number(body?.byteLength ?? body?.size ?? 0),
            bodyHash: text ? hashText(text) : '',
            bodyShape,
            errorDetails,
            stack: safeStack(),
          });
        } else if (path === '/webapi/logging/jserror') {
          const body = init?.body;
          const text = typeof body === 'string'
            ? body
            : (typeof URLSearchParams !== 'undefined' && body instanceof URLSearchParams)
              ? body.toString()
              : '';
          calls.push({
            kind: 'page-error',
            bodyKind: body == null ? 'none' : Object.prototype.toString.call(body),
            body: text.slice(0, 16_384),
            truncated: text.length > 16_384,
            stack: safeStack(),
          });
        } else if (/^https:\/\/marketplace-sentry\.wb\.ru\/api\/(?:183|355)\/envelope\//.test(requestUrl)) {
          const body = init?.body;
          const text = typeof body === 'string' ? body : '';
          calls.push({
            kind: 'sentry-error',
            bodyKind: body == null ? 'none' : Object.prototype.toString.call(body),
            body: text.slice(0, 16_384),
            truncated: text.length > 16_384,
            stack: safeStack(),
          });
        }
        return Reflect.apply(target, thisArg, args);
      },
    });
  })()`;
}

function wbStartupTraceScript() {
  return `(() => {
    const trace = {errors: [], initCalls: 0, initFulfilled: 0};
    Object.defineProperty(globalThis, '__abWbSpaTrace', {
      value: trace, writable: false, enumerable: false, configurable: true,
    });
    const record = (phase, error, context) => trace.errors.push({
      phase,
      name: String(error?.name || ''),
      message: String(error?.message || error || '').slice(0, 2000),
      stack: String(error?.stack || '').slice(0, 8000),
      context,
    });
    const originalConsoleError = globalThis.console?.error;
    if (typeof originalConsoleError === 'function') {
      globalThis.console.error = new Proxy(originalConsoleError, {
        apply(target, thisArg, args) {
          if ((trace.consoleErrors || (trace.consoleErrors = [])).length < 100) {
            trace.consoleErrors.push(args.map(arg => ({
              text: String(arg?.message || arg || '').slice(0, 2000),
              stack: String(arg?.stack || '').slice(0, 8000),
            })));
          }
          return Reflect.apply(target, thisArg, args);
        },
      });
    }
    const wrapSpa = spa => {
      if (!spa || typeof spa !== 'object') return false;
      const originalLogError = spa.logError;
      if (typeof originalLogError === 'function' && !originalLogError.__abStartupTrace) {
        const wrappedLogError = new Proxy(originalLogError, {
          apply(target, thisArg, args) {
            record('logError', args[0], args[1]);
            return Reflect.apply(target, thisArg, args);
          },
        });
        Object.defineProperty(wrappedLogError, '__abStartupTrace', {value: true});
        spa.logError = wrappedLogError;
      }
      const originalInit = spa.init;
      if (typeof originalInit === 'function' && !originalInit.__abStartupTrace) {
        const wrappedInit = new Proxy(originalInit, {
          apply(target, thisArg, args) {
            trace.initCalls++;
            let result;
            try {
              result = Reflect.apply(target, thisArg, args);
            } catch (error) {
              record('init-sync', error);
              throw error;
            }
            Promise.resolve(result).then(
              () => trace.initFulfilled++,
              error => record('init-rejection', error),
            );
            return result;
          },
        });
        Object.defineProperty(wrappedInit, '__abStartupTrace', {value: true});
        spa.init = wrappedInit;
      }
      return Boolean(spa.init?.__abStartupTrace);
    };
    const wrapNamespace = original => {
      if (typeof original !== 'function' || original.__abStartupTrace) return original;
      const wrapped = new Proxy(original, {
        apply(target, thisArg, args) {
          trace.namespaceCalls = (trace.namespaceCalls || 0) + 1;
          if (args[0] === 'spa' && typeof args[1] === 'function') {
            const factory = args[1];
            args = [...args];
            args[1] = new Proxy(factory, {
              apply(factoryTarget, factoryThis, factoryArgs) {
                const value = Reflect.apply(factoryTarget, factoryThis, factoryArgs);
                trace.spaCreated = (trace.spaCreated || 0) + 1;
                wrapSpa(value);
                return value;
              },
            });
          }
          const value = Reflect.apply(target, thisArg, args);
          wrapSpa(globalThis.wb?.spa);
          return value;
        },
      });
      Object.defineProperty(wrapped, '__abStartupTrace', {value: true});
      return wrapped;
    };
    const wrapWb = value => {
      if (!value || (typeof value !== 'object' && typeof value !== 'function')) return value;
      if (value.__abWbTraceProxy) return value;
      const wrapped = new Proxy(value, {
        get(target, key, receiver) {
          const current = Reflect.get(target, key, receiver);
          if (key === 'namespace') {
            const next = wrapNamespace(current);
            if (next !== current) Reflect.set(target, key, next, receiver);
            return next;
          }
          return current;
        },
        set(target, key, next, receiver) {
          return Reflect.set(target, key,
            key === 'namespace' ? wrapNamespace(next) : next, receiver);
        },
      });
      Object.defineProperty(wrapped, '__abWbTraceProxy', {value: true});
      return wrapped;
    };
    const wrapProduct = value => {
      if (!value || typeof value !== 'object' || value.__abProductTraceProxy) return value;
      const originalInit = value.init;
      if (typeof originalInit === 'function') {
        value.init = new Proxy(originalInit, {
          apply(target, thisArg, args) {
            trace.productInitCalls = (trace.productInitCalls || 0) + 1;
            try {
              const result = Reflect.apply(target, thisArg, args);
              Promise.resolve(result).then(
                () => trace.productInitFulfilled = (trace.productInitFulfilled || 0) + 1,
                error => record('product-init-rejection', error),
              );
              return result;
            } catch (error) {
              record('product-init-sync', error);
              throw error;
            }
          },
        });
      }
      const originalGet = value.get;
      if (typeof originalGet === 'function') {
        value.get = new Proxy(originalGet, {
          apply(target, thisArg, args) {
            trace.productGetCalls = (trace.productGetCalls || 0) + 1;
            (trace.productGetArgs || (trace.productGetArgs = [])).push(
              args.map(arg => String(arg)).slice(0, 8));
            let result;
            try {
              result = Reflect.apply(target, thisArg, args);
            } catch (error) {
              record('product-get-sync', error);
              throw error;
            }
            return Promise.resolve(result).then(factory => {
              trace.productGetFulfilled = (trace.productGetFulfilled || 0) + 1;
              if (typeof factory !== 'function') return factory;
              return new Proxy(factory, {
                apply(factoryTarget, factoryThis, factoryArgs) {
                  trace.productFactoryCalls = (trace.productFactoryCalls || 0) + 1;
                  try {
                    const exports = Reflect.apply(factoryTarget, factoryThis, factoryArgs);
                    trace.productExportKeys = exports && typeof exports === 'object'
                      ? Object.keys(exports).slice(0, 100) : [];
                    return exports;
                  } catch (error) {
                    record('product-factory-sync', error);
                    throw error;
                  }
                },
              });
            }, error => {
              record('product-get-rejection', error);
              throw error;
            });
          },
        });
      }
      Object.defineProperty(value, '__abProductTraceProxy', {value: true});
      return value;
    };
    const wrapMfProduct = value => {
      if (!value || typeof value !== 'object' || value.__abMfProductTrace) return value;
      const originalInit = value.init;
      if (typeof originalInit === 'function') {
        value.init = new Proxy(originalInit, {
          apply(target, thisArg, args) {
            trace.mfProductInitCalls = (trace.mfProductInitCalls || 0) + 1;
            return Reflect.apply(target, thisArg, args);
          },
        });
      }
      const originalGet = value.get;
      if (typeof originalGet === 'function') {
        value.get = new Proxy(originalGet, {
          apply(target, thisArg, args) {
            trace.mfProductGetCalls = (trace.mfProductGetCalls || 0) + 1;
            const moduleName = String(args[0] || '');
            (trace.mfProductGetArgs || (trace.mfProductGetArgs = [])).push(
              args.map(arg => String(arg)).slice(0, 8));
            let result;
            try {
              result = Reflect.apply(target, thisArg, args);
            } catch (error) {
              record('mf-product-get-sync', error);
              throw error;
            }
            return Promise.resolve(result).then(factory => {
              trace.mfProductGetFulfilled = (trace.mfProductGetFulfilled || 0) + 1;
              if (typeof factory !== 'function') return factory;
              return new Proxy(factory, {
                apply(factoryTarget, factoryThis, factoryArgs) {
                  trace.mfProductFactoryCalls = (trace.mfProductFactoryCalls || 0) + 1;
                  try {
                    const exports = Reflect.apply(factoryTarget, factoryThis, factoryArgs);
                    trace.mfProductExportKeys = exports && typeof exports === 'object'
                      ? Object.keys(exports).slice(0, 100) : [];
                    if (!exports || typeof exports !== 'object') return exports;
                    const wrappedValues = new Map();
                    return new Proxy(exports, {
                      get(exportsTarget, key, receiver) {
                        const exported = Reflect.get(exportsTarget, key, receiver);
                        if (typeof exported !== 'function') return exported;
                        if (wrappedValues.has(key)) return wrappedValues.get(key);
                        const label = moduleName + ':' + String(key);
                        const wrapped = new Proxy(exported, {
                          apply(exportTarget, exportThis, exportArgs) {
                            (trace.mfExportCalls || (trace.mfExportCalls = [])).push({
                              label,
                              argCount: exportArgs.length,
                              firstArgKeys: exportArgs[0] && typeof exportArgs[0] === 'object'
                                ? Object.keys(exportArgs[0]).slice(0, 100) : [],
                            });
                            try {
                              const result = Reflect.apply(exportTarget, exportThis, exportArgs);
                              (trace.mfExportReturns || (trace.mfExportReturns = [])).push({
                                label,
                                type: typeof result,
                                keys: result && typeof result === 'object'
                                  ? Object.keys(result).slice(0, 50) : [],
                              });
                              return result;
                            } catch (error) {
                              record('mf-export-call:' + label, error);
                              throw error;
                            }
                          },
                          construct(exportTarget, exportArgs, newTarget) {
                            (trace.mfExportConstructs || (trace.mfExportConstructs = [])).push({
                              label,
                              argCount: exportArgs.length,
                            });
                            try {
                              return Reflect.construct(exportTarget, exportArgs, newTarget);
                            } catch (error) {
                              record('mf-export-construct:' + label, error);
                              throw error;
                            }
                          },
                        });
                        wrappedValues.set(key, wrapped);
                        return wrapped;
                      },
                    });
                  } catch (error) {
                    record('mf-product-factory-sync', error);
                    throw error;
                  }
                },
              });
            }, error => {
              record('mf-product-get-rejection', error);
              throw error;
            });
          },
        });
      }
      Object.defineProperty(value, '__abMfProductTrace', {value: true});
      return value;
    };
    let mfProductValue = globalThis.__MF_PROMISE__product;
    if (mfProductValue) Promise.resolve(mfProductValue).then(wrapMfProduct);
    try {
      const descriptor = Object.getOwnPropertyDescriptor(globalThis, '__MF_PROMISE__product');
      if (!descriptor || descriptor.configurable) {
        Object.defineProperty(globalThis, '__MF_PROMISE__product', {
          configurable: true,
          enumerable: true,
          get: () => mfProductValue,
          set: value => {
            trace.mfProductAssignments = (trace.mfProductAssignments || 0) + 1;
            mfProductValue = value;
            Promise.resolve(value).then(wrapMfProduct,
              error => record('mf-product-promise-rejection', error));
          },
        });
      }
    } catch (error) {
      record('mf-product-hook', error);
    }
    let productValue = wrapProduct(globalThis.product);
    try {
      const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'product');
      if (!descriptor || descriptor.configurable) {
        Object.defineProperty(globalThis, 'product', {
          configurable: true,
          enumerable: true,
          get: () => productValue,
          set: value => {
            trace.productAssignments = (trace.productAssignments || 0) + 1;
            productValue = wrapProduct(value);
          },
        });
      }
    } catch (error) {
      record('product-hook', error);
    }
    let wbValue = wrapWb(globalThis.wb);
    try {
      const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'wb');
      if (!descriptor || descriptor.configurable) {
        Object.defineProperty(globalThis, 'wb', {
          configurable: true,
          enumerable: true,
          get: () => wbValue,
          set: value => {
            trace.wbAssignments = (trace.wbAssignments || 0) + 1;
            wbValue = wrapWb(value);
          },
        });
      }
    } catch (error) {
      record('wb-hook', error);
    }
    let attempts = 0;
    const timer = setInterval(() => {
      attempts++;
      if (wrapSpa(globalThis.wb?.spa)) {
        clearInterval(timer);
      } else if (attempts >= 10_000) {
        clearInterval(timer);
      }
    }, 0);
  })()`;
}

function replayHelperTraceScript() {
  return `<script>(function(){
    const trace = [];
    const shape = (value, depth) => {
      if (value === null) return 'null';
      if (value === undefined) return 'undefined';
      if (typeof value === 'string') return 'string(' + value.length + ')';
      if (typeof value === 'function') {
        return {
          functionName: value.name,
          hasOwnPrototype: Object.prototype.hasOwnProperty.call(value, 'prototype'),
        };
      }
      if (typeof value !== 'object') return typeof value;
      if (Array.isArray(value)) {
        if (depth >= 2) return 'array(' + value.length + ')';
        return { arrayLength: value.length, items: value.slice(0, 20).map(item => shape(item, depth + 1)) };
      }
      const keys = Object.keys(value).sort();
      if (depth >= 2) return 'object(' + keys.length + ')';
      return Object.fromEntries(keys.slice(0, 80).map(key => [key, shape(value[key], depth + 1)]));
    };
    const nativeStringify = JSON.stringify;
    const canvasModes = new WeakMap();
    const scalarCanvasArg = value => {
      if (typeof value === 'number' || typeof value === 'boolean') return value;
      if (typeof value === 'string') return 'string(' + value.length + ')';
      if (value && typeof value === 'object' && Number.isFinite(value.width) && Number.isFinite(value.height)) {
        return { width: value.width, height: value.height,
          dataLength: value.data && Number.isFinite(value.data.length) ? value.data.length : undefined };
      }
      return value === null ? 'null' : typeof value;
    };
    const canvasPrototype = globalThis.HTMLCanvasElement && HTMLCanvasElement.prototype;
    if (canvasPrototype && typeof canvasPrototype.getContext === 'function') {
      const nativeGetContext = canvasPrototype.getContext;
      canvasPrototype.getContext = new Proxy(nativeGetContext, {
        apply(target, thisArg, args) {
          const result = Reflect.apply(target, thisArg, args);
          canvasModes.set(thisArg, String(args[0]));
          if (trace.length < 300) trace.push({
            key: 'canvas.getContext', mode: String(args[0]), width: thisArg.width,
            height: thisArg.height, resultType: result && result.constructor && result.constructor.name,
          });
          return result;
        },
      });
    }
    if (canvasPrototype && typeof canvasPrototype.toDataURL === 'function') {
      const nativeToDataURL = canvasPrototype.toDataURL;
      canvasPrototype.toDataURL = new Proxy(nativeToDataURL, {
        apply(target, thisArg, args) {
          const result = Reflect.apply(target, thisArg, args);
          let binaryLength = 0;
          let chunks = [];
          try {
            const binary = atob(String(result).split(',')[1] || '');
            binaryLength = binary.length;
            for (let offset = 8; offset + 12 <= binary.length;) {
              const length = ((binary.charCodeAt(offset) << 24) |
                (binary.charCodeAt(offset + 1) << 16) |
                (binary.charCodeAt(offset + 2) << 8) |
                binary.charCodeAt(offset + 3)) >>> 0;
              const type = binary.slice(offset + 4, offset + 8);
              chunks.push({ type, length });
              offset += length + 12;
              if (chunks.length >= 12) break;
            }
          } catch {}
          if (trace.length < 300) trace.push({
            key: 'canvas.toDataURL', mode: canvasModes.get(thisArg),
            width: thisArg.width, height: thisArg.height,
            dataUrlLength: typeof result === 'string' ? result.length : 0,
            binaryLength, chunks,
          });
          return result;
        },
      });
    }
    const contextPrototype = globalThis.CanvasRenderingContext2D && CanvasRenderingContext2D.prototype;
    if (contextPrototype) {
      for (const name of ['fillRect', 'clearRect', 'strokeRect', 'fillText', 'strokeText',
        'drawImage', 'putImageData', 'arc', 'rect', 'fill', 'stroke', 'beginPath',
        'moveTo', 'lineTo', 'bezierCurveTo', 'quadraticCurveTo']) {
        const descriptor = Object.getOwnPropertyDescriptor(contextPrototype, name);
        if (!descriptor || typeof descriptor.value !== 'function') continue;
        descriptor.value = new Proxy(descriptor.value, {
          apply(target, thisArg, args) {
            if (trace.length < 300) trace.push({
              key: 'canvas2d.' + name,
              args: args.slice(0, 10).map(scalarCanvasArg),
              width: thisArg.canvas && thisArg.canvas.width,
              height: thisArg.canvas && thisArg.canvas.height,
              fillStyle: typeof thisArg.fillStyle === 'string' ? thisArg.fillStyle : typeof thisArg.fillStyle,
              strokeStyle: typeof thisArg.strokeStyle === 'string' ? thisArg.strokeStyle : typeof thisArg.strokeStyle,
              font: thisArg.font,
              alpha: thisArg.globalAlpha,
              composite: thisArg.globalCompositeOperation,
            });
            return Reflect.apply(target, thisArg, args);
          },
        });
        Object.defineProperty(contextPrototype, name, descriptor);
      }
    }
    const webglIds = new WeakMap();
    let nextWebglId = 1;
    const webglObject = value => {
      if (!value || (typeof value !== 'object' && typeof value !== 'function')) return scalarCanvasArg(value);
      if (!webglIds.has(value)) webglIds.set(value, nextWebglId++);
      if (ArrayBuffer.isView(value)) {
        return { type: value.constructor.name, length: value.length,
          sample: Array.from(value.slice ? value.slice(0, 12) : []).slice(0, 12) };
      }
      return { type: value.constructor && value.constructor.name, id: webglIds.get(value) };
    };
    const webglArg = value => {
      if (typeof value === 'string') {
        return { stringLength: value.length, prefix: value.slice(0, 500) };
      }
      return webglObject(value);
    };
    const webglPrototype = globalThis.WebGLRenderingContext && WebGLRenderingContext.prototype;
    if (webglPrototype) {
      for (const name of ['createBuffer', 'bindBuffer', 'bufferData', 'createProgram',
        'createShader', 'shaderSource', 'compileShader', 'getShaderParameter',
        'getShaderInfoLog', 'attachShader', 'linkProgram', 'getProgramParameter',
        'getProgramInfoLog', 'useProgram', 'getAttribLocation', 'enableVertexAttribArray',
        'vertexAttribPointer', 'getUniformLocation', 'uniform1f', 'uniform1i', 'uniform2f',
        'uniform4f', 'viewport', 'clearColor', 'clear', 'drawArrays', 'drawElements']) {
        const descriptor = Object.getOwnPropertyDescriptor(webglPrototype, name);
        if (!descriptor || typeof descriptor.value !== 'function') continue;
        descriptor.value = new Proxy(descriptor.value, {
          apply(target, thisArg, args) {
            let result;
            let error;
            try { result = Reflect.apply(target, thisArg, args); }
            catch (caught) { error = caught; }
            if (trace.length < 300) trace.push({
              key: 'webgl.' + name,
              context: webglObject(thisArg),
              args: args.slice(0, 12).map(webglArg),
              result: error ? undefined : webglObject(result),
              error: error && error.name,
            });
            if (error) throw error;
            return result;
          },
        });
        Object.defineProperty(webglPrototype, name, descriptor);
      }
    }
    JSON.stringify = new Proxy(nativeStringify, {
      apply(target, thisArg, args) {
        const result = Reflect.apply(target, thisArg, args);
        if (typeof result === 'string' && result.length >= 100 && trace.length < 300) {
          trace.push({
            key: 'JSON.stringify',
            args: shape(args[0], 0),
            result: 'string(' + result.length + ')',
            resultJsonLength: result.length + 2,
          });
        }
        return result;
      },
    });
    const longestString = (value, depth) => {
      if (typeof value === 'string') return value.length;
      if (!value || typeof value !== 'object' || depth >= 3) return 0;
      let longest = 0;
      const values = Array.isArray(value) ? value : Object.values(value).slice(0, 80);
      for (const item of values) longest = Math.max(longest, longestString(item, depth + 1));
      return longest;
    };
    const wrappedFunctions = new WeakMap();
    const wrappedObjects = new WeakMap();
    const wrapHelper = (value, path, depth) => {
      if (typeof value === 'function') {
        if (wrappedFunctions.has(value)) return wrappedFunctions.get(value);
        const wrapped = new Proxy(value, {
          apply(callTarget, callThis, callArgs) {
            let result;
            let error;
            try { result = Reflect.apply(callTarget, callThis, callArgs); }
            catch (caught) { error = caught; }
            if (path.indexOf('.') < 0 || longestString(callArgs, 0) >= 100 ||
                longestString(result, 0) >= 100) {
              const event = { key: path, args: shape(callArgs, 0) };
              if (path === 'checkIn') {
                event.checksPrototype = callArgs[0] === 'prototype';
              }
              if (path === 'tryCatch') {
                event.scalarArgs = callArgs.map(value =>
                  typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean'
                    ? value
                    : typeof value);
              }
              if (path === 's' && callArgs[0] &&
                  Object.prototype.hasOwnProperty.call(callArgs[0], 'dfe')) {
                event.viewportFlags = Object.fromEntries(
                  ['dfe', 'dwfe', 'dwh', 'dwvs'].map(key => [key, callArgs[0][key]]));
              }
              if (path === 's' && callArgs[0]) {
                const proto = Object.getPrototypeOf(callArgs[0]);
                const brand = proto && proto.constructor && proto.constructor.name;
                if (brand === 'Navigator' || brand === 'Performance') {
                  event.getterDiagnostics = Object.getOwnPropertyNames(proto).flatMap(name => {
                    const descriptor = Object.getOwnPropertyDescriptor(proto, name);
                    if (!descriptor || typeof descriptor.get !== 'function') return [];
                    const source = Function.prototype.toString.call(descriptor.get);
                    let constructable = true;
                    try { Reflect.construct(Object, [], descriptor.get); } catch { constructable = false; }
                    let bareCallThrows = false;
                    try { descriptor.get(); } catch { bareCallThrows = true; }
                    return [{
                      name,
                      source,
                      constructable,
                      hasOwnPrototype: Object.prototype.hasOwnProperty.call(descriptor.get, 'prototype'),
                      bareCallThrows,
                      hasSetter: typeof descriptor.set === 'function',
                    }];
                  });
                }
              }
              if (error) event.error = String(error && error.name || typeof error);
              else {
                event.result = shape(result, 0);
                try {
                  const serialized = nativeStringify(result);
                  event.resultJsonLength = serialized.length;
                  if (path === 's' && serialized.length < 15000) event.rawResult = result;
                } catch {}
              }
              trace.push(event);
            }
            if (error) throw error;
            return result;
          },
        });
        wrappedFunctions.set(value, wrapped);
        return wrapped;
      }
      if (!value || typeof value !== 'object' || depth >= 3) return value;
      if (wrappedObjects.has(value)) return wrappedObjects.get(value);
      const wrapped = new Proxy(value, {
        get(target, key, receiver) {
          return wrapHelper(Reflect.get(target, key, receiver), path + '.' + String(key), depth + 1);
        },
      });
      wrappedObjects.set(value, wrapped);
      return wrapped;
    };
    const nativeDefineProperty = Object.defineProperty;
    Object.defineProperty = new Proxy(nativeDefineProperty, {
      apply(target, thisArg, args) {
        const [object, name, descriptor] = args;
        if (object === window && name === 'btoam' && descriptor && descriptor.value) {
          for (const key of Object.keys(descriptor.value)) {
            descriptor.value[key] = wrapHelper(descriptor.value[key], key, 0);
          }
        }
        return Reflect.apply(target, thisArg, args);
      },
    });
    const nativeFetch = window.fetch;
    window.fetch = new Proxy(nativeFetch, {
      apply(target, thisArg, args) {
        const url = String(typeof args[0] === 'string' ? args[0] : args[0] && args[0].url || args[0]);
        if (!url.includes('/abt/result')) return Reflect.apply(target, thisArg, args);
        const body = nativeStringify(trace.slice(0, 300));
        return Reflect.apply(target, window, ['/abt/helper-trace', {
          method: 'POST', headers: { 'content-type': 'application/json' }, body,
        }]).catch(() => {}).then(() => Reflect.apply(target, thisArg, args));
      },
    });
  })();</script>`;
}

async function startReplayServer(path) {
  let html = readFileSync(resolve(path));
  if (opts.traceReplayHelpers) {
    const source = html.toString('utf8');
    const script = replayHelperTraceScript();
    html = Buffer.from(source.replace(/<head([^>]*)>/i, `<head$1>${script}`), 'utf8');
  }
  const submissions = [];
  const helperTraces = [];
  const server = createServer(async (request, response) => {
    const url = new URL(request.url, 'http://127.0.0.1');
    if (request.method === 'POST' && url.pathname === '/abt/helper-trace') {
      const chunks = [];
      let bytes = 0;
      for await (const chunk of request) {
        bytes += chunk.length;
        if (bytes > 1024 * 1024) {
          response.writeHead(413).end();
          return;
        }
        chunks.push(chunk);
      }
      try {
        const trace = JSON.parse(Buffer.concat(chunks).toString('utf8'));
        if (Array.isArray(trace)) helperTraces.push(...trace.slice(0, 300));
      } catch { /* diagnostic input only */ }
      response.writeHead(204).end();
      return;
    }
    if (request.method === 'POST' && url.pathname === '/abt/result') {
      const chunks = [];
      let bytes = 0;
      for await (const chunk of request) {
        bytes += chunk.length;
        if (bytes > 4 * 1024 * 1024) {
          response.writeHead(413).end();
          return;
        }
        chunks.push(chunk);
      }
      let body = {};
      try { body = JSON.parse(Buffer.concat(chunks).toString('utf8')); } catch {}
      const fp = typeof body.fp === 'string' ? body.fp : '';
      const token = typeof body.token === 'string' ? body.token : '';
      const error = typeof body.error === 'string' ? body.error : '';
      const fpBytes = Buffer.from(fp, 'base64');
      let errorDetails = [];
      try {
        const parsed = JSON.parse(error);
        if (Array.isArray(parsed)) {
          errorDetails = parsed.slice(0, 4).map(item => ({
            level: String(item?.level || '').slice(0, 32),
            message: String(item?.message || '').slice(0, 300),
            stackTrace: String(item?.stack_trace || '').slice(0, 1200),
            bytecode: String(item?.bytecode || '').slice(0, 120),
          }));
        }
      } catch { /* error text is optional diagnostic input */ }
      submissions.push({
        requestHeaders: safeRequestHeaders(request.headers),
        keys: Object.keys(body).sort(),
        fpLength: fp.length,
        fpSha256: createHash('sha256').update(fp).digest('hex'),
        fpDecodedBytes: fpBytes.length,
        fpEnvelope: fpBytes.subarray(0, 8).toString('ascii') === 'Salted__'
          ? 'openssl-salted'
          : 'unknown',
        tokenLength: token.length,
        errorLength: error.length,
        errorDetails,
        infoLength: typeof body.info === 'string' ? body.info.length : 0,
        hasTimings: Boolean(body.timings && typeof body.timings === 'object'),
      });
      response.writeHead(200, { 'content-type': 'application/json' });
      response.end('{"ok":true}');
      return;
    }
    if (url.pathname === '/abt/challenge/ok') {
      response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
      response.end('<!doctype html><title>replay complete</title><p>replay complete</p>');
      return;
    }
    response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
    response.end(html);
  });
  await new Promise((done, fail) => {
    server.once('error', fail);
    server.listen(0, '127.0.0.1', done);
  });
  const { port } = server.address();
  return {
    home: `http://127.0.0.1:${port}/?mode=m`,
    submissions,
    helperTraces,
    close: () => new Promise(done => server.close(done)),
  };
}

const replay = opts.replay ? await startReplayServer(opts.replay) : null;
const HOME = opts.url || replay?.home || site.home;
const dumpDir = opts.dumpDir ? resolve(opts.dumpDir) : null;

const CHROME_CANDIDATES = [
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
];
const chromePath = opts.chromeBin
  ? resolve(opts.chromeBin)
  : CHROME_CANDIDATES.find(existsSync) || CHROME_CANDIDATES[0];
if (opts.engine === 'chrome' && !existsSync(chromePath)) {
  throw new Error(`Chrome binary was not found: ${chromePath}`);
}
const obscuraPath = resolve(import.meta.dirname, '..', '..', 'target', 'release', 'obscura.exe');

function freePort() {
  return new Promise((done, fail) => {
    const server = net.createServer();
    server.once('error', fail);
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      server.close(() => done(port));
    });
  });
}

const chromeProxy = opts.engine === 'chrome' && opts.proxy ? new URL(opts.proxy) : null;
if (chromeProxy?.username && /^socks/i.test(chromeProxy.protocol)) {
  throw new Error('Chrome does not support SOCKS proxy authentication; use this endpoint in HTTP mode');
}
const profileDir = opts.engine === 'chrome'
  ? mkdtempSync(join(tmpdir(), 'chrome-raw-'))
  : null;
const port = await freePort();
const executable = opts.engine === 'chrome' ? chromePath : obscuraPath;
const args = opts.engine === 'chrome'
  ? [
      `--remote-debugging-port=${port}`,
      `--user-data-dir=${profileDir}`,
      // The three things that make a launched Chrome look launched.
      '--disable-blink-features=AutomationControlled',
      '--no-first-run',
      '--no-default-browser-check',
      '--no-service-autorun',
      '--password-store=basic',
    ]
  : [
      '--stealth',
      ...(replay ? ['--allow-private-network'] : []),
      'serve',
      '--port', String(port),
      ...(opts.profileWorkbenchDir ? ['--profile-workbench-dir', resolve(opts.profileWorkbenchDir)] : []),
    ];
if (opts.engine === 'chrome') {
  if (!opts.headed) args.push('--headless=new');
  if (opts.disableQuic) args.push('--disable-quic');
  if (chromeProxy) {
    // Chrome names remote-DNS SOCKS5 as `socks5`; unlike curl and wreq it
    // rejects the equivalent `socks5h` spelling with ERR_NO_SUPPORTED_PROXIES.
    const protocol = chromeProxy.protocol === 'socks5h:' ? 'socks5:' : chromeProxy.protocol;
    args.push(`--proxy-server=${protocol}//${chromeProxy.host}`);
  }
  args.push('about:blank');
}

function directChildEnv(extra = {}) {
  const env = { ...process.env };
  for (const name of ['OBSCURA_PROXY', 'OBSCURA_PROFILE', 'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY',
                      'NO_PROXY', 'http_proxy', 'https_proxy', 'all_proxy', 'no_proxy']) {
    delete env[name];
  }
  env.NO_PROXY = '*';
  env.no_proxy = '*';
  return { ...env, ...extra };
}

let chrome;
let cleanHostDir;
const engineLogFd = opts.engineLog && !opts.cleanHost
  ? openSync(resolve(opts.engineLog), 'w')
  : null;
if (opts.cleanHost) {
  const powerShellQuote = value => `'${String(value).replaceAll("'", "''")}'`;
  const windowsArgument = value => /[\s"]/.test(String(value))
    ? `"${String(value).replaceAll('"', '\\"')}"`
    : String(value);
  const argumentString = args.map(windowsArgument).join(' ');
  if (opts.engine === 'obscura') {
    cleanHostDir = mkdtempSync(join(tmpdir(), 'obscura-raw-clean-host-'));
    const launcher = join(cleanHostDir, 'launch.cmd');
    writeFileSync(launcher, [
      '@echo off',
      'set OBSCURA_PROXY=',
      'set OBSCURA_PROFILE=',
      'set HTTP_PROXY=', 'set HTTPS_PROXY=', 'set ALL_PROXY=',
      'set http_proxy=', 'set https_proxy=', 'set all_proxy=',
      'set NO_PROXY=*', 'set no_proxy=*',
      'set OBSCURA_NAV_TIMEOUT_MS=90000',
      ...(opts.engineLog ? [
        'set RUST_LOG=obscura_browser::page=debug,obscura_js::runtime=debug,obscura_js::ops=debug,obscura_net=debug',
      ] : []),
      ...(opts.profile ? [`set OBSCURA_PROFILE=${opts.profile}`] : []),
      `${windowsArgument(executable)} ${argumentString}` +
        (opts.engineLog ? ` > ${windowsArgument(resolve(opts.engineLog))} 2>&1` : ''),
    ].join('\r\n'), 'utf8');
    const script = [
      '$shell = New-Object -ComObject Shell.Application',
      `$shell.ShellExecute(${powerShellQuote(launcher)}, '', ` +
        `${powerShellQuote(process.cwd())}, 'open', 0)`,
    ].join('; ');
    execFileSync('powershell.exe', ['-NoProfile', '-Command', script], {
      stdio: 'ignore',
      windowsHide: true,
    });
  } else {
    const script = [
      '$shell = New-Object -ComObject Shell.Application',
      `$shell.ShellExecute(${powerShellQuote(executable)}, ` +
        `${powerShellQuote(argumentString)}, ${powerShellQuote(process.cwd())}, 'open', ` +
        `${opts.headed ? 1 : 0})`,
    ].join('; ');
    execFileSync('powershell.exe', ['-NoProfile', '-Command', script], {
      stdio: 'ignore',
      windowsHide: true,
    });
  }
} else {
  chrome = spawn(executable, args, {
    stdio: engineLogFd === null ? 'ignore' : ['ignore', engineLogFd, engineLogFd],
    windowsHide: true,
    env: directChildEnv({
      OBSCURA_NAV_TIMEOUT_MS: '90000',
      ...(opts.proxy ? { OBSCURA_PROXY: opts.proxy } : {}),
      ...(opts.profile ? { OBSCURA_PROFILE: opts.profile } : {}),
    }),
  });
}

const sleep = ms => new Promise(done => setTimeout(done, ms));

async function targetWebSocket() {
  const deadline = Date.now() + 30000;
  for (;;) {
    try {
      if (opts.engine === 'obscura') {
        const version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
        if (version.webSocketDebuggerUrl) return version.webSocketDebuggerUrl;
      }
      const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      const page = list.find(t => t.type === 'page');
      if (page?.webSocketDebuggerUrl) return page.webSocketDebuggerUrl;
    } catch { /* not up yet */ }
    if (Date.now() > deadline) throw new Error(`${opts.engine} did not expose a page target`);
    await sleep(200);
  }
}

let nextId = 0;
function connect(url) {
  const socket = new WebSocket(url);
  const pending = new Map();
  const listeners = new Set();
  socket.addEventListener('message', event => {
    const message = JSON.parse(event.data);
    const waiter = pending.get(message.id);
    if (waiter) {
      pending.delete(message.id);
      waiter(message);
      return;
    }
    for (const listener of listeners) listener(message);
  });
  const ready = new Promise((done, fail) => {
    socket.addEventListener('open', done);
    socket.addEventListener('error', fail);
  });
  const send = (method, params = {}, timeoutMs = 90000, sessionId) => new Promise((done, fail) => {
    const id = ++nextId;
    const timer = setTimeout(() => {
      pending.delete(id);
      fail(new Error(`${method} did not answer within ${timeoutMs}ms`));
    }, timeoutMs);
    pending.set(id, message => {
      clearTimeout(timer);
      if (message.error) fail(new Error(`${method}: ${message.error.message}`));
      else done(message.result);
    });
    socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
  });
  return {
    ready,
    send,
    onMessage: listener => listeners.add(listener),
    close: () => socket.close(),
  };
}

// Runtime.evaluate works without Runtime.enable; that is the whole point.
async function evaluate(cdp, expression) {
  const result = await cdp.send('Runtime.evaluate', {
    expression,
    returnByValue: true,
    awaitPromise: true,
  });
  return result?.result?.value;
}

async function navigate(cdp, url) {
  await cdp.send('Page.navigate', { url });
  // No Page.lifecycleEvent without Page.enable, so poll readyState instead.
  const deadline = Date.now() + 90000;
  const targetIsBlank = url === 'about:blank';
  for (;;) {
    await sleep(500);
    const state = await evaluate(cdp, 'document.readyState + "|" + location.href');
    if (typeof state === 'string' && state.startsWith('complete') &&
        (targetIsBlank ? state.endsWith('|about:blank') : !state.endsWith('|about:blank'))) return;
    if (Date.now() > deadline) throw new Error(`navigation to ${url} did not complete`);
  }
}

const productId = site.idFrom;

let cdp;
let pageCdp;
try {
  cdp = connect(await targetWebSocket());
  await cdp.ready;

  if (opts.engine === 'obscura') {
    if (opts.profile) {
      const selected = await cdp.send('Obscura.setProfile', { profileId: opts.profile });
      console.log('profile:', selected.profileId);
    }
    const { targetInfos } = await cdp.send('Target.getTargets');
    let pageTarget = targetInfos.find(target => target.type === 'page');
    if (!pageTarget) {
      const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' });
      pageTarget = { targetId };
    }
    const { sessionId } = await cdp.send('Target.attachToTarget', {
      targetId: pageTarget.targetId,
      flatten: true,
    });
    pageCdp = {
      send: (method, params = {}, timeoutMs = 90000) =>
        cdp.send(method, params, timeoutMs, sessionId),
      onMessage: listener => cdp.onMessage(message => {
        if (message.sessionId === sessionId) listener(message);
      }),
    };
  } else {
    pageCdp = cdp;
  }

  let proxyAuthEnabled = false;
  let proxyAuthError;
  if (chromeProxy?.username) {
    const username = decodeURIComponent(chromeProxy.username);
    const password = decodeURIComponent(chromeProxy.password);
    pageCdp.onMessage(message => {
      if (message.method === 'Fetch.requestPaused') {
        void pageCdp.send('Fetch.continueRequest', {
          requestId: message.params.requestId,
        }).catch(error => { proxyAuthError = error; });
      } else if (message.method === 'Fetch.authRequired') {
        const isProxy = message.params.authChallenge?.source === 'Proxy';
        void pageCdp.send('Fetch.continueWithAuth', {
          requestId: message.params.requestId,
          authChallengeResponse: isProxy
            ? { response: 'ProvideCredentials', username, password }
            : { response: 'Default' },
        }).catch(error => { proxyAuthError = error; });
      }
    });
    await pageCdp.send('Fetch.enable', { handleAuthRequests: true });
    proxyAuthEnabled = true;
  }

  if (opts.emulateMajor) {
    const major = String(opts.emulateMajor);
    const full = `${major}.0.0.0`;
    await pageCdp.send('Emulation.setUserAgentOverride', {
      userAgent: `Mozilla/5.0 (Windows NT 10.0; Win64; x64) ` +
        `AppleWebKit/537.36 (KHTML, like Gecko) Chrome/${full} Safari/537.36`,
      // CDP adds quality values to this list. Supplying one here produces an
      // invalid duplicate such as `en-US,en;q=0.9;q=0.9` on the wire.
      acceptLanguage: 'en-US,en',
      platform: 'Win32',
      userAgentMetadata: {
        brands: [
          { brand: 'Not:A-Brand', version: '99' },
          { brand: 'Google Chrome', version: major },
          { brand: 'Chromium', version: major },
        ],
        fullVersionList: [
          { brand: 'Not:A-Brand', version: '99.0.0.0' },
          { brand: 'Google Chrome', version: full },
          { brand: 'Chromium', version: full },
        ],
        fullVersion: full,
        platform: 'Windows',
        platformVersion: '15.0.0',
        architecture: 'x86',
        model: '',
        mobile: false,
        bitness: '64',
        wow64: false,
      },
    });
  }

  if (opts.traceChallenge) {
    await pageCdp.send('Page.addScriptToEvaluateOnNewDocument', {
      source: challengeFetchTraceScript(),
    });
  }
  if (opts.probeWbStartup) {
    await pageCdp.send('Page.addScriptToEvaluateOnNewDocument', {
      source: wbStartupTraceScript(),
    });
  }

  let documentRequest;
  let documentResponse;
  let documentResponseRequestId;
  const diagnosticScripts = new Map();
  const challengeRequests = new Map();
  const pageErrorReports = [];
  const watchedNetwork = new Map();
  if (opts.traceNetwork || dumpDir) {
    const networkMessageSource = opts.engine === 'obscura' ? cdp : pageCdp;
    networkMessageSource.onMessage(message => {
      if (opts.traceNetwork && message.method === 'Network.requestWillBeSent') {
        const request = message.params.request;
        if (/\/__internal\/u-card\/cards\/v4\/detail|\/vol2\/product\/dist\/.*\.js(?:\?|$)|\/vol2\/site\/app\/.*\.js(?:\?|$)/
          .test(request.url)) {
          watchedNetwork.set(message.params.requestId, {
            url: request.url.split('?')[0],
            method: request.method,
            type: message.params.type,
          });
        }
      }
      if (opts.traceNetwork && message.method === 'Network.responseReceived') {
        const item = watchedNetwork.get(message.params.requestId);
        if (item) {
          item.status = message.params.response.status;
          item.protocol = message.params.response.protocol;
          item.mimeType = message.params.response.mimeType;
        }
      }
      if (opts.traceNetwork && message.method === 'Network.loadingFailed') {
        const item = watchedNetwork.get(message.params.requestId);
        if (item) item.error = message.params.errorText;
      }
      if (message.method === 'Network.requestWillBeSent' &&
          message.params.type === 'Document' && message.params.request.url.startsWith(HOME)) {
        documentRequest = message.params.request;
      }
      if (message.method === 'Network.responseReceived' &&
          message.params.type === 'Document' && message.params.response.url.startsWith(HOME)) {
        documentResponse = message.params.response;
        documentResponseRequestId = message.params.requestId;
      }
      if (dumpDir && message.method === 'Network.responseReceived' &&
          message.params.type === 'Script' &&
          /client-metrics|cpm-ozon|fingerprint|headless|antibot|captcha|challenge/i
            .test(message.params.response.url)) {
        diagnosticScripts.set(message.params.requestId, message.params.response);
      }
      if (opts.traceChallenge && message.method === 'Network.requestWillBeSent') {
        const request = message.params.request;
        let path = '';
        try { path = new URL(request.url).pathname; } catch {}
        if (path.startsWith('/abt/')) {
          const existing = challengeRequests.get(message.params.requestId);
          if (existing) {
            if (message.sessionId && !existing.sessionIds.includes(message.sessionId)) {
              existing.sessionIds.push(message.sessionId);
              existing.sessionIds.sort();
            }
            return;
          }
          const postData = request.postData || '';
          challengeRequests.set(message.params.requestId, {
            sessionIds: message.sessionId ? [message.sessionId] : [],
            url: (() => {
              try {
                const value = new URL(request.url);
                return `${value.origin}${value.pathname}`;
              } catch { return request.url.split('?')[0]; }
            })(),
            method: request.method,
            headers: safeRequestHeaders(Object.fromEntries(
              Object.entries(request.headers || {}).map(([name, value]) =>
                [name.toLowerCase(), value]))),
            bodyLength: Buffer.byteLength(postData),
            bodySha256: postData
              ? createHash('sha256').update(postData).digest('hex')
              : undefined,
          });
        }
      }
      if (opts.traceNetwork && message.method === 'Network.requestWillBeSent' &&
          /\/webapi\/logging\/jserror(?:\?|$)/.test(message.params.request.url)) {
        const postData = message.params.request.postData || '';
        let query = [];
        try {
          query = [...new URL(message.params.request.url).searchParams]
            .map(([name, value]) => [name, value.slice(0, 2000)]);
        } catch {}
        pageErrorReports.push({
          url: message.params.request.url.split('?')[0],
          method: message.params.request.method,
          type: message.params.type,
          query,
          body: postData.slice(0, 16_384),
          truncated: postData.length > 16_384,
        });
      }
      if (opts.traceChallenge && message.method === 'Network.responseReceived') {
        const item = challengeRequests.get(message.params.requestId);
        if (item) {
          const response = message.params.response;
          item.status = response.status;
          item.protocol = response.protocol;
          item.responseHeaders = safeRequestHeaders(Object.fromEntries(
            Object.entries(response.headers || {}).map(([name, value]) =>
              [name.toLowerCase(), value])));
        }
      }
      if (opts.traceChallenge && message.method === 'Network.loadingFailed') {
        const item = challengeRequests.get(message.params.requestId);
        if (item) item.error = message.params.errorText;
      }
    });
    await pageCdp.send('Network.enable');
  }

  let routeControl = null;
  if (!replay) {
    await navigate(pageCdp, 'https://ipv6.one/');
    const routeText = await evaluate(pageCdp,
      '(document.body && document.body.innerText || "").trim()');
    try { routeControl = JSON.parse(routeText); }
    catch { routeControl = { ip: routeText }; }
    // Keep the IP control from becoming the referrer or opener state for the
    // site under test. A fresh direct product navigation starts at about:blank.
    await navigate(pageCdp, 'about:blank');
  }
  const exitIp = replay ? 'local-replay' : String(routeControl?.ip || '');
  if (proxyAuthEnabled) {
    await pageCdp.send('Fetch.disable');
    if (proxyAuthError) throw proxyAuthError;
  }
  console.log('exit ip:', exitIp);
  if (routeControl) {
    console.log('route control:', JSON.stringify({
      ip: routeControl.ip,
      asn: routeControl.asn,
      organization: routeControl.asOrganization,
      country: routeControl.country,
      city: routeControl.city,
      httpProtocol: routeControl.httpProtocol,
      tlsVersion: routeControl.tlsVersion,
      botScore: routeControl.botManagement?.score,
    }));
  }
  console.log('automation tells:');
  const webdriver = await evaluate(pageCdp, 'String(navigator.webdriver)');
  console.log('   navigator.webdriver =', webdriver);

  let homeDone = false;
  let homeError;
  const homeNavigation = navigate(pageCdp, HOME).then(
    () => { homeDone = true; },
    error => { homeDone = true; homeError = error; },
  );
  if (opts.traceChallenge) {
    const states = [];
    const deadline = Date.now() + 10000;
    while (!homeDone && Date.now() < deadline) {
      let state;
      try {
        state = JSON.parse(await evaluate(pageCdp, `JSON.stringify({
          status: document.querySelector('#run-status')?.textContent || '',
          failed: Boolean(document.querySelector('#reload-button')),
        })`));
      } catch {
        await sleep(20);
        continue;
      }
      const label = state.status === '\u29d7' ? 'running'
        : state.status === '\u2714' ? 'vm-pass'
        : state.status === '\u2716' ? 'vm-error'
        : state.failed ? 'server-fail-page'
        : 'none';
      if (states.at(-1) !== label) states.push(label);
      if (state.failed) break;
      await sleep(20);
    }
    console.log('challenge states:', states.join(' -> '));
  }
  await homeNavigation;
  if (homeError) throw homeError;
  let challengeFetchCalls = [];
  if (opts.traceChallenge) {
    try {
      challengeFetchCalls = JSON.parse(await evaluate(
        pageCdp, 'JSON.stringify(globalThis.__abChallengeFetchCalls || [])'));
    } catch { /* a cross-document transition can discard the page trace */ }
    console.log('challenge network:', JSON.stringify([...challengeRequests.values()]));
    console.log('challenge fetch calls:', JSON.stringify(challengeFetchCalls));
  }
  let links = [];
  for (let second = 1; second <= opts.wait; second++) {
    await sleep(1000);
    links = await evaluate(pageCdp,
      `JSON.stringify([...document.querySelectorAll(${JSON.stringify(site.link)})].map(a => a.href))`);
    links = JSON.parse(links || '[]');
    if (links.length >= 3) break;
  }
  const unique = [...new Set(links.filter(productId))];
  console.log(`home: ${unique.length} product links`);
  if (opts.hold > 0) {
    console.log(`holding accepted page for ${opts.hold}s`);
    await sleep(opts.hold * 1000);
  }
  if (opts.url) {
    const expectedId = productId(HOME) || '';
    const directState = await evaluate(pageCdp, `JSON.stringify((() => {
      const root = document.querySelector('#appReactRoot');
      return {
        url: location.href,
        title: document.title,
        hasExpectedId: (document.body?.innerText || '').includes(${JSON.stringify(expectedId)}),
        rootChildren: root?.childNodes.length ?? null,
        rootHtmlLength: root?.innerHTML.length ?? null,
        text: (document.body?.innerText || '').replace(/\\s+/g, ' ').slice(0, 180),
      };
    })())`);
    console.log('direct state:', directState);
  }
  if (opts.traceChallenge) {
    try {
      const finalFetchCalls = JSON.parse(await evaluate(
        pageCdp, 'JSON.stringify(globalThis.__abChallengeFetchCalls || [])'));
      console.log('final traced fetch calls:', JSON.stringify(finalFetchCalls));
      const finalPageErrors = JSON.parse(await evaluate(
        pageCdp, 'JSON.stringify(globalThis.__abPageErrors || [])'));
      console.log('final page errors:', JSON.stringify(finalPageErrors));
    } catch { /* page may have moved while the final state was read */ }
  }
  if (opts.traceNetwork) {
    const safeNames = [
      'accept', 'accept-encoding', 'accept-language', 'priority',
      'sec-ch-ua', 'sec-ch-ua-mobile', 'sec-ch-ua-platform',
      'sec-fetch-dest', 'sec-fetch-mode', 'sec-fetch-site', 'sec-fetch-user',
      'upgrade-insecure-requests', 'user-agent',
    ];
    const headers = documentRequest?.headers || {};
    const safeHeaders = Object.fromEntries(safeNames.flatMap(name => {
      const entry = Object.entries(headers).find(([key]) => key.toLowerCase() === name);
      return entry ? [[name, entry[1]]] : [];
    }));
    console.log('document trace:', JSON.stringify({
      url: documentRequest?.url,
      method: documentRequest?.method,
      headers: safeHeaders,
      status: documentResponse?.status,
      protocol: documentResponse?.protocol,
    }));
    console.log('page error reports:', JSON.stringify(pageErrorReports));
    console.log('watched network:', JSON.stringify([...watchedNetwork.values()]));
    console.log('app runtime:', await evaluate(pageCdp, `JSON.stringify((() => {
      const root = document.querySelector('#appReactRoot');
      const globals = Object.keys(globalThis)
        .filter(key => /webpack|product|remote/i.test(key))
        .sort()
        .slice(0, 100);
      return {
        readyState: document.readyState,
        root: root && {
          childNodes: root.childNodes.length,
          htmlLength: root.innerHTML.length,
          ownKeys: Object.getOwnPropertyNames(root)
            .filter(key => /react/i.test(key))
            .slice(0, 20),
        },
        globals: Object.fromEntries(globals.map(key => {
          const value = globalThis[key];
          return [key, Array.isArray(value)
            ? {kind: 'array', length: value.length,
              chunks: value.flatMap(entry => Array.isArray(entry?.[0])
                ? entry[0].map(String) : []).slice(0, 200)}
            : {kind: typeof value, keys: value && typeof value === 'object'
              ? Object.keys(value).slice(0, 20) : []}];
        })),
        productScripts: [...document.scripts]
          .filter(script => script.src.includes('/vol2/product/dist/'))
          .map(script => ({src: script.src.split('?')[0], async: script.async,
            connected: script.isConnected}))
          .slice(0, 40),
      };
    })())`));
  }
  if (opts.probeWbStartup) {
    console.log('wb startup errors:', await evaluate(pageCdp, `JSON.stringify((() => {
      const root = document.querySelector('#appReactRoot');
      return {
        trace: globalThis.__abWbSpaTrace || null,
        wrapped: Boolean(globalThis.wb?.spa?.init?.__abStartupTrace),
        initType: typeof globalThis.initWbSpa,
        spaType: typeof globalThis.wb?.spa,
        rootChildren: root?.childNodes.length ?? null,
      };
    })())`));
  }
  if (dumpDir) {
    mkdirSync(dumpDir, { recursive: true });
    const capture = JSON.parse(await evaluate(pageCdp, `JSON.stringify({
      url: location.href,
      title: document.title,
      html: document.documentElement?.outerHTML || '',
      text: (document.body?.innerText || '').replace(/\\s+/g, ' ').slice(0, 1000),
      scripts: [...document.scripts].map((script, index) => ({
        index,
        src: script.src || '',
        type: script.type || '',
        async: script.async,
        defer: script.defer,
        integrity: script.integrity || '',
        inlineText: script.src ? '' : script.textContent || '',
      })),
    })`));
    const stem = `${opts.engine}-${opts.site}-home`;
    writeFileSync(join(dumpDir, `${stem}.html`), capture.html, 'utf8');
    writeFileSync(join(dumpDir, `${stem}-scripts.json`),
      JSON.stringify(capture.scripts, null, 2), 'utf8');
    const resourceDir = join(dumpDir, `${stem}-resources`);
    mkdirSync(resourceDir, { recursive: true });
    const resourceManifest = [];
    for (const [requestId, response] of diagnosticScripts) {
      const item = {
        url: response.url,
        status: response.status,
        protocol: response.protocol,
      };
      try {
        const body = await pageCdp.send('Network.getResponseBody', { requestId });
        const bytes = body.base64Encoded
          ? Buffer.from(body.body, 'base64')
          : Buffer.from(body.body, 'utf8');
        item.file = `resource-${String(resourceManifest.length).padStart(2, '0')}.js`;
        item.bytes = bytes.length;
        writeFileSync(join(resourceDir, item.file), bytes);
      } catch (error) {
        item.error = String(error);
      }
      resourceManifest.push(item);
    }
    writeFileSync(join(resourceDir, 'manifest.json'),
      JSON.stringify(resourceManifest, null, 2), 'utf8');
    let responseBody;
    let responseBodyError;
    if (documentResponseRequestId) {
      try {
        const body = await pageCdp.send('Network.getResponseBody', {
          requestId: documentResponseRequestId,
        });
        responseBody = body.base64Encoded
          ? Buffer.from(body.body, 'base64')
          : Buffer.from(body.body, 'utf8');
        writeFileSync(join(dumpDir, `${stem}-response.bin`), responseBody);
      } catch (error) {
        responseBodyError = String(error);
      }
    }
    writeFileSync(join(dumpDir, `${stem}-summary.json`), JSON.stringify({
      engine: opts.engine,
      site: opts.site,
      exitIp,
      routeControl,
      webdriver,
      url: capture.url,
      title: capture.title,
      text: capture.text,
      productLinks: unique.length,
      status: documentResponse?.status,
      protocol: documentResponse?.protocol,
      responseBytes: responseBody?.length,
      responseBodyError,
      challengeNetwork: [...challengeRequests.values()],
      challengeFetchCalls,
      pageErrorReports,
      watchedNetwork: [...watchedNetwork.values()],
    }, null, 2), 'utf8');
    console.log('capture dir:', dumpDir);
  }
  if (!unique.length) {
    console.log('empty state:', await evaluate(pageCdp,
      'JSON.stringify({title:document.title,text:(document.body?.innerText||"").replace(/\\s+/g," ").slice(0,180)})'));
  }

  const picked = [];
  while (picked.length < Math.min(opts.cards, unique.length)) {
    const candidate = unique[Math.floor(Math.random() * unique.length)];
    if (!picked.includes(candidate)) picked.push(candidate);
  }

  let opened = 0;
  for (const url of picked) {
    const id = productId(url);
    await sleep(1500 + Math.random() * 2000);
    try {
      await navigate(pageCdp, url);
      let at = null;
      for (let second = 1; second <= opts.wait; second++) {
        await sleep(1000);
        const found = await evaluate(pageCdp,
          `(document.body ? document.body.innerText : '').includes(${JSON.stringify(id)})`);
        if (found) { at = second; break; }
      }
      const length = await evaluate(pageCdp,
        "(document.body ? document.body.innerText.replace(/\\s+/g, ' ') : '').length");
      if (at !== null) opened += 1;
      console.log(`card ${id}: ${at !== null ? `opened after ${at}s` : 'NEVER rendered'} (${length} chars)`);
    } catch (error) {
      console.log(`card ${id}: FAILED ${String(error).slice(0, 140)}`);
    }
  }
  console.log(`${opened}/${picked.length} cards opened`);
  if (replay) {
    console.log('replay submissions:', JSON.stringify(replay.submissions));
    if (opts.traceReplayHelpers) {
      console.log('replay helper traces:', JSON.stringify(replay.helperTraces));
    }
    if (dumpDir) {
      writeFileSync(join(dumpDir, `${opts.engine}-${opts.site}-replay-submissions.json`),
        JSON.stringify(replay.submissions, null, 2), 'utf8');
      if (opts.traceReplayHelpers) {
        writeFileSync(join(dumpDir, `${opts.engine}-${opts.site}-replay-helper-traces.json`),
          JSON.stringify(replay.helperTraces, null, 2), 'utf8');
      }
    }
  }
} finally {
  // Ask the engine to shut itself down before the scoped process backstop.
  try { await cdp?.send('Browser.close', {}, 3000); } catch { /* already gone */ }
  try { cdp?.close(); } catch { /* already closed */ }
  await sleep(500);
  chrome?.kill();
  // Backstop for launcher stubs. Match the exact engine executable and this
  // run's unique profile or CDP port, never another browser process.
  if (process.platform === 'win32') {
    try {
      const psQuote = value => `'${String(value).replaceAll("'", "''")}'`;
      const needle = profileDir ? profileDir.split(/[\\/]/).pop() : `--port ${port}`;
      execFileSync('powershell.exe', ['-NoProfile', '-Command',
        `$executable = ${psQuote(resolve(executable))}; ` +
        'Get-CimInstance Win32_Process | Where-Object { ' +
        `$_.ExecutablePath -eq $executable -and $_.CommandLine -like ${psQuote(`*${needle}*`)} } | ` +
        'ForEach-Object { Stop-Process -Id $_.ProcessId -Force }',
      ], { stdio: 'ignore' });
    } catch { /* best effort */ }
  }
  if (profileDir) {
    try { rmSync(profileDir, { recursive: true, force: true }); } catch { /* best effort */ }
  }
  if (cleanHostDir) {
    try { rmSync(cleanHostDir, { recursive: true, force: true }); } catch { /* best effort */ }
  }
  if (engineLogFd !== null) {
    try { closeSync(engineLogFd); } catch { /* best effort */ }
  }
  await replay?.close();
}
