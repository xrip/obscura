use serde_json::{json, Value};

use crate::dispatch::CdpContext;

pub async fn handle(
    method: &str,
    params: &Value,
    ctx: &mut CdpContext,
    session_id: &Option<String>,
) -> Result<Value, String> {
    match method {
        "setProfile" => {
            if let Some(session_id) = session_id {
                if ctx.sessions.get(session_id).map(String::as_str) != Some("browser") {
                    return Err(
                        "Obscura.setProfile is available only on the root CDP connection or its browser session"
                            .to_string(),
                    );
                }
            }
            let profile_id = params
                .get("profileId")
                .and_then(Value::as_str)
                .ok_or_else(|| "profileId required".to_string())?;
            let profile_id = ctx.set_profile(profile_id)?;
            Ok(json!({ "profileId": profile_id }))
        }
        _ => Err(format!("Unknown Obscura method: {method}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch::dispatch;
    use crate::types::CdpRequest;

    fn profile_ids() -> (String, String) {
        let index: Value = serde_json::from_str(
            &obscura_browser::profiles::catalog()
                .unwrap()
                .index_json()
                .unwrap(),
        )
        .unwrap();
        let default_id = index["defaultProfileId"].as_str().unwrap().to_string();
        let parts: Vec<&str> = default_id.split(':').collect();
        let graphics_id = index["graphicsProfiles"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|row| row["id"].as_str().filter(|id| *id != parts[2]))
            .unwrap();
        let alternate = format!("c145w1:{}:{}:{}", parts[1], graphics_id, parts[3]);
        (default_id, alternate)
    }

    fn request(profile_id: &str, session_id: Option<&str>) -> CdpRequest {
        CdpRequest {
            id: 1,
            method: "Obscura.setProfile".to_string(),
            params: json!({ "profileId": profile_id }),
            session_id: session_id.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn profile_selection_is_connection_local_and_inherited() {
        let (_, alternate) = profile_ids();
        let left = CdpContext::new();
        let left_id = left.default_context.profile_id().to_string();
        let mut right = CdpContext::new();

        let response = dispatch(&request(&alternate, None), &mut right).await;
        assert!(response.error.is_none());
        assert_eq!(response.result.unwrap()["profileId"], alternate);
        assert_eq!(right.default_context.profile_id(), alternate);
        assert_eq!(left.default_context.profile_id(), left_id);

        let browser_context_id = right.create_browser_context();
        assert_eq!(
            right
                .browser_context(&browser_context_id)
                .unwrap()
                .profile_id(),
            alternate
        );
    }

    #[tokio::test]
    async fn browser_session_can_set_profile_but_page_session_cannot() {
        let (_, alternate) = profile_ids();
        let mut browser = CdpContext::new();
        browser
            .sessions
            .insert("browser-session".to_string(), "browser".to_string());
        let response = dispatch(&request(&alternate, Some("browser-session")), &mut browser).await;
        assert!(response.error.is_none());
        assert_eq!(browser.default_context.profile_id(), alternate);

        let mut page = CdpContext::new();
        page.sessions
            .insert("page-session".to_string(), "page-1".to_string());
        let response = dispatch(&request(&alternate, Some("page-session")), &mut page).await;
        assert!(response
            .error
            .unwrap()
            .message
            .contains("root CDP connection"));
    }

    #[tokio::test]
    async fn profile_selection_rejects_late_and_unknown_ids() {
        let (_, alternate) = profile_ids();
        let mut late = CdpContext::new();
        late.create_browser_context();
        let response = dispatch(&request(&alternate, None), &mut late).await;
        assert!(response
            .error
            .unwrap()
            .message
            .contains("before the first page"));

        let mut unknown = CdpContext::new();
        let initial = unknown.default_context.profile_id().to_string();
        let response = dispatch(&request("c145w1:bad:bad:bad", None), &mut unknown).await;
        assert!(response
            .error
            .unwrap()
            .message
            .contains("invalid fingerprint profile ID"));
        assert_eq!(unknown.default_context.profile_id(), initial);
    }
}
