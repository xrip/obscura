//! A frame's own scripts must run, in the frame's own realm.
//!
//! Before frame realms existed the page fetched a frame's HTML and dropped it
//! into a detached shim document, so nothing inside a frame ever executed. The
//! proof used here is a beacon: only a script that actually ran could have sent
//! it, and only a script whose document is the frame's could have read the
//! frame's own DOM to build it.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use obscura::Browser;

/// Serves a page holding one iframe, the frame document, and the frame's
/// external script. Returns the base URL and the list of paths requested.
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

                let (content_type, body) = if path.starts_with("/frame.js") {
                    (
                        "application/javascript",
                        "fetch('/beacon?from=external&tag=' + \
                         document.querySelector('#tag').textContent);"
                            .to_string(),
                    )
                } else if path.starts_with("/frame") {
                    (
                        "text/html",
                        "<!doctype html><html><body><span id=\"tag\">child</span>\
                         <script>fetch('/beacon?from=inline&url=' + \
                         encodeURIComponent(location.pathname));</script>\
                         <script src=\"/frame.js\"></script>\
                         </body></html>"
                            .to_string(),
                    )
                } else if path.starts_with("/beacon") {
                    ("text/plain", "ok".to_string())
                } else {
                    (
                        "text/html",
                        "<!doctype html><html><body><iframe src=\"/frame\"></iframe>\
                         </body></html>"
                            .to_string(),
                    )
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

#[tokio::test(flavor = "current_thread")]
async fn a_frames_own_scripts_run_against_the_frames_own_document() {
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    let (base, seen) = spawn_server();

    let browser = Browser::new().unwrap();
    let mut page = browser.new_page().await.unwrap();
    page.goto(&base).await.unwrap();
    for _ in 0..20 {
        page.settle(250).await;
        if seen.lock().unwrap().iter().any(|path| path.contains("from=external")) {
            break;
        }
    }

    let seen = seen.lock().unwrap().clone();
    assert!(
        seen.iter().any(|path| path.starts_with("/frame") && !path.starts_with("/frame.js")),
        "the frame document was never fetched: {seen:?}"
    );
    // The inline script ran, and `location` inside it was the frame's, not the
    // page's — the page is served from "/".
    assert!(
        seen.iter().any(|path| path.contains("from=inline") && path.contains("%2Fframe")),
        "the frame's inline script did not run against the frame's location: {seen:?}"
    );
    // The external script was resolved against the frame's URL, fetched, and run
    // against the frame's DOM.
    assert!(
        seen.iter().any(|path| path.contains("from=external") && path.contains("tag=child")),
        "the frame's external script did not run against the frame's DOM: {seen:?}"
    );
}
