//! Fork-only. `performance` must be a `Performance` instance, not an object
//! literal.
//!
//! Found from a real failure, not from a spec read: Ozon's anti-bot challenge
//! calls `performance[...].toJSON()` and died with
//! "TypeError: performance[b[127]].toJSON is not a function", leaving the page
//! stuck on "Please enable JavaScript". Upstream builds `performance` as a plain
//! object with every method as an own property, so `constructor.name` was
//! "Object" and no `toJSON` existed anywhere.

use std::sync::Arc;

use obscura_browser::{BrowserContext, Page};

async fn probe(expression: &str) -> serde_json::Value {
    let context = Arc::new(BrowserContext::new("fork-performance".to_string()));
    let mut page = Page::new("fork-performance-page".to_string(), context);
    page.navigate("data:text/html,<p>x</p>")
        .await
        .expect("the fixture page must load");
    page.evaluate(expression)
}

#[tokio::test(flavor = "current_thread")]
async fn performance_is_a_performance_instance() {
    let result = probe(
        r#"
        (() => ({
            ctor: performance.constructor.name,
            tag: Object.prototype.toString.call(performance),
            // Chrome keeps the methods on the prototype, not on the instance.
            nowOnPrototype: Object.getPrototypeOf(performance).hasOwnProperty('now'),
            nowIsOwn: Object.prototype.hasOwnProperty.call(performance, 'now'),
            nowWorks: typeof performance.now() === 'number',
            parent: Object.getPrototypeOf(Performance.prototype).constructor.name,
            own: Object.getOwnPropertyNames(performance),
            illegal: (() => {
                try { new Performance(); return 'no throw'; }
                catch (e) { return e.constructor.name; }
            })(),
        }))()
        "#,
    )
    .await;

    assert_eq!(result["ctor"], serde_json::json!("Performance"));
    assert_eq!(result["tag"], serde_json::json!("[object Performance]"));
    assert_eq!(result["nowOnPrototype"], serde_json::json!(true));
    assert_eq!(result["nowIsOwn"], serde_json::json!(false));
    assert_eq!(result["nowWorks"], serde_json::json!(true));
    assert_eq!(result["parent"], serde_json::json!("EventTarget"));
    assert_eq!(result["own"], serde_json::json!([]));
    assert_eq!(result["illegal"], serde_json::json!("TypeError"));
}

#[tokio::test(flavor = "current_thread")]
async fn timing_and_navigation_serialize() {
    let result = probe(
        r#"
        (() => ({
            perf: typeof performance.toJSON,
            timingCtor: performance.timing.constructor.name,
            timing: typeof performance.timing.toJSON,
            timingOwn: Object.getOwnPropertyNames(performance.timing),
            timingTag: Object.prototype.toString.call(performance.timing),
            // Chrome's PerformanceTiming has twenty-one fields. Reporting the
            // three upstream happens to set is itself a tell.
            timingFields: Object.keys(performance.timing.toJSON()).length,
            lateField: typeof performance.timing.domComplete,
            navigationType: performance.navigation.type,
            redirectCount: performance.navigation.redirectCount,
            navigationCtor: performance.navigation.constructor.name,
            navigationToJSON: typeof performance.navigation.toJSON,
            navigationOwn: Object.getOwnPropertyNames(performance.navigation),
            navigationTag: Object.prototype.toString.call(performance.navigation),
            navigationFields: Object.keys(performance.navigation.toJSON()).length,
            // The exact call the Ozon challenge makes.
            navigationStart: performance.timing.toJSON().navigationStart > 0,
            // Constructing it from script must throw, as in Chrome.
            illegal: (() => {
                try { new PerformanceTiming(); return 'no throw'; }
                catch (e) { return e.constructor.name; }
            })(),
            navigationIllegal: (() => {
                try { new PerformanceNavigation(); return 'no throw'; }
                catch (e) { return e.constructor.name; }
            })(),
        }))()
        "#,
    )
    .await;

    assert_eq!(result["perf"], serde_json::json!("function"));
    assert_eq!(result["timing"], serde_json::json!("function"));
    assert_eq!(result["timingCtor"], serde_json::json!("PerformanceTiming"));
    assert_eq!(result["timingOwn"], serde_json::json!([]));
    assert_eq!(result["timingTag"], serde_json::json!("[object PerformanceTiming]"));
    assert_eq!(result["timingFields"].as_f64(), Some(21.0));
    assert_eq!(result["lateField"], serde_json::json!("number"));
    assert_eq!(result["illegal"], serde_json::json!("TypeError"));
    // Compared numerically: V8 hands integers back as either Number(0) or
    // Number(0.0) depending on the value, so json!(0.0) does not always match.
    assert_eq!(result["navigationType"].as_f64(), Some(0.0));
    assert_eq!(result["redirectCount"].as_f64(), Some(0.0));
    assert_eq!(result["navigationCtor"], serde_json::json!("PerformanceNavigation"));
    assert_eq!(result["navigationToJSON"], serde_json::json!("function"));
    assert_eq!(result["navigationOwn"], serde_json::json!([]));
    assert_eq!(result["navigationTag"], serde_json::json!("[object PerformanceNavigation]"));
    assert_eq!(result["navigationFields"].as_f64(), Some(2.0));
    assert_eq!(result["navigationIllegal"], serde_json::json!("TypeError"));
    assert_eq!(result["navigationStart"], serde_json::json!(true));
}

#[tokio::test(flavor = "current_thread")]
async fn marks_and_high_resolution_time_match_chrome_shape() {
    let result = probe(
        r#"
        (() => {
            try {
            const samples = Array.from({length: 32}, () => performance.now());
            const mark = performance.mark('jobStart', {
                startTime: Date.now(), detail: {source: 'test'},
            });
            const entries = performance.getEntriesByName('jobStart');
            const result = {
                hasFraction: samples.some(value => value % 1 !== 0),
                stable: entries.length === 1 && entries[0] === mark,
                ctor: mark.constructor.name,
                tag: Object.prototype.toString.call(mark),
                own: Object.getOwnPropertyNames(mark),
                name: mark.name,
                entryType: mark.entryType,
                duration: mark.duration,
                startTime: mark.startTime,
                detail: mark.detail,
                json: mark.toJSON(),
                markPrototype: Object.getOwnPropertyNames(PerformanceMark.prototype),
                entryPrototype: Object.getOwnPropertyNames(PerformanceEntry.prototype),
                nativeMark: performance.mark.toString(),
                nativeNameGetter: Object.getOwnPropertyDescriptor(
                    PerformanceEntry.prototype, 'name').get.toString(),
            };
            performance.clearMarks('jobStart');
            result.cleared = performance.getEntriesByName('jobStart').length === 0;
            return result;
            } catch (error) {
                return {
                    errorName: error.name,
                    errorMessage: error.message,
                    errorStack: error.stack,
                };
            }
        })()
        "#,
    )
    .await;

    assert!(result.get("errorName").is_none(), "JavaScript error: {result}");
    assert_eq!(result["hasFraction"], serde_json::json!(true));
    assert_eq!(result["stable"], serde_json::json!(true));
    assert_eq!(result["ctor"], serde_json::json!("PerformanceMark"));
    assert_eq!(result["tag"], serde_json::json!("[object PerformanceMark]"));
    assert_eq!(result["own"], serde_json::json!([]));
    assert_eq!(result["name"], serde_json::json!("jobStart"));
    assert_eq!(result["entryType"], serde_json::json!("mark"));
    assert_eq!(result["duration"].as_f64(), Some(0.0));
    assert!(result["startTime"].as_f64().is_some_and(|value| value > 0.0));
    assert_eq!(result["detail"], serde_json::json!({"source": "test"}));
    assert_eq!(
        result["json"]["name"],
        serde_json::json!("jobStart")
    );
    assert!(result["json"].get("detail").is_none());
    assert!(result["json"]["navigationId"].as_u64().is_some());
    assert_eq!(result["markPrototype"], serde_json::json!(["detail", "constructor"]));
    assert_eq!(
        result["entryPrototype"],
        serde_json::json!([
            "name", "entryType", "startTime", "duration", "toJSON",
            "constructor", "navigationId"
        ])
    );
    assert_eq!(
        result["nativeMark"],
        serde_json::json!("function mark() { [native code] }")
    );
    assert_eq!(
        result["nativeNameGetter"],
        serde_json::json!("function get name() { [native code] }")
    );
    assert_eq!(result["cleared"], serde_json::json!(true));
}

#[tokio::test(flavor = "current_thread")]
async fn the_interfaces_are_not_enumerable_on_the_global() {
    // Same WebIDL rule the graphics interfaces follow: an interface object that
    // shows up in Object.keys(window) is a one-line detection.
    let result = probe(
        r#"
        (() => ['Performance','PerformanceTiming','PerformanceNavigation']
            .filter(n => Object.getOwnPropertyDescriptor(globalThis, n)?.enumerable))()
        "#,
    )
    .await;
    assert_eq!(result, serde_json::json!([]));
}

#[tokio::test(flavor = "current_thread")]
async fn deno_bridge_is_not_enumerable_on_the_global() {
    let result = probe(
        r#"
        (() => ({
            type: typeof Deno,
            enumerable: Object.getOwnPropertyDescriptor(globalThis, 'Deno')?.enumerable,
        }))()
        "#,
    )
    .await;
    assert_eq!(result["type"], serde_json::json!("object"));
    assert_eq!(result["enumerable"], serde_json::json!(false));
}
