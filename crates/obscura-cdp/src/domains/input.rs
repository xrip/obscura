use serde_json::{json, Value};

use crate::dispatch::CdpContext;

// Insert `escaped_text` at the caret, replacing any non-collapsed selection
// the way a real browser does when you type over selected text (for example
// after a triple-click select-all). selectionStart is null during ordinary
// typing, so the legacy append path is kept when no selection is tracked.
fn insert_text_js(escaped_text: &str) -> String {
    format!(
        "(function() {{\
            var t = document.activeElement;\
            if (!t || (t.localName !== 'input' && t.localName !== 'textarea')) return;\
            var v = t.value || '';\
            var s = t.selectionStart, e = t.selectionEnd;\
            if (s == null) {{\
                globalThis.__obscura_setFieldValue(t, 'value', v + '{text}');\
            }} else {{\
                s = Math.max(0, Math.min(s, v.length));\
                e = (e == null) ? s : Math.max(0, Math.min(e, v.length));\
                var lo = Math.min(s, e), hi = Math.max(s, e);\
                globalThis.__obscura_setFieldValue(t, 'value', v.slice(0, lo) + '{text}' + v.slice(hi));\
                var caret = lo + ('{text}').length;\
                t.setSelectionRange(caret, caret);\
            }}\
            t.dispatchEvent(globalThis.__obscura_markTrusted(new Event('input', {{bubbles:true}})));\
        }})()",
        text = escaped_text,
    )
}

// Backspace deletes the selected range when there is one, so the common
// "triple-click to select-all, then Backspace to clear" pattern works. With a
// collapsed caret it removes the character before the caret, and with no
// selection tracked it falls back to trimming the last character (legacy).
const BACKSPACE_JS: &str = "(function() {\
    var t = document.activeElement;\
    if (!t || (t.localName !== 'input' && t.localName !== 'textarea')) return;\
    var v = t.value || '';\
    var s = t.selectionStart, e = t.selectionEnd;\
    if (s == null) {\
        globalThis.__obscura_setFieldValue(t, 'value', v.slice(0, -1));\
    } else {\
        s = Math.max(0, Math.min(s, v.length));\
        e = (e == null) ? s : Math.max(0, Math.min(e, v.length));\
        if (s !== e) {\
            var lo = Math.min(s, e), hi = Math.max(s, e);\
            globalThis.__obscura_setFieldValue(t, 'value', v.slice(0, lo) + v.slice(hi));\
            t.setSelectionRange(lo, lo);\
        } else if (s > 0) {\
            globalThis.__obscura_setFieldValue(t, 'value', v.slice(0, s - 1) + v.slice(s));\
            t.setSelectionRange(s - 1, s - 1);\
        }\
    }\
    t.dispatchEvent(globalThis.__obscura_markTrusted(new Event('input', {bubbles:true})));\
})()";

pub async fn handle(
    method: &str,
    params: &Value,
    ctx: &mut CdpContext,
    session_id: &Option<String>,
) -> Result<Value, String> {
    match method {
        "dispatchMouseEvent" => {
            let event_type = params.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let x = params.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let y = params.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let _button = params.get("button").and_then(|v| v.as_str()).unwrap_or("left");
            let click_count = params.get("clickCount").and_then(|v| v.as_u64()).unwrap_or(1);

            // One entry point in the page, so a click driven over the wire
            // produces the same event stream as one made by page script: the
            // move onto the element, the press, the pixel of drift a hand adds
            // between press and release, then the release and the click. This
            // used to fire mousedown and click together with no mouseup and no
            // preceding move, and carried its own copy of the link, form and
            // checkbox activation that Element.click() already had.
            if matches!(event_type, "mouseMoved" | "mousePressed" | "mouseReleased") {
                if let Some(page) = ctx.get_session_page_mut(session_id) {
                    let routed_to_frame =
                        page.dispatch_mouse_event(event_type, x, y, click_count);
                    if routed_to_frame && event_type == "mouseReleased" {
                        page.settle(100).await;
                    }
                    let moved = page
                        .process_pending_navigation()
                        .await
                        .map_err(|e| e.to_string())?;
                    // A single page app answers a click by routing itself, with
                    // no document fetch. The client still has to be told the
                    // frame moved, or the click looks like it did nothing.
                    if moved {
                        // The document has been replaced, so every execution
                        // context id the client is holding is stale. Emitting
                        // frameNavigated alone left Playwright evaluating
                        // against a dead context, where every expression came
                        // back as an empty object — including `() => 42`.
                        let url = page.url_string();
                        let frame_id = page.frame_id.clone();
                        let page_id = page.id.clone();
                        let network_events: Vec<_> = page.network_events.drain(..).collect();
                        crate::domains::page::emit_navigation_events(
                            ctx,
                            session_id,
                            &frame_id,
                            &format!("loader-click-{page_id}"),
                            &url,
                            &page_id,
                            &network_events,
                            obscura_browser::WaitUntil::Load,
                            true,
                        );
                    }
                }
            }

            Ok(json!({}))
        }
        "dispatchKeyEvent" => {
            let event_type = params.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
            let code = params.get("code").and_then(|v| v.as_str()).unwrap_or("");
            let text = params.get("text").and_then(|v| v.as_str()).unwrap_or("");

            if let Some(page) = ctx.get_session_page_mut(session_id) {
                match event_type {
                    "keyDown" | "rawKeyDown" => {
                        let js = format!(
                            "(function() {{\
                                var target = document.activeElement || document.body;\
                                var evt = globalThis.__obscura_markTrusted(new KeyboardEvent('keydown', {{bubbles:true,cancelable:true,key:'{key}',code:'{code}'}}));\
                                target.dispatchEvent(evt);\
                            }})()",
                            // Escape backslash BEFORE single-quote (as the text
                            // path below does) so a key like "\" — Chrome's
                            // backslash key — doesn't escape the closing quote
                            // and produce a syntax error that drops the event.
                            key = key.replace('\\', "\\\\").replace('\'', "\\'"),
                            code = code.replace('\\', "\\\\").replace('\'', "\\'"),
                        );
                        page.evaluate(&js);

                        if !text.is_empty() && text != "\r" && text != "\n" {
                            // Need to escape backslash BEFORE single-quote so the new
                            // backslashes from quote escaping don't get double-escaped.
                            let escaped_text = text.replace('\\', "\\\\").replace('\'', "\\'");
                            page.evaluate(&insert_text_js(&escaped_text));
                        }

                        if key == "Enter" {
                            // In a textarea Enter inserts a newline; in input fields
                            // it submits the containing form. Real Chrome distinguishes
                            // these two and we should too: previously every Enter tried
                            // to submit the nearest form even from a textarea.
                            let js = "(function() {\
                                var target = document.activeElement;\
                                if (!target) return;\
                                target.dispatchEvent(globalThis.__obscura_markTrusted(new KeyboardEvent('keypress', {bubbles:true,key:'Enter',code:'Enter'})));\
                                if (target.localName === 'textarea') {\
                                    globalThis.__obscura_setFieldValue(target, 'value', (target.value || '') + '\\n');\
                                    target.dispatchEvent(globalThis.__obscura_markTrusted(new Event('input', {bubbles:true})));\
                                } else {\
                                    var form = target.form || (target.closest && target.closest('form'));\
                                    if (form) {{ try {{ if (typeof form.requestSubmit === 'function') {{ form.requestSubmit(); }} else {{ form.submit(); }} }} catch(e) {{}} }}\
                                }\
                            })()";
                            page.evaluate(js);
                        }

                        if key == "Backspace" {
                            page.evaluate(BACKSPACE_JS);
                        }
                    }
                    "keyUp" => {
                        let js = format!(
                            "(function() {{\
                                var target = document.activeElement || document.body;\
                                var evt = globalThis.__obscura_markTrusted(new KeyboardEvent('keyup', {{bubbles:true,key:'{key}',code:'{code}'}}));\
                                target.dispatchEvent(evt);\
                            }})()",
                            key = key.replace('\\', "\\\\").replace('\'', "\\'"),
                            code = code.replace('\\', "\\\\").replace('\'', "\\'"),
                        );
                        page.evaluate(&js);
                    }
                    "char" => {
                        if !text.is_empty() {
                            let escaped_text = text.replace('\\', "\\\\").replace('\'', "\\'");
                            page.evaluate(&insert_text_js(&escaped_text));
                            // Pump event loop so Angular change detection picks up the input
                            page.settle(50).await;
                        }
                    }
                    _ => {}
                }
            }

            Ok(json!({}))
        }
        "dispatchTouchEvent" => Ok(json!({})),
        "setIgnoreInputEvents" => Ok(json!({})),
        _ => Err(format!("Unknown Input method: {}", method)),
    }
}
