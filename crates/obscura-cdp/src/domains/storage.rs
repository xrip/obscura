use serde_json::{json, Value};

use crate::cookie_params::{parse_cdp_cookie, parse_delete_cookies_params};
use crate::dispatch::CdpContext;
use crate::domains::network::cookie_info_to_cdp_json;

fn cookie_jar_for(
    ctx: &CdpContext,
    params: &Value,
    session_id: &Option<String>,
) -> Result<std::sync::Arc<obscura_net::CookieJar>, String> {
    match params.get("browserContextId").and_then(|value| value.as_str()) {
        Some(id) => ctx
            .browser_context(id)
            .map(|context| context.cookie_jar.clone())
            .ok_or_else(|| format!("Browser context not found: {}", id)),
        None => Ok(ctx
            .get_session_page(session_id)
            .map(|page| page.context.cookie_jar.clone())
            .unwrap_or_else(|| ctx.default_context.cookie_jar.clone())),
    }
}

pub async fn handle(
    method: &str,
    params: &Value,
    ctx: &mut CdpContext,
    session_id: &Option<String>,
) -> Result<Value, String> {
    match method {
        "getCookies" => {
            let cookies = cookie_jar_for(ctx, params, session_id)?.get_all_cookies();
            let cdp_cookies: Vec<Value> = cookies.iter().map(cookie_info_to_cdp_json).collect();
            Ok(json!({ "cookies": cdp_cookies }))
        }
        "setCookies" => {
            if let Some(cookies) = params.get("cookies").and_then(|v| v.as_array()) {
                let parsed: Vec<_> = cookies.iter().filter_map(parse_cdp_cookie).collect();
                cookie_jar_for(ctx, params, session_id)?.set_cookies_from_cdp(parsed);
            }
            Ok(json!({}))
        }
        "clearCookies" => {
            cookie_jar_for(ctx, params, session_id)?.clear();
            Ok(json!({}))
        }
        "deleteCookies" => {
            if let Some(filter) = parse_delete_cookies_params(params) {
                cookie_jar_for(ctx, params, session_id)?.delete_cookies_filtered(
                    &filter.name,
                    &filter.domain,
                    filter.path.as_deref(),
                );
            }
            Ok(json!({}))
        }
        _ => Ok(json!({})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn playwright_cookie_methods_are_scoped_to_browser_context() {
        let mut ctx = CdpContext::new();
        let browser_context_id = ctx.create_browser_context();
        let params = json!({
            "browserContextId": browser_context_id,
            "cookies": [{
                "name": "sid",
                "value": "account",
                "domain": "example.com",
                "path": "/",
                "expires": -1,
                "httpOnly": true,
                "secure": true,
                "sameSite": "Lax"
            }]
        });
        handle("setCookies", &params, &mut ctx, &None).await.unwrap();

        let result = handle(
            "getCookies",
            &json!({ "browserContextId": browser_context_id }),
            &mut ctx,
            &None,
        )
        .await
        .unwrap();
        assert_eq!(result["cookies"].as_array().unwrap().len(), 1);
        assert!(ctx.default_context.cookie_jar.get_all_cookies().is_empty());

        handle(
            "clearCookies",
            &json!({ "browserContextId": browser_context_id }),
            &mut ctx,
            &None,
        )
        .await
        .unwrap();
        let context = ctx.browser_context(&browser_context_id).unwrap();
        assert!(context.cookie_jar.get_all_cookies().is_empty());
    }
}
