#![cfg(feature = "stealth")]

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use obscura_browser::profiles;
use obscura_browser::{BrowserContext, Page, WaitUntil};
use serde_json::Value;

const DEFAULT_PROFILE: &str =
    "c145w1:d2e85f68f4092704b75e2a9fe7145fd7:8546ea21d519f56b82c1099e4893e49b:be830914f3fb0f9eb2577321d1d4a9fa";

struct ProductCase {
    name: &'static str,
    url: &'static str,
    product_id: &'static str,
    product_marker: &'static str,
}

const CASES: &[ProductCase] = &[
    ProductCase {
        name: "wildberries",
        url: "https://www.wildberries.ru/catalog/1193913221/detail.aspx?targetUrl=MI",
        product_id: "1193913221",
        product_marker: "geforce rtx5060 ti",
    },
    ProductCase {
        name: "ozon",
        url: "https://www.ozon.ru/product/ochistitel-vyhlopnoy-sistemy-rastvor-mocheviny-adblue-10-l-sintec-804-1902651403",
        product_id: "1902651403",
        product_marker: "adblue",
    },
];

fn product_eval(product_id: &str, product_marker: &str) -> String {
    format!(
        r#"(function() {{
            var expectedId = "{product_id}";
            var expectedMarker = "{product_marker}";
            function clean(value) {{
                return value == null ? "" : String(value).replace(/\s+/g, " ").trim();
            }}
            function findProduct(value) {{
                if (!value || typeof value !== "object") return null;
                if (Array.isArray(value)) {{
                    for (var i = 0; i < value.length; i++) {{
                        var item = findProduct(value[i]);
                        if (item) return item;
                    }}
                    return null;
                }}
                var type = value["@type"];
                if (type === "Product" || (Array.isArray(type) && type.indexOf("Product") >= 0)) {{
                    return value;
                }}
                return findProduct(value["@graph"]);
            }}
            var product = null;
            var jsonLd = document.querySelectorAll("script[type=\"application/ld+json\"]");
            for (var i = 0; i < jsonLd.length && !product; i++) {{
                try {{ product = findProduct(JSON.parse(jsonLd[i].textContent)); }} catch (e) {{}}
            }}
            product = product || {{}};
            var offers = Array.isArray(product.offers) ? (product.offers[0] || {{}}) : (product.offers || {{}});
            var h1 = document.querySelector("h1");
            var ogTitle = document.querySelector("meta[property=\"og:title\"]");
            var priceNode = document.querySelector("[itemprop=\"price\"]");
            var priceMeta = document.querySelector("meta[property=\"product:price:amount\"]");
            var currencyMeta = document.querySelector("meta[property=\"product:price:currency\"]");
            var body = clean(document.body && document.body.innerText);
            var blockSurface = clean(document.title) + " " + clean(h1 && h1.innerText);
            return {{
                url: location.href,
                title: clean(product.name) || clean(h1 && h1.innerText) || clean(ogTitle && ogTitle.content),
                price: clean(offers.price) || clean(priceNode && (priceNode.content || priceNode.textContent)) || clean(priceMeta && priceMeta.content),
                currency: clean(offers.priceCurrency) || clean(currencyMeta && currencyMeta.content),
                productId: clean(product.sku || product.mpn || product.productID),
                hasExpectedId: body.indexOf(expectedId) >= 0,
                hasProductMarker: body.toLowerCase().indexOf(expectedMarker) >= 0,
                hasPurchaseControl: /купить|в корзину|add to cart/i.test(body),
                blocked: /captcha|robot|доступ ограничен|не робот/i.test(blockSurface),
                bodySample: body.slice(0, 1200)
            }};
        }})()"#
    )
}

fn product_card_error(case: &ProductCase, value: Value) -> Option<String> {
    let title = value["title"].as_str().unwrap_or_default();
    let rendered_card = value["hasExpectedId"].as_bool().unwrap_or(false)
        && value["hasProductMarker"].as_bool().unwrap_or(false)
        && value["hasPurchaseControl"].as_bool().unwrap_or(false);
    let generic_title = title.to_ascii_lowercase().contains("internet shop")
        || title.contains("широкий ассортимент")
        || title.contains("интернет-магазин")
        || title.contains("главная страница");

    if title.trim().is_empty() && !rendered_card {
        return Some(format!("{} returned no product title: {value}", case.name));
    }
    if generic_title && !rendered_card {
        return Some(format!(
            "{} returned a generic page, not a product card: {value}",
            case.name
        ));
    }
    if value["blocked"].as_bool().unwrap_or(false) {
        return Some(format!(
            "{} returned a challenge or blocked page: {value}",
            case.name
        ));
    }
    if !value["hasExpectedId"].as_bool().unwrap_or(false) {
        return Some(format!(
            "{} product id {} was not found in the rendered card: {value}",
            case.name, case.product_id
        ));
    }
    if !value["hasProductMarker"].as_bool().unwrap_or(false) {
        return Some(format!(
            "{} product marker {} was not found in the rendered card: {value}",
            case.name, case.product_marker
        ));
    }
    None
}

async fn wait_for_product_card(page: &mut Page, case: &ProductCase) -> Result<Value, String> {
    let started = Instant::now();
    let timeout = Duration::from_secs(60);
    let mut task_budget_errors = 0;

    loop {
        let value = page.evaluate(&product_eval(case.product_id, case.product_marker));
        if product_card_error(case, value.clone()).is_none() || started.elapsed() >= timeout {
            return Ok(value);
        }

        page.process_pending_navigation()
            .await
            .map_err(|error| format!("{} challenge navigation failed: {error}", case.name))?;
        let reached_idle = match page.run_autonomous_event_loop_turn().await {
            Ok(reached_idle) => {
                task_budget_errors = 0;
                reached_idle
            }
            Err(error)
                if error.contains("exceeded its task budget") && task_budget_errors < 3 =>
            {
                task_budget_errors += 1;
                false
            }
            Err(error) => {
                return Err(format!("{} challenge task failed: {error}", case.name));
            }
        };
        page.process_pending_navigation()
            .await
            .map_err(|error| format!("{} challenge navigation failed: {error}", case.name))?;
        if reached_idle {
            tokio::task::yield_now().await;
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn live_product_cards_load_with_the_selected_profile() {
    if let Ok(runtime_dir) = std::env::var("OBSCURA_PROFILE_RUNTIME_DIR") {
        profiles::load_runtime_profiles(Path::new(&runtime_dir))
            .unwrap_or_else(|error| panic!("cannot load runtime profiles from {runtime_dir}: {error}"));
    }
    let profile = std::env::var("OBSCURA_TEST_PROFILE")
        .unwrap_or_else(|_| DEFAULT_PROFILE.to_string());
    profiles::resolve_profile_id(&profile)
        .unwrap_or_else(|error| panic!("profile {profile} is not available: {error}"));
    // BrowserContext reads the same selector when it builds the fingerprint.
    unsafe { std::env::set_var("OBSCURA_PROFILE", &profile); }
    let proxy = std::env::var("OBSCURA_PROXY").ok();

    let context = Arc::new(BrowserContext::with_full_options(
        "live-product-smoke".to_string(),
        proxy,
        true,
        None,
    ));
    let mut page = Page::new("live-product-smoke-page".to_string(), context);
    page.set_navigation_timeout(Duration::from_secs(90));
    let mut failures = Vec::new();

    for case in CASES {
        let navigation = tokio::time::timeout(
            Duration::from_secs(90),
            page.navigate_with_wait(case.url, WaitUntil::Load),
        )
        .await;
        match navigation {
            Err(_) => {
                failures.push(format!("{} navigation timed out", case.name));
                continue;
            }
            Ok(Err(error)) => {
                failures.push(format!("{} navigation failed: {error}", case.name));
                continue;
            }
            Ok(Ok(())) => {}
        }

        let value = match wait_for_product_card(&mut page, case).await {
            Ok(value) => value,
            Err(error) => {
                failures.push(error);
                continue;
            }
        };
        if let Some(error) = product_card_error(case, value) {
            failures.push(error);
        }
    }

    assert!(failures.is_empty(), "product smoke failures:\n{}", failures.join("\n"));
}
