#[cfg(feature = "stealth")]
use std::collections::{HashMap, HashSet};
#[cfg(feature = "stealth")]
use std::error::Error;
#[cfg(feature = "stealth")]
use std::sync::{Arc, Mutex, OnceLock};
#[cfg(feature = "stealth")]
use std::time::Duration;

#[cfg(feature = "stealth")]
use tokio::sync::RwLock;
#[cfg(feature = "stealth")]
use url::Url;

#[cfg(feature = "stealth")]
use crate::cookies::CookieJar;
#[cfg(feature = "stealth")]
use crate::client::{CallbackRegistry, RequestInfo, ResourceType, Response, ObscuraNetError};

// The wreq emulation (Profile::Chrome145, Platform::Windows) sends this exact
// UA and sec-ch-ua-platform "Windows" on the wire. navigator has to report the
// same identity, otherwise the TLS/HTTP layer and the JS layer disagree and a
// site cross-checks the mismatch as a bot signal.
#[cfg(feature = "stealth")]
pub const STEALTH_NAVIGATOR_PLATFORM: &str = "Win32";
#[cfg(feature = "stealth")]
pub const STEALTH_UA_PLATFORM: &str = "Windows";
#[cfg(feature = "stealth")]
pub const STEALTH_UA_PLATFORM_VERSION: &str = "15.0.0";

#[cfg(feature = "stealth")]
const CHROME_TRANSPORT_PROFILES: &[(u32, wreq_util::Profile)] = &[
    (100, wreq_util::Profile::Chrome100),
    (101, wreq_util::Profile::Chrome101),
    (104, wreq_util::Profile::Chrome104),
    (105, wreq_util::Profile::Chrome105),
    (106, wreq_util::Profile::Chrome106),
    (107, wreq_util::Profile::Chrome107),
    (108, wreq_util::Profile::Chrome108),
    (109, wreq_util::Profile::Chrome109),
    (110, wreq_util::Profile::Chrome110),
    (114, wreq_util::Profile::Chrome114),
    (116, wreq_util::Profile::Chrome116),
    (117, wreq_util::Profile::Chrome117),
    (118, wreq_util::Profile::Chrome118),
    (119, wreq_util::Profile::Chrome119),
    (120, wreq_util::Profile::Chrome120),
    (123, wreq_util::Profile::Chrome123),
    (124, wreq_util::Profile::Chrome124),
    (126, wreq_util::Profile::Chrome126),
    (127, wreq_util::Profile::Chrome127),
    (128, wreq_util::Profile::Chrome128),
    (129, wreq_util::Profile::Chrome129),
    (130, wreq_util::Profile::Chrome130),
    (131, wreq_util::Profile::Chrome131),
    (132, wreq_util::Profile::Chrome132),
    (133, wreq_util::Profile::Chrome133),
    (134, wreq_util::Profile::Chrome134),
    (135, wreq_util::Profile::Chrome135),
    (136, wreq_util::Profile::Chrome136),
    (137, wreq_util::Profile::Chrome137),
    (138, wreq_util::Profile::Chrome138),
    (139, wreq_util::Profile::Chrome139),
    (140, wreq_util::Profile::Chrome140),
    (141, wreq_util::Profile::Chrome141),
    (142, wreq_util::Profile::Chrome142),
    (143, wreq_util::Profile::Chrome143),
    (144, wreq_util::Profile::Chrome144),
    (145, wreq_util::Profile::Chrome145),
    (146, wreq_util::Profile::Chrome146),
    (147, wreq_util::Profile::Chrome147),
    (148, wreq_util::Profile::Chrome148),
];

#[cfg(feature = "stealth")]
fn chrome_transport_profile(browser_major: u32) -> (u32, wreq_util::Profile) {
    CHROME_TRANSPORT_PROFILES
        .iter()
        .copied()
        .min_by_key(|(major, _)| major.abs_diff(browser_major))
        .expect("wreq Chrome transport profile table is not empty")
}

#[cfg(feature = "stealth")]
fn warn_transport_mismatch_once(browser_major: u32, transport_major: u32) {
    static WARNED: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
    let Ok(mut warned) = WARNED.get_or_init(|| Mutex::new(HashSet::new())).lock() else {
        return;
    };
    if warned.insert(browser_major) {
        tracing::warn!(
            browser_major,
            transport_major,
            "selected Chrome profile has no exact wreq transport; using the nearest transport profile"
        );
    }
}

#[cfg(feature = "stealth")]
struct StealthSsrfResolver {
    allow_private_network: bool,
}

#[cfg(feature = "stealth")]
impl StealthSsrfResolver {
    fn new(allow_private_network: bool) -> Self {
        Self { allow_private_network }
    }
}

#[cfg(feature = "stealth")]
impl wreq::dns::Resolve for StealthSsrfResolver {
    fn resolve(&self, name: wreq::dns::Name) -> wreq::dns::Resolving {
        let allow_private_network =
            self.allow_private_network || crate::client::env_allows_private_network();
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addresses: Vec<std::net::SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            if !allow_private_network {
                if let Some(address) = addresses
                    .iter()
                    .find(|address| crate::client::is_forbidden_ip(address.ip()))
                {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!("SSRF blocked: '{host}' resolves to forbidden address {address}"),
                    )
                    .into());
                }
            }
            Ok(Box::new(addresses.into_iter()) as wreq::dns::Addrs)
        })
    }
}

#[cfg(feature = "stealth")]
pub struct StealthHttpClient {
    client: wreq::Client,
    user_agent: String,
    sec_ch_ua: String,
    sec_ch_ua_platform: String,
    accept_encoding: String,
    allow_private_network: bool,
    transport_browser_major: u32,
    pub cookie_jar: Arc<CookieJar>,
    pub extra_headers: RwLock<HashMap<String, String>>,
    pub in_flight: Arc<std::sync::atomic::AtomicU32>,
}

#[cfg(feature = "stealth")]
impl StealthHttpClient {
    pub fn new(cookie_jar: Arc<CookieJar>) -> Self {
        Self::with_browser_identity(cookie_jar, None, "", "", "", 0, false)
    }

    pub fn with_proxy(cookie_jar: Arc<CookieJar>, proxy_url: Option<&str>) -> Self {
        Self::with_browser_identity(cookie_jar, proxy_url, "", "", "", 0, false)
    }

    pub fn with_proxy_and_user_agent(
        cookie_jar: Arc<CookieJar>,
        proxy_url: Option<&str>,
        user_agent: &str,
    ) -> Self {
        let (sec_ch_ua, sec_ch_ua_platform) = crate::client::chrome_client_hints(user_agent);
        let browser_major = user_agent
            .split("Chrome/")
            .nth(1)
            .and_then(|value| value.split('.').next())
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0);
        Self::with_browser_identity(
            cookie_jar,
            proxy_url,
            user_agent,
            &sec_ch_ua,
            &sec_ch_ua_platform,
            browser_major,
            false,
        )
    }

    pub fn with_browser_identity(
        cookie_jar: Arc<CookieJar>,
        proxy_url: Option<&str>,
        user_agent: &str,
        sec_ch_ua: &str,
        sec_ch_ua_platform: &str,
        browser_major: u32,
        allow_private_network: bool,
    ) -> Self {
        let (transport_browser_major, transport_profile) = chrome_transport_profile(browser_major);
        if transport_browser_major != browser_major {
            warn_transport_mismatch_once(browser_major, transport_browser_major);
        }
        let emulation_opts = wreq_util::Emulation::builder()
            .profile(transport_profile)
            .platform(wreq_util::Platform::Windows)
            .build();
        let accept_encoding = {
            use wreq::IntoEmulation as _;
            emulation_opts
                .clone()
                .into_emulation()
                .headers
                .get(wreq::header::ACCEPT_ENCODING)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned()
        };

        let mut builder = wreq::Client::builder()
            .no_proxy()
            .emulation(emulation_opts)
            .timeout(Duration::from_secs(30))
            .redirect(wreq::redirect::Policy::none())
            .dns_resolver(StealthSsrfResolver::new(allow_private_network));

        // Honor SSL_CERT_FILE / SSL_CERT_DIR in the stealth client too.
        //
        // `client.rs` (the reqwest path) already reads these via `configured_root_paths()` and
        // feeds `add_root_certificate`, so a private CA works there. This client did not, which
        // made the *better-fingerprinted* transport the only one unable to reach hosts behind a
        // private/national CA (measured against a Brazilian government portal whose leaf is
        // issued by an ICP-Brasil intermediate: `--stealth` failed with CERTIFICATE_VERIFY_FAILED
        // while the reqwest path, with SSL_CERT_FILE set, completed the handshake).
        //
        // Two deliberate constraints:
        //
        // 1. `tls_cert_store` is used, NOT `tls_options`. `emulation()` overwrites `tls_options`
        //    wholesale ("This will overwrite the existing configuration"), so setting TLS options
        //    here would silently discard the Chrome fingerprint — the whole point of this client.
        //    `tls_cert_store` is a separate field on the config and composes with emulation.
        //
        // 2. Opt-in only. Supplying a store REPLACES the webpki roots (see `set_cert_store` in
        //    `tls/conn/ext.rs`), it does not add to them. Applying it unconditionally would break
        //    every ordinary site whenever the bundle is incomplete. With neither variable set,
        //    behaviour is byte-for-byte what it was before.
        if std::env::var_os("SSL_CERT_FILE").is_some()
            || std::env::var_os("SSL_CERT_DIR").is_some()
        {
            match wreq::tls::trust::CertStore::builder().set_default_paths().build() {
                Ok(store) => builder = builder.tls_cert_store(store),
                Err(error) => tracing::warn!(
                    %error,
                    "SSL_CERT_FILE/SSL_CERT_DIR set but the certificate store failed to build; \
                     continuing with the default roots"
                ),
            }
        }

        if let Some(proxy) = proxy_url {
            if let Ok(p) = wreq::Proxy::all(proxy) {
                builder = builder.proxy(p);
            }
        }

        let client = builder.build().expect("failed to build wreq stealth client");

        StealthHttpClient {
            client,
            user_agent: user_agent.to_owned(),
            sec_ch_ua: sec_ch_ua.to_owned(),
            sec_ch_ua_platform: sec_ch_ua_platform.to_owned(),
            accept_encoding,
            allow_private_network,
            transport_browser_major,
            cookie_jar,
            extra_headers: RwLock::new(HashMap::new()),
            in_flight: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        }
    }

    pub async fn fetch(&self, url: &Url) -> Result<Response, ObscuraNetError> {
        self.fetch_with_context(url, None, None, ResourceType::Document).await
    }

    pub async fn fetch_with_referrer(
        &self,
        url: &Url,
        referrer: Option<&Url>,
    ) -> Result<Response, ObscuraNetError> {
        self.fetch_with_context(url, referrer, None, ResourceType::Document)
            .await
    }

    pub async fn fetch_with_context(
        &self,
        url: &Url,
        referrer: Option<&Url>,
        callbacks: Option<&CallbackRegistry>,
        resource_type: ResourceType,
    ) -> Result<Response, ObscuraNetError> {
        self.fetch_with_context_policy(url, referrer, callbacks, resource_type, false, false)
            .await
    }

    /// Fetch a browser subresource while preserving the full HTTPS referrer.
    /// Some pages opt into `no-referrer-when-downgrade`, which Chrome exposes
    /// on cross-origin HTTPS script requests instead of reducing it to the
    /// origin. The caller must use this only when that policy was observed.
    pub async fn fetch_with_full_referrer(
        &self,
        url: &Url,
        referrer: Option<&Url>,
        callbacks: Option<&CallbackRegistry>,
        resource_type: ResourceType,
    ) -> Result<Response, ObscuraNetError> {
        self.fetch_with_context_policy(url, referrer, callbacks, resource_type, true, false)
            .await
    }

    /// Fetch a dynamically inserted script or stylesheet with the identity
    /// and full referrer Chrome exposes for the resource. Keep the request
    /// metadata minimal because the browser request observer does not expose
    /// all transport-managed headers for these loads.
    pub async fn fetch_dynamic_subresource(
        &self,
        url: &Url,
        referrer: Option<&Url>,
        callbacks: Option<&CallbackRegistry>,
        resource_type: ResourceType,
    ) -> Result<Response, ObscuraNetError> {
        self.fetch_with_context_policy(url, referrer, callbacks, resource_type, true, true)
            .await
    }

    async fn fetch_with_context_policy(
        &self,
        url: &Url,
        referrer: Option<&Url>,
        callbacks: Option<&CallbackRegistry>,
        resource_type: ResourceType,
        preserve_full_referrer: bool,
        minimal_subresource_headers: bool,
    ) -> Result<Response, ObscuraNetError> {
        crate::client::validate_url(url, self.allow_private_network)?;
        let mut current_url = url.clone();
        let mut redirects = Vec::new();

        for _ in 0..20 {
            crate::client::validate_url(&current_url, self.allow_private_network)?;
            if let Some(host) = current_url.host_str() {
                if crate::blocklist::is_blocked(host) {
                    tracing::debug!("Blocked tracker: {}", current_url);
                    return Ok(Response {
                        status: 0,
                        url: current_url,
                        headers: HashMap::new(),
                        body: Vec::new(),
                        redirected_from: Vec::new(),
                    });
                }
            }

            let referrer = referrer.and_then(|value| {
                if preserve_full_referrer {
                    if value.scheme() == "https" && current_url.scheme() == "http" {
                        None
                    } else {
                        let mut value = value.clone();
                        value.set_fragment(None);
                        Some(value)
                    }
                } else {
                    referrer_for_target(value, &current_url)
                }
            });
            let extra_headers = self.extra_headers.read().await.clone();
            let cookie_header = self.cookie_jar.get_cookie_header(&current_url);
            let mut callback_headers = extra_headers.clone();
            if !self.user_agent.is_empty() {
                callback_headers.insert("user-agent".to_string(), self.user_agent.clone());
            }
            if !self.sec_ch_ua.is_empty() {
                callback_headers.insert("sec-ch-ua".to_string(), self.sec_ch_ua.clone());
            }
            if !self.sec_ch_ua_platform.is_empty() {
                callback_headers.insert(
                    "sec-ch-ua-platform".to_string(),
                    self.sec_ch_ua_platform.clone(),
                );
            }
            if !self.accept_encoding.is_empty() {
                callback_headers
                    .insert("accept-encoding".to_string(), self.accept_encoding.clone());
            }
            if let Some(referrer) = referrer.as_ref() {
                callback_headers.insert("referer".to_string(), referrer.to_string());
            }
            if !cookie_header.is_empty() {
                callback_headers.insert("cookie".to_string(), cookie_header.clone());
            }
            let request_info = RequestInfo {
                url: current_url.clone(),
                method: "GET".to_string(),
                headers: callback_headers,
                resource_type: resource_type.clone(),
            };
            if let Some(callbacks) = callbacks {
                callbacks.fire_request(&request_info).await;
            }

            let mut req = self
                .client
                .get(current_url.as_str())
                .default_headers(false);
            if !self.accept_encoding.is_empty() {
                req = req.header("accept-encoding", self.accept_encoding.as_str());
            }
            if !minimal_subresource_headers {
                let (accept, fetch_mode, fetch_dest) =
                    crate::client::resource_request_headers(resource_type);
                let fetch_site = crate::client::fetch_site_for_target(referrer.as_ref(), &current_url);
                req = req
                    .header("accept", accept)
                    .header("accept-language", "en-US,en;q=0.9")
                    .header("sec-fetch-site", fetch_site)
                    .header("sec-fetch-mode", fetch_mode)
                    .header("sec-fetch-dest", fetch_dest);
            }
            if !self.user_agent.is_empty() {
                req = req.header("user-agent", self.user_agent.as_str());
            }
            if !self.sec_ch_ua.is_empty() {
                req = req
                    .header("sec-ch-ua", self.sec_ch_ua.as_str())
                    .header("sec-ch-ua-mobile", "?0");
            }
            if !self.sec_ch_ua_platform.is_empty() {
                req = req.header("sec-ch-ua-platform", self.sec_ch_ua_platform.as_str());
            }
            if resource_type == ResourceType::Document {
                req = req
                    .header("upgrade-insecure-requests", "1")
                    .header("sec-fetch-user", "?1");
            }
            if let Some(referrer) = referrer.as_ref() {
                req = req.header("referer", referrer.as_str());
            }

            if !cookie_header.is_empty() {
                req = req.header("Cookie", &cookie_header);
            }

            for (k, v) in &extra_headers {
                req = req.header(k.as_str(), v.as_str());
            }

            self.in_flight.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let resp = req.send().await.map_err(|e| {
                self.in_flight.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                ObscuraNetError::Network(format!("{}: {} (source: {:?})", current_url, e, e.source()))
            })?;
            self.in_flight.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            let status = resp.status();

            for val in resp.headers().get_all("set-cookie") {
                if let Ok(s) = val.to_str() {
                    self.cookie_jar.set_cookie(s, &current_url);
                }
            }

            let response_headers: HashMap<String, String> = resp
                .headers()
                .iter()
                .map(|(k, v)| (k.as_str().to_lowercase(), v.to_str().unwrap_or("").to_string()))
                .collect();

            if status.is_redirection() {
                if let Some(location) = resp.headers().get("location") {
                    let location_str = location.to_str().map_err(|_| {
                        ObscuraNetError::Network("Invalid redirect Location".into())
                    })?;
                    let next_url = current_url.join(location_str).map_err(|e| {
                        ObscuraNetError::Network(format!("Invalid redirect URL: {}", e))
                    })?;
                    if next_url.scheme() == "file"
                        && current_url.scheme() != "file"
                        && resource_type != ResourceType::Document
                        && referrer.is_some_and(|value| value.scheme() != "file")
                    {
                        return Err(ObscuraNetError::Network(
                            "Cross-scheme redirect to file is not allowed for a subresource"
                                .into(),
                        ));
                    }
                    crate::client::validate_url(&next_url, self.allow_private_network)?;
                    redirects.push(current_url.clone());
                    current_url = next_url;
                    continue;
                }
            }

            let body = resp.bytes().await.map_err(|e| {
                ObscuraNetError::Network(format!("Failed to read body: {}", e))
            })?.to_vec();

            let response = Response {
                url: current_url,
                status: status.as_u16(),
                headers: response_headers,
                body,
                redirected_from: redirects,
            };
            if let Some(callbacks) = callbacks {
                callbacks.fire_response(&request_info, &response).await;
            }
            return Ok(response);
        }

        Err(ObscuraNetError::TooManyRedirects(url.to_string()))
    }

    /// One request with no redirect following, for scripted fetch()/XHR. Reads
    /// the cookie jar for the Cookie header and stores Set-Cookie back into it,
    /// so the caller only owns redirect hops and SSRF re-validation. Used in
    /// stealth mode so JS-level requests carry the same Chrome TLS fingerprint
    /// and client hints as the main navigation instead of the rustls ClientHello
    /// that op_fetch_url would otherwise send (which bot managers read as a
    /// non-browser script and reject, e.g. the AWS WAF challenge verify call).
    pub async fn send_single(
        &self,
        method: &str,
        url: &Url,
        headers: &HashMap<String, String>,
        body: &str,
    ) -> Result<Response, ObscuraNetError> {
        crate::client::validate_url(url, self.allow_private_network)?;
        if let Some(host) = url.host_str() {
            if crate::blocklist::is_blocked(host) {
                tracing::debug!("Blocked tracker: {}", url);
                return Ok(Response {
                    status: 0,
                    url: url.clone(),
                    headers: HashMap::new(),
                    body: Vec::new(),
                    redirected_from: Vec::new(),
                });
            }
        }

        let req_method = method
            .parse::<wreq::Method>()
            .map_err(|e| ObscuraNetError::Network(format!("invalid method '{}': {}", method, e)))?;
        let mut req = self
            .client
            .request(req_method, url.as_str())
            // The emulation profile's defaults describe a top-level document
            // navigation. JS fetch()/XHR has a different Fetch metadata
            // contract, so build that small common browser header set here.
            .default_headers(false);
        if !self.accept_encoding.is_empty() {
            req = req.header("accept-encoding", self.accept_encoding.as_str());
        }
        if !self.sec_ch_ua.is_empty() {
            req = req
                .header("sec-ch-ua", self.sec_ch_ua.as_str())
                .header("sec-ch-ua-mobile", "?0");
        }
        if !self.sec_ch_ua_platform.is_empty() {
            req = req.header("sec-ch-ua-platform", self.sec_ch_ua_platform.as_str());
        }
        if !self.user_agent.is_empty() {
            req = req.header("user-agent", self.user_agent.as_str());
        }

        let cookie_header = self.cookie_jar.get_cookie_header(url);
        if !cookie_header.is_empty() {
            req = req.header("cookie", &cookie_header);
        }
        for (k, v) in self.extra_headers.read().await.iter() {
            req = req.header(k.as_str(), v.as_str());
        }
        for (k, v) in headers.iter() {
            req = req.header(k.as_str(), v.as_str());
        }
        if !body.is_empty() {
            req = req.body(body.to_string());
        }

        self.in_flight.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let resp = req.send().await.map_err(|e| {
            self.in_flight.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            ObscuraNetError::Network(format!("{}: {}", url, e))
        })?;
        self.in_flight.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        let status = resp.status();
        for val in resp.headers().get_all("set-cookie") {
            if let Ok(s) = val.to_str() {
                self.cookie_jar.set_cookie(s, url);
            }
        }
        let response_headers: HashMap<String, String> = resp
            .headers()
            .iter()
            .map(|(k, v)| (k.as_str().to_lowercase(), v.to_str().unwrap_or("").to_string()))
            .collect();
        let resp_body = resp
            .bytes()
            .await
            .map_err(|e| ObscuraNetError::Network(format!("Failed to read body: {}", e)))?
            .to_vec();
        Ok(Response {
            url: url.clone(),
            status: status.as_u16(),
            headers: response_headers,
            body: resp_body,
            redirected_from: Vec::new(),
        })
    }

    pub async fn set_extra_headers(&self, headers: HashMap<String, String>) {
        *self.extra_headers.write().await = headers;
    }

    pub fn active_requests(&self) -> u32 {
        self.in_flight.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn transport_browser_major(&self) -> u32 {
        self.transport_browser_major
    }

    pub fn is_network_idle(&self) -> bool {
        self.active_requests() == 0
    }
}

#[cfg(feature = "stealth")]
fn referrer_for_target(referrer: &Url, target: &Url) -> Option<Url> {
    if referrer.scheme() == "https" && target.scheme() == "http" {
        return None;
    }

    if referrer.origin().ascii_serialization() == target.origin().ascii_serialization() {
        let mut value = referrer.clone();
        value.set_fragment(None);
        return Some(value);
    }
    Url::parse(&referrer.origin().ascii_serialization()).ok()
}

#[cfg(all(test, feature = "stealth"))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn browser_major_uses_exact_or_nearest_transport() {
        assert_eq!(chrome_transport_profile(143).0, 143);
        assert_eq!(chrome_transport_profile(148).0, 148);
        assert_eq!(chrome_transport_profile(150).0, 148);
        assert_eq!(chrome_transport_profile(103).0, 104);
    }

    #[tokio::test]
    async fn private_targets_are_blocked_before_the_request() {
        let client = StealthHttpClient::new(Arc::new(CookieJar::new()));
        let error = client
            .fetch(&Url::parse("http://127.0.0.1/private").unwrap())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("private/internal IP address"));
    }

    #[tokio::test]
    async fn profile_headers_referrer_callbacks_and_compression_share_the_stealth_path() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut chunk).await.unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
            }
            const GZIP_OK: &[u8] = &[
                0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0a, 0xcb, 0xcf, 0x06, 0x00,
                0x47, 0xdd, 0xdc, 0x79, 0x02, 0x00, 0x00, 0x00,
            ];
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 22\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            stream.write_all(GZIP_OK).await.unwrap();
            String::from_utf8(request).unwrap()
        });

        let sec_ch_ua = r#""Profile Brand";v="145", "Chromium";v="145""#;
        let client = StealthHttpClient::with_browser_identity(
            Arc::new(CookieJar::new()),
            None,
            "Profile User Agent",
            sec_ch_ua,
            r#""Windows""#,
            145,
            true,
        );
        let request_count = Arc::new(AtomicUsize::new(0));
        let response_count = Arc::new(AtomicUsize::new(0));
        let callbacks = CallbackRegistry::new();
        let request_count_clone = request_count.clone();
        callbacks.add_request(Arc::new(move |info| {
            assert_eq!(info.resource_type, ResourceType::Script);
            assert_eq!(
                info.headers.get("sec-ch-ua").map(String::as_str),
                Some(sec_ch_ua)
            );
            assert_eq!(
                info.headers.get("accept-encoding").map(String::as_str),
                Some("gzip, deflate, br, zstd")
            );
            request_count_clone.fetch_add(1, Ordering::Relaxed);
        }));
        let response_count_clone = response_count.clone();
        callbacks.add_response(Arc::new(move |_, response| {
            assert_eq!(response.status, 200);
            response_count_clone.fetch_add(1, Ordering::Relaxed);
        }));

        let target = Url::parse(&format!("http://{address}/asset.js")).unwrap();
        let referrer = Url::parse(&format!("http://{address}/page#fragment")).unwrap();
        let response = client
            .fetch_with_context(&target, Some(&referrer), Some(&callbacks), ResourceType::Script)
            .await
            .unwrap();
        assert_eq!(response.body, b"ok");
        assert!(!response.headers.contains_key("content-encoding"));
        assert!(!response.headers.contains_key("content-length"));
        assert_eq!(request_count.load(Ordering::Relaxed), 1);
        assert_eq!(response_count.load(Ordering::Relaxed), 1);

        let request = server.await.unwrap().to_ascii_lowercase();
        assert!(request.contains("user-agent: profile user agent\r\n"));
        assert!(
            request.contains("accept-encoding: gzip, deflate, br, zstd\r\n"),
            "unexpected request headers:\n{request}"
        );
        assert!(request.contains(&format!("sec-ch-ua: {}\r\n", sec_ch_ua.to_ascii_lowercase())));
        assert!(request.contains(&format!("referer: http://{address}/page\r\n")));
    }

}
