//! Every way a bundler inserts a script has to reach the network.
//!
//! On a real SPA product page Chrome issues ~190 requests and Obscura issues 9:
//! the entry bundles load, and then nothing the app asks for afterwards does.
//! This pins down which insertion shapes actually fetch, using a local fixture
//! so the answer does not depend on a live site.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use obscura::Browser;

/// Serves the page plus one chunk per pattern. Returns the base URL and the
/// paths that were requested.
fn spawn_server() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let recorder = seen.clone();

    std::thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { continue };
            let recorder = recorder.clone();
            std::thread::spawn(move || {
                let mut request = [0u8; 4096];
                let _ = stream.read(&mut request);
                let request = String::from_utf8_lossy(&request).to_string();
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                recorder.lock().unwrap().push(path.clone());

                let (content_type, body) = if path.starts_with("/chunk-") {
                    let name = path.trim_start_matches('/').trim_end_matches(".js");
                    (
                        "application/javascript",
                        format!("globalThis.__ran = (globalThis.__ran || []); globalThis.__ran.push('{name}');"),
                    )
                } else {
                    ("text/html", PAGE.to_string())
                };

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\n\
                     Connection: close\r\n\r\n{}",
                    content_type,
                    body.len(),
                    body,
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.shutdown(std::net::Shutdown::Both);
            });
        }
    });

    (format!("http://{}", addr), seen)
}

/// One inline script that inserts chunks the way real bundlers do.
const PAGE: &str = r#"<!doctype html><html><body><div id="root"></div>
<script>
globalThis.__loaded = [];
function track(name, el) {
  el.addEventListener('load', function() { globalThis.__loaded.push(name); });
  el.addEventListener('error', function() { globalThis.__loaded.push(name + ':error'); });
}

// 1. webpack's own shape: build, set src, then append to head.
var a = document.createElement('script');
a.charset = 'utf-8';
a.timeout = 120;
a.src = '/chunk-head-append.js';
track('head-append', a);
document.head.appendChild(a);

// 2. src set AFTER the element is already in the tree.
var b = document.createElement('script');
document.head.appendChild(b);
track('src-after-append', b);
b.src = '/chunk-src-after-append.js';

// 3. inserted with insertBefore, which is what several loaders use.
var c = document.createElement('script');
c.src = '/chunk-insert-before.js';
track('insert-before', c);
document.head.insertBefore(c, document.head.firstChild);

// 4. appended to body rather than head.
var d = document.createElement('script');
d.src = '/chunk-body-append.js';
track('body-append', d);
document.body.appendChild(d);

// 5. async + crossOrigin, the modern bundler default.
var e = document.createElement('script');
e.async = true;
e.crossOrigin = 'anonymous';
e.setAttribute('src', '/chunk-async-crossorigin.js');
track('async-crossorigin', e);
document.head.appendChild(e);

// 6. after the load event, the way a route change loads a chunk.
window.addEventListener('load', function() {
  setTimeout(function() {
    var f = document.createElement('script');
    f.src = '/chunk-after-load.js';
    track('after-load', f);
    document.head.appendChild(f);
  }, 50);
});
</script>
</body></html>"#;

const PATTERNS: &[&str] = &[
    "head-append",
    "src-after-append",
    "insert-before",
    "body-append",
    "async-crossorigin",
    "after-load",
];

#[tokio::test(flavor = "current_thread")]
async fn every_bundler_script_insertion_reaches_the_network() {
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    let (base, seen) = spawn_server();

    let browser = Browser::new().unwrap();
    let mut page = browser.new_page().await.unwrap();
    page.goto(&base).await.unwrap();
    for _ in 0..20 {
        page.settle(250).await;
        let requested = seen.lock().unwrap().len();
        if requested >= PATTERNS.len() + 1 {
            break;
        }
    }

    let seen = seen.lock().unwrap().clone();
    let ran = page.evaluate("JSON.stringify(globalThis.__ran || [])");
    let loaded = page.evaluate("JSON.stringify(globalThis.__loaded || [])");

    let missing: Vec<&str> = PATTERNS
        .iter()
        .copied()
        .filter(|pattern| !seen.iter().any(|path| path.contains(pattern)))
        .collect();
    assert!(
        missing.is_empty(),
        "these insertions never reached the network: {missing:?}\n  requested: {seen:?}\n  ran: {ran}\n  load events: {loaded}"
    );

    // Fetching is not enough: the chunk has to execute, or the loader's promise
    // never settles and the app stalls with no error, which is what a real SPA
    // does when this breaks.
    let ran = ran.as_str().unwrap_or_default().to_string();
    let not_run: Vec<&str> = PATTERNS
        .iter()
        .copied()
        .filter(|pattern| !ran.contains(&format!("chunk-{pattern}")))
        .collect();
    assert!(
        not_run.is_empty(),
        "these chunks were fetched but never ran: {not_run:?}\n  ran: {ran}\n  load events: {loaded}"
    );
}

/// A click that arrives from nowhere is a tell. Chrome precedes it with a move
/// onto the element, and every event in the sequence carries coordinates that
/// agree with the target's own rect.
#[tokio::test(flavor = "current_thread")]
async fn a_click_arrives_with_the_events_a_real_one_would() {
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    let browser = Browser::new().unwrap();
    let mut page = browser.new_page().await.unwrap();
    page.goto("about:blank").await.unwrap();
    page.evaluate(
        r#"(function() {
            document.body.innerHTML = '<button id="b">go</button>';
            globalThis.__seen = [];
            const b = document.getElementById('b');
            for (const type of ['pointerover','pointerenter','mouseover','mouseenter',
                                'pointermove','mousemove','pointerdown','mousedown',
                                'pointerup','mouseup','click']) {
                b.addEventListener(type, e => globalThis.__seen.push(
                    type + ':' + e.clientX + ',' + e.clientY));
            }
            b.click();
        })()"#,
    );

    let seen = page.evaluate("JSON.stringify(globalThis.__seen)");
    let seen: Vec<String> = serde_json::from_str(seen.as_str().unwrap()).unwrap();
    let order: Vec<&str> = seen.iter().map(|e| e.split(':').next().unwrap()).collect();
    assert_eq!(
        order,
        vec![
            "pointerover", "pointerenter", "mouseover", "mouseenter",
            "pointermove", "mousemove", "pointerdown", "mousedown",
            // the pixel of drift a hand adds between press and release
            "pointermove", "mousemove",
            "pointerup", "mouseup", "click",
        ],
        "click sequence does not match Chrome's: {seen:?}"
    );

    // Every event has to report the same point, and it has to be the middle of
    // the element — a click whose coordinates contradict the target's rect is
    // as good a signal as no coordinates at all.
    let centre = page.evaluate(
        "(function() { const r = document.getElementById('b').getBoundingClientRect();
          return Math.round(r.left + r.width / 2) + ',' + Math.round(r.top + r.height / 2); })()",
    );
    let centre = centre.as_str().unwrap();
    let (cx, cy) = centre.split_once(',').unwrap();
    let (cx, cy): (i64, i64) = (cx.parse().unwrap(), cy.parse().unwrap());
    for (index, event) in seen.iter().enumerate() {
        let (x, y) = event.split_once(':').unwrap().1.split_once(',').unwrap();
        let (x, y): (i64, i64) = (x.parse().unwrap(), y.parse().unwrap());
        // Everything up to mousedown is on the centre; from the drift onward it
        // is within a pixel of it, which is the point of the drift.
        let slack = if index <= 7 { 0 } else { 1 };
        assert!(
            (x - cx).abs() <= slack && (y - cy).abs() <= slack,
            "{event} is more than {slack}px from the centre {centre}"
        );
    }
}

/// The invariant every click depends on: an element is found at its own centre.
#[tokio::test(flavor = "current_thread")]
async fn every_element_is_found_at_its_own_centre() {
    let browser = Browser::new().unwrap();
    let mut page = browser.new_page().await.unwrap();
    page.goto("about:blank").await.unwrap();
    let report = page.evaluate(
        r#"(function() {
            const parts = [];
            for (let i = 0; i < 400; i++) parts.push('<a href="/p' + i + '" id="a' + i + '">card ' + i + '</a>');
            document.body.innerHTML = parts.join('');
            const bad = [];
            for (let i = 0; i < 400; i++) {
                const el = document.getElementById('a' + i);
                el.scrollIntoView();
                const r = el.getBoundingClientRect();
                const hit = document.elementFromPoint(
                    Math.round(r.left + r.width / 2), Math.round(r.top + r.height / 2));
                if (hit !== el) {
                    bad.push('a' + i + ' -> ' + (hit ? (hit.id || hit.tagName) : 'null'));
                }
            }
            return JSON.stringify({ bad: bad.length, first: bad.slice(0, 5) });
        })()"#,
    );
    let report: serde_json::Value =
        serde_json::from_str(report.as_str().expect("report")).unwrap();
    assert_eq!(
        report["bad"], serde_json::json!(0),
        "elements not found at their own centre: {report}"
    );
}
