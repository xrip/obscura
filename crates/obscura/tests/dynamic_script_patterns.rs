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
