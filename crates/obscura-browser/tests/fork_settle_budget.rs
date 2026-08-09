//! Fork-only, and currently `#[ignore]`d: this is the stage 4 acceptance gate,
//! written now because the measurement that motivates it is cheap and offline.
//!
//! `live_product_smoke` is red because Wildberries, Ozon and Avito all answer
//! with a JavaScript challenge, and the challenge never finishes. That is not a
//! fingerprint problem. Measured against a local fixture whose only content is a
//! 10ms `setTimeout` chain, so 200 ticks is about two seconds of page time:
//!
//! | budget | ticks | page time |
//! |---|---|---|
//! | `settle(500)`   | 44 | ~440ms |
//! | `settle(2000)`  | 57 | ~570ms |
//!
//! and through the CLI, where `OBSCURA_DYNAMIC_SCRIPT_SETTLE_MS` of 1000, 5000
//! and 15000 all produced 57, 57 and 58 ticks in ~650ms.
//!
//! So the budget is not ignored outright, but page time plateaus around 600ms
//! however much is asked for: `settle` returns once V8 looks momentarily idle,
//! and a pending timer chain does not count as busy. A proof-of-work needing
//! two seconds cannot finish, whatever the caller asks for.
//!
//! `ce18b78` restructured the settle loop to drive the event loop in slices.
//! When that lands, drop the `#[ignore]` here. Keeping the test ignored rather
//! than red keeps the suite's failure set comparable against the upstream
//! baseline.

use std::sync::Arc;

use obscura_browser::{BrowserContext, Page};

/// A page whose script does nothing but tick a 10ms timer, counting as it goes.
const TICKING_PAGE: &str = "data:text/html,<pre id=out>0</pre><script>\
     var n=0;(function tick(){n++;document.getElementById('out').textContent=String(n);\
     if(n<500)setTimeout(tick,10);})();</script>";

async fn ticks_after_settle(budget_ms: u64) -> u64 {
    let context = Arc::new(BrowserContext::new("fork-settle".to_string()));
    let mut page = Page::new("fork-settle-page".to_string(), context);
    page.navigate(TICKING_PAGE)
        .await
        .expect("the fixture page must load");
    page.settle(budget_ms).await;
    // as_f64, not as_u64: V8 hands integers back as JSON floats, and
    // serde_json's as_u64 returns None for Number(58.0).
    page.evaluate("Number(document.getElementById('out').textContent)")
        .as_f64()
        .unwrap_or(0.0) as u64
}

#[ignore = "stage 4: settle does not yet honour its budget"]
#[tokio::test(flavor = "current_thread")]
async fn settle_runs_timers_for_roughly_its_budget() {
    // At 10ms per tick, two seconds of page time is ~200 ticks. Allow a wide
    // margin for a loaded machine; the failure this guards against is ~57 ticks
    // regardless of budget, which is an order of magnitude away from the floor.
    let ticks = ticks_after_settle(2_000).await;
    assert!(
        ticks > 120,
        "settle(2000) should let a 10ms timer chain run about 200 times, got {ticks}"
    );
}

#[ignore = "stage 4: settle does not yet honour its budget"]
#[tokio::test(flavor = "current_thread")]
async fn a_longer_budget_runs_more_timers() {
    // The specific bug: the budget makes no difference at all. Whatever the
    // absolute numbers, four times the budget must do materially more work.
    let short = ticks_after_settle(500).await;
    let long = ticks_after_settle(2_000).await;
    assert!(
        long > short * 2,
        "settle(2000) should outrun settle(500) by more than 2x, got {long} vs {short}"
    );
}
