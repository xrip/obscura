// Managed Turnstile click check over raw CDP. Runtime.enable is never sent.

import { spawn } from 'node:child_process';
import net from 'node:net';
import { join, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..', '..');
const port = await new Promise((done, fail) => {
  const server = net.createServer();
  server.once('error', fail);
  server.listen(0, '127.0.0.1', () => {
    const { port } = server.address();
    server.close(() => done(port));
  });
});
const env = {
  ...process.env,
  OBSCURA_NAV_TIMEOUT_MS: '90000',
  OBSCURA_SCRIPT_DEADLINE_MS: '90000',
};
for (const name of ['OBSCURA_PROXY', 'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY',
                    'http_proxy', 'https_proxy', 'all_proxy']) {
  delete env[name];
  delete process.env[name];
}
const child = spawn(join(root, 'target', 'release', 'obscura.exe'),
  ['--stealth', 'serve', '--port', String(port)],
  { cwd: root, env, stdio: ['ignore', 'inherit', 'inherit'], windowsHide: true });
child.on('exit', (code, signal) => console.error(`Obscura exited code=${code} signal=${signal}`));
const sleep = ms => new Promise(done => setTimeout(done, ms));
const initialWaitSeconds = Number(process.env.OBSCURA_RAW_INITIAL_WAIT_SECONDS || 0);
const maxSeconds = Number(process.env.OBSCURA_RAW_MAX_SECONDS || 60);

async function targetWebSocket() {
  const deadline = Date.now() + 30000;
  for (;;) {
    try {
      const version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
      if (version.webSocketDebuggerUrl) return version.webSocketDebuggerUrl;
    } catch { /* server is still starting */ }
    if (Date.now() > deadline) throw new Error('Obscura did not expose a page target');
    await sleep(200);
  }
}

function connect(url) {
  const socket = new WebSocket(url);
  const pending = new Map();
  let nextId = 0;
  socket.addEventListener('message', event => {
    const message = JSON.parse(event.data);
    const done = pending.get(message.id);
    if (done) { pending.delete(message.id); done(message); }
  });
  socket.addEventListener('close', () => {
    for (const [id, done] of pending) {
      pending.delete(id);
      done({ error: { message: 'CDP connection closed' } });
    }
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
  return { ready, send, close: () => socket.close() };
}

async function evaluate(cdp, expression) {
  const out = await cdp.send('Runtime.evaluate', { expression, returnByValue: true });
  return out?.result?.value;
}

const report = `JSON.stringify((() => {
  const host = document.getElementById('tf') || document.querySelector('.cf-turnstile');
  const rect = host && host.getBoundingClientRect();
  const widget = document.querySelector('.cf-turnstile');
  const internalShadow = widget && widget._shadowRoot;
  const frame = internalShadow && internalShadow.querySelector('iframe') ||
    host && host.querySelector('iframe') || document.querySelector('iframe[src*="challenges.cloudflare.com"]');
  const frameRect = frame && frame.getBoundingClientRect();
  const input = document.querySelector('input[name="cf-turnstile-response"]');
  return { token: input && input.value || '', tokenInput: !!input,
    host: rect && { x:rect.x,y:rect.y,width:rect.width,height:rect.height },
    frame: frameRect && { x:frameRect.x,y:frameRect.y,width:frameRect.width,height:frameRect.height },
    frames: Array.from(document.querySelectorAll('iframe')).map(f => ({
      src:f.getAttribute('src'), id:f.id, frameId:f._frameId,
      rect:(r=>({x:r.x,y:r.y,width:r.width,height:r.height}))(f.getBoundingClientRect())
    })),
    hostHtml: host && host.innerHTML.slice(0,500),
    shadowHtml: internalShadow && internalShadow.innerHTML.slice(0,500) };
})())`;

let cdp;
try {
  cdp = connect(await targetWebSocket());
  await cdp.ready;
  console.log('CDP connected');
  const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' });
  console.log(`target created ${targetId}`);
  const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true });
  console.log(`target attached ${sessionId}`);
  const pageSend = (method, params = {}) => cdp.send(method, params, sessionId);
  void pageSend('Page.navigate', { url: 'https://turnstile-test.vercel.app/' })
    .catch(error => console.error('navigation:', error.message));

  if (initialWaitSeconds > 0) {
    console.log(`waiting ${initialWaitSeconds}s before the first Runtime.evaluate`);
    await sleep(initialWaitSeconds * 1000);
  }

  let state;
  let lastClickSecond = 0;
  for (let second = 1; second <= maxSeconds; second++) {
    await sleep(1000);
    const out = await pageSend('Runtime.evaluate', { expression: report, returnByValue: true });
    state = JSON.parse(out?.result?.value || '{}');
    if (second === 5) console.log(`state at 5s ${JSON.stringify(state)}`);
    const elapsed = initialWaitSeconds + second;
    const clickRect = state.frame || state.host;
    if (clickRect && elapsed >= 5 && elapsed - lastClickSecond >= 5) {
      const x = clickRect.x + clickRect.width / 2;
      const y = clickRect.y + clickRect.height / 2;
      await pageSend('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y });
      await pageSend('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', clickCount: 1 });
      await sleep(100);
      await pageSend('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', clickCount: 1 });
      console.log(`clicked page (${x},${y}), frame=${JSON.stringify(state.frame)}, host=${JSON.stringify(state.host)}`);
      lastClickSecond = elapsed;
    }
    if (state.token) { console.log(`PASS token after ${second}s`); break; }
    if (second % 10 === 0) console.log(`${initialWaitSeconds + second}s token=empty`);
  }
  if (!state?.token) console.log('FAIL', JSON.stringify(state));
} finally {
  if (cdp) {
    try {
      await Promise.race([cdp.send('Browser.close'), sleep(2000)]);
    } catch { /* best effort */ }
    cdp.close();
  }
  child.kill();
}
