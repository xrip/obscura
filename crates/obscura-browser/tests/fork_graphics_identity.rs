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
async fn simple_webgl_gradient_draws_compressible_pixels() {
    let result = evaluate_on_blank_canvas(
        r#"
        (() => {
            const canvas = document.getElementById('c');
            const gl = canvas.getContext('webgl');
            const compile = (type, source) => {
                const shader = gl.createShader(type);
                gl.shaderSource(shader, source);
                gl.compileShader(shader);
                return shader;
            };
            const program = gl.createProgram();
            gl.attachShader(program, compile(gl.VERTEX_SHADER,
                'attribute vec2 attrVertex;varying vec2 varyinTexCoordinate;uniform vec2 uniformOffset;void main(){varyinTexCoordinate=attrVertex+uniformOffset;gl_Position=vec4(attrVertex,0,1);}'));
            gl.attachShader(program, compile(gl.FRAGMENT_SHADER,
                'precision mediump float;varying vec2 varyinTexCoordinate;void main() {gl_FragColor=vec4(varyinTexCoordinate,0,1);}'));
            gl.linkProgram(program);
            gl.useProgram(program);
            const buffer = gl.createBuffer();
            gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
            gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([
                -0.2, -0.9, 0,
                 0.4, -0.26, 0,
                 0,    0.73213446, 0,
            ]), gl.STATIC_DRAW);
            gl.enableVertexAttribArray();
            gl.vertexAttribPointer(0, 3, gl.FLOAT, false, 0, 0);
            gl.uniform2f(gl.getUniformLocation(program, 'uniformOffset'), 1, 1);
            gl.drawArrays(gl.TRIANGLE_STRIP, 0, 3);
            const center = new Uint8Array(4);
            const corner = new Uint8Array(4);
            gl.readPixels(150, 65, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, center);
            gl.readPixels(5, 5, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, corner);
            return {
                dataUrlLength: canvas.toDataURL().length,
                center: Array.from(center),
                corner: Array.from(corner),
                error: gl.getError(),
            };
        })()
        "#,
    )
    .await;

    assert_eq!(result["error"], serde_json::json!(0));
    assert_eq!(result["center"][3], serde_json::json!(255));
    assert_ne!(result["center"][0], serde_json::json!(0));
    assert_ne!(result["center"][1], serde_json::json!(0));
    assert_eq!(result["corner"], serde_json::json!([0, 0, 0, 0]));
    assert!(
        result["dataUrlLength"].as_u64().unwrap_or(u64::MAX) < 10_000,
        "simple WebGL output should compress like a rasterized gradient: {}",
        result["dataUrlLength"]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn navigator_and_window_report_the_selected_profile() {
    let profile = obscura_browser::profiles::resolve_profile().expect("a profile must resolve");
    let context = Arc::new(BrowserContext::new("fork-profile-surfaces".to_string()));
    assert_eq!(context.profile_id(), profile.id);

    let mut page = Page::new("fork-profile-surfaces-page".to_string(), context);
    page.navigate("data:text/html,<body></body>")
        .await
        .expect("the fixture page must load");
    let result = page.evaluate(
        r#"
        (() => ({
            navigator: {
                hardwareConcurrency: navigator.hardwareConcurrency,
                deviceMemory: navigator.deviceMemory,
                maxTouchPoints: navigator.maxTouchPoints,
                language: navigator.language,
                languages: navigator.languages,
            },
            screen: {
                width: screen.width,
                height: screen.height,
                availWidth: screen.availWidth,
                availHeight: screen.availHeight,
                availLeft: screen.availLeft,
                availTop: screen.availTop,
                colorDepth: screen.colorDepth,
                pixelDepth: screen.pixelDepth,
            },
            screenShape: {
                own: Object.getOwnPropertyNames(screen),
                prototype: Object.getOwnPropertyNames(Screen.prototype),
                tag: Object.prototype.toString.call(screen),
                eventTarget: screen instanceof EventTarget,
                hasIsExtended: 'isExtended' in screen,
                hasOnchange: 'onchange' in screen,
                isExtended: screen.isExtended,
                onchange: screen.onchange,
                widthGetterConstructable:
                    'prototype' in Object.getOwnPropertyDescriptor(Screen.prototype, 'width').get,
                isExtendedIllegalReceiver: (() => {
                    const descriptor = Object.getOwnPropertyDescriptor(
                        Screen.prototype, 'isExtended');
                    if (!descriptor) return null;
                    try {
                        descriptor.get.call({});
                        return false;
                    } catch (error) { return error instanceof TypeError; }
                })(),
                illegalConstructor: (() => {
                    try { new Screen(); return false; }
                    catch (error) { return error instanceof TypeError; }
                })(),
            },
            visualViewportShape: {
                own: Object.getOwnPropertyNames(visualViewport),
                prototype: Object.getOwnPropertyNames(VisualViewport.prototype),
                parent: Object.getPrototypeOf(VisualViewport.prototype).constructor.name,
                tag: Object.prototype.toString.call(visualViewport),
                eventTarget: visualViewport instanceof EventTarget,
                width: visualViewport.width,
                height: visualViewport.height,
                offsetLeft: visualViewport.offsetLeft,
                offsetTop: visualViewport.offsetTop,
                pageLeft: visualViewport.pageLeft,
                pageTop: visualViewport.pageTop,
                scale: visualViewport.scale,
                onresize: visualViewport.onresize,
                onscroll: visualViewport.onscroll,
                onscrollend: visualViewport.onscrollend,
                windowGetter: Object.getOwnPropertyDescriptor(window, 'visualViewport').get.toString(),
                illegalConstructor: (() => {
                    try { new VisualViewport(); return false; }
                    catch (error) { return error instanceof TypeError; }
                })(),
            },
            batteryInterface: {
                prototype: Object.getOwnPropertyNames(BatteryManager.prototype),
                parent: Object.getPrototypeOf(BatteryManager.prototype).constructor.name,
                getBattery: navigator.getBattery.toString(),
                getBatteryConstructable: 'prototype' in navigator.getBattery,
                illegalConstructor: (() => {
                    try { new BatteryManager(); return false; }
                    catch (error) { return error instanceof TypeError; }
                })(),
            },
            window: {
                innerWidth,
                innerHeight,
                outerWidth,
                outerHeight,
                screenX,
                screenY,
                devicePixelRatio,
            },
            navigatorPrototype: Object.getPrototypeOf(navigator) === Navigator.prototype,
            connection: {
                own: Object.getOwnPropertyNames(navigator.connection),
                prototype: Object.getOwnPropertyNames(Object.getPrototypeOf(navigator.connection)),
                downlink: navigator.connection.downlink,
                rtt: navigator.connection.rtt,
                effectiveType: navigator.connection.effectiveType,
                saveData: navigator.connection.saveData,
                hasType: 'type' in navigator.connection,
                hasDownlinkMax: 'downlinkMax' in navigator.connection,
                tag: Object.prototype.toString.call(navigator.connection),
                eventTarget: navigator.connection instanceof EventTarget,
            },
            nativeAccessors: {
                userAgent: Object.getOwnPropertyDescriptor(Navigator.prototype, 'userAgent').get.toString(),
                platform: Object.getOwnPropertyDescriptor(Navigator.prototype, 'platform').get.toString(),
                screenWidth: Object.getOwnPropertyDescriptor(Screen.prototype, 'width').get.toString(),
            },
        }))()
        "#,
    );

    assert_eq!(
        result["navigator"]["deviceMemory"].as_f64(),
        Some(profile.navigator.device_memory)
    );
    assert_eq!(
        serde_json::json!({
            "hardwareConcurrency": result["navigator"]["hardwareConcurrency"],
            "maxTouchPoints": result["navigator"]["maxTouchPoints"],
            "language": result["navigator"]["language"],
            "languages": result["navigator"]["languages"],
        }),
        serde_json::json!({
            "hardwareConcurrency": profile.navigator.hardware_concurrency,
            "maxTouchPoints": profile.navigator.max_touch_points,
            "language": profile.navigator.languages[0],
            "languages": profile.navigator.languages,
        })
    );
    assert_eq!(
        result["screen"],
        serde_json::json!({
            "width": profile.screen.width,
            "height": profile.screen.height,
            "availWidth": profile.screen.avail_width,
            "availHeight": profile.screen.avail_height,
            "availLeft": profile.screen.avail_left,
            "availTop": profile.screen.avail_top,
            "colorDepth": profile.screen.color_depth,
            "pixelDepth": profile.screen.pixel_depth,
        })
    );
    assert_eq!(
        result["screenShape"],
        serde_json::json!({
            "own": [],
            "prototype": [
                "availWidth", "availHeight", "width", "height", "colorDepth",
                "pixelDepth", "availLeft", "availTop", "orientation", "constructor"
            ],
            "tag": "[object Screen]",
            "eventTarget": true,
            "hasIsExtended": false,
            "hasOnchange": false,
            "widthGetterConstructable": false,
            "isExtendedIllegalReceiver": null,
            "illegalConstructor": true,
        })
    );
    assert_eq!(
        result["visualViewportShape"],
        serde_json::json!({
            "own": [],
            "prototype": [
                "offsetLeft", "offsetTop", "pageLeft", "pageTop", "width", "height",
                "scale", "onresize", "onscroll", "onscrollend", "constructor"
            ],
            "parent": "EventTarget",
            "tag": "[object VisualViewport]",
            "eventTarget": true,
            "width": profile.screen.inner_width,
            "height": profile.screen.inner_height,
            "offsetLeft": 0,
            "offsetTop": 0,
            "pageLeft": 0,
            "pageTop": 0,
            "scale": 1,
            "onresize": null,
            "onscroll": null,
            "onscrollend": null,
            "windowGetter": "function get visualViewport() { [native code] }",
            "illegalConstructor": true,
        })
    );
    assert_eq!(
        result["batteryInterface"],
        serde_json::json!({
            "prototype": [
                "charging", "chargingTime", "dischargingTime", "level",
                "onchargingchange", "onchargingtimechange", "ondischargingtimechange",
                "onlevelchange", "constructor"
            ],
            "parent": "EventTarget",
            "getBattery": "function getBattery() { [native code] }",
            "getBatteryConstructable": false,
            "illegalConstructor": true,
        })
    );
    assert_eq!(
        result["window"],
        serde_json::json!({
            "innerWidth": profile.screen.inner_width,
            "innerHeight": profile.screen.inner_height,
            "outerWidth": profile.screen.outer_width,
            "outerHeight": profile.screen.outer_height,
            "screenX": profile.screen.screen_x,
            "screenY": profile.screen.screen_y,
            "devicePixelRatio": profile.screen.device_pixel_ratio,
        })
    );
    assert_eq!(result["navigatorPrototype"], serde_json::json!(true));
    assert_eq!(
        result["connection"],
        serde_json::json!({
            "own": [],
            "prototype": ["onchange", "effectiveType", "rtt", "downlink", "saveData", "constructor"],
            "downlink": profile.network.downlink,
            "rtt": profile.network.rtt,
            "effectiveType": profile.network.effective_type,
            "saveData": profile.network.save_data,
            "hasType": false,
            "hasDownlinkMax": false,
            "tag": "[object NetworkInformation]",
            "eventTarget": true,
        })
    );
    assert_eq!(
        result["nativeAccessors"],
        serde_json::json!({
            "userAgent": "function get userAgent() { [native code] }",
            "platform": "function get platform() { [native code] }",
            "screenWidth": "function get width() { [native code] }",
        })
    );
}

#[tokio::test(flavor = "current_thread")]
async fn user_agent_high_entropy_values_report_the_selected_profile() {
    let profile = obscura_browser::profiles::resolve_profile().expect("a profile must resolve");
    let context = Arc::new(BrowserContext::new("fork-ua-data".to_string()));
    let mut page = Page::new("fork-ua-data-page".to_string(), context);
    page.navigate("data:text/html,<body></body>")
        .await
        .expect("the fixture page must load");

    let result = page
        .evaluate_for_cdp(
            r#"navigator.userAgentData.getHighEntropyValues([
                'architecture', 'bitness', 'fullVersionList', 'uaFullVersion'
            ])"#,
            true,
            true,
        )
        .await
        .value
        .expect("the promise must resolve by value");

    assert_eq!(result["architecture"], profile.navigator.architecture);
    assert_eq!(result["bitness"], profile.navigator.bitness);
    assert_eq!(
        result["brands"],
        serde_json::to_value(&profile.navigator.brands).unwrap()
    );
    assert_eq!(
        result["fullVersionList"],
        serde_json::to_value(&profile.navigator.full_version_list).unwrap()
    );
    assert_eq!(result["uaFullVersion"], profile.browser.version);
}

#[tokio::test(flavor = "current_thread")]
async fn canvas_rect_only_draws_when_the_path_is_filled() {
    let result = evaluate_on_blank_canvas(
        r#"
        (() => {
            const ctx = document.getElementById('c').getContext('2d');
            ctx.fillStyle = '#ff0000';
            ctx.rect(0, 0, 10, 10);
            const beforeFill = ctx.getImageData(1, 1, 1, 1).data[3];
            ctx.fill();
            const afterFill = ctx.getImageData(1, 1, 1, 1).data[3];
            ctx.clearRect(0, 0, 10, 10);
            ctx.fill();
            const afterSecondFill = ctx.getImageData(1, 1, 1, 1).data[3];
            return { beforeFill, afterFill, afterSecondFill };
        })()
        "#,
    )
    .await;

    assert_eq!(
        result,
        serde_json::json!({
            "beforeFill": 0,
            "afterFill": 255,
            "afterSecondFill": 255,
        })
    );
}

#[tokio::test(flavor = "current_thread")]
async fn canvas_output_is_stable_for_one_profile() {
    let expression = r#"
        (() => {
            const canvas = document.getElementById('c');
            const ctx = canvas.getContext('2d');
            ctx.font = '18pt Arial';
            ctx.fillText('profile-stable', 10, 30);
            return canvas.toDataURL();
        })()
    "#;
    let first = evaluate_on_blank_canvas(expression).await;
    let second = evaluate_on_blank_canvas(expression).await;
    assert_eq!(first, second);
}

#[tokio::test(flavor = "current_thread")]
async fn canvas_text_uses_real_font_metrics_and_canonical_units() {
    let result = evaluate_on_blank_canvas(
        r#"
        (() => {
            const ctx = document.getElementById('c').getContext('2d');
            ctx.font = '11pt Arial';
            const canonical = ctx.font;
            const sansWidth = ctx.measureText('iiiiiiii').width;
            ctx.font = '11pt monospace';
            const monoWidth = ctx.measureText('iiiiiiii').width;
            ctx.fillText('fingerprint', 10, 30);
            const painted = ctx.getImageData(0, 0, 300, 50).data
                .filter((value, index) => index % 4 === 3 && value > 0).length;
            return { canonical, sansWidth, monoWidth, painted };
        })()
        "#,
    )
    .await;

    assert_eq!(result["canonical"], serde_json::json!("14.6667px Arial"));
    assert_ne!(result["sansWidth"], result["monoWidth"]);
    assert!(
        result["painted"].as_u64().unwrap_or(0) > 100,
        "expected rasterized glyphs, got {result}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn canvas_dom_wrapper_does_not_enumerate_engine_state() {
    let result = evaluate_on_blank_canvas(
        r#"
        (() => {
            const canvas = document.getElementById('c');
            canvas.getContext('webgl');
            return {
                keys: Object.keys(canvas),
                ownNames: Object.getOwnPropertyNames(canvas),
                privateDescriptor: Object.getOwnPropertyDescriptor(canvas, '_style') || null,
                json: JSON.stringify(canvas),
            };
        })()
        "#,
    )
    .await;

    assert_eq!(
        result,
        serde_json::json!({
            "keys": [],
            "ownNames": [],
            "privateDescriptor": null,
            "json": "{}",
        })
    );
}

#[tokio::test(flavor = "current_thread")]
async fn webgl_internal_digest_does_not_call_page_json_stringify() {
    let calls = evaluate_on_blank_canvas(
        r#"
        (() => {
            const original = JSON.stringify;
            let calls = 0;
            JSON.stringify = function () {
                calls++;
                return original.apply(this, arguments);
            };
            try {
                const gl = document.getElementById('c').getContext('webgl');
                const vertex = gl.createShader(gl.VERTEX_SHADER);
                gl.shaderSource(vertex, 'void main(){gl_Position=vec4(0.0);}');
                gl.compileShader(vertex);
                const fragment = gl.createShader(gl.FRAGMENT_SHADER);
                gl.shaderSource(fragment, 'void main(){gl_FragColor=vec4(1.0);}');
                gl.compileShader(fragment);
                const program = gl.createProgram();
                gl.attachShader(program, vertex);
                gl.attachShader(program, fragment);
                gl.linkProgram(program);
                gl.useProgram(program);
                gl.viewport(0, 0, 300, 150);
                gl.drawArrays(gl.TRIANGLES, 0, 3);
            } finally {
                JSON.stringify = original;
            }
            return calls;
        })()
        "#,
    )
    .await;

    assert_eq!(calls.as_f64(), Some(0.0));
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

#[tokio::test(flavor = "current_thread")]
async fn webgpu_getter_has_native_accessor_shape() {
    let result = evaluate_on_blank_canvas(
        r#"
        (() => {
            const getter = Object.getOwnPropertyDescriptor(Navigator.prototype, 'gpu').get;
            let constructError = '';
            try { new getter(); } catch (error) { constructError = error.name; }
            let bareCallError = '';
            try { getter(); } catch (error) { bareCallError = error.name; }
            return {
                source: getter.toString(),
                hasOwnPrototype: Object.prototype.hasOwnProperty.call(getter, 'prototype'),
                constructError,
                bareCallError,
            };
        })()
        "#,
    )
    .await;
    assert_eq!(
        result,
        serde_json::json!({
            "source": "function get gpu() { [native code] }",
            "hasOwnPrototype": false,
            "constructError": "TypeError",
            "bareCallError": "TypeError",
        })
    );
}
