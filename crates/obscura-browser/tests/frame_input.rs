use std::io::{Read, Write};
use std::sync::Arc;

use obscura_browser::{BrowserContext, Page, WaitUntil};

fn spawn_page() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { continue };
            std::thread::spawn(move || {
                let mut request = [0u8; 4096];
                let _ = stream.read(&mut request);
                let path = String::from_utf8_lossy(&request)
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .to_string();
                let body = if path.starts_with("/frame") {
                    "<!doctype html><button id=check>Check</button><script>\
                     document.getElementById('check').addEventListener('click', function () {\
                       parent.postMessage('child-clicked', '*');\
                     });</script>"
                } else {
                    "<!doctype html><iframe src=/frame></iframe>"
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\
                     Connection: close\r\n\r\n{}",
                    body.len(), body,
                );
                let _ = stream.write_all(response.as_bytes());
            });
        }
    });
    format!("http://{address}/")
}

#[tokio::test(flavor = "current_thread")]
async fn a_mouse_click_enters_the_child_frame() {
    unsafe { std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1") }
    let context = Arc::new(BrowserContext::new("frame-input".to_string()));
    let mut page = Page::new("frame-input-page".to_string(), context);
    page.add_preload_script(
        "globalThis.__clicked = false; \
         addEventListener('message', function (event) { \
           if (event.data === 'child-clicked') globalThis.__clicked = true; \
         });",
    );
    page.navigate_with_wait(&spawn_page(), WaitUntil::Load).await.unwrap();
    for _ in 0..20 {
        page.settle(100).await;
        if !page.frame_urls().is_empty() { break; }
    }

    let frame_state = page.evaluate_in_frame(0,
        "(function(){ var b=document.getElementById('check'), r=b.getBoundingClientRect(); return {\
         frameId:globalThis.__obscura_frameId, x:r.x, y:r.y, width:r.width, height:r.height,\
         at30:(document.elementFromPoint(30,30)||{}).tagName, preload:globalThis.__clicked}; })()",
    );
    assert_eq!(frame_state["frameId"], serde_json::json!(1), "{frame_state}");
    assert_eq!(frame_state["preload"], serde_json::json!(false), "{frame_state}");

    let rect = page.evaluate(
        "(function(){ var r=document.querySelector('iframe').getBoundingClientRect(); \
         return {x:r.x,y:r.y,width:r.width,height:r.height}; })()",
    );
    let x = rect["x"].as_f64().unwrap() + rect["width"].as_f64().unwrap() / 2.0;
    let y = rect["y"].as_f64().unwrap() + rect["height"].as_f64().unwrap() / 2.0;
    assert!(page.dispatch_mouse_event("mouseMoved", x, y, 1));
    assert!(page.dispatch_mouse_event("mousePressed", x, y, 1));
    assert!(page.dispatch_mouse_event("mouseReleased", x, y, 1));
    page.settle(100).await;

    assert_eq!(page.evaluate("globalThis.__clicked"), serde_json::json!(true));
}
