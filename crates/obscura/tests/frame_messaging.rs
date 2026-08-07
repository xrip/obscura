//! A frame must be able to talk to the page that holds it.
//!
//! Every embedded widget that reports a result — a payment form, a consent
//! dialog, a captcha — does it the same way: the frame computes something and
//! `parent.postMessage`s it out, because a cross-origin frame cannot touch the
//! parent's DOM. Without that route the frame runs perfectly and its answer
//! goes nowhere, which looks exactly like the frame never working at all.
//!
//! The other half is the framing check. `window.parent === window` is how a
//! document decides it is top-level; a frame that answers "yes" is telling every
//! widget in it that it is not embedded, and they take different paths on that.

use std::io::{Read, Write};

use obscura::Browser;

/// Serves a page holding one iframe, plus the frame document. The frame reports
/// what it sees back to the page over postMessage; the page records it where a
/// test can read it.
fn spawn_server(frame_body: &'static str) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    std::thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { continue };
            std::thread::spawn(move || {
                let mut request = [0u8; 4096];
                let _ = stream.read(&mut request);
                let request = String::from_utf8_lossy(&request).to_string();
                let path = request.split_whitespace().nth(1).unwrap_or("/");

                let body = if path.starts_with("/frame") {
                    format!("<!doctype html><html><body>{frame_body}</body></html>")
                } else {
                    "<!doctype html><html><body><iframe src=\"/frame\"></iframe>\
                     <script>window.fromFrame = [];\
                     addEventListener('message', e => window.fromFrame.push(e));\
                     </script></body></html>"
                        .to_string()
                };

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

/// Loads `base`, settles until `probe` returns something other than null, and
/// returns that. Frames arrive over the network, so nothing here is immediate.
async fn settle_for(page: &mut obscura::Page, probe: &str) -> serde_json::Value {
    for _ in 0..40 {
        page.settle(250).await;
        let value = page.evaluate(probe);
        if !value.is_null() {
            return value;
        }
    }
    serde_json::Value::Null
}

/// The capability itself: a frame's message reaches the page.
#[tokio::test(flavor = "current_thread")]
async fn a_frame_can_post_a_message_to_the_page_that_holds_it() {
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    let base = spawn_server(
        "<script>parent.postMessage({ token: 'from-the-frame' }, '*');</script>",
    );

    let browser = Browser::new().unwrap();
    let mut page = browser.new_page().await.unwrap();
    page.goto(&base).await.unwrap();

    let seen = settle_for(
        &mut page,
        "window.fromFrame && window.fromFrame.length \
         ? window.fromFrame[0].data.token : null",
    )
    .await;

    assert_eq!(
        seen,
        serde_json::json!("from-the-frame"),
        "the frame's postMessage never reached the page"
    );
}

/// The message must arrive as a real MessageEvent, not just with the right
/// payload. Listeners route on `origin` and reply through `source`, and a widget
/// that cannot tell which frame spoke will refuse the message.
#[tokio::test(flavor = "current_thread")]
async fn the_page_learns_which_frame_spoke_and_from_where() {
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    let base = spawn_server("<script>parent.postMessage('hello', '*');</script>");

    let browser = Browser::new().unwrap();
    let mut page = browser.new_page().await.unwrap();
    page.goto(&base).await.unwrap();

    let seen = settle_for(
        &mut page,
        "window.fromFrame && window.fromFrame.length ? (() => { \
           const e = window.fromFrame[0]; \
           return { data: e.data, origin: e.origin, type: e.type, \
                    hasSource: !!e.source, \
                    isMessageEvent: e instanceof MessageEvent }; \
         })() : null",
    )
    .await;

    assert_eq!(seen["data"], serde_json::json!("hello"));
    assert_eq!(seen["type"], serde_json::json!("message"));
    assert_eq!(
        seen["isMessageEvent"],
        serde_json::json!(true),
        "the page got something that was not a MessageEvent"
    );
    // The frame is served from the same host and port as the page here, so its
    // origin is the page's own.
    assert_eq!(
        seen["origin"].as_str().unwrap_or_default(),
        base.as_str(),
        "the page cannot tell where the message came from"
    );
    assert_eq!(
        seen["hasSource"],
        serde_json::json!(true),
        "the page has no handle to reply through"
    );
}

/// A framed document must know it is framed. This is one comparison, and
/// widgets branch on it before doing anything else.
#[tokio::test(flavor = "current_thread")]
async fn a_framed_document_does_not_claim_to_be_the_top_document() {
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    let base = spawn_server(
        "<script>parent.postMessage({ \
           isTop: parent === window, topIsSelf: top === window, \
           hasParentPost: typeof parent.postMessage }, '*');</script>",
    );

    let browser = Browser::new().unwrap();
    let mut page = browser.new_page().await.unwrap();
    page.goto(&base).await.unwrap();

    let seen = settle_for(
        &mut page,
        "window.fromFrame && window.fromFrame.length ? window.fromFrame[0].data : null",
    )
    .await;

    assert_eq!(
        seen["isTop"],
        serde_json::json!(false),
        "the frame thinks it is the top document, so nothing in it will behave as embedded"
    );
    assert_eq!(seen["topIsSelf"], serde_json::json!(false));
    assert_eq!(seen["hasParentPost"], serde_json::json!("function"));
    // The page itself is genuinely top-level, and must still say so.
    assert_eq!(
        page.evaluate("parent === window && top === window"),
        serde_json::json!(true),
        "the page stopped being its own top document"
    );
}

/// `contentWindow` must be the same object before and after the frame loads.
///
/// An embedder takes `contentWindow` the moment it creates the iframe and later
/// compares it against `event.source` to decide whether a message really came
/// from its own frame. Handing out a fresh object on load makes that comparison
/// fail, and nothing reports an error — the embedder just quietly ignores its
/// own frame, which is indistinguishable from the frame never speaking.
#[tokio::test(flavor = "current_thread")]
async fn a_frames_window_is_the_same_object_before_and_after_it_loads() {
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    let base = spawn_server("<script>parent.postMessage({ hello: true }, '*');</script>");

    let browser = Browser::new().unwrap();
    let mut page = browser.new_page().await.unwrap();
    // Grab the window as soon as the element exists, the way an embedder does,
    // and keep it to compare against later.
    page.add_preload_script(
        "globalThis.__early = null; \
         addEventListener('message', e => { globalThis.__source = e.source; }); \
         const grab = () => { \
           const frame = document.querySelector('iframe'); \
           if (frame) { globalThis.__early = frame.contentWindow; } else { setTimeout(grab, 5); } \
         }; \
         setTimeout(grab, 0);",
    );
    page.goto(&base).await.unwrap();

    let verdict = settle_for(
        &mut page,
        "globalThis.__source ? ({ \
           sameAsEarly: globalThis.__early === globalThis.__source, \
           sameAsNow: document.querySelector('iframe').contentWindow === globalThis.__source, \
           grabbedEarly: !!globalThis.__early \
         }) : null",
    )
    .await;

    assert_eq!(
        verdict["grabbedEarly"],
        serde_json::json!(true),
        "the test never captured contentWindow before the load, so it proves nothing"
    );
    assert_eq!(
        verdict["sameAsEarly"],
        serde_json::json!(true),
        "contentWindow was replaced when the frame loaded, so a reference taken \
         beforehand no longer matches event.source"
    );
    assert_eq!(
        verdict["sameAsNow"],
        serde_json::json!(true),
        "event.source is not the frame's current contentWindow"
    );
}

/// A delivered message must be trusted, and a page-built one must not be.
///
/// `isTrusted` says the user agent produced the event rather than script. A
/// postMessage arrives that way, and real embedders check: Cloudflare's
/// Turnstile drops every message from its own frame unless the flag is set, so
/// an untrusted one is not merely suspicious, it is discarded in silence and
/// the widget waits forever. Answering `true` for everything would be just as
/// wrong — a trivial bot tell — so the negative half is checked too.
#[tokio::test(flavor = "current_thread")]
async fn a_delivered_message_is_trusted_and_a_forged_one_is_not() {
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    let base = spawn_server("<script>parent.postMessage({ real: true }, '*');</script>");

    let browser = Browser::new().unwrap();
    let mut page = browser.new_page().await.unwrap();
    page.goto(&base).await.unwrap();

    let trusted = settle_for(
        &mut page,
        "(window.fromFrame || []).some(e => e.data && e.data.real) \
         ? (window.fromFrame.find(e => e.data && e.data.real).isTrusted ? 'yes' : 'no') \
         : null",
    )
    .await;
    assert_eq!(
        trusted,
        serde_json::json!("yes"),
        "a message delivered from a frame was not trusted, so an embedder that \
         checks isTrusted will drop it"
    );

    // The same event type, built by page script, must still report false.
    assert_eq!(
        page.evaluate("new MessageEvent('message', { data: 1 }).isTrusted"),
        serde_json::json!(false),
        "a script-built event claims to be trusted, which is a bot tell"
    );
}

/// The reverse direction, as a full round trip. A parent configures a widget by
/// posting into it, so a one-way bridge only solves half the problem.
///
/// Driven as a handshake rather than by posting from the test: a frame is loaded
/// over the network, so posting at a fixed moment either races the frame's
/// arrival or waits on a guess. Waiting for the frame to speak first is both
/// reliable and what widgets actually do — and it makes the page reply through
/// `event.source`, which is the only handle it has to a cross-origin frame.
#[tokio::test(flavor = "current_thread")]
async fn a_page_and_a_frame_can_hold_a_conversation() {
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    let base = spawn_server(
        "<script>\
           addEventListener('message', e => { \
             if (e.data && e.data.ping) parent.postMessage({ echo: e.data.ping }, '*'); \
           }); \
           parent.postMessage({ ready: true }, '*');\
         </script>",
    );

    let browser = Browser::new().unwrap();
    let mut page = browser.new_page().await.unwrap();
    // The page answers the frame's greeting by posting back through the source
    // it was handed.
    page.add_preload_script(
        "addEventListener('message', e => { \
           if (e.data && e.data.ready && e.source) e.source.postMessage({ ping: 'hi' }, '*'); \
         });",
    );
    page.goto(&base).await.unwrap();

    let echoed = settle_for(
        &mut page,
        "(window.fromFrame || []).map(e => e.data && e.data.echo).find(Boolean) || null",
    )
    .await;
    assert_eq!(
        echoed,
        serde_json::json!("hi"),
        "the page's reply never reached the frame, or the frame's echo never came back"
    );
}
