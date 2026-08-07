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
         });",
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
        let heard = page.evaluate("JSON.stringify(globalThis.__seen || [])");
        println!("  page heard: {}", heard.as_str().unwrap_or("[]"));
        if token.as_str().map(|t| !t.is_empty()).unwrap_or(false) {
            println!("\nPASS: token issued");
            return;
        }
    }
    println!("\nFAIL: no token");
}
