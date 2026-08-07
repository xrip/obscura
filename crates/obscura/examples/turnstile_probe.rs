//! What the Turnstile challenge frame does inside Obscura.
//!
//!   cargo run -p obscura --example turnstile_probe --features stealth
//!
//! Serves a page carrying a Turnstile widget with Cloudflare's dummy sitekey —
//! the one that issues a token without scoring the visitor — then reports what
//! the page and the challenge frame each ended up with. The dummy key is what
//! makes this a test of the engine rather than of Cloudflare's opinion of this
//! machine, which is a distinction a live site never lets you draw.

use std::io::{Read, Write};

use obscura::Browser;

const DUMMY_SITEKEY: &str = "1x00000000000000000000AA";

fn spawn_server() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let body = format!(
        "<!doctype html><html><head>\
         <script src=\"https://challenges.cloudflare.com/turnstile/v0/api.js\" async defer></script>\
         </head><body><form><div class=\"cf-turnstile\" data-sitekey=\"{DUMMY_SITEKEY}\"></div>\
         </form></body></html>"
    );

    std::thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { continue };
            let body = body.clone();
            std::thread::spawn(move || {
                let mut request = [0u8; 4096];
                let _ = stream.read(&mut request);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\
                     Connection: close\r\n\r\n{}",
                    body.len(),
                    body,
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.shutdown(std::net::Shutdown::Both);
            });
        }
    });
    format!("http://{}", addr)
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    let base = spawn_server();
    println!("fixture at {base}");

    let browser = Browser::new().unwrap();
    let mut page = browser.new_page().await.unwrap();
    // Record what the page hears, so a token that arrives and is then dropped
    // can be told apart from one that never arrives.
    page.add_preload_script(
        "globalThis.__seen = []; \
         addEventListener('message', e => { \
           try { __seen.push({ origin: e.origin, data: JSON.stringify(e.data).slice(0, 200) }); } \
           catch (_) { __seen.push({ origin: e.origin, data: '<unserialisable>' }); } \
         }); \
         globalThis.__roots = []; \
         const _attach = Element.prototype.attachShadow; \
         Element.prototype.attachShadow = function (init) { \
           const r = _attach.call(this, init); globalThis.__roots.push(r); return r; \
         };",
    );
    page.goto(&base).await.unwrap();

    for round in 1..=8 {
        page.settle(2000).await;
        let token = page.evaluate(
            "(document.querySelector('input[name=\"cf-turnstile-response\"]') || {}).value || ''",
        );
        let frames = page.frame_urls();
        println!(
            "\n--- round {round}: {} frame(s), token={:?}",
            frames.len(),
            token.as_str().unwrap_or("")
        );
        for (index, url) in frames.iter().enumerate() {
            println!("  frame {index}: {}", &url[..url.len().min(110)]);
            for (label, expression) in [
                ("body chars", "document.body ? document.body.innerHTML.length : -1"),
                ("head chars", "document.head ? document.head.innerHTML.length : -1"),
                ("doc chars", "document.documentElement ? document.documentElement.innerHTML.length : -1"),
                ("readyState", "document.readyState"),
                ("scripts", "document.querySelectorAll('script').length"),
                ("is framed", "parent !== window"),
                ("errors", "(globalThis.__obscura_errors || []).length"),
                ("first error", "JSON.stringify((globalThis.__obscura_errors || [])[0] || null)"),
                ("rejections", "JSON.stringify((globalThis.__probeRejections || []).slice(0, 3))"),
                ("child frames", "document.querySelectorAll('iframe').length"),
                ("WebAssembly", "typeof WebAssembly"),
                ("wasm instantiate", "typeof (WebAssembly || {}).instantiate"),
                ("timers pending", "typeof globalThis.__obscura_timerCount === 'function' ? globalThis.__obscura_timerCount() : 'n/a'"),
                // What OBSCURA_FRAME_PRELOAD recorded, if it was set.
                ("trace", "JSON.stringify((globalThis.__trace || []).slice(0, 45))"),
                // Turnstile's own capability gate, reproduced. If any of these
                // fails the frame declares the browser unsupported and stops
                // before building anything — silently, from outside.
                ("cf gate", "JSON.stringify((() => { \
                    const out = {}; \
                    try { \
                      const url = URL.createObjectURL(new Blob([''], { type: 'text/javascript' })); \
                      const w = new Worker(url); \
                      URL.revokeObjectURL(url); \
                      w.terminate(); \
                      out.workerBlob = 'ok'; \
                    } catch (e) { out.workerBlob = 'THREW ' + e.message; } \
                    out.pipeTo = typeof (globalThis.ReadableStream || {}).prototype === 'object' \
                      && ReadableStream.prototype.pipeTo !== undefined; \
                    out.BigInt = !!globalThis.BigInt; \
                    out.getRandomValues = !!(globalThis.crypto && crypto.getRandomValues); \
                    out.getEntries = !!(globalThis.performance) \
                      && typeof performance.getEntries === 'function'; \
                    out.PerformanceObserver = typeof globalThis.PerformanceObserver === 'function'; \
                    out.notTop = globalThis.top !== globalThis.self && !!globalThis.parent; \
                    return out; \
                  })())"),
                // The frame's own server-sent config, and the two slots the
                // UI-build path fills in. DkJB9 holds the closed shadow root it
                // renders into, so its absence says the build never ran.
                ("cf state", "JSON.stringify((() => { \
                    const o = globalThis._cf_chl_opt; \
                    if (!o) return 'no _cf_chl_opt'; \
                    return { keys: Object.keys(o).length, \
                             vYBZG: typeof o.vYBZG, \
                             shadowRoot: typeof o.DkJB9, \
                             ThJsN9: Array.isArray(o.ThJsN9) ? o.ThJsN9.length : typeof o.ThJsN9, \
                             mhJdn5: o.mhJdn5, hJgZ2: o.hJgZ2, \
                             widgetId: o.dFcg0 }; \
                  })())"),
                // Inside the closed shadow root: this is where the checkbox
                // actually goes, so it is the only honest measure of whether
                // the widget rendered.
                ("shadow content", "JSON.stringify((() => { \
                    const r = (globalThis._cf_chl_opt || {}).DkJB9; \
                    if (!r) return 'no shadow root'; \
                    try { \
                      return { children: r.childNodes ? r.childNodes.length : 'n/a', \
                               html: (r.innerHTML || '').length, \
                               sample: (r.innerHTML || '').slice(0, 100) }; \
                    } catch (e) { return 'threw: ' + e.message; } \
                  })())"),
                ("page globals", "JSON.stringify(Object.keys(globalThis).filter(k => !k.startsWith('__') && k.length <= 12).slice(0, 40))"),
            ] {
                let value = page.evaluate_in_frame(index, expression);
                println!("      {label:<13}: {value}");
            }
        }
        // The challenge document itself, for reading offline. It is the only
        // copy that exists: the URL is single use and tied to this session.
        if let Ok(path) = std::env::var("TURNSTILE_DUMP") {
            if !frames.is_empty() {
                let html = page.evaluate_in_frame(0, "document.documentElement.outerHTML");
                if let Some(html) = html.as_str() {
                    let _ = std::fs::write(&path, html);
                    println!("  dumped {} bytes of frame document to {path}", html.len());
                }
            }
        }
        // The page side of the same question. api.js looks its widget up with
        // `shadowRoot.querySelector('#'+id)` and gives up if it is not there —
        // and that lookup is what acknowledges the frame's `init`, which is
        // what starts the heartbeat the overrun watchdog is waiting on.
        let roots = page.evaluate(
            "JSON.stringify((globalThis.__roots || []).map(r => ({ \
               children: r.childNodes ? r.childNodes.length : 'n/a', \
               html: (r.innerHTML || '').length, \
               anyElement: !!r.querySelector('*'), \
               byId: [...(r.querySelectorAll ? r.querySelectorAll('[id]') : [])] \
                 .map(e => e.id).slice(0, 5), \
               foundById: [...(r.querySelectorAll ? r.querySelectorAll('[id]') : [])] \
                 .every(e => r.querySelector('#' + e.id) === e) \
             })))",
        );
        println!("  page shadow roots: {}", roots.as_str().unwrap_or("[]"));
        let heard = page.evaluate("JSON.stringify(globalThis.__seen || [])");
        println!("  page heard: {}", heard.as_str().unwrap_or("[]"));
        if token.as_str().map(|t| !t.is_empty()).unwrap_or(false) {
            println!("\nPASS: token issued");
            return;
        }
    }
    println!("\nFAIL: no token");
}
