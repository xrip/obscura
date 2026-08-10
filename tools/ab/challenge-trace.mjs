// A/B-only challenge trace. Values and token bodies are reduced to shapes and
// lengths so a run cannot print challenge tokens or cookies. Navigation is
// kept in Node; the page trace is per-document and never calls back into CDP.

const traces = new WeakMap();

function challengeTraceInit() {
  const trace = { vmfp: [], token: [], fetches: [] };
  Object.defineProperty(globalThis, '__abChallengeTrace', {
    value: trace, writable: false, enumerable: false, configurable: true,
  });
  const shape = value => {
    if (value === null) return 'null';
    if (Array.isArray(value)) return `array(${value.length})`;
    if (typeof value === 'object') {
      return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, shape(item)]));
    }
    if (typeof value === 'string') return `string(${value.length})`;
    return typeof value;
  };

  const originalFetch = globalThis.fetch;
  if (typeof originalFetch === 'function') {
    globalThis.fetch = function (input, init) {
      const requestUrl = String(typeof input === 'string' ? input : input?.url || input);
      const method = String(init?.method || 'GET');
      if (/__wbaas|create-token|challenge/i.test(requestUrl)) {
        trace.fetches.push({ url: requestUrl.split('?')[0].slice(0, 160), method: method.slice(0, 16) });
      }
      const result = originalFetch.call(this, input, init);
      if (!requestUrl.includes('/api/v1/create-token')) return result;
      const body = typeof init?.body === 'string' ? init.body : '';
      let requestShape = null;
      try { requestShape = shape(JSON.parse(body)); } catch {}
      return result.then(async response => {
        let responseLength = 0;
        let responseShape = null;
        try {
          const text = await response.clone().text();
          responseLength = text.length;
          responseShape = shape(JSON.parse(text));
        } catch {}
        trace.token.push({
          status: response.status,
          requestLength: body.length,
          requestShape,
          responseLength,
          responseShape,
        });
        return response;
      });
    };
  }

  let vmfp;
  const wrapVmfp = value => {
    if (!value || typeof value.run !== 'function' || value.run.__abWrapped) return;
    const originalRun = value.run;
    const wrapped = function (...args) {
      const event = { phase: 'call', args: shape(args), result: null, error: null };
      try {
        const result = originalRun.apply(this, args);
        event.result = shape(result);
        trace.vmfp.push(event);
        if (result && typeof result.then === 'function') {
          result.then(
            value => trace.vmfp.push({ phase: 'resolve', result: shape(value), error: null }),
            error => trace.vmfp.push({ phase: 'reject', result: null, error: String(error).slice(0, 160) }),
          );
        }
        return result;
      } catch (error) {
        event.error = String(error).slice(0, 160);
        trace.vmfp.push(event);
        throw error;
      }
    };
    Object.defineProperty(wrapped, '__abWrapped', { value: true });
    value.run = wrapped;
  };
  Object.defineProperty(globalThis, '__vmfp', {
    get() { return vmfp; },
    set(value) { vmfp = value; wrapVmfp(value); },
    enumerable: false,
    configurable: true,
  });
}

export async function installChallengeTrace(page) {
  const trace = { documents: [] };
  traces.set(page, trace);
  page.on('framenavigated', frame => {
    if (frame === page.mainFrame()) {
      trace.documents.push({ url: frame.url().split('?')[0].slice(0, 160) });
    }
  });
  await page.addInitScript(challengeTraceInit);
}

export async function readChallengeTrace(page, tryEvaluate, evaluated) {
  const aggregate = traces.get(page) || { documents: [] };
  const local = evaluated(await tryEvaluate(page, () => globalThis.__abChallengeTrace)) || {};
  return {
    documents: aggregate.documents,
    vmfp: local.vmfp || [],
    token: local.token || [],
    fetches: local.fetches || [],
  };
}
