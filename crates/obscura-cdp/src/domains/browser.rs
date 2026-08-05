use serde_json::{json, Value};

use crate::dispatch::CdpContext;

pub async fn handle(method: &str, _params: &Value, ctx: &CdpContext) -> Result<Value, String> {
    let screen = ctx.default_context.screen_profile();
    match method {
        "getVersion" => Ok(json!({
            "protocolVersion": "1.3",
            "product": format!("Chrome/{}", ctx.default_context.browser_version()),
            "revision": "@0000000000000000000000000000000000000000",
            "userAgent": ctx.default_context.user_agent,
            "jsVersion": "14.5.0.0",
        })),
        "close" => {
            Ok(json!({}))
        }
        "getWindowForTarget" => Ok(json!({
            "windowId": 1,
            "bounds": {
                "left": screen.screen_x,
                "top": screen.screen_y,
                "width": screen.outer_width,
                "height": screen.outer_height,
                "windowState": "normal",
            }
        })),
        "setDownloadBehavior" => Ok(json!({})),
        "getWindowBounds" => Ok(json!({
            "bounds": {
                "left": screen.screen_x,
                "top": screen.screen_y,
                "width": screen.outer_width,
                "height": screen.outer_height,
                "windowState": "normal"
            }
        })),
        // No-op acks for window-management methods Playwright sends during
        // page setup. We don't model real OS windows, but answering with {}
        // lets the client's setup sequence complete instead of tearing down
        // the page on an unknown-method error.
        "setWindowBounds" => Ok(json!({})),
        _ => Err(format!("Unknown Browser method: {}", method)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn version_and_bounds_follow_the_connection_profile() {
        let ctx = CdpContext::new();
        let version = handle("getVersion", &json!({}), &ctx).await.unwrap();
        assert_eq!(version["userAgent"], ctx.default_context.user_agent);
        assert_eq!(
            version["product"],
            format!("Chrome/{}", ctx.default_context.browser_version())
        );

        let bounds = handle("getWindowBounds", &json!({}), &ctx).await.unwrap();
        let screen = ctx.default_context.screen_profile();
        assert_eq!(bounds["bounds"]["left"], screen.screen_x);
        assert_eq!(bounds["bounds"]["top"], screen.screen_y);
        assert_eq!(bounds["bounds"]["width"], screen.outer_width);
        assert_eq!(bounds["bounds"]["height"], screen.outer_height);
    }
}
