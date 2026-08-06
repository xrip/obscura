use std::pin::Pin;
use std::sync::Arc;
use std::sync::RwLock;

use deno_core::error::ModuleLoaderError;
use deno_core::ModuleLoadResponse;
use deno_core::ModuleLoader;
use deno_core::ModuleSource;
use deno_core::ModuleSourceCode;
use deno_core::ModuleSpecifier;
use deno_core::RequestedModuleType;
use obscura_net::{CallbackRegistry, CookieJar, ObscuraHttpClient, ResourceType};
#[cfg(feature = "stealth")]
use obscura_net::StealthHttpClient;

struct ModuleRequestContext {
    http_client: RwLock<Option<Arc<ObscuraHttpClient>>>,
    callbacks: RwLock<Option<Arc<CallbackRegistry>>>,
    #[cfg(feature = "stealth")]
    stealth_client: RwLock<Option<Arc<StealthHttpClient>>>,
}

impl ModuleRequestContext {
    fn new(proxy_url: Option<&str>) -> Self {
        Self {
            http_client: RwLock::new(Some(Arc::new(ObscuraHttpClient::with_options(
                Arc::new(CookieJar::new()),
                proxy_url,
            )))),
            callbacks: RwLock::new(None),
            #[cfg(feature = "stealth")]
            stealth_client: RwLock::new(None),
        }
    }
}

pub struct ObscuraModuleLoader {
    pub base_url: String,
    /// Proxy URL threaded through to every dynamic ES-module fetch (#139).
    /// `None` keeps the pre-#139 direct-connection behaviour for callers
    /// that haven't been updated.
    pub proxy_url: Option<String>,
    request_context: Arc<ModuleRequestContext>,
}

impl ObscuraModuleLoader {
    pub fn new(base_url: &str) -> Self {
        Self::with_proxy(base_url, None)
    }

    pub fn with_proxy(base_url: &str, proxy_url: Option<String>) -> Self {
        ObscuraModuleLoader {
            base_url: base_url.to_string(),
            request_context: Arc::new(ModuleRequestContext::new(proxy_url.as_deref())),
            proxy_url,
        }
    }

    #[cfg(feature = "stealth")]
    pub fn with_proxy_and_stealth(
        base_url: &str,
        proxy_url: Option<String>,
        stealth_client: Option<Arc<StealthHttpClient>>,
        callbacks: Option<Arc<CallbackRegistry>>,
    ) -> Self {
        let client_proxy_url = proxy_url.clone();
        ObscuraModuleLoader {
            base_url: base_url.to_string(),
            proxy_url,
            request_context: Arc::new(ModuleRequestContext {
                http_client: RwLock::new(Some(Arc::new(ObscuraHttpClient::with_options(
                    Arc::new(CookieJar::new()),
                    client_proxy_url.as_deref(),
                )))),
                callbacks: RwLock::new(callbacks),
                stealth_client: RwLock::new(stealth_client),
            }),
        }
    }

    pub fn set_http_client(&self, client: Arc<ObscuraHttpClient>) {
        *self.request_context.http_client.write().unwrap() = Some(client);
    }

    pub fn set_callbacks(&self, callbacks: Arc<CallbackRegistry>) {
        *self.request_context.callbacks.write().unwrap() = Some(callbacks);
    }

    #[cfg(feature = "stealth")]
    pub fn set_stealth_client(&self, client: Arc<StealthHttpClient>) {
        *self.request_context.stealth_client.write().unwrap() = Some(client);
    }
}

fn io_err(msg: String) -> ModuleLoaderError {
    std::io::Error::new(std::io::ErrorKind::Other, msg).into()
}

fn module_url_allowed(base_url: &str, module_url: &ModuleSpecifier) -> bool {
    match module_url.scheme() {
        "http" | "https" => true,
        "file" => ModuleSpecifier::parse(base_url)
            .map(|base| base.scheme() == "file")
            .unwrap_or(false),
        _ => false,
    }
}

impl ModuleLoader for ObscuraModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: deno_core::ResolutionKind,
    ) -> Result<ModuleSpecifier, ModuleLoaderError> {
        let base = if referrer.is_empty()
            || referrer.starts_with('<')
            || referrer == "."
            || referrer == "about:blank"
        {
            &self.base_url
        } else {
            referrer
        };

        let resolved = deno_core::resolve_import(specifier, base)
            .map_err(ModuleLoaderError::from)?;
        if !module_url_allowed(&self.base_url, &resolved) {
            return Err(io_err(format!("Forbidden module URL {}", resolved)));
        }
        Ok(resolved)
    }

    fn load(
        &self,
        module_specifier: &ModuleSpecifier,
        _maybe_referrer: Option<&ModuleSpecifier>,
        _is_dyn_import: bool,
        _requested_module_type: RequestedModuleType,
    ) -> ModuleLoadResponse {
        if !module_url_allowed(&self.base_url, module_specifier) {
            return ModuleLoadResponse::Sync(Err(io_err(format!(
                "Forbidden module URL {}",
                module_specifier
            ))));
        }
        let url = module_specifier.to_string();
        // Capture the loader's proxy here so the async closure below owns a
        // plain Option<String> rather than borrowing &self across an `await`.
        let request_context = self.request_context.clone();
        let referrer = _maybe_referrer
            .cloned()
            .or_else(|| ModuleSpecifier::parse(&self.base_url).ok());
        ModuleLoadResponse::Async(Pin::from(Box::new(async move {
            let http_client = request_context.http_client.read().unwrap().clone();
            let callbacks = request_context.callbacks.read().unwrap().clone();
            #[cfg(feature = "stealth")]
            if let Some(stealth) = request_context.stealth_client.read().unwrap().clone() {
                let specifier = ModuleSpecifier::parse(&url)
                    .map_err(|e| io_err(format!("Invalid module URL {}: {}", url, e)))?;
                let resp = stealth
                    .fetch_with_context(
                        &specifier,
                        referrer.as_ref(),
                        callbacks.as_deref(),
                        ResourceType::Script,
                    )
                    .await
                    .map_err(|e| io_err(format!("Failed to fetch module {}: {}", url, e)))?;
                if !(200..300).contains(&resp.status) {
                    return Err(io_err(format!(
                        "Module {} returned HTTP {}",
                        url, resp.status
                    )));
                }
                let code = obscura_net::decode_non_html(&resp.body, resp.content_type());
                return Ok(ModuleSource::new(
                    deno_core::ModuleType::JavaScript,
                    ModuleSourceCode::String(code.into()),
                    &specifier,
                    None,
                ));
            }

            let client = http_client
                .ok_or_else(|| io_err(format!("No HTTP client for module {}", url)))?;
            let specifier = ModuleSpecifier::parse(&url)
                .map_err(|e| io_err(format!("Invalid module URL {}: {}", url, e)))?;
            let resp = client
                .fetch_with_context(
                    &specifier,
                    referrer.as_ref(),
                    callbacks.as_deref(),
                    ResourceType::Script,
                )
                .await
                .map_err(|e| io_err(format!("Failed to fetch module {}: {}", url, e)))?;

            if !(200..300).contains(&resp.status) {
                return Err(io_err(format!(
                    "Module {} returned HTTP {}",
                    url,
                    resp.status
                )));
            }

            let code = obscura_net::decode_non_html(&resp.body, resp.content_type());

            Ok(ModuleSource::new(
                deno_core::ModuleType::JavaScript,
                ModuleSourceCode::String(code.into()),
                &specifier,
                None,
            ))
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::{module_url_allowed, ModuleSpecifier};

    #[test]
    fn module_url_policy_blocks_cross_scheme_local_files() {
        let web_module = ModuleSpecifier::parse("https://example.com/app.mjs").unwrap();
        let file_module = ModuleSpecifier::parse("file:///tmp/app.mjs").unwrap();
        let data_module = ModuleSpecifier::parse("data:text/javascript,export%20default%201").unwrap();
        assert!(module_url_allowed("https://example.com/", &web_module));
        assert!(!module_url_allowed("https://example.com/", &file_module));
        assert!(module_url_allowed("file:///tmp/index.html", &file_module));
        assert!(!module_url_allowed("https://example.com/", &data_module));
    }
}
