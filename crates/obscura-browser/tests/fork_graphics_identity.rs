//! Fork-only. The graphics identity a page sees must come from the selected
//! fingerprint profile, and must agree with itself across surfaces.
//!
//! Upstream deliberately returns `null` from `getContext('webgl')`, because a
//! shim that reports success while every draw is a no-op makes applications
//! choose the WebGL path and render nothing. Its test
//! `unavailable_webgl_context_does_not_claim_success` guards that, and it still
//! passes: the fork's facade only appears once a profile is loaded, which is
//! what lets it answer truthfully. This test covers the other half.

use std::sync::Arc;

use obscura_browser::{BrowserContext, Page};

async fn evaluate_on_blank_canvas(expression: &str) -> serde_json::Value {
    let context = Arc::new(BrowserContext::new("fork-graphics".to_string()));
    let mut page = Page::new("fork-graphics-page".to_string(), context);
    page.navigate("data:text/html,<canvas id=c></canvas>")
        .await
        .expect("the fixture page must load");
    page.evaluate(expression)
}

#[tokio::test(flavor = "current_thread")]
async fn webgl_reports_the_selected_profile_gpu() {
    let profile = obscura_browser::profiles::resolve_profile().expect("a profile must resolve");
    let expected_vendor = profile.graphics.unmasked_vendor.clone();
    let expected_renderer = profile.graphics.unmasked_renderer.clone();

    let result = evaluate_on_blank_canvas(
        r#"
        (() => {
            const gl = document.getElementById('c').getContext('webgl');
            if (!gl) return null;
            const info = gl.getExtension('WEBGL_debug_renderer_info');
            return {
                vendor: gl.getParameter(info.UNMASKED_VENDOR_WEBGL),
                renderer: gl.getParameter(info.UNMASKED_RENDERER_WEBGL),
                version: gl.getParameter(gl.VERSION),
                extensions: (gl.getSupportedExtensions() || []).length,
            };
        })()
        "#,
    )
    .await;

    assert_eq!(result["vendor"], serde_json::json!(expected_vendor));
    assert_eq!(result["renderer"], serde_json::json!(expected_renderer));
    assert_eq!(
        result["version"],
        serde_json::json!("WebGL 1.0 (OpenGL ES 2.0 Chromium)")
    );
    // A renderer that advertises no extensions is as good a tell as one that
    // advertises the wrong ones.
    assert!(
        result["extensions"].as_u64().unwrap_or(0) > 20,
        "expected a realistic extension list, got {}",
        result["extensions"]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn one_canvas_yields_one_context_type() {
    // Chrome refuses a second context of a different type on the same canvas.
    // Getting this wrong is trivially detectable.
    let result = evaluate_on_blank_canvas(
        r#"
        (() => {
            const canvas = document.getElementById('c');
            const gl = canvas.getContext('webgl');
            return {
                first: !!gl,
                sameAgain: canvas.getContext('webgl') === gl,
                experimentalAlias: canvas.getContext('experimental-webgl') === gl,
                otherType: canvas.getContext('webgl2'),
                twoD: canvas.getContext('2d'),
            };
        })()
        "#,
    )
    .await;

    assert_eq!(result["first"], serde_json::json!(true));
    assert_eq!(result["sameAgain"], serde_json::json!(true));
    assert_eq!(result["experimentalAlias"], serde_json::json!(true));
    assert_eq!(result["otherType"], serde_json::Value::Null);
    assert_eq!(result["twoD"], serde_json::Value::Null);
}

#[tokio::test(flavor = "current_thread")]
async fn webgpu_is_absent_outside_a_secure_context() {
    // WebGPU is secure-context only. A `data:` URL is not one, and reporting a
    // GPU there would contradict Chrome.
    let result = evaluate_on_blank_canvas("typeof navigator.gpu").await;
    assert_eq!(result, serde_json::json!("undefined"));
}
