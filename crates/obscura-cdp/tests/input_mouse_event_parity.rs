#![cfg(feature = "render")]

use obscura_cdp::dispatch::{dispatch, CdpContext};
use obscura_cdp::types::CdpRequest;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn serve_fixture() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 2048];
            let read = socket.read(&mut buf).await.unwrap();
            let request = String::from_utf8_lossy(&buf[..read]);
            let body = if request.starts_with("GET /next ") {
                "<!doctype html><html><body id=next-page>next document</body></html>"
            } else {
                r#"<!doctype html><html><head><style>
            html, body { margin: 0; }
            #page { width: 1800px; height: 2400px; }
            #box { position: absolute; left: 20px; top: 20px; width: 180px;
                   height: 120px; overflow: auto; border: 10px solid black; }
            #inner { width: 700px; height: 800px; }
        </style></head><body>
          <div id="page"></div>
          <div id="box"><div id="inner"></div></div>
          <input id="check" type="checkbox">
          <form id="radio-form">
            <input id="radio-a" type="radio" name="choice" checked>
            <input id="radio-b" type="radio" name="choice">
          </form>
        </body></html>"#
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });
    format!("http://{addr}/")
}

async fn serve_iframe_click_fixture() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0u8; 2048];
            let read = socket.read(&mut buf).await.unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..read]);
            let body = if request.starts_with("GET /frame.html ") {
                r#"<!doctype html><html><body style="margin:0">
                    <script>
                        globalThis.clicks = 0;
                        const root = document.body.attachShadow({ mode: 'closed' });
                        root.innerHTML = '<input id="inside" type="checkbox" style="position:absolute;left:10px;top:10px;width:40px;height:40px">';
                        const inside = root.querySelector('#inside');
                        inside.addEventListener('click', () => {
                            globalThis.clicks++;
                            globalThis.checked = inside.checked;
                        });
                    </script>
                </body></html>"#
            } else if request.starts_with("GET /outer.html ") {
                r#"<!doctype html><html><body style="margin:0">
                    <iframe id="nested" src="/frame.html" style="position:absolute;left:20px;top:20px;width:180px;height:120px;border:0"></iframe>
                </body></html>"#
            } else {
                r#"<!doctype html><html><body style="margin:0">
                    <iframe id="child" src="/outer.html" style="position:absolute;left:20px;top:20px;width:180px;height:120px;border:0"></iframe>
                </body></html>"#
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });
    format!("http://{addr}/")
}

async fn cdp(
    ctx: &mut CdpContext,
    id: u64,
    method: &str,
    params: Value,
    session_id: &str,
) -> Value {
    let response = dispatch(
        &CdpRequest {
            id,
            method: method.to_string(),
            params,
            session_id: Some(session_id.to_string()),
        },
        ctx,
    )
    .await;
    assert!(response.error.is_none(), "CDP {method} failed: {:?}", response.error);
    response.result.unwrap_or_else(|| json!({}))
}

async fn evaluate(ctx: &mut CdpContext, id: u64, expression: &str, session_id: &str) -> Value {
    cdp(
        ctx,
        id,
        "Runtime.evaluate",
        json!({"expression": expression, "returnByValue": true, "awaitPromise": true}),
        session_id,
    )
    .await
}

async fn setup() -> (CdpContext, String) {
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    let url = serve_fixture().await;
    let mut ctx = CdpContext::new();
    let page_id = ctx.create_page();
    let session_id = "input-mouse-session";
    ctx.sessions.insert(session_id.to_string(), page_id);
    cdp(
        &mut ctx,
        1,
        "Page.navigate",
        json!({"url": url, "waitUntil": "load"}),
        session_id,
    )
    .await;
    (ctx, session_id.to_string())
}

async fn wheel(ctx: &mut CdpContext, id: u64, sid: &str, x: f64, y: f64, dx: f64, dy: f64) {
    cdp(
        ctx,
        id,
        "Input.dispatchMouseEvent",
        json!({"type": "mouseWheel", "x": x, "y": y, "deltaX": dx, "deltaY": dy}),
        sid,
    )
    .await;
}

async fn click(ctx: &mut CdpContext, id: u64, sid: &str, x: f64, y: f64) {
    cdp(
        ctx,
        id,
        "Input.dispatchMouseEvent",
        json!({"type": "mousePressed", "x": x, "y": y, "button": "left"}),
        sid,
    )
    .await;
    cdp(
        ctx,
        id + 1,
        "Input.dispatchMouseEvent",
        json!({"type": "mouseReleased", "x": x, "y": y, "button": "left"}),
        sid,
    )
    .await;
}

async fn scroll_state(ctx: &mut CdpContext, id: u64, sid: &str) -> Value {
    let result = evaluate(
        ctx,
        id,
        r#"JSON.stringify({
            rootX: scrollX, rootY: scrollY,
            boxX: document.getElementById('box').scrollLeft,
            boxY: document.getElementById('box').scrollTop,
            rootScrollWidth: document.scrollingElement.scrollWidth,
            rootClientWidth: document.scrollingElement.clientWidth,
            pageRect: document.getElementById('page').getBoundingClientRect().toJSON(),
            maxBoxX: document.getElementById('box').scrollWidth - document.getElementById('box').clientWidth,
            maxBoxY: document.getElementById('box').scrollHeight - document.getElementById('box').clientHeight
        })"#,
        sid,
    )
    .await;
    serde_json::from_str(result["result"]["value"].as_str().unwrap()).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn wheel_over_page_scrolls_the_root_on_both_axes() {
    let (mut ctx, sid) = setup().await;
    wheel(&mut ctx, 2, &sid, 600.0, 300.0, 45.0, 160.0).await;
    let state = scroll_state(&mut ctx, 3, &sid).await;
    assert_eq!(state["rootX"], 45.0, "unexpected root geometry: {state}");
    assert_eq!(state["rootY"], 160.0);
    assert_eq!(state["boxX"], 0.0);
    assert_eq!(state["boxY"], 0.0);
}

#[tokio::test(flavor = "current_thread")]
async fn wheel_over_nested_overflow_scrolls_the_nested_container() {
    let (mut ctx, sid) = setup().await;
    wheel(&mut ctx, 2, &sid, 50.0, 50.0, 70.0, 110.0).await;
    let state = scroll_state(&mut ctx, 3, &sid).await;
    assert_eq!(state["boxX"], 70.0);
    assert_eq!(state["boxY"], 110.0);
    assert_eq!(state["rootX"], 0.0, "nested wheel must not leak to the viewport");
    assert_eq!(state["rootY"], 0.0, "nested wheel must not leak to the viewport");
}

#[tokio::test(flavor = "current_thread")]
async fn wheel_offsets_clamp_to_nested_scroll_extents() {
    let (mut ctx, sid) = setup().await;
    wheel(&mut ctx, 2, &sid, 50.0, 50.0, 100_000.0, 100_000.0).await;
    let state = scroll_state(&mut ctx, 3, &sid).await;
    assert_eq!(state["boxX"], state["maxBoxX"]);
    assert_eq!(state["boxY"], state["maxBoxY"]);

    wheel(&mut ctx, 4, &sid, 50.0, 50.0, -100_000.0, -100_000.0).await;
    let state = scroll_state(&mut ctx, 5, &sid).await;
    assert_eq!(state["boxX"], 0.0);
    assert_eq!(state["boxY"], 0.0);
}

#[tokio::test(flavor = "current_thread")]
async fn wheel_chains_to_root_when_nested_scroller_is_saturated() {
    let (mut ctx, sid) = setup().await;
    evaluate(
        &mut ctx,
        2,
        "(() => { const box = document.getElementById('box'); box.scrollTop = box.scrollHeight; })()",
        &sid,
    )
    .await;
    let saturated = scroll_state(&mut ctx, 3, &sid).await;
    assert_eq!(saturated["boxY"], saturated["maxBoxY"]);

    wheel(&mut ctx, 4, &sid, 50.0, 50.0, 0.0, 90.0).await;
    let state = scroll_state(&mut ctx, 5, &sid).await;
    assert_eq!(state["boxY"], state["maxBoxY"], "inner remains clamped");
    assert_eq!(state["rootY"], 90.0, "remaining wheel gesture chains to the viewport");
}

#[tokio::test(flavor = "current_thread")]
async fn canceling_wheel_prevents_its_scroll_default() {
    let (mut ctx, sid) = setup().await;
    evaluate(
        &mut ctx,
        2,
        r#"(() => {
            globalThis.wheelProbe = null;
            const page = document.getElementById('page');
            document.elementFromPoint = () => page;
            page.addEventListener('wheel', event => {
                wheelProbe = {
                    x: event.clientX, y: event.clientY,
                    dx: event.deltaX, dy: event.deltaY,
                    ctrl: event.ctrlKey, trusted: event.isTrusted
                };
                event.preventDefault();
            });
        })()"#,
        &sid,
    )
    .await;
    cdp(
        &mut ctx,
        3,
        "Input.dispatchMouseEvent",
        json!({
            "type": "mouseWheel", "x": 600.0, "y": 300.0,
            "deltaX": 25.0, "deltaY": 75.0, "modifiers": 2
        }),
        &sid,
    )
    .await;
    let state = scroll_state(&mut ctx, 4, &sid).await;
    assert_eq!(state["rootX"], 0.0);
    assert_eq!(state["rootY"], 0.0);
    let probe = evaluate(&mut ctx, 5, "JSON.stringify(wheelProbe)", &sid).await;
    let probe: Value = serde_json::from_str(probe["result"]["value"].as_str().unwrap()).unwrap();
    assert_eq!(probe["x"], 600.0);
    assert_eq!(probe["y"], 300.0);
    assert_eq!(probe["dx"], 25.0);
    assert_eq!(probe["dy"], 75.0);
    assert_eq!(probe["ctrl"], true);
    assert_eq!(probe["trusted"], true);
}

#[tokio::test(flavor = "current_thread")]
async fn hit_testing_clips_scrolled_children_at_overflow_padding_edge() {
    let (mut ctx, sid) = setup().await;
    let result = evaluate(
        &mut ctx,
        2,
        r#"(() => {
            const box = document.getElementById('box');
            box.scrollLeft = 50;
            const inner = document.getElementById('inner').getBoundingClientRect();
            return JSON.stringify({
                hit: document.elementFromPoint(25, 50).id,
                innerLeft: inner.left, innerRight: inner.right,
                boxLeft: box.getBoundingClientRect().left
            });
        })()"#,
        &sid,
    )
    .await;
    let result: Value = serde_json::from_str(result["result"]["value"].as_str().unwrap()).unwrap();
    assert!(result["innerLeft"].as_f64().unwrap() <= 25.0);
    assert!(result["innerRight"].as_f64().unwrap() >= 25.0);
    assert_eq!(result["boxLeft"], 20.0);
    assert_eq!(result["hit"], "box", "content hidden behind the border cannot win hit testing");
}

#[tokio::test(flavor = "current_thread")]
async fn content_quad_centers_hit_their_dense_wrapped_inline_owners() {
    let (mut ctx, sid) = setup().await;
    evaluate(
        &mut ctx,
        2,
        r#"(() => {
            document.body.innerHTML = '<style>html,body,p{margin:0}p{width:800px;font:16px/20px monospace}</style><p>' +
                Array.from({length:400}, (_, i) => '<a id="a' + i + '">a' + i + '</a>').join(' ') +
                '</p>';
        })()"#,
        &sid,
    )
    .await;

    for (request_id, id) in [(3, "a0"), (6, "a1"), (9, "a399")] {
        let query = cdp(
            &mut ctx,
            request_id,
            "DOM.querySelector",
            json!({"nodeId": 0, "selector": format!("#{id}")}),
            &sid,
        )
        .await;
        let node_id = query["nodeId"].as_u64().expect("inline owner nodeId");
        let quads = cdp(
            &mut ctx,
            request_id + 1,
            "DOM.getContentQuads",
            json!({"nodeId": node_id}),
            &sid,
        )
        .await;
        let quad: Vec<f64> = quads["quads"][0]
            .as_array()
            .expect("one content quad")
            .iter()
            .map(|value| value.as_f64().expect("numeric quad coordinate"))
            .collect();

        let page = evaluate(
            &mut ctx,
            request_id + 2,
            &format!(
                "(() => {{ const el = document.getElementById('{id}'); const r = el.getBoundingClientRect(); const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2); return JSON.stringify({{rect:[r.left,r.top,r.right,r.bottom],hit:hit && hit.id}}); }})()"
            ),
            &sid,
        )
        .await;
        let page: Value =
            serde_json::from_str(page["result"]["value"].as_str().unwrap()).unwrap();
        let rect = page["rect"].as_array().unwrap();
        let left = rect[0].as_f64().unwrap();
        let top = rect[1].as_f64().unwrap();
        let right = rect[2].as_f64().unwrap();
        let bottom = rect[3].as_f64().unwrap();
        let expected = [left, top, right, top, right, bottom, left, bottom];

        assert_eq!(quad.len(), expected.len());
        for (actual, expected) in quad.iter().zip(expected) {
            assert!((actual - expected).abs() < 0.01, "{id} quad differs from its page rect");
        }
        assert_eq!(page["hit"], id, "{id} content-quad center hit another owner");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn click_document_navigation_clears_and_recreates_execution_contexts() {
    let (mut ctx, sid) = setup().await;
    evaluate(
        &mut ctx,
        2,
        r#"(() => {
            const link = document.createElement('a');
            link.id = 'next';
            link.href = '/next';
            link.textContent = 'next';
            document.body.appendChild(link);
            document.elementFromPoint = () => link;
        })()"#,
        &sid,
    )
    .await;
    ctx.pending_events.clear();
    ctx.valid_context_ids.insert(999);

    click(&mut ctx, 3, &sid, 10.0, 10.0).await;

    let methods: Vec<&str> = ctx.pending_events.iter().map(|event| event.method.as_str()).collect();
    let cleared = methods
        .iter()
        .position(|method| *method == "Runtime.executionContextsCleared")
        .expect("document click navigation must clear old contexts");
    let navigated = methods
        .iter()
        .position(|method| *method == "Page.frameNavigated")
        .expect("document click navigation must report the new frame");
    let created = methods
        .iter()
        .position(|method| *method == "Runtime.executionContextCreated")
        .expect("document click navigation must create the new default context");
    assert!(cleared < navigated && navigated < created, "wrong context event order: {methods:?}");
    assert!(!ctx.valid_context_ids.contains(&999), "stale click-navigation context survived");
    assert!(ctx.valid_context_ids.contains(&2), "new default context was not registered");

    let page = ctx.get_session_page_mut(&Some(sid.clone())).unwrap();
    assert_eq!(page.url.as_ref().unwrap().path(), "/next");
    assert!(page.evaluate("document.body.id") == "next-page");
}

#[tokio::test(flavor = "current_thread")]
async fn click_push_state_navigation_keeps_the_live_execution_context() {
    let (mut ctx, sid) = setup().await;
    evaluate(
        &mut ctx,
        2,
        r#"(() => {
            const link = document.createElement('a');
            link.id = 'spa';
            link.href = '/spa';
            link.textContent = 'spa';
            link.addEventListener('click', event => {
                event.preventDefault();
                history.pushState({}, '', '/spa');
            });
            document.body.appendChild(link);
            document.elementFromPoint = () => link;
        })()"#,
        &sid,
    )
    .await;
    ctx.pending_events.clear();
    ctx.valid_context_ids.insert(999);

    click(&mut ctx, 3, &sid, 10.0, 10.0).await;

    let methods: Vec<&str> = ctx.pending_events.iter().map(|event| event.method.as_str()).collect();
    assert!(methods.contains(&"Page.frameNavigated"), "SPA click did not report its URL: {methods:?}");
    assert!(
        !methods.contains(&"Runtime.executionContextsCleared"),
        "pushState must not clear a still-live realm: {methods:?}"
    );
    assert!(ctx.valid_context_ids.contains(&999), "pushState pruned a live execution context");
    let page = ctx.get_session_page_mut(&Some(sid.clone())).unwrap();
    assert_eq!(page.url.as_ref().unwrap().path(), "/spa");
}

#[tokio::test(flavor = "current_thread")]
async fn press_release_orders_events_and_defers_click_activation() {
    let (mut ctx, sid) = setup().await;
    evaluate(
        &mut ctx,
        2,
        r#"(() => {
            const target = document.getElementById('check');
            document.elementFromPoint = () => target;
            globalThis.mouseLog = [];
            for (const type of ['mousedown', 'mouseup', 'click', 'input', 'change']) {
                target.addEventListener(type, event => mouseLog.push({
                    type, checked: target.checked, x: event.clientX,
                    ctrl: event.ctrlKey, shift: event.shiftKey, trusted: event.isTrusted
                }));
            }
        })()"#,
        &sid,
    )
    .await;

    cdp(
        &mut ctx,
        3,
        "Input.dispatchMouseEvent",
        json!({
            "type": "mousePressed", "x": 31.0, "y": 42.0,
            "button": "left", "clickCount": 1, "modifiers": 10
        }),
        &sid,
    )
    .await;
    let pressed = evaluate(
        &mut ctx,
        4,
        "JSON.stringify({log: mouseLog, checked: document.getElementById('check').checked})",
        &sid,
    )
    .await;
    let pressed: Value = serde_json::from_str(pressed["result"]["value"].as_str().unwrap()).unwrap();
    assert_eq!(pressed["checked"], false, "checkbox activation must wait for release");
    assert_eq!(pressed["log"][0]["type"], "mousedown");
    assert_eq!(pressed["log"].as_array().unwrap().len(), 1, "press must not synthesize click");

    cdp(
        &mut ctx,
        5,
        "Input.dispatchMouseEvent",
        json!({
            "type": "mouseReleased", "x": 31.0, "y": 42.0,
            "button": "left", "clickCount": 1, "modifiers": 10
        }),
        &sid,
    )
    .await;
    let released = evaluate(
        &mut ctx,
        6,
        "JSON.stringify({log: mouseLog, checked: document.getElementById('check').checked})",
        &sid,
    )
    .await;
    let released: Value = serde_json::from_str(released["result"]["value"].as_str().unwrap()).unwrap();
    let types: Vec<&str> = released["log"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["mousedown", "mouseup", "click", "input", "change"]);
    assert_eq!(released["checked"], true);
    assert_eq!(released["log"][2]["checked"], true, "click sees checkbox pre-activation");
    assert_eq!(released["log"][2]["x"], 31.0);
    assert_eq!(released["log"][2]["ctrl"], true);
    assert_eq!(released["log"][2]["shift"], true);
    assert_eq!(released["log"][2]["trusted"], true);
}

#[tokio::test(flavor = "current_thread")]
async fn page_mouse_click_enters_a_child_frame() {
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    let url = serve_iframe_click_fixture().await;
    let mut ctx = CdpContext::new();
    let page_id = ctx.create_page();
    let sid = "iframe-mouse-session";
    ctx.sessions.insert(sid.to_string(), page_id);
    cdp(
        &mut ctx,
        1,
        "Page.navigate",
        json!({"url": url, "waitUntil": "load"}),
        sid,
    )
    .await;
    // Frame attachment is progressed by the next page commands.
    evaluate(&mut ctx, 2, "1", sid).await;
    evaluate(&mut ctx, 3, "1", sid).await;

    click(&mut ctx, 4, sid, 70.0, 70.0).await;

    let result = evaluate(
        &mut ctx,
        6,
        r#"(() => {
            const iframe = document.getElementById('child');
            const outer = globalThis.__obscura_frameObjects[iframe._frameId];
            const nested = outer.document.getElementById('nested');
            const realm = outer.window.__obscura_frameObjects[nested._frameId];
            return JSON.stringify({
                frameId: iframe._frameId,
                nestedFrameId: nested._frameId,
                clicks: realm.window.clicks,
                closedShadowChecked: realm.window.checked
            });
        })()"#,
        sid,
    )
    .await;
    let result: Value = serde_json::from_str(result["result"]["value"].as_str().unwrap()).unwrap();
    assert!(result["frameId"].as_u64().unwrap_or(0) > 0, "child frame was not attached");
    assert!(result["nestedFrameId"].as_u64().unwrap_or(0) > 0, "nested frame was not attached");
    assert_eq!(result["clicks"], 1, "nested child click listener did not run exactly once");
    assert_eq!(result["closedShadowChecked"], true, "page click did not activate closed-shadow control");
}

#[tokio::test(flavor = "current_thread")]
async fn radio_release_selects_only_the_target_in_its_group() {
    let (mut ctx, sid) = setup().await;
    evaluate(
        &mut ctx,
        2,
        r#"(() => {
            const a = document.getElementById('radio-a');
            const b = document.getElementById('radio-b');
            document.elementFromPoint = () => b;
            globalThis.radioEvents = [];
            for (const radio of [a, b]) {
                for (const type of ['mousedown', 'mouseup', 'click', 'input', 'change']) {
                    radio.addEventListener(type, () => radioEvents.push(radio.id + ':' + type));
                }
            }
        })()"#,
        &sid,
    )
    .await;
    cdp(
        &mut ctx,
        3,
        "Input.dispatchMouseEvent",
        json!({"type": "mousePressed", "x": 10.0, "y": 10.0, "button": "left"}),
        &sid,
    )
    .await;
    cdp(
        &mut ctx,
        4,
        "Input.dispatchMouseEvent",
        json!({"type": "mouseReleased", "x": 10.0, "y": 10.0, "button": "left"}),
        &sid,
    )
    .await;
    let result = evaluate(
        &mut ctx,
        5,
        "JSON.stringify({a: document.getElementById('radio-a').checked, b: document.getElementById('radio-b').checked, events: radioEvents})",
        &sid,
    )
    .await;
    let result: Value = serde_json::from_str(result["result"]["value"].as_str().unwrap()).unwrap();
    assert_eq!(result["a"], false);
    assert_eq!(result["b"], true);
    assert_eq!(
        result["events"],
        json!(["radio-b:mousedown", "radio-b:mouseup", "radio-b:click", "radio-b:input", "radio-b:change"]),
        "the newly selected radio alone receives activation events"
    );
}
