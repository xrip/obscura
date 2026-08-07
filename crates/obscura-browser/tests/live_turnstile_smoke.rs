//! Cloudflare Turnstile issues a token, end to end, against the real service.
//!
//! This is the regression guard for child frames as a whole. A Turnstile widget
//! exercises the parts of a browsing context that nothing else in the suite
//! does: a cross-origin frame that runs its own scripts, a two-way postMessage
//! conversation with the page, `event.isTrusted` and `event.source` on the
//! messages, a stable `contentWindow` across the frame's load, and the frame's
//! own load lifecycle. Break any one of those and the token never arrives —
//! silently, because Cloudflare's script simply ignores what it does not trust.
//!
//! The always-pass sitekey is deliberate. It serves the real 265 KB challenge
//! framework from Cloudflare and runs the real message protocol, but returns a
//! fixed token instead of scoring the visitor. So this measures our engine
//! rather than Cloudflare's opinion of whatever IP it runs from — which is the
//! distinction the product smoke in this directory cannot draw, and why that
//! one flaps while this one should not. It is also why a failure here is worth
//! believing: no risk engine is involved.
//!
//! It does not prove we can solve a real proof-of-work challenge. That path
//! never runs for this sitekey, and none of it needs WebAssembly or the GPU.

#![cfg(feature = "stealth")]

use std::io::{Read, Write};
use std::sync::Arc;
use std::time::Duration;

use obscura_browser::{BrowserContext, Page, WaitUntil};

/// Cloudflare's documented "always passes" test sitekey.
const ALWAYS_PASS_SITEKEY: &str = "1x00000000000000000000AA";

/// The token Cloudflare hands back for that sitekey.
const EXPECTED_TOKEN: &str = "XXXX.DUMMY.TOKEN.XXXX";

/// Serves one page carrying a Turnstile widget, and returns its URL.
///
/// Served locally rather than from a hosted page so the test cannot start
/// failing because someone else's site changed — the failure mode that makes
/// the product smoke next door hard to trust.
fn spawn_widget_page() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <script src=\"https://challenges.cloudflare.com/turnstile/v0/api.js\" async defer>\
         </script></head><body>\
         <form><div class=\"cf-turnstile\" data-sitekey=\"{ALWAYS_PASS_SITEKEY}\"></div></form>\
         </body></html>"
    );

    std::thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { continue };
            let body = body.clone();
            std::thread::spawn(move || {
                let mut request = [0u8; 4096];
                let _ = stream.read(&mut request);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body,
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.shutdown(std::net::Shutdown::Both);
            });
        }
    });

    format!("http://{address}/")
}

/// Reads the token Turnstile writes into the form, plus the widget's
/// conversation with its frame.
///
/// The conversation is only used to describe a failure. An empty token says
/// nothing about where the flow stopped, whereas the last message received
/// names the step: no `init` means the frame never ran, `init` but no
/// `translationInit` means the page ignored the frame, and `translationInit`
/// without `complete` means the challenge itself did not finish.
const READ_STATE: &str = r#"(function () {
    var input = document.querySelector('input[name="cf-turnstile-response"]');
    return {
        token: (input && input.value) || "",
        widget: document.querySelectorAll('.cf-turnstile').length,
        api: typeof globalThis.turnstile,
        messages: (globalThis.__events || []).join(" -> ")
    };
})()"#;

#[tokio::test(flavor = "current_thread")]
async fn turnstile_issues_a_token() {
    unsafe { std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1") }
    let url = spawn_widget_page();

    let context = Arc::new(BrowserContext::with_full_options(
        "live-turnstile-smoke".to_string(),
        std::env::var("OBSCURA_PROXY").ok(),
        true,
        None,
    ));
    let mut page = Page::new("live-turnstile-smoke-page".to_string(), context);
    // Recorded before any of the page's own scripts run, so the first message
    // is captured too.
    page.add_preload_script(
        "globalThis.__events = []; \
         addEventListener('message', function (e) { \
           var event = e && e.data && e.data.event; \
           if (event && __events[__events.length - 1] !== event) __events.push(event); \
         });",
    );

    let navigation = tokio::time::timeout(
        Duration::from_secs(90),
        page.navigate_with_wait(&url, WaitUntil::Load),
    )
    .await;
    match navigation {
        Err(_) => panic!("navigation to the widget page timed out"),
        Ok(Err(error)) => panic!("navigation to the widget page failed: {error}"),
        Ok(Ok(())) => {}
    }

    // The token has arrived within a second or two in practice; the rest is
    // headroom for a slow network rather than an expected wait.
    let mut state = serde_json::Value::Null;
    for _ in 0..20 {
        page.settle(1_000).await;
        state = page.evaluate(READ_STATE);
        if !state["token"].as_str().unwrap_or_default().is_empty() {
            break;
        }
    }

    let token = state["token"].as_str().unwrap_or_default();
    assert!(
        !token.is_empty(),
        "Turnstile issued no token. The widget's conversation with its frame was \
         [{}], which says where it stopped. Full state: {state}",
        state["messages"].as_str().unwrap_or("(nothing received)"),
    );
    assert_eq!(
        token, EXPECTED_TOKEN,
        "Turnstile issued a token that is not the one this sitekey always \
         returns, so the widget did something other than pass: {state}"
    );
}
