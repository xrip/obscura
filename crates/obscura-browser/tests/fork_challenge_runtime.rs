//! Fork-only checks for browser behavior used by challenge runtimes.

use std::sync::Arc;

use obscura_browser::{BrowserContext, Page};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn probe(expression: &str) -> serde_json::Value {
    let context = Arc::new(BrowserContext::new("fork-challenge-runtime".to_string()));
    let mut page = Page::new("fork-challenge-runtime-page".to_string(), context);
    page.navigate("data:text/html,<p>x</p>")
        .await
        .expect("the fixture page must load");
    page.evaluate(expression)
}

#[tokio::test(flavor = "current_thread")]
async fn btoa_uses_latin1_code_units() {
    let result = probe("btoa(String.fromCharCode(0xe3, 0x91, 0xee))").await;
    assert_eq!(result, serde_json::json!("45Hu"));
}

#[tokio::test(flavor = "current_thread")]
async fn performance_time_origin_is_not_in_the_future() {
    let result = probe(
        r#"
        (() => ({
            now: performance.now(),
            originNotFuture: performance.timeOrigin <= Date.now(),
        }))()
        "#,
    )
    .await;
    assert!(result["now"].as_f64().is_some_and(|value| value >= 0.0));
    assert_eq!(result["originNotFuture"], serde_json::json!(true));
}

#[tokio::test(flavor = "current_thread")]
async fn function_to_string_has_the_native_non_constructable_shape() {
    let result = probe(
        r#"
        (() => {
            const fn = Function.prototype.toString;
            let constructError = '';
            try { new fn(); } catch (error) { constructError = error.name; }
            return {
                source: fn.toString(),
                hasOwnPrototype: Object.prototype.hasOwnProperty.call(fn, 'prototype'),
                constructError,
            };
        })()
        "#,
    )
    .await;
    assert_eq!(
        result,
        serde_json::json!({
            "source": "function toString() { [native code] }",
            "hasOwnPrototype": false,
            "constructError": "TypeError",
        })
    );
}

#[tokio::test(flavor = "current_thread")]
async fn chrome_app_has_the_measured_chrome_151_shape() {
    let result = probe(
        r#"
        (() => ({
            own: Object.getOwnPropertyNames(chrome.app),
            getDetails: chrome.app.getDetails(),
            getIsInstalled: chrome.app.getIsInstalled(),
            installState: chrome.app.installState(),
            runningState: chrome.app.runningState(),
            sources: [
                chrome.app.getDetails,
                chrome.app.getIsInstalled,
                chrome.app.installState,
                chrome.app.runningState,
            ].map(fn => ({name: fn.name, length: fn.length, source: fn.toString()})),
            callbackErrors: ['getDetails', 'getIsInstalled'].map(name => {
                try { chrome.app[name](() => {}); return ''; }
                catch (error) { return error.name; }
            }),
        }))()
        "#,
    )
    .await;

    assert_eq!(
        result["own"],
        serde_json::json!([
            "isInstalled", "getDetails", "getIsInstalled", "installState",
            "runningState", "InstallState", "RunningState"
        ])
    );
    assert_eq!(result["getDetails"], serde_json::Value::Null);
    assert_eq!(result["getIsInstalled"], serde_json::json!(false));
    assert_eq!(result["installState"], serde_json::Value::Null);
    assert_eq!(result["runningState"], serde_json::json!("cannot_run"));
    assert_eq!(result["callbackErrors"], serde_json::json!(["TypeError", "TypeError"]));
    for source in result["sources"].as_array().unwrap() {
        assert_eq!(source["length"], serde_json::json!(0));
        assert_eq!(
            source["source"],
            serde_json::json!(format!(
                "function {}() {{ [native code] }}",
                source["name"].as_str().unwrap()
            ))
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn chrome_timing_helpers_have_the_measured_chrome_151_shape() {
    let result = probe(
        r#"
        (() => ['csi', 'loadTimes'].map(name => {
            const fn = chrome[name];
            return {
                name: fn.name,
                length: fn.length,
                source: fn.toString(),
                ownPrototype: Object.prototype.hasOwnProperty.call(fn, 'prototype'),
            };
        }))()
        "#,
    )
    .await;

    for shape in result.as_array().unwrap() {
        assert_eq!(shape["name"], serde_json::json!(""));
        assert_eq!(shape["length"], serde_json::json!(0));
        assert_eq!(shape["source"], serde_json::json!("function () { [native code] }"));
        assert_eq!(shape["ownPrototype"], serde_json::json!(true));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn navigator_api_objects_have_the_measured_chrome_151_shape() {
    let result = probe(
        r#"
        (() => { try {
            const names = [
                'clipboard', 'credentials', 'geolocation', 'keyboard', 'locks',
                'mediaCapabilities', 'serviceWorker', 'storage', 'wakeLock',
            ];
            return {
                objects: Object.fromEntries(names.map(name => {
                    const value = navigator[name];
                    const prototype = Object.getPrototypeOf(value);
                    const parent = Object.getPrototypeOf(prototype);
                    return [name, {
                        own: Object.getOwnPropertyNames(value),
                        tag: Object.prototype.toString.call(value),
                        members: Object.getOwnPropertyNames(prototype)
                            .filter(member => member !== 'constructor').sort(),
                        parent: parent?.constructor?.name || null,
                    }];
                })),
                plugins: {
                    own: Object.getOwnPropertyNames(navigator.plugins),
                    prototype: Object.getOwnPropertyNames(PluginArray.prototype),
                    parent: Object.getPrototypeOf(PluginArray.prototype).constructor.name,
                },
                mimeTypes: {
                    own: Object.getOwnPropertyNames(navigator.mimeTypes),
                    prototype: Object.getOwnPropertyNames(MimeTypeArray.prototype),
                },
            };
        } catch (error) {
            return {error: error.stack || String(error)};
        }
        })()
        "#,
    )
    .await;

    assert!(result.get("error").is_none(), "{result}");

    let objects = &result["objects"];
    for (name, tag, parent, members) in [
        ("clipboard", "[object Clipboard]", "EventTarget", vec!["onclipboardchange", "read", "readText", "write", "writeText"]),
        ("credentials", "[object CredentialsContainer]", "Object", vec!["create", "get", "preventSilentAccess", "store"]),
        ("geolocation", "[object Geolocation]", "Object", vec!["clearWatch", "getCurrentPosition", "watchPosition"]),
        ("keyboard", "[object Keyboard]", "Object", vec!["getLayoutMap", "lock", "unlock"]),
        ("locks", "[object LockManager]", "Object", vec!["query", "request"]),
        ("mediaCapabilities", "[object MediaCapabilities]", "Object", vec!["decodingInfo", "encodingInfo"]),
        ("serviceWorker", "[object ServiceWorkerContainer]", "EventTarget", vec!["controller", "getRegistration", "getRegistrations", "oncontrollerchange", "onmessage", "onmessageerror", "ready", "register", "startMessages"]),
        ("storage", "[object StorageManager]", "Object", vec!["estimate", "getDirectory", "persist", "persisted"]),
        ("wakeLock", "[object WakeLock]", "Object", vec!["request"]),
    ] {
        assert_eq!(objects[name]["own"], serde_json::json!([]), "{name}: {result}");
        assert_eq!(objects[name]["tag"], serde_json::json!(tag), "{name}: {result}");
        assert_eq!(objects[name]["parent"], serde_json::json!(parent), "{name}: {result}");
        assert_eq!(objects[name]["members"], serde_json::json!(members), "{name}: {result}");
    }

    assert_eq!(
        result["plugins"],
        serde_json::json!({
            "own": [
                "0", "1", "2", "3", "4", "PDF Viewer", "Chrome PDF Viewer",
                "Chromium PDF Viewer", "Microsoft Edge PDF Viewer", "WebKit built-in PDF"
            ],
            "prototype": ["length", "item", "namedItem", "refresh", "constructor"],
            "parent": "Object",
        })
    );
    assert_eq!(
        result["mimeTypes"],
        serde_json::json!({
            "own": ["0", "1", "application/pdf", "text/pdf"],
            "prototype": ["length", "item", "namedItem", "constructor"],
        })
    );
}

#[tokio::test(flavor = "current_thread")]
async fn browser_getters_reject_a_missing_receiver() {
    let result = probe(
        r#"
        (() => {
            const rejectsBareCall = getter => {
                try { getter(); return false; }
                catch (error) { return error instanceof TypeError; }
            };
            const navigatorGetters = Object.getOwnPropertyNames(Navigator.prototype)
                .map(name => Object.getOwnPropertyDescriptor(Navigator.prototype, name)?.get)
                .filter(getter => typeof getter === 'function');
            const performanceGetters = Object.getOwnPropertyNames(Performance.prototype)
                .map(name => Object.getOwnPropertyDescriptor(Performance.prototype, name)?.get)
                .filter(getter => typeof getter === 'function');
            return {
                navigatorCount: navigatorGetters.length,
                navigatorAllReject: navigatorGetters.every(rejectsBareCall),
                performanceCount: performanceGetters.length,
                performanceAllReject: performanceGetters.every(rejectsBareCall),
            };
        })()
        "#,
    )
    .await;

    assert!(result["navigatorCount"].as_u64().unwrap_or(0) > 20);
    assert_eq!(result["navigatorAllReject"], serde_json::json!(true));
    assert_eq!(result["performanceCount"], serde_json::json!(7));
    assert_eq!(result["performanceAllReject"], serde_json::json!(true));
}

#[tokio::test(flavor = "current_thread")]
async fn document_challenge_compatibility_flags_match_chrome() {
    let result = probe(
        r#"
        (() => ({
            fullscreenEnabled: document.fullscreenEnabled,
            webkitFullscreenEnabled: document.webkitFullscreenEnabled,
            webkitHidden: document.webkitHidden,
            webkitVisibilityState: document.webkitVisibilityState,
            nativeFullscreenGetter:
                Object.getOwnPropertyDescriptor(Document.prototype, 'fullscreenEnabled').get.toString(),
        }))()
        "#,
    )
    .await;
    assert_eq!(
        result,
        serde_json::json!({
            "fullscreenEnabled": true,
            "webkitFullscreenEnabled": true,
            "webkitHidden": false,
            "webkitVisibilityState": "visible",
            "nativeFullscreenGetter": "function get fullscreenEnabled() { [native code] }",
        })
    );
}

#[tokio::test(flavor = "current_thread")]
async fn rtp_sender_capabilities_match_chrome_windows_shape() {
    let result = probe(
        r#"
        (() => {
            const audio = RTCRtpSender.getCapabilities('audio');
            const video = RTCRtpSender.getCapabilities('video');
            let constructError = '';
            try { new RTCRtpSender.getCapabilities('audio'); }
            catch (error) { constructError = error.name; }
            return {
                senderType: typeof RTCRtpSender,
                nativeGetter: RTCRtpSender.getCapabilities.toString(),
                hasOwnPrototype: Object.prototype.hasOwnProperty.call(
                    RTCRtpSender.getCapabilities, 'prototype'),
                constructError,
                audioCodecs: audio.codecs.length,
                audioHeaders: audio.headerExtensions.length,
                videoCodecs: video.codecs.length,
                videoHeaders: video.headerExtensions.length,
                opus: audio.codecs[0],
                invalid: RTCRtpSender.getCapabilities('data'),
            };
        })()
        "#,
    )
    .await;
    assert_eq!(result["senderType"], serde_json::json!("function"));
    assert_eq!(
        result["nativeGetter"],
        serde_json::json!("function getCapabilities() { [native code] }")
    );
    assert_eq!(result["hasOwnPrototype"], serde_json::json!(false));
    assert_eq!(result["constructError"], serde_json::json!("TypeError"));
    assert_eq!(result["audioCodecs"], serde_json::json!(8));
    assert_eq!(result["audioHeaders"], serde_json::json!(4));
    assert_eq!(result["videoCodecs"], serde_json::json!(15));
    assert_eq!(result["videoHeaders"], serde_json::json!(11));
    assert_eq!(
        result["opus"],
        serde_json::json!({
            "channels": 2,
            "clockRate": 48000,
            "mimeType": "audio/opus",
            "sdpFmtpLine": "minptime=10;useinbandfec=1",
        })
    );
    assert_eq!(result["invalid"], serde_json::Value::Null);
}

#[tokio::test(flavor = "current_thread")]
async fn ozon_media_codec_answers_match_chrome_windows() {
    let result = probe(
        r#"
        (() => ({
            aac: document.createElement('audio').canPlayType('audio/aac'),
            m4a: document.createElement('audio').canPlayType('audio/x-m4a'),
            h264: document.createElement('video')
                .canPlayType('video/mp4; codecs="avc1.42E01E"'),
        }))()
        "#,
    )
    .await;
    assert_eq!(
        result,
        serde_json::json!({
            "aac": "probably",
            "m4a": "maybe",
            "h264": "probably",
        })
    );
}

#[tokio::test(flavor = "current_thread")]
async fn protected_audience_and_managed_data_match_chrome_shape() {
    let result = probe(
        r#"
        (() => {
            const audience = navigator.protectedAudience;
            const managed = navigator.managed;
            const audienceDescriptor = Object.getOwnPropertyDescriptor(
                Navigator.prototype, 'protectedAudience');
            const managedDescriptor = Object.getOwnPropertyDescriptor(
                Navigator.prototype, 'managed');
            let audienceConstructorError = '';
            let managedConstructorError = '';
            let audienceInvocationError = '';
            let managedInvocationError = '';
            try { new ProtectedAudience(); }
            catch (error) { audienceConstructorError = error.name; }
            try { new NavigatorManagedData(); }
            catch (error) { managedConstructorError = error.name; }
            try { audience.queryFeatureSupport.call({}); }
            catch (error) { audienceInvocationError = error.name; }
            try { managed.getManagedConfiguration.call({}); }
            catch (error) { managedInvocationError = error.name; }
            return {
                audienceTag: Object.prototype.toString.call(audience),
                managedTag: Object.prototype.toString.call(managed),
                audienceStable: audience === navigator.protectedAudience,
                managedStable: managed === navigator.managed,
                managedIsEventTarget: managed instanceof EventTarget,
                mediaDevicesIsEventTarget: navigator.mediaDevices instanceof EventTarget,
                orientationIsEventTarget: screen.orientation instanceof EventTarget,
                orientationMatchesScreen: screen.orientation.type ===
                    (screen.width >= screen.height ? 'landscape-primary' : 'portrait-primary'),
                orientationOnchange: screen.orientation.onchange,
                orientationLock: screen.orientation.lock.toString(),
                orientationUnlock: screen.orientation.unlock.toString(),
                audienceGetter: audienceDescriptor.get.toString(),
                managedGetter: managedDescriptor.get.toString(),
                audienceMethod: audience.queryFeatureSupport.toString(),
                managedMethod: managed.getManagedConfiguration.toString(),
                audienceConstructorError,
                managedConstructorError,
                audienceInvocationError,
                managedInvocationError,
                adComponentsLimit: audience.queryFeatureSupport('adComponentsLimit'),
                unknownFeature: audience.queryFeatureSupport('unknown') === undefined,
                deprecatedFlag: navigator.deprecatedRunAdAuctionEnforcesKAnonymity,
            };
        })()
        "#,
    )
    .await;

    assert_eq!(
        result,
        serde_json::json!({
            "audienceTag": "[object ProtectedAudience]",
            "managedTag": "[object NavigatorManagedData]",
            "audienceStable": true,
            "managedStable": true,
            "managedIsEventTarget": true,
            "mediaDevicesIsEventTarget": true,
            "orientationIsEventTarget": true,
            "orientationMatchesScreen": true,
            "orientationOnchange": null,
            "orientationLock": "function lock() { [native code] }",
            "orientationUnlock": "function unlock() { [native code] }",
            "audienceGetter": "function get protectedAudience() { [native code] }",
            "managedGetter": "function get managed() { [native code] }",
            "audienceMethod": "function queryFeatureSupport() { [native code] }",
            "managedMethod": "function getManagedConfiguration() { [native code] }",
            "audienceConstructorError": "TypeError",
            "managedConstructorError": "TypeError",
            "audienceInvocationError": "TypeError",
            "managedInvocationError": "TypeError",
            "adComponentsLimit": 40,
            "unknownFeature": true,
            "deprecatedFlag": false,
        })
    );
}

#[tokio::test(flavor = "current_thread")]
async fn only_iframes_are_html_iframe_elements() {
    let result = probe(
        r#"
        (() => {
            const frame = document.createElement('iframe');
            return {
                headIsFrame: document.head instanceof HTMLIFrameElement,
                divIsFrame: document.createElement('div') instanceof HTMLIFrameElement,
                frameIsFrame: frame instanceof HTMLIFrameElement,
                frameConstructor: frame.constructor.name,
            };
        })()
        "#,
    )
    .await;
    assert_eq!(result["headIsFrame"], serde_json::json!(false));
    assert_eq!(result["divIsFrame"], serde_json::json!(false));
    assert_eq!(result["frameIsFrame"], serde_json::json!(true));
    assert_eq!(result["frameConstructor"], serde_json::json!("HTMLIFrameElement"));
}

fn register_chrome_151_test_profile() -> String {
    let index: serde_json::Value = serde_json::from_str(
        &obscura_browser::profiles::catalog()
            .expect("the profile catalog must load")
            .index_json()
            .expect("the profile index must serialize"),
    )
    .expect("the profile index must be JSON");
    let default_id = index["defaultProfileId"]
        .as_str()
        .expect("the profile index needs a default ID");
    let default = obscura_browser::profiles::resolve_profile_id(default_id)
        .expect("the default profile must resolve");
    let mut runtime: serde_json::Value = serde_json::from_str(default.runtime_json())
        .expect("the default runtime profile must be JSON");
    let components = default_id
        .split_once(':')
        .expect("the composed profile ID needs components")
        .1;
    let id = format!("c151w1:{components}");

    runtime["id"] = serde_json::json!(id);
    runtime["browser"]["major"] = serde_json::json!(151);
    runtime["browser"]["version"] = serde_json::json!("151.0.7922.108");
    runtime["browser"]["userAgent"] = serde_json::json!(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
         (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36"
    );
    runtime["graphics"]["webgl1Id"] = serde_json::json!(default.graphics.webgl1_id);
    runtime["graphics"]["webgl2Id"] = serde_json::json!(default.graphics.webgl2_id);
    runtime["graphics"]["webgpuId"] = serde_json::json!(default.graphics.webgpu_id);
    runtime["graphics"]["observationsByBrowserVersion"] =
        serde_json::json!({"151.0.7922.108": default.graphics.weight});
    runtime["graphics"]["weight"] = serde_json::json!(default.graphics.weight);
    for brand in runtime["navigator"]["brands"]
        .as_array_mut()
        .expect("brands must be an array")
    {
        if brand["brand"].as_str() != Some("Not=A?Brand") {
            brand["version"] = serde_json::json!("151");
        }
    }
    for brand in runtime["navigator"]["fullVersionList"]
        .as_array_mut()
        .expect("fullVersionList must be an array")
    {
        if brand["brand"].as_str() != Some("Not=A?Brand") {
            brand["version"] = serde_json::json!("151.0.7922.108");
        }
    }
    let mut hasher = Sha256::new();
    hasher.update(b"graphics-render-v1");
    hasher.update(id.as_bytes());
    runtime["renderSeed"] = serde_json::json!(
        hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>()
    );

    obscura_browser::profiles::register_runtime_profile(&runtime)
        .expect("the Chrome 151 test profile must register")
}

#[tokio::test(flavor = "current_thread")]
async fn secure_chrome_151_navigator_surface_matches_the_headed_control() {
    let (url, _requests) = parser_script_fixture().await;
    let profile_id = register_chrome_151_test_profile();
    let base = BrowserContext::with_storage_and_network(
        "fork-chrome-151-navigator".to_string(),
        None,
        false,
        None,
        None,
        true,
    );
    let context = Arc::new(
        base.copy_with_profile_id(&profile_id)
            .expect("the Chrome 151 profile must be selected"),
    );
    let mut page = Page::new("fork-chrome-151-navigator-page".to_string(), context);
    page.navigate(&url)
        .await
        .expect("the secure loopback fixture must load");

    let result = page.evaluate(
        r#"
        (() => {
            const names = Object.getOwnPropertyNames(Navigator.prototype);
            const methodNames = [
                'vibrate', 'adAuctionComponents', 'runAdAuction',
                'canLoadAdAuctionFencedFrame', 'clearAppBadge', 'getUserMedia',
                'requestMIDIAccess', 'requestMediaKeySystemAccess', 'setAppBadge',
                'webkitGetUserMedia', 'clearOriginJoinedAdInterestGroups',
                'createAuctionNonce', 'joinAdInterestGroup', 'leaveAdInterestGroup',
                'updateAdInterestGroups', 'deprecatedReplaceInURN',
                'deprecatedURNToURL', 'getInstalledRelatedApps',
                'getInterestGroupAdAuctionData', 'registerProtocolHandler',
                'unregisterProtocolHandler',
            ];
            const getterNames = [
                'scheduling', 'userActivation', 'webkitTemporaryStorage',
                'webkitPersistentStorage', 'windowControlsOverlay', 'bluetooth',
                'virtualKeyboard', 'login', 'ink', 'devicePosture', 'hid',
                'mediaSession', 'presentation', 'serial', 'usb', 'xr',
                'storageBuckets',
            ];
            return {
                secure: isSecureContext,
                names,
                methods: Object.fromEntries(methodNames.map(name => {
                    const fn = Navigator.prototype[name];
                    return [name, {
                        name: fn.name,
                        length: fn.length,
                        source: fn.toString(),
                        constructable: Object.prototype.hasOwnProperty.call(fn, 'prototype'),
                    }];
                })),
                getters: Object.fromEntries(getterNames.map(name => {
                    const descriptor = Object.getOwnPropertyDescriptor(Navigator.prototype, name);
                    const value = navigator[name];
                    return [name, {
                        source: descriptor.get.toString(),
                        tag: Object.prototype.toString.call(value),
                        own: Object.getOwnPropertyNames(value),
                    }];
                })),
                nestedLengths: {
                    scheduling: navigator.scheduling.isInputPending.length,
                    overlay: navigator.windowControlsOverlay.getTitlebarAreaRect.length,
                    bluetooth: [navigator.bluetooth.getAvailability.length,
                        navigator.bluetooth.requestDevice.length],
                    virtualKeyboard: [navigator.virtualKeyboard.hide.length,
                        navigator.virtualKeyboard.show.length],
                    login: navigator.login.setStatus.length,
                    ink: navigator.ink.requestPresenter.length,
                    hid: [navigator.hid.getDevices.length, navigator.hid.requestDevice.length],
                    mediaSession: [navigator.mediaSession.setActionHandler.length,
                        navigator.mediaSession.setCameraActive.length,
                        navigator.mediaSession.setMicrophoneActive.length,
                        navigator.mediaSession.setPositionState.length],
                    serial: [navigator.serial.getPorts.length, navigator.serial.requestPort.length],
                    usb: [navigator.usb.getDevices.length, navigator.usb.requestDevice.length],
                    xr: [navigator.xr.isSessionSupported.length, navigator.xr.requestSession.length],
                    buckets: [navigator.storageBuckets.delete.length,
                        navigator.storageBuckets.keys.length, navigator.storageBuckets.open.length],
                },
                prototypeOrder: Object.fromEntries([
                    'Clipboard', 'CredentialsContainer', 'Geolocation', 'Keyboard',
                    'LockManager', 'MediaCapabilities', 'ServiceWorkerContainer',
                    'StorageManager', 'WakeLock', 'HID', 'Serial', 'USB',
                ].map(name => [name, Object.getOwnPropertyNames(globalThis[name].prototype)])),
                defaults: {
                    activation: [navigator.userActivation.hasBeenActive,
                        navigator.userActivation.isActive],
                    overlay: [navigator.windowControlsOverlay.visible,
                        navigator.windowControlsOverlay.ongeometrychange],
                    keyboard: [Object.prototype.toString.call(navigator.virtualKeyboard.boundingRect),
                        navigator.virtualKeyboard.boundingRect.toJSON(),
                        navigator.virtualKeyboard.overlaysContent,
                        navigator.virtualKeyboard.ongeometrychange],
                    posture: [navigator.devicePosture.type, navigator.devicePosture.onchange],
                    mediaSession: [navigator.mediaSession.metadata,
                        navigator.mediaSession.playbackState],
                    presentation: [navigator.presentation.defaultRequest,
                        navigator.presentation.receiver],
                },
            };
        })()
        "#,
    );

    assert_eq!(result["secure"], serde_json::json!(true), "{result}");
    assert_eq!(
        result["names"],
        serde_json::json!([
            "vendorSub", "productSub", "vendor", "maxTouchPoints", "scheduling",
            "userActivation", "geolocation", "doNotTrack", "webkitTemporaryStorage",
            "webkitPersistentStorage", "windowControlsOverlay", "hardwareConcurrency",
            "cookieEnabled", "appCodeName", "appName", "appVersion", "platform",
            "product", "userAgent", "language", "languages", "onLine", "webdriver",
            "plugins", "mimeTypes", "pdfViewerEnabled", "connection", "getGamepads",
            "javaEnabled", "sendBeacon", "vibrate", "constructor",
            "deprecatedRunAdAuctionEnforcesKAnonymity", "protectedAudience", "bluetooth",
            "clipboard", "credentials", "keyboard", "managed", "mediaDevices",
            "serviceWorker", "virtualKeyboard", "wakeLock", "deviceMemory",
            "userAgentData", "locks", "storage", "gpu", "login", "ink",
            "mediaCapabilities", "permissions", "devicePosture", "hid", "mediaSession",
            "presentation", "serial", "usb", "xr", "storageBuckets",
            "adAuctionComponents", "runAdAuction", "canLoadAdAuctionFencedFrame",
            "canShare", "share", "clearAppBadge", "getBattery", "getUserMedia",
            "requestMIDIAccess", "requestMediaKeySystemAccess", "setAppBadge",
            "webkitGetUserMedia", "clearOriginJoinedAdInterestGroups",
            "createAuctionNonce", "joinAdInterestGroup", "leaveAdInterestGroup",
            "updateAdInterestGroups", "deprecatedReplaceInURN", "deprecatedURNToURL",
            "getInstalledRelatedApps", "getInterestGroupAdAuctionData",
            "registerProtocolHandler", "unregisterProtocolHandler",
        ]),
        "{result}",
    );
    for (name, shape) in result["methods"].as_object().expect("method shapes") {
        assert_eq!(shape["name"], serde_json::json!(name), "{name}: {result}");
        assert_eq!(
            shape["source"],
            serde_json::json!(format!("function {name}() {{ [native code] }}")),
            "{name}: {result}",
        );
        assert_eq!(shape["constructable"], serde_json::json!(false), "{name}: {result}");
    }
    assert_eq!(
        result["methods"].as_object().map(|methods| methods.iter()
            .map(|(name, shape)| (name.as_str(), shape["length"].as_u64().unwrap()))
            .collect::<std::collections::BTreeMap<_, _>>()),
        Some(std::collections::BTreeMap::from([
            ("adAuctionComponents", 1), ("canLoadAdAuctionFencedFrame", 0),
            ("clearAppBadge", 0), ("clearOriginJoinedAdInterestGroups", 1),
            ("createAuctionNonce", 0), ("deprecatedReplaceInURN", 2),
            ("deprecatedURNToURL", 1), ("getInstalledRelatedApps", 0),
            ("getInterestGroupAdAuctionData", 1), ("getUserMedia", 3),
            ("joinAdInterestGroup", 1), ("leaveAdInterestGroup", 0),
            ("registerProtocolHandler", 2), ("requestMIDIAccess", 0),
            ("requestMediaKeySystemAccess", 2), ("runAdAuction", 1),
            ("setAppBadge", 0), ("unregisterProtocolHandler", 2),
            ("updateAdInterestGroups", 0), ("vibrate", 1),
            ("webkitGetUserMedia", 3),
        ])),
    );
    assert_eq!(
        result["nestedLengths"],
        serde_json::json!({
            "scheduling": 0, "overlay": 0, "bluetooth": [0, 0],
            "virtualKeyboard": [0, 0], "login": 1, "ink": 0,
            "hid": [0, 1], "mediaSession": [2, 1, 1, 0],
            "serial": [0, 0], "usb": [0, 1], "xr": [1, 1],
            "buckets": [1, 0, 1],
        }),
    );
    assert_eq!(
        result["prototypeOrder"],
        serde_json::json!({
            "Clipboard": ["onclipboardchange", "read", "readText", "write", "writeText", "constructor"],
            "CredentialsContainer": ["create", "get", "preventSilentAccess", "store", "constructor"],
            "Geolocation": ["clearWatch", "getCurrentPosition", "watchPosition", "constructor"],
            "Keyboard": ["getLayoutMap", "lock", "unlock", "constructor"],
            "LockManager": ["query", "request", "constructor"],
            "MediaCapabilities": ["decodingInfo", "encodingInfo", "constructor"],
            "ServiceWorkerContainer": ["controller", "ready", "oncontrollerchange", "onmessage",
                "onmessageerror", "getRegistration", "getRegistrations", "register", "startMessages",
                "constructor"],
            "StorageManager": ["estimate", "persisted", "constructor", "getDirectory", "persist"],
            "WakeLock": ["request", "constructor"],
            "HID": ["onconnect", "ondisconnect", "getDevices", "constructor", "requestDevice"],
            "Serial": ["onconnect", "ondisconnect", "getPorts", "constructor", "requestPort"],
            "USB": ["onconnect", "ondisconnect", "getDevices", "constructor", "requestDevice"],
        }),
    );
    assert_eq!(
        result["defaults"],
        serde_json::json!({
            "activation": [false, false],
            "overlay": [false, null],
            "keyboard": ["[object DOMRect]", {"x": 0, "y": 0, "width": 0, "height": 0,
                "top": 0, "right": 0, "bottom": 0, "left": 0}, false, null],
            "posture": ["continuous", null],
            "mediaSession": [null, "none"],
            "presentation": [null, null],
        }),
    );
    for (name, shape) in result["getters"].as_object().expect("getter shapes") {
        assert_eq!(
            shape["source"],
            serde_json::json!(format!("function get {name}() {{ [native code] }}")),
            "{name}: {result}",
        );
        assert_eq!(shape["own"], serde_json::json!([]), "{name}: {result}");
    }
    assert_eq!(
        result["getters"]["virtualKeyboard"]["tag"],
        serde_json::json!("[object VirtualKeyboard]"),
    );

    page.navigate("data:text/html,<p>opaque</p>")
        .await
        .expect("the opaque fixture must load");
    let insecure = page.evaluate(
        r#"
        (() => ({
            secure: isSecureContext,
            members: ['scheduling', 'bluetooth', 'vibrate', 'runAdAuction']
                .filter(name => name in Navigator.prototype),
            constructors: ['Scheduling', 'Bluetooth', 'MediaSession', 'XRSystem']
                .filter(name => name in globalThis),
            screen: ['isExtended', 'onchange'].filter(name => name in Screen.prototype),
        }))()
        "#,
    );
    assert_eq!(
        insecure,
        serde_json::json!({
            "secure": false,
            "members": [],
            "constructors": [],
            "screen": [],
        }),
    );
}

async fn parser_script_fixture() -> (String, tokio::sync::mpsc::UnboundedReceiver<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("the fixture listener must bind");
    let address = listener.local_addr().expect("the listener needs an address");
    let (request_tx, request_rx) = tokio::sync::mpsc::unbounded_channel();

    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let request_tx = request_tx.clone();
            tokio::spawn(async move {
                let mut raw = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let Ok(read) = stream.read(&mut chunk).await else {
                        return;
                    };
                    if read == 0 {
                        return;
                    }
                    raw.extend_from_slice(&chunk[..read]);
                    if raw.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }

                let request = String::from_utf8_lossy(&raw).into_owned();
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/");
                let (content_type, body) = if path == "/parser-script.js" {
                    ("application/javascript", "globalThis.__parserScriptRan = true;")
                } else {
                    (
                        "text/html; charset=utf-8",
                        "<!doctype html><script src=\"/parser-script.js\"></script>",
                    )
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = request_tx.send(request);
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });

    (format!("http://{address}/"), request_rx)
}

#[cfg(feature = "stealth")]
#[tokio::test(flavor = "current_thread")]
async fn parser_scripts_use_the_stealth_transport_identity() {
    let (url, mut requests) = parser_script_fixture().await;
    let context = BrowserContext::with_storage_and_network(
        "fork-parser-script-stealth".to_string(),
        None,
        true,
        None,
        None,
        true,
    );
    context
        .http_client
        .set_user_agent("Plain-Client-Sentinel/1.0")
        .await;
    let context = Arc::new(context);
    let mut page = Page::new("fork-parser-script-stealth-page".to_string(), context);
    page.navigate(&url)
        .await
        .expect("the fixture page must load");
    assert_eq!(page.evaluate("globalThis.__parserScriptRan === true"), serde_json::json!(true));

    let mut observed = Vec::new();
    for _ in 0..2 {
        observed.push(
            tokio::time::timeout(std::time::Duration::from_secs(5), requests.recv())
                .await
                .expect("the fixture request must arrive")
                .expect("the fixture request channel must stay open"),
        );
    }
    let script_request = observed
        .iter()
        .find(|request| request.starts_with("GET /parser-script.js "))
        .expect("the parser script request must be present")
        .to_ascii_lowercase();
    assert!(
        script_request.contains("\r\nsec-ch-ua:"),
        "parser script request did not use the stealth identity:\n{script_request}"
    );
    assert!(
        !script_request.contains("plain-client-sentinel"),
        "parser script request used the plain client:\n{script_request}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn local_storage_survives_a_same_origin_reload() {
    let (url, _requests) = parser_script_fixture().await;
    let context = Arc::new(BrowserContext::with_storage_and_network(
        "fork-local-storage".to_string(),
        None,
        false,
        None,
        None,
        true,
    ));
    let mut page = Page::new("fork-local-storage-page".to_string(), context);
    page.navigate(&url)
        .await
        .expect("the first fixture page must load");

    let stored = page.evaluate(
        r#"
        (() => {
            localStorage.setItem('x_wbaas_token_treshold', JSON.stringify({tries: 1}));
            return {
                value: localStorage.getItem('x_wbaas_token_treshold'),
                key: localStorage.key(0),
                length: localStorage.length,
            };
        })()
        "#,
    );
    assert_eq!(stored["value"], serde_json::json!(r#"{"tries":1}"#));
    assert_eq!(stored["key"], serde_json::json!("x_wbaas_token_treshold"));
    assert_eq!(stored["length"].as_f64(), Some(1.0));

    page.navigate(&url)
        .await
        .expect("the reloaded fixture page must load");
    assert_eq!(
        page.evaluate("localStorage.getItem('x_wbaas_token_treshold')"),
        serde_json::json!(r#"{"tries":1}"#),
    );
}
