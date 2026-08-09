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
        }))()
        "#,
    )
    .await;

    assert_eq!(result["ctor"], serde_json::json!("Performance"));
    assert_eq!(result["tag"], serde_json::json!("[object Performance]"));
    assert_eq!(result["nowOnPrototype"], serde_json::json!(true));
    assert_eq!(result["nowIsOwn"], serde_json::json!(false));
    assert_eq!(result["nowWorks"], serde_json::json!(true));
}

#[tokio::test(flavor = "current_thread")]
async fn timing_and_navigation_serialize() {
    let result = probe(
        r#"
        (() => ({
            perf: typeof performance.toJSON,
            timingCtor: performance.timing.constructor.name,
            timing: typeof performance.timing.toJSON,
            // Chrome's PerformanceTiming has twenty-one fields. Reporting the
            // three upstream happens to set is itself a tell.
            timingFields: Object.keys(performance.timing.toJSON()).length,
            lateField: typeof performance.timing.domComplete,
            navigationType: performance.navigation.type,
            redirectCount: performance.navigation.redirectCount,
            // The exact call the Ozon challenge makes.
            navigationStart: performance.timing.toJSON().navigationStart > 0,
            // Constructing it from script must throw, as in Chrome.
            illegal: (() => {
                try { new PerformanceTiming(); return 'no throw'; }
                catch (e) { return e.constructor.name; }
            })(),
        }))()
        "#,
    )
    .await;

    assert_eq!(result["perf"], serde_json::json!("function"));
    assert_eq!(result["timing"], serde_json::json!("function"));
    // Note: `performance.navigation` is a plain object here, so it has no
    // toJSON. Chrome exposes a PerformanceNavigation with one. Left as-is
    // because this is the shape c59cd68 shipped and passed with; worth
    // revisiting if a challenge is ever seen calling navigation.toJSON().
    assert_eq!(result["timingCtor"], serde_json::json!("PerformanceTiming"));
    assert_eq!(result["timingFields"].as_f64(), Some(21.0));
    assert_eq!(result["lateField"], serde_json::json!("number"));
    assert_eq!(result["illegal"], serde_json::json!("TypeError"));
    // Compared numerically: V8 hands integers back as either Number(0) or
    // Number(0.0) depending on the value, so json!(0.0) does not always match.
    assert_eq!(result["navigationType"].as_f64(), Some(0.0));
    assert_eq!(result["redirectCount"].as_f64(), Some(0.0));
    assert_eq!(result["navigationStart"], serde_json::json!(true));
}

#[tokio::test(flavor = "current_thread")]
async fn the_interfaces_are_not_enumerable_on_the_global() {
    // Same WebIDL rule the graphics interfaces follow: an interface object that
    // shows up in Object.keys(window) is a one-line detection.
    let result = probe(
        r#"
        (() => ['Performance','PerformanceTiming']
            .filter(n => Object.getOwnPropertyDescriptor(globalThis, n)?.enumerable))()
        "#,
    )
    .await;
    assert_eq!(result, serde_json::json!([]));
}
