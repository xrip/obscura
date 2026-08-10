use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use obscura_js::ops::OriginStorage;
use obscura_net::{CookieJar, ObscuraHttpClient, RobotsCache};

pub struct BrowserContext {
    pub id: String,
    pub cookie_jar: Arc<CookieJar>,
    pub local_storage: Arc<OriginStorage>,
    pub http_client: Arc<ObscuraHttpClient>,
    pub user_agent: String,
    pub platform: String,
    pub ua_platform: String,
    pub ua_platform_version: String,
    pub(crate) fingerprint_profile: Arc<crate::profiles::ResolvedFingerprintProfile>,
    pub proxy_url: Option<String>,
    pub robots_cache: Arc<RobotsCache>,
    pub obey_robots: bool,
    pub stealth: bool,
    /// When true, CDP-driven navigation to file:// URLs is permitted.
    /// Default is false: a remote CDP client cannot point the browser
    /// at /etc/shadow even if Obscura is running as a privileged user.
    /// Flip on via `obscura serve --allow-file-access` for legitimate
    /// local-HTML testing workflows. The CLI's own `obscura fetch
    /// file://...` path is unaffected because it does not go through
    /// the CDP server.
    pub allow_file_access: bool,
    pub storage_dir: Option<PathBuf>,
    /// When true, the http client allows fetching localhost / RFC1918 /
    /// link-local addresses. Set via `--allow-private-network` (issue #33).
    /// Independent of `allow_file_access` because they cover different threat
    /// models: file:// is a local file-system read, while private-network is
    /// the broader SSRF gate from issue #4.
    pub allow_private_network: bool,
}

fn warn_profile_consistency(profile: &crate::profiles::ResolvedFingerprintProfile) {
    let browser_major = profile.browser.major;
    if browser_major == crate::profiles::GRAPHICS_API_BROWSER_MAJOR {
        return;
    }
    static WARNED_MAJORS: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
    let Ok(mut warned) = WARNED_MAJORS
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
    else {
        return;
    };
    if warned.insert(browser_major) {
        tracing::warn!(
            profile_id = profile.id,
            browser_major,
            graphics_api_browser_major = crate::profiles::GRAPHICS_API_BROWSER_MAJOR,
            "selected Chrome profile uses a different browser major than the JS graphics API shape; cross-surface inconsistencies are possible"
        );
    }
}

static WARNED_CUSTOM_USER_AGENT: OnceLock<()> = OnceLock::new();

impl BrowserContext {
    pub fn new(id: String) -> Self {
        Self::_new_inner(id, None, false, None, None, false)
    }

    /// Create a BrowserContext with an optional storage directory.
    /// When `storage_dir` is set, cookies are automatically loaded from
    /// `{storage_dir}/cookies.json` on creation.
    pub fn with_storage(
        id: String,
        storage_dir: Option<PathBuf>,
    ) -> Self {
        Self::_new_inner(id, None, false, None, storage_dir, false)
    }

    /// Create a BrowserContext with full options including storage_dir.
    pub fn with_storage_full(
        id: String,
        proxy_url: Option<String>,
        stealth: bool,
        user_agent: Option<String>,
        storage_dir: Option<PathBuf>,
    ) -> Self {
        Self::_new_inner(id, proxy_url, stealth, user_agent, storage_dir, false)
    }

    /// Variant that also accepts the `allow_private_network` opt-in. All
    /// pre-existing constructors default it to `false`; callers that want the
    /// CLI's `--allow-private-network` (issue #33) behaviour go through here.
    pub fn with_storage_and_network(
        id: String,
        proxy_url: Option<String>,
        stealth: bool,
        user_agent: Option<String>,
        storage_dir: Option<PathBuf>,
        allow_private_network: bool,
    ) -> Self {
        Self::_new_inner(id, proxy_url, stealth, user_agent, storage_dir, allow_private_network)
    }

    fn _new_inner(
        id: String,
        proxy_url: Option<String>,
        stealth: bool,
        user_agent: Option<String>,
        storage_dir: Option<PathBuf>,
        allow_private_network: bool,
    ) -> Self {
        let cookie_jar = Arc::new(CookieJar::new());

        // Restore cookies from disk if storage_dir is configured
        if let Some(ref dir) = storage_dir {
            let cookie_path = dir.join("cookies.json");
            if cookie_path.exists() {
                match cookie_jar.load_from_file(&cookie_path) {
                    Ok(n) if n > 0 => {
                        tracing::info!("Loaded {} cookies from {}", n, cookie_path.display());
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!("Failed to load cookies from {}: {}", cookie_path.display(), e);
                    }
                }
            }
        }

        let mut client = ObscuraHttpClient::with_full_options(
            cookie_jar.clone(),
            proxy_url.as_deref(),
            allow_private_network,
        );
        if stealth {
            client.block_trackers = true;
        }
        let profile = crate::profiles::resolve_profile()
            .unwrap_or_else(|error| panic!("failed to load the browser fingerprint profile: {error}"));
        warn_profile_consistency(&profile);
        if user_agent
            .as_deref()
            .is_some_and(|value| value != profile.browser.user_agent)
            && WARNED_CUSTOM_USER_AGENT.set(()).is_ok()
        {
            tracing::warn!(
                browser_major = profile.browser.major,
                "custom user agent does not match the selected Chrome Windows profile; the caller owns cross-surface consistency"
            );
        }
        let resolved_ua = user_agent.unwrap_or_else(|| profile.browser.user_agent.clone());
        let platform = profile.navigator.platform.clone();
        let ua_platform = profile.navigator.ua_platform.clone();
        let ua_platform_version = profile.navigator.ua_platform_version.clone();
        // Sync the http client's UA at construction so navigation requests pick it
        // up before any async setup runs. The lock has no other holders here, so
        // try_write always succeeds; we fall back silently if it ever fails.
        if let Ok(mut guard) = client.user_agent.try_write() {
            *guard = resolved_ua.clone();
        }
        if let Ok(mut guard) = client.accept_language.try_write() {
            *guard = profile.navigator.accept_language_header();
        }
        let http_client = Arc::new(client);
        BrowserContext {
            id,
            cookie_jar,
            local_storage: Arc::new(OriginStorage::default()),
            http_client,
            user_agent: resolved_ua,
            platform,
            ua_platform,
            ua_platform_version,
            fingerprint_profile: profile,
            proxy_url,
            robots_cache: Arc::new(RobotsCache::new()),
            obey_robots: false,
            stealth,
            allow_file_access: false,
            storage_dir,
            allow_private_network,
        }
    }

    pub fn with_options(id: String, proxy_url: Option<String>, stealth: bool) -> Self {
        Self::with_full_options(id, proxy_url, stealth, None)
    }

    pub fn with_full_options(
        id: String,
        proxy_url: Option<String>,
        stealth: bool,
        user_agent: Option<String>,
    ) -> Self {
        Self::_new_inner(id, proxy_url, stealth, user_agent, None, false)
    }

    pub fn with_proxy(id: String, proxy_url: Option<String>) -> Self {
        Self::with_options(id, proxy_url, false)
    }

    /// Create a context with the same browser configuration but independent
    /// mutable network state. Persistent copies start with the template's
    /// current cookies; incognito copies start empty and never write to the
    /// template's storage directory.
    pub fn isolated_copy(&self, id: String, persistent: bool) -> Self {
        self.isolated_copy_with_profile(id, persistent, self.fingerprint_profile.clone())
    }

    /// Create an isolated copy with one exact catalog profile. This is used by
    /// a root CDP connection before it creates any pages or browser contexts.
    pub fn isolated_copy_with_profile_id(
        &self,
        id: String,
        persistent: bool,
        profile_id: &str,
    ) -> Result<Self, crate::profiles::ProfileError> {
        let profile = crate::profiles::resolve_profile_id(profile_id)?;
        Ok(self.isolated_copy_with_profile(id, persistent, profile))
    }

    fn isolated_copy_with_profile(
        &self,
        id: String,
        persistent: bool,
        profile: Arc<crate::profiles::ResolvedFingerprintProfile>,
    ) -> Self {
        let cookie_jar = Arc::new(CookieJar::new());
        if persistent {
            cookie_jar.set_cookies_from_cdp(self.cookie_jar.get_all_cookies());
        }
        let storage_dir = persistent.then(|| self.storage_dir.clone()).flatten();
        self.copy_with_profile(
            id,
            cookie_jar,
            Arc::new(OriginStorage::default()),
            storage_dir,
            profile,
        )
    }

    /// Replace only the identity of one connection context. Mutable cookie
    /// state stays attached to the connection so its normal persistence merge
    /// still sees later changes.
    pub fn copy_with_profile_id(
        &self,
        profile_id: &str,
    ) -> Result<Self, crate::profiles::ProfileError> {
        let profile = crate::profiles::resolve_profile_id(profile_id)?;
        Ok(self.copy_with_profile(
            self.id.clone(),
            self.cookie_jar.clone(),
            self.local_storage.clone(),
            self.storage_dir.clone(),
            profile,
        ))
    }

    fn copy_with_profile(
        &self,
        id: String,
        cookie_jar: Arc<CookieJar>,
        local_storage: Arc<OriginStorage>,
        storage_dir: Option<PathBuf>,
        profile: Arc<crate::profiles::ResolvedFingerprintProfile>,
    ) -> Self {
        warn_profile_consistency(&profile);
        let user_agent = if self.user_agent == self.fingerprint_profile.browser.user_agent {
            profile.browser.user_agent.clone()
        } else {
            self.user_agent.clone()
        };

        let mut client = ObscuraHttpClient::with_full_options(
            cookie_jar.clone(),
            self.proxy_url.as_deref(),
            self.allow_private_network,
        );
        if self.stealth {
            client.block_trackers = true;
        }
        if let Ok(mut guard) = client.user_agent.try_write() {
            *guard = user_agent.clone();
        }
        if let Ok(mut guard) = client.accept_language.try_write() {
            *guard = profile.navigator.accept_language_header();
        }

        BrowserContext {
            id,
            cookie_jar,
            local_storage,
            http_client: Arc::new(client),
            user_agent,
            platform: profile.navigator.platform.clone(),
            ua_platform: profile.navigator.ua_platform.clone(),
            ua_platform_version: profile.navigator.ua_platform_version.clone(),
            fingerprint_profile: profile,
            proxy_url: self.proxy_url.clone(),
            robots_cache: Arc::new(RobotsCache::new()),
            obey_robots: self.obey_robots,
            stealth: self.stealth,
            allow_file_access: self.allow_file_access,
            storage_dir,
            allow_private_network: self.allow_private_network,
        }
    }

    pub fn profile_id(&self) -> &str {
        &self.fingerprint_profile.id
    }

    pub fn browser_version(&self) -> &str {
        &self.fingerprint_profile.browser.version
    }

    pub fn screen_profile(&self) -> &crate::profiles::ScreenWindowProfile {
        &self.fingerprint_profile.screen
    }

    /// Persist cookies to disk if storage_dir is configured.
    /// Called during graceful shutdown.
    pub fn save_cookies(&self) {
        if let Some(ref dir) = self.storage_dir {
            let _ = std::fs::create_dir_all(dir);
            let cookie_path = dir.join("cookies.json");
            if let Err(e) = self.cookie_jar.save_to_file(&cookie_path) {
                tracing::warn!("Failed to save cookies to {}: {}", cookie_path.display(), e);
            } else {
                tracing::info!("Saved cookies to {}", cookie_path.display());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alternate_profile_id() -> String {
        let index: serde_json::Value = serde_json::from_str(
            &crate::profiles::catalog().unwrap().index_json().unwrap(),
        )
        .unwrap();
        let default_id = index["defaultProfileId"].as_str().unwrap();
        let parts: Vec<&str> = default_id.split(':').collect();
        let graphics_id = index["graphicsProfiles"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|row| {
                let supports_145 = row["observationsByBrowserVersion"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .any(|version| version.starts_with("145."));
                supports_145.then(|| row["id"].as_str()).flatten().filter(|id| *id != parts[2])
            })
            .unwrap();
        format!("{}:{}:{}:{}", parts[0], parts[1], graphics_id, parts[3])
    }

    #[tokio::test(flavor = "current_thread")]
    async fn with_full_options_propagates_user_agent_to_http_client() {
        let ctx = BrowserContext::with_full_options(
            "test".to_string(),
            None,
            false,
            Some("Custom-UA/1.0".to_string()),
        );
        assert_eq!(ctx.user_agent, "Custom-UA/1.0");
        let client_ua = ctx.http_client.user_agent.read().await.clone();
        assert_eq!(client_ua, "Custom-UA/1.0");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn with_full_options_falls_back_to_chrome_default() {
        let ctx = BrowserContext::with_full_options(
            "test".to_string(),
            None,
            false,
            None,
        );
        assert!(ctx.user_agent.contains("Chrome"));
        let client_ua = ctx.http_client.user_agent.read().await.clone();
        assert!(client_ua.contains("Chrome"));
        assert_eq!(ctx.user_agent, client_ua);
        assert_eq!(
            ctx.http_client.accept_language.read().await.as_str(),
            ctx.fingerprint_profile.navigator.accept_language_header()
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn with_options_keeps_default_user_agent() {
        let ctx = BrowserContext::with_options("test".to_string(), None, false);
        assert!(ctx.user_agent.contains("Chrome"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn isolated_copy_does_not_share_mutable_network_state() {
        let source = BrowserContext::with_full_options(
            "source".to_string(),
            None,
            false,
            Some("Template-UA/1.0".to_string()),
        );
        source.cookie_jar.set_cookie("sid=source", &url::Url::parse("https://example.com").unwrap());

        let persistent = source.isolated_copy("persistent".to_string(), true);
        let incognito = source.isolated_copy("incognito".to_string(), false);

        assert_eq!(persistent.cookie_jar.get_all_cookies().len(), 1);
        assert!(incognito.cookie_jar.get_all_cookies().is_empty());
        persistent.cookie_jar.clear();
        persistent.http_client.set_user_agent("Changed-UA/2.0").await;

        assert_eq!(source.cookie_jar.get_all_cookies().len(), 1);
        assert_eq!(source.http_client.user_agent.read().await.as_str(), "Template-UA/1.0");
        assert_eq!(source.profile_id(), persistent.profile_id());
        assert_eq!(source.profile_id(), incognito.profile_id());
        assert!(!Arc::ptr_eq(&source.local_storage, &persistent.local_storage));
        assert!(!Arc::ptr_eq(&source.local_storage, &incognito.local_storage));
        assert!(Arc::ptr_eq(
            &source.fingerprint_profile,
            &persistent.fingerprint_profile
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn isolated_copy_can_select_an_exact_profile_and_keep_a_custom_ua() {
        let profile_id = alternate_profile_id();
        let source = BrowserContext::new("source".to_string());
        let selected = source
            .isolated_copy_with_profile_id("selected".to_string(), true, &profile_id)
            .unwrap();
        assert_eq!(selected.profile_id(), profile_id);
        assert_eq!(
            selected.user_agent,
            selected.http_client.user_agent.read().await.as_str()
        );
        assert_eq!(
            selected.ua_platform_version,
            selected.fingerprint_profile.navigator.ua_platform_version
        );

        let custom = BrowserContext::with_full_options(
            "custom".to_string(),
            None,
            false,
            Some("Custom-UA/1.0".to_string()),
        );
        let custom_selected = custom
            .isolated_copy_with_profile_id("custom-selected".to_string(), true, &profile_id)
            .unwrap();
        assert_eq!(custom_selected.profile_id(), profile_id);
        assert_eq!(custom_selected.user_agent, "Custom-UA/1.0");
        assert_eq!(
            custom_selected.http_client.user_agent.read().await.as_str(),
            "Custom-UA/1.0"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn connection_profile_copy_keeps_cookie_state() {
        let profile_id = alternate_profile_id();
        let source = BrowserContext::new("source".to_string());
        let selected = source.copy_with_profile_id(&profile_id).unwrap();
        assert_eq!(selected.profile_id(), profile_id);
        assert!(Arc::ptr_eq(&source.cookie_jar, &selected.cookie_jar));
        assert!(Arc::ptr_eq(&source.local_storage, &selected.local_storage));

        selected.cookie_jar.set_cookie(
            "sid=selected",
            &url::Url::parse("https://example.com").unwrap(),
        );
        assert_eq!(source.cookie_jar.get_all_cookies().len(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn default_profile_is_coherent_and_stable_across_contexts() {
        let first = BrowserContext::new("first".to_string());
        let second = BrowserContext::new("second".to_string());
        assert_eq!(first.profile_id(), second.profile_id());
        assert!(first.profile_id().starts_with("c145w1:"));
        assert!(first.user_agent.contains("Chrome/145.0.0.0"));
        assert_eq!(first.platform, "Win32");
        assert_eq!(first.ua_platform, "Windows");
        assert!(first.fingerprint_profile.browser.version.starts_with("145."));
        assert_eq!(first.user_agent, first.http_client.user_agent.read().await.as_str());
    }
}
