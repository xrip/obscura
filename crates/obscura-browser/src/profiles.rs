use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock};

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

const CATALOG_GZIP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/chrome-windows-v1.json.gz"));
const DEFAULT_CATALOG_JSON: &str = include_str!(concat!(env!("OUT_DIR"), "/chrome-windows-v1.default.json"));
const CATALOG_ID: &str = "chrome-windows-v1";
const MAX_RUNTIME_PROFILES: usize = 256;
const MAX_RUNTIME_PROFILE_BYTES: usize = 16 * 1024 * 1024;
pub const GRAPHICS_API_BROWSER_MAJOR: u32 = 145;

#[derive(Debug, Clone, Error)]
pub enum ProfileError {
    #[error("fingerprint catalog error: {0}")]
    Catalog(String),
    #[error("invalid fingerprint profile ID: {0}")]
    Selector(String),
    #[error("fingerprint profile serialization error: {0}")]
    Serialization(String),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FingerprintCatalog {
    pub schema_version: u32,
    pub catalog_id: String,
    target: CatalogTarget,
    pub default_composition: CatalogComposition,
    base_profiles: Vec<BaseCatalogProfile>,
    screen_profiles: Vec<ScreenWindowProfile>,
    graphics_profiles: Vec<GraphicsProfile>,
    components: CatalogComponents,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogTarget {
    browser: String,
    default_browser_major: u32,
    graphics_api_browser_major: u32,
    graphics_api_revision: String,
    transport_browser_majors: Vec<u32>,
    os: String,
    graphics_backend: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogComposition {
    pub base_id: String,
    pub graphics_id: String,
    pub screen_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrandVersion {
    pub brand: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BaseCatalogProfile {
    id: String,
    browser_version: String,
    user_agent: String,
    brands: Vec<BrandVersion>,
    full_version_list: Vec<BrandVersion>,
    platform: String,
    platform_version: String,
    architecture: String,
    bitness: String,
    languages: Vec<String>,
    hardware_concurrency: u32,
    device_memory: f64,
    max_touch_points: u32,
    weight: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenWindowProfile {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub avail_width: u32,
    pub avail_height: u32,
    pub avail_left: i32,
    pub avail_top: i32,
    pub color_depth: u32,
    pub pixel_depth: u32,
    pub device_pixel_ratio: f64,
    pub inner_width: u32,
    pub inner_height: u32,
    pub outer_width: u32,
    pub outer_height: u32,
    pub screen_x: i32,
    pub screen_y: i32,
    pub weight: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphicsProfile {
    pub id: String,
    pub masked_vendor: String,
    pub masked_renderer: String,
    pub unmasked_vendor: String,
    pub unmasked_renderer: String,
    pub webgl1_id: String,
    pub webgl2_id: String,
    pub webgpu_id: String,
    pub preferred_canvas_format: String,
    pub wgsl_language_features: Vec<String>,
    pub observations_by_browser_version: BTreeMap<String, u64>,
    pub weight: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct CatalogComponent {
    id: String,
    #[serde(flatten)]
    data: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogComponents {
    webgl1: Vec<CatalogComponent>,
    webgl2: Vec<CatalogComponent>,
    webgpu: Vec<CatalogWebGpuComponent>,
    webgpu_adapters: Vec<CatalogWebGpuAdapterComponent>,
    webgpu_limits: Vec<CatalogWebGpuLimitsComponent>,
}

#[derive(Debug, Clone, Deserialize)]
struct CatalogWebGpuComponent {
    id: String,
    adapters: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogWebGpuAdapterComponent {
    id: String,
    info: Value,
    features: Vec<String>,
    limits_id: String,
    default_device_limits_id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CatalogWebGpuLimitsComponent {
    id: String,
    values: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DefaultCatalog {
    schema_version: u32,
    catalog_id: String,
    target: CatalogTarget,
    default_composition: CatalogComposition,
    base_profile: BaseCatalogProfile,
    screen_profile: ScreenWindowProfile,
    graphics_profile: GraphicsProfile,
    webgl1: CatalogComponent,
    webgl2: CatalogComponent,
    webgpu: CatalogWebGpuComponent,
    #[serde(rename = "webgpuAdapters")]
    webgpu_adapters: Vec<CatalogWebGpuAdapterComponent>,
    #[serde(rename = "webgpuLimits")]
    webgpu_limits: Vec<CatalogWebGpuLimitsComponent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserIdentity {
    pub major: u32,
    pub version: String,
    pub user_agent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkIdentity {
    pub downlink: f64,
    pub rtt: u32,
    pub effective_type: String,
    pub save_data: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigatorIdentity {
    pub platform: String,
    pub ua_platform: String,
    pub ua_platform_version: String,
    pub architecture: String,
    pub bitness: String,
    pub brands: Vec<BrandVersion>,
    pub full_version_list: Vec<BrandVersion>,
    pub languages: Vec<String>,
    pub hardware_concurrency: u32,
    pub device_memory: f64,
    pub max_touch_points: u32,
}

impl NavigatorIdentity {
    pub fn sec_ch_ua_header(&self) -> String {
        self.brands
            .iter()
            .map(|brand| {
                format!(
                    "\"{}\";v=\"{}\"",
                    escape_client_hint(&brand.brand),
                    escape_client_hint(&brand.version),
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn sec_ch_ua_platform_header(&self) -> String {
        format!("\"{}\"", escape_client_hint(&self.ua_platform))
    }

    pub fn accept_language_header(&self) -> String {
        self.languages
            .iter()
            .take(10)
            .enumerate()
            .map(|(index, language)| {
                if index == 0 {
                    language.clone()
                } else {
                    format!("{language};q=0.{}", 10 - index)
                }
            })
            .collect::<Vec<_>>()
            .join(",")
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeGraphics<'a> {
    id: &'a str,
    masked_vendor: &'a str,
    masked_renderer: &'a str,
    unmasked_vendor: &'a str,
    unmasked_renderer: &'a str,
    preferred_canvas_format: &'a str,
    wgsl_language_features: &'a [String],
    webgl1: &'a Value,
    webgl2: &'a Value,
    webgpu: &'a Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeFingerprint<'a> {
    id: &'a str,
    catalog_id: &'a str,
    render_seed: &'a str,
    browser: &'a BrowserIdentity,
    navigator: &'a NavigatorIdentity,
    network: &'a NetworkIdentity,
    screen: &'a ScreenWindowProfile,
    graphics: RuntimeGraphics<'a>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeGraphicsCapture {
    id: String,
    masked_vendor: String,
    masked_renderer: String,
    unmasked_vendor: String,
    unmasked_renderer: String,
    webgl1_id: String,
    webgl2_id: String,
    webgpu_id: String,
    preferred_canvas_format: String,
    wgsl_language_features: Vec<String>,
    observations_by_browser_version: BTreeMap<String, u64>,
    weight: u64,
    webgl1: Value,
    webgl2: Value,
    webgpu: Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeFingerprintCapture {
    id: String,
    catalog_id: String,
    render_seed: String,
    browser: BrowserIdentity,
    navigator: NavigatorIdentity,
    network: NetworkIdentity,
    screen: ScreenWindowProfile,
    graphics: RuntimeGraphicsCapture,
}

#[derive(Debug, Clone)]
pub struct ResolvedFingerprintProfile {
    pub id: String,
    pub browser: BrowserIdentity,
    pub navigator: NavigatorIdentity,
    pub network: NetworkIdentity,
    pub screen: ScreenWindowProfile,
    pub graphics: GraphicsProfile,
    pub webgl1: Value,
    pub webgl2: Value,
    pub webgpu: Value,
    pub render_seed: String,
    runtime_json: String,
}

impl ResolvedFingerprintProfile {
    pub fn runtime_json(&self) -> &str {
        &self.runtime_json
    }
}

fn browser_major(version: &str) -> Result<u32, ProfileError> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 4
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(ProfileError::Catalog(format!(
            "browser version {version} is not four numeric parts"
        )));
    }
    parts[0]
        .parse()
        .map_err(|_| ProfileError::Catalog(format!("browser major is invalid in {version}")))
}

fn composed_id(major: u32, base_id: &str, graphics_id: &str, screen_id: &str) -> String {
    format!("c{major}w1:{base_id}:{graphics_id}:{screen_id}")
}

fn graphics_major_weight(profile: &GraphicsProfile, major: u32) -> u64 {
    profile
        .observations_by_browser_version
        .iter()
        .filter(|(version, _)| {
            version
                .split('.')
                .next()
                .and_then(|value| value.parse::<u32>().ok())
                == Some(major)
        })
        .map(|(_, weight)| *weight)
        .sum()
}

impl FingerprintCatalog {
    pub fn base_profile_count(&self) -> usize {
        self.base_profiles.len()
    }

    pub fn screen_profile_count(&self) -> usize {
        self.screen_profiles.len()
    }

    pub fn graphics_profile_count(&self) -> usize {
        self.graphics_profiles.len()
    }

    pub fn index_json(&self) -> Result<String, ProfileError> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct CatalogIndex<'a> {
            schema_version: u32,
            catalog_id: &'a str,
            default_browser_major: u32,
            graphics_api_browser_major: u32,
            transport_browser_majors: &'a [u32],
            default_profile_id: String,
            base_profiles: &'a [BaseCatalogProfile],
            graphics_profiles: &'a [GraphicsProfile],
            screen_profiles: &'a [ScreenWindowProfile],
        }

        let ids = &self.default_composition;
        let base = self
            .base_profiles
            .iter()
            .find(|row| row.id == ids.base_id)
            .ok_or_else(|| ProfileError::Catalog("default base ID is missing".to_string()))?;
        serde_json::to_string_pretty(&CatalogIndex {
            schema_version: self.schema_version,
            catalog_id: &self.catalog_id,
            default_browser_major: self.target.default_browser_major,
            graphics_api_browser_major: self.target.graphics_api_browser_major,
            transport_browser_majors: &self.target.transport_browser_majors,
            default_profile_id: composed_id(
                browser_major(&base.browser_version)?,
                &ids.base_id,
                &ids.graphics_id,
                &ids.screen_id,
            ),
            base_profiles: &self.base_profiles,
            graphics_profiles: &self.graphics_profiles,
            screen_profiles: &self.screen_profiles,
        })
        .map_err(|error| ProfileError::Serialization(error.to_string()))
    }

    pub fn index_json_with_runtime(&self) -> Result<String, ProfileError> {
        let mut index: Value = serde_json::from_str(&self.index_json()?)
            .map_err(|error| ProfileError::Serialization(error.to_string()))?;
        let mut runtime = runtime_profiles()
            .read()
            .map_err(|_| ProfileError::Catalog("runtime profile registry is poisoned".to_string()))?
            .values()
            .cloned()
            .collect::<Vec<_>>();
        runtime.sort_by(|left, right| left.id.cmp(&right.id));
        for profile in runtime {
            let (_, base_id, graphics_id, screen_id) = parse_runtime_id(&profile.id)?;
            let base_rows = index["baseProfiles"]
                .as_array_mut()
                .ok_or_else(|| ProfileError::Catalog("catalog baseProfiles is not an array".to_string()))?;
            if !base_rows.iter().any(|row| row["id"] == base_id) {
                base_rows.push(serde_json::json!({
                    "id": base_id,
                    "browserVersion": profile.browser.version,
                    "userAgent": profile.browser.user_agent,
                    "brands": profile.navigator.brands,
                    "fullVersionList": profile.navigator.full_version_list,
                    "platform": profile.navigator.ua_platform,
                    "platformVersion": profile.navigator.ua_platform_version,
                    "architecture": profile.navigator.architecture,
                    "bitness": profile.navigator.bitness,
                    "languages": profile.navigator.languages,
                    "hardwareConcurrency": profile.navigator.hardware_concurrency,
                    "deviceMemory": profile.navigator.device_memory,
                    "maxTouchPoints": profile.navigator.max_touch_points,
                    "weight": 1,
                }));
            }
            let graphics_rows = index["graphicsProfiles"]
                .as_array_mut()
                .ok_or_else(|| ProfileError::Catalog("catalog graphicsProfiles is not an array".to_string()))?;
            if !graphics_rows.iter().any(|row| row["id"] == graphics_id) {
                graphics_rows.push(serde_json::to_value(&profile.graphics)
                    .map_err(|error| ProfileError::Serialization(error.to_string()))?);
            }
            let screen_rows = index["screenProfiles"]
                .as_array_mut()
                .ok_or_else(|| ProfileError::Catalog("catalog screenProfiles is not an array".to_string()))?;
            if !screen_rows.iter().any(|row| row["id"] == screen_id) {
                screen_rows.push(serde_json::to_value(&profile.screen)
                    .map_err(|error| ProfileError::Serialization(error.to_string()))?);
            }
        }
        serde_json::to_string_pretty(&index)
            .map_err(|error| ProfileError::Serialization(error.to_string()))
    }
}

static CATALOG: OnceLock<Result<FingerprintCatalog, ProfileError>> = OnceLock::new();
static DEFAULT_PROFILE: OnceLock<Result<Arc<ResolvedFingerprintProfile>, ProfileError>> = OnceLock::new();
static WARNED_INVALID_SELECTOR: OnceLock<()> = OnceLock::new();
static RUNTIME_PROFILES: OnceLock<RwLock<HashMap<String, Arc<ResolvedFingerprintProfile>>>> = OnceLock::new();

fn runtime_profiles() -> &'static RwLock<HashMap<String, Arc<ResolvedFingerprintProfile>>> {
    RUNTIME_PROFILES.get_or_init(|| RwLock::new(HashMap::new()))
}

fn runtime_profile_from_value(value: &Value) -> Result<Arc<ResolvedFingerprintProfile>, ProfileError> {
    let capture: RuntimeFingerprintCapture = serde_json::from_value(value.clone())
        .map_err(|error| ProfileError::Catalog(format!("invalid runtime profile: {error}")))?;
    if capture.catalog_id != CATALOG_ID {
        return Err(ProfileError::Catalog(
            "runtime profile has an unsupported catalog ID".to_string(),
        ));
    }
    let (major, _base_id, graphics_id, screen_id) = parse_runtime_id(&capture.id)?;
    if capture.browser.major != major || browser_major(&capture.browser.version)? != major {
        return Err(ProfileError::Catalog(
            "runtime profile browser major does not match its ID".to_string(),
        ));
    }
    if capture.navigator.ua_platform != "Windows"
        || capture.navigator.architecture != "x86"
        || capture.navigator.bitness != "64"
    {
        return Err(ProfileError::Catalog(
            "runtime profile is not a Chrome Windows x86-64 profile".to_string(),
        ));
    }
    if capture.screen.id != screen_id || capture.graphics.id != graphics_id {
        return Err(ProfileError::Catalog(
            "runtime profile component IDs do not match its composed ID".to_string(),
        ));
    }
    if capture.screen.width == 0
        || capture.screen.height == 0
        || !capture.screen.device_pixel_ratio.is_finite()
        || capture.screen.device_pixel_ratio <= 0.0
        || capture.graphics.weight == 0
        || capture.graphics.observations_by_browser_version.is_empty()
        || capture.graphics.webgl1.is_null()
        || capture.graphics.webgl2.is_null()
        || capture.graphics.webgpu.get("adapters").and_then(Value::as_object).and_then(|adapters| adapters.get("default")).is_none()
    {
        return Err(ProfileError::Catalog(
            "runtime profile has incomplete graphics or screen data".to_string(),
        ));
    }
    if !capture.graphics.unmasked_renderer.contains("D3D11")
        || !matches!(capture.graphics.preferred_canvas_format.as_str(), "bgra8unorm" | "rgba8unorm")
    {
        return Err(ProfileError::Catalog(
            "runtime profile is not an ANGLE/D3D11 profile".to_string(),
        ));
    }
    let mut seed_hasher = Sha256::new();
    seed_hasher.update(b"graphics-render-v1");
    seed_hasher.update(capture.id.as_bytes());
    if capture.render_seed != hex(&seed_hasher.finalize()) {
        return Err(ProfileError::Catalog(
            "runtime profile render seed does not match its ID".to_string(),
        ));
    }
    let graphics = GraphicsProfile {
        id: capture.graphics.id.clone(),
        masked_vendor: capture.graphics.masked_vendor.clone(),
        masked_renderer: capture.graphics.masked_renderer.clone(),
        unmasked_vendor: capture.graphics.unmasked_vendor.clone(),
        unmasked_renderer: capture.graphics.unmasked_renderer.clone(),
        webgl1_id: capture.graphics.webgl1_id.clone(),
        webgl2_id: capture.graphics.webgl2_id.clone(),
        webgpu_id: capture.graphics.webgpu_id.clone(),
        preferred_canvas_format: capture.graphics.preferred_canvas_format.clone(),
        wgsl_language_features: capture.graphics.wgsl_language_features.clone(),
        observations_by_browser_version: capture.graphics.observations_by_browser_version.clone(),
        weight: capture.graphics.weight,
    };
    let runtime = RuntimeFingerprint {
        id: &capture.id,
        catalog_id: &capture.catalog_id,
        render_seed: &capture.render_seed,
        browser: &capture.browser,
        navigator: &capture.navigator,
        network: &capture.network,
        screen: &capture.screen,
        graphics: RuntimeGraphics {
            id: &graphics.id,
            masked_vendor: &graphics.masked_vendor,
            masked_renderer: &graphics.masked_renderer,
            unmasked_vendor: &graphics.unmasked_vendor,
            unmasked_renderer: &graphics.unmasked_renderer,
            preferred_canvas_format: &graphics.preferred_canvas_format,
            wgsl_language_features: &graphics.wgsl_language_features,
            webgl1: &capture.graphics.webgl1,
            webgl2: &capture.graphics.webgl2,
            webgpu: &capture.graphics.webgpu,
        },
    };
    let runtime_json = serde_json::to_string(&runtime)
        .map_err(|error| ProfileError::Serialization(error.to_string()))?;
    Ok(Arc::new(ResolvedFingerprintProfile {
        id: capture.id,
        browser: capture.browser,
        navigator: capture.navigator,
        network: capture.network,
        screen: capture.screen,
        graphics,
        webgl1: capture.graphics.webgl1,
        webgl2: capture.graphics.webgl2,
        webgpu: capture.graphics.webgpu,
        render_seed: capture.render_seed,
        runtime_json,
    }))
}

fn parse_runtime_id(id: &str) -> Result<(u32, String, String, String), ProfileError> {
    let parts: Vec<&str> = id.split(':').collect();
    if parts.len() != 4 || !parts[0].starts_with('c') || !parts[0].ends_with("w1") {
        return Err(ProfileError::Selector(id.to_string()));
    }
    let major = parts[0][1..parts[0].len() - 2]
        .parse::<u32>()
        .map_err(|_| ProfileError::Selector(id.to_string()))?;
    for component in &parts[1..] {
        if component.len() != 32
            || !component
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(ProfileError::Selector(id.to_string()));
        }
    }
    Ok((
        major,
        parts[1].to_string(),
        parts[2].to_string(),
        parts[3].to_string(),
    ))
}

pub fn register_runtime_profile(value: &Value) -> Result<String, ProfileError> {
    let profile = runtime_profile_from_value(value)?;
    let id = profile.id.clone();
    let mut profiles = runtime_profiles()
        .write()
        .map_err(|_| ProfileError::Catalog("runtime profile registry is poisoned".to_string()))?;
    if let Some(existing) = profiles.get(&id) {
        if existing.runtime_json() != profile.runtime_json() {
            return Err(ProfileError::Catalog(
                "runtime profile ID is already registered with different data".to_string(),
            ));
        }
    } else {
        if profiles.len() >= MAX_RUNTIME_PROFILES {
            return Err(ProfileError::Catalog(format!(
                "runtime profile limit of {MAX_RUNTIME_PROFILES} was reached"
            )));
        }
        profiles.insert(id.clone(), profile);
    }
    Ok(id)
}

pub fn load_runtime_profiles(directory: &Path) -> Result<usize, ProfileError> {
    if !directory.exists() {
        return Ok(0);
    }
    let mut paths = std::fs::read_dir(directory)
        .map_err(|error| ProfileError::Catalog(format!("cannot read runtime profile directory: {error}")))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();
    let mut loaded = 0;
    for path in paths {
        let bytes = std::fs::read(&path).map_err(|error| {
            ProfileError::Catalog(format!("cannot read runtime profile {}: {error}", path.display()))
        })?;
        if bytes.len() > MAX_RUNTIME_PROFILE_BYTES {
            return Err(ProfileError::Catalog(format!(
                "runtime profile {} is too large",
                path.display()
            )));
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
            ProfileError::Catalog(format!("invalid runtime profile {}: {error}", path.display()))
        })?;
        register_runtime_profile(&value)?;
        loaded += 1;
    }
    Ok(loaded)
}

fn runtime_profile(id: &str) -> Option<Arc<ResolvedFingerprintProfile>> {
    runtime_profiles()
        .read()
        .ok()
        .and_then(|profiles| profiles.get(id).cloned())
}

pub fn catalog() -> Result<&'static FingerprintCatalog, ProfileError> {
    match CATALOG.get_or_init(load_catalog) {
        Ok(catalog) => Ok(catalog),
        Err(error) => Err(error.clone()),
    }
}

fn load_catalog() -> Result<FingerprintCatalog, ProfileError> {
    let mut json = String::new();
    GzDecoder::new(CATALOG_GZIP)
        .read_to_string(&mut json)
        .map_err(|error| ProfileError::Catalog(format!("cannot decompress embedded JSON: {error}")))?;
    validate_catalog_size(json.len())?;
    let catalog: FingerprintCatalog = serde_json::from_str(&json)
        .map_err(|error| ProfileError::Catalog(format!("invalid JSON: {error}")))?;
    validate_catalog(&catalog)?;
    Ok(catalog)
}

fn validate_catalog(catalog: &FingerprintCatalog) -> Result<(), ProfileError> {
    validate_catalog_header(catalog.schema_version, &catalog.catalog_id, &catalog.target)?;
    if catalog.base_profiles.is_empty()
        || catalog.screen_profiles.is_empty()
        || catalog.graphics_profiles.is_empty()
    {
        return Err(ProfileError::Catalog("a profile table is empty".to_string()));
    }
    let bases = unique_ids(catalog.base_profiles.iter().map(|row| row.id.as_str()), "base")?;
    let screens = unique_ids(catalog.screen_profiles.iter().map(|row| row.id.as_str()), "screen")?;
    let graphics = unique_ids(catalog.graphics_profiles.iter().map(|row| row.id.as_str()), "graphics")?;
    let webgl1 = unique_ids(catalog.components.webgl1.iter().map(|row| row.id.as_str()), "webgl1")?;
    let webgl2 = unique_ids(catalog.components.webgl2.iter().map(|row| row.id.as_str()), "webgl2")?;
    let webgpu = unique_ids(catalog.components.webgpu.iter().map(|row| row.id.as_str()), "webgpu")?;
    let webgpu_adapters = unique_ids(
        catalog.components.webgpu_adapters.iter().map(|row| row.id.as_str()),
        "webgpu adapters",
    )?;
    let webgpu_limits = unique_ids(
        catalog.components.webgpu_limits.iter().map(|row| row.id.as_str()),
        "webgpu limits",
    )?;
    if !bases.contains(catalog.default_composition.base_id.as_str())
        || !screens.contains(catalog.default_composition.screen_id.as_str())
        || !graphics.contains(catalog.default_composition.graphics_id.as_str())
    {
        return Err(ProfileError::Catalog("default composition has an unknown ID".to_string()));
    }
    for row in &catalog.graphics_profiles {
        if row.weight == 0
            || row.observations_by_browser_version.is_empty()
            || row.observations_by_browser_version.iter().any(|(version, weight)| {
                let parts: Vec<&str> = version.split('.').collect();
                *weight == 0
                    || parts.len() != 4
                    || parts.iter().any(|part| {
                        part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit())
                    })
            })
            || row
                .observations_by_browser_version
                .values()
                .try_fold(0u64, |total, weight| total.checked_add(*weight))
                != Some(row.weight)
            || !webgl1.contains(row.webgl1_id.as_str())
            || !webgl2.contains(row.webgl2_id.as_str())
            || !webgpu.contains(row.webgpu_id.as_str())
        {
            return Err(ProfileError::Catalog(format!(
                "graphics profile {} has an invalid weight or component ID",
                row.id
            )));
        }
    }
    if catalog.components.webgpu.iter().any(|row| {
        !row.adapters.contains_key("default")
            || row.adapters.keys().any(|name| {
                !matches!(name.as_str(), "default" | "lowPower" | "highPerformance")
            })
            || row
                .adapters
                .values()
                .any(|id| !webgpu_adapters.contains(id.as_str()))
    }) || catalog.components.webgpu_adapters.iter().any(|row| {
        !webgpu_limits.contains(row.limits_id.as_str())
            || !webgpu_limits.contains(row.default_device_limits_id.as_str())
    }) {
        return Err(ProfileError::Catalog(
            "a WebGPU component has an unknown nested component".to_string(),
        ));
    }
    if catalog.base_profiles.iter().any(|row| row.weight == 0)
        || catalog.screen_profiles.iter().any(|row| row.weight == 0)
    {
        return Err(ProfileError::Catalog("a profile has zero weight".to_string()));
    }
    let default_base = catalog
        .base_profiles
        .iter()
        .find(|row| row.id == catalog.default_composition.base_id)
        .ok_or_else(|| ProfileError::Catalog("default base ID is missing".to_string()))?;
    let default_graphics = catalog
        .graphics_profiles
        .iter()
        .find(|row| row.id == catalog.default_composition.graphics_id)
        .ok_or_else(|| ProfileError::Catalog("default graphics ID is missing".to_string()))?;
    let default_major = browser_major(&default_base.browser_version)?;
    if default_major != catalog.target.default_browser_major
        || graphics_major_weight(default_graphics, default_major) == 0
        || catalog.base_profiles.iter().any(|base| {
            browser_major(&base.browser_version).ok().is_none_or(|major| {
                !catalog
                    .graphics_profiles
                    .iter()
                    .any(|graphics| graphics_major_weight(graphics, major) > 0)
            })
        })
    {
        return Err(ProfileError::Catalog(
            "a browser major has no compatible graphics profile".to_string(),
        ));
    }
    Ok(())
}

fn validate_catalog_size(size: usize) -> Result<(), ProfileError> {
    if size > 2 * 1024 * 1024 + 1 {
        return Err(ProfileError::Catalog(format!(
            "embedded catalog is {} bytes; limit is 2097152 bytes",
            size
        )));
    }
    Ok(())
}

fn validate_catalog_header(
    schema_version: u32,
    catalog_id: &str,
    target: &CatalogTarget,
) -> Result<(), ProfileError> {
    if schema_version != 1 || catalog_id != CATALOG_ID {
        return Err(ProfileError::Catalog(
            "unsupported schemaVersion or catalogId".to_string(),
        ));
    }
    if target.browser != "Chrome"
        || target.default_browser_major != 145
        || target.graphics_api_browser_major != GRAPHICS_API_BROWSER_MAJOR
        || target.graphics_api_revision != "145.0.7632.75"
        || target.transport_browser_majors.is_empty()
        || !target.transport_browser_majors.contains(&145)
        || target.os != "Windows"
        || target.graphics_backend != "ANGLE/D3D11"
    {
        return Err(ProfileError::Catalog("target is not the supported Chrome Windows ANGLE/D3D11 catalog".to_string()));
    }
    Ok(())
}

fn unique_ids<'a>(
    ids: impl Iterator<Item = &'a str>,
    table: &str,
) -> Result<HashSet<&'a str>, ProfileError> {
    let mut out = HashSet::new();
    for id in ids {
        if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
            return Err(ProfileError::Catalog(format!("{table} has an invalid ID")));
        }
        if !out.insert(id) {
            return Err(ProfileError::Catalog(format!("{table} has a duplicate ID")));
        }
    }
    Ok(out)
}

fn default_profile() -> Result<Arc<ResolvedFingerprintProfile>, ProfileError> {
    match DEFAULT_PROFILE.get_or_init(load_default_profile) {
        Ok(profile) => Ok(Arc::clone(profile)),
        Err(error) => Err(error.clone()),
    }
}

fn load_default_profile() -> Result<Arc<ResolvedFingerprintProfile>, ProfileError> {
    let selected: DefaultCatalog = serde_json::from_str(DEFAULT_CATALOG_JSON)
        .map_err(|error| ProfileError::Catalog(format!("invalid fixed profile: {error}")))?;
    validate_catalog_header(selected.schema_version, &selected.catalog_id, &selected.target)?;
    let ids = &selected.default_composition;
    let base = &selected.base_profile;
    let screen = &selected.screen_profile;
    let graphics = &selected.graphics_profile;
    if base.id != ids.base_id || screen.id != ids.screen_id || graphics.id != ids.graphics_id {
        return Err(ProfileError::Catalog(
            "the build-time fixed profile does not match defaultComposition".to_string(),
        ));
    }
    if selected.webgl1.id != graphics.webgl1_id
        || selected.webgl2.id != graphics.webgl2_id
        || selected.webgpu.id != graphics.webgpu_id
    {
        return Err(ProfileError::Catalog(
            "the build-time fixed profile has a wrong component ID".to_string(),
        ));
    }
    if base.weight == 0 || screen.weight == 0 || graphics.weight == 0 {
        return Err(ProfileError::Catalog("the default composition has zero weight".to_string()));
    }
    let webgl1 = component_value_owned(selected.webgl1)?;
    let webgl2 = component_value_owned(selected.webgl2)?;
    let webgpu = expand_webgpu_component(
        &selected.webgpu,
        &selected.webgpu_adapters,
        &selected.webgpu_limits,
    )?;
    Ok(Arc::new(compose_selected(
        &selected.catalog_id,
        base,
        graphics,
        screen,
        webgl1,
        webgl2,
        webgpu,
    )?))
}

pub fn resolve_profile() -> Result<Arc<ResolvedFingerprintProfile>, ProfileError> {
    let selector = std::env::var("OBSCURA_PROFILE").ok();
    if let Some(selector) = selector.as_deref().map(str::trim) {
        if let Some(profile) = runtime_profile(selector) {
            return Ok(profile);
        }
    }
    let rotate = selector.is_none() && env_enabled("OBSCURA_ROTATE_PROFILE");
    if !rotate && selector.as_deref().map(str::trim).map_or(true, |value| value == "0") {
        return default_profile();
    }
    let random = if rotate {
        let mut bytes = [0u8; 24];
        match getrandom::getrandom(&mut bytes) {
            Ok(()) => Some(bytes),
            Err(error) => {
                tracing::warn!(%error, "OS random profile selection failed; using the fixed default");
                None
            }
        }
    } else {
        None
    };
    if rotate && random.is_none() {
        return default_profile();
    }
    let (resolved, fallback) = resolve_with_options(catalog()?, selector.as_deref(), rotate, random)?;
    if fallback && WARNED_INVALID_SELECTOR.set(()).is_ok() {
        tracing::warn!(
            selector = selector.as_deref().unwrap_or(""),
            "invalid OBSCURA_PROFILE selector; using the fixed default"
        );
    }
    Ok(Arc::new(resolved))
}

pub fn resolve_profile_id(id: &str) -> Result<Arc<ResolvedFingerprintProfile>, ProfileError> {
    if let Some(profile) = runtime_profile(id) {
        return Ok(profile);
    }
    let catalog = catalog()?;
    let (base_id, graphics_id, screen_id) = parse_composed_selector(catalog, id)
        .ok_or_else(|| ProfileError::Selector(id.to_string()))?;
    Ok(Arc::new(compose(catalog, base_id, graphics_id, screen_id)?))
}

fn resolve_with_options(
    catalog: &FingerprintCatalog,
    selector: Option<&str>,
    rotate: bool,
    random: Option<[u8; 24]>,
) -> Result<(ResolvedFingerprintProfile, bool), ProfileError> {
    let default = (
        catalog.default_composition.base_id.as_str(),
        catalog.default_composition.graphics_id.as_str(),
        catalog.default_composition.screen_id.as_str(),
    );
    let (ids, fallback) = if let Some(selector) = selector.map(str::trim) {
        if selector == "0" {
            (default, false)
        } else if let Some(ids) = parse_composed_selector(catalog, selector) {
            (ids, false)
        } else if let Ok(seed) = selector.parse::<u64>() {
            if seed == 0 {
                (default, false)
            } else {
                (seeded_ids(catalog, seed), false)
            }
        } else {
            (default, true)
        }
    } else if rotate {
        if let Some(bytes) = random {
            let base_draw = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
            let graphics_draw = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
            let screen_draw = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
            (drawn_ids(catalog, base_draw, graphics_draw, screen_draw), false)
        } else {
            (default, false)
        }
    } else {
        (default, false)
    };
    Ok((compose(catalog, ids.0, ids.1, ids.2)?, fallback))
}

fn parse_composed_selector<'a>(
    catalog: &'a FingerprintCatalog,
    selector: &str,
) -> Option<(&'a str, &'a str, &'a str)> {
    let mut parts = selector.split(':');
    let prefix = parts.next()?;
    let base = parts.next()?;
    let graphics = parts.next()?;
    let screen = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let base = catalog.base_profiles.iter().find(|row| row.id == base)?;
    let graphics = catalog.graphics_profiles.iter().find(|row| row.id == graphics)?;
    let screen = catalog.screen_profiles.iter().find(|row| row.id == screen)?;
    let major = browser_major(&base.browser_version).ok()?;
    if prefix != format!("c{major}w1") || graphics_major_weight(graphics, major) == 0 {
        return None;
    }
    Some((base.id.as_str(), graphics.id.as_str(), screen.id.as_str()))
}

fn seeded_ids(catalog: &FingerprintCatalog, seed: u64) -> (&str, &str, &str) {
    let draw = |part: &str| {
        let mut hasher = Sha256::new();
        hasher.update(b"catalog-v1");
        hasher.update(seed.to_le_bytes());
        hasher.update(part.as_bytes());
        let digest = hasher.finalize();
        u64::from_le_bytes(digest[..8].try_into().unwrap())
    };
    drawn_ids(catalog, draw("base"), draw("graphics"), draw("screen"))
}

fn drawn_ids(
    catalog: &FingerprintCatalog,
    base_draw: u64,
    graphics_draw: u64,
    screen_draw: u64,
) -> (&str, &str, &str) {
    let base_id = weighted_pick(
        &catalog.base_profiles,
        base_draw,
        |row| row.weight,
        |row| row.id.as_str(),
    );
    let base = catalog
        .base_profiles
        .iter()
        .find(|row| row.id == base_id)
        .expect("weighted base ID is present");
    let major = browser_major(&base.browser_version).expect("validated browser major");
    (
        base_id,
        weighted_pick(
            &catalog.graphics_profiles,
            graphics_draw,
            |row| graphics_major_weight(row, major),
            |row| row.id.as_str(),
        ),
        weighted_pick(
            &catalog.screen_profiles,
            screen_draw,
            |row| row.weight,
            |row| row.id.as_str(),
        ),
    )
}

fn weighted_pick<T, W, I>(rows: &[T], draw: u64, weight: W, id: I) -> &str
where
    W: Fn(&T) -> u64,
    I: Fn(&T) -> &str,
{
    let total: u64 = rows.iter().map(&weight).sum();
    let mut point = draw % total;
    for row in rows {
        let row_weight = weight(row);
        if point < row_weight {
            return id(row);
        }
        point -= row_weight;
    }
    id(&rows[rows.len() - 1])
}

fn compose(
    catalog: &FingerprintCatalog,
    base_id: &str,
    graphics_id: &str,
    screen_id: &str,
) -> Result<ResolvedFingerprintProfile, ProfileError> {
    let base = catalog
        .base_profiles
        .iter()
        .find(|row| row.id == base_id)
        .ok_or_else(|| ProfileError::Catalog("selected base ID is missing".to_string()))?;
    let graphics = catalog
        .graphics_profiles
        .iter()
        .find(|row| row.id == graphics_id)
        .ok_or_else(|| ProfileError::Catalog("selected graphics ID is missing".to_string()))?;
    let screen = catalog
        .screen_profiles
        .iter()
        .find(|row| row.id == screen_id)
        .ok_or_else(|| ProfileError::Catalog("selected screen ID is missing".to_string()))?;
    let webgl1 = component_value(&catalog.components.webgl1, &graphics.webgl1_id)?;
    let webgl2 = component_value(&catalog.components.webgl2, &graphics.webgl2_id)?;
    let webgpu = webgpu_component_value(&catalog.components, &graphics.webgpu_id)?;
    compose_selected(
        &catalog.catalog_id,
        base,
        graphics,
        screen,
        webgl1,
        webgl2,
        webgpu,
    )
}

fn compose_selected(
    catalog_id: &str,
    base: &BaseCatalogProfile,
    graphics: &GraphicsProfile,
    screen: &ScreenWindowProfile,
    webgl1: Value,
    webgl2: Value,
    webgpu: Value,
) -> Result<ResolvedFingerprintProfile, ProfileError> {
    let base_id = &base.id;
    let graphics_id = &graphics.id;
    let screen_id = &screen.id;
    let major = browser_major(&base.browser_version)?;
    if graphics_major_weight(graphics, major) == 0 {
        return Err(ProfileError::Catalog(format!(
            "graphics profile {graphics_id} was not observed in Chrome {major}"
        )));
    }
    let id = composed_id(major, base_id, graphics_id, screen_id);
    let mut seed_hasher = Sha256::new();
    seed_hasher.update(b"graphics-render-v1");
    seed_hasher.update(id.as_bytes());
    let render_seed = hex(&seed_hasher.finalize());
    let browser = BrowserIdentity {
        major,
        version: base.browser_version.clone(),
        user_agent: base.user_agent.clone(),
    };
    let navigator = NavigatorIdentity {
        platform: "Win32".to_string(),
        ua_platform: base.platform.clone(),
        ua_platform_version: base.platform_version.clone(),
        architecture: base.architecture.clone(),
        bitness: base.bitness.clone(),
        brands: base.brands.clone(),
        full_version_list: base.full_version_list.clone(),
        languages: base.languages.clone(),
        hardware_concurrency: base.hardware_concurrency,
        device_memory: base.device_memory,
        max_touch_points: base.max_touch_points,
    };
    let network = network_identity(base_id);
    let runtime = RuntimeFingerprint {
        id: &id,
        catalog_id,
        render_seed: &render_seed,
        browser: &browser,
        navigator: &navigator,
        network: &network,
        screen,
        graphics: RuntimeGraphics {
            id: &graphics.id,
            masked_vendor: &graphics.masked_vendor,
            masked_renderer: &graphics.masked_renderer,
            unmasked_vendor: &graphics.unmasked_vendor,
            unmasked_renderer: &graphics.unmasked_renderer,
            preferred_canvas_format: &graphics.preferred_canvas_format,
            wgsl_language_features: &graphics.wgsl_language_features,
            webgl1: &webgl1,
            webgl2: &webgl2,
            webgpu: &webgpu,
        },
    };
    let runtime_json = serde_json::to_string(&runtime)
        .map_err(|error| ProfileError::Serialization(error.to_string()))?;
    Ok(ResolvedFingerprintProfile {
        id,
        browser,
        navigator,
        network,
        screen: screen.clone(),
        graphics: graphics.clone(),
        webgl1,
        webgl2,
        webgpu,
        render_seed,
        runtime_json,
    })
}

fn escape_client_hint(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn network_identity(base_id: &str) -> NetworkIdentity {
    let mut hasher = Sha256::new();
    hasher.update(b"network-profile-v1");
    hasher.update(base_id.as_bytes());
    let digest = hasher.finalize();
    NetworkIdentity {
        downlink: f64::from(26 + digest[0] % 9) / 20.0,
        rtt: 50 + u32::from(digest[1] % 5) * 25,
        effective_type: "4g".to_string(),
        save_data: false,
    }
}

fn component_value(components: &[CatalogComponent], id: &str) -> Result<Value, ProfileError> {
    let component = components
        .iter()
        .find(|component| component.id == id)
        .ok_or_else(|| ProfileError::Catalog(format!("component {id} is missing")))?;
    component_value_owned(component.clone())
}

fn webgpu_component_value(
    components: &CatalogComponents,
    id: &str,
) -> Result<Value, ProfileError> {
    let component = components
        .webgpu
        .iter()
        .find(|component| component.id == id)
        .ok_or_else(|| ProfileError::Catalog(format!("component {id} is missing")))?;
    expand_webgpu_component(
        component,
        &components.webgpu_adapters,
        &components.webgpu_limits,
    )
}

fn expand_webgpu_component(
    component: &CatalogWebGpuComponent,
    adapters: &[CatalogWebGpuAdapterComponent],
    limits: &[CatalogWebGpuLimitsComponent],
) -> Result<Value, ProfileError> {
    let mut expanded = serde_json::Map::new();
    for (name, adapter_id) in &component.adapters {
        let adapter = adapters
            .iter()
            .find(|row| row.id == *adapter_id)
            .ok_or_else(|| ProfileError::Catalog(format!("WebGPU adapter {adapter_id} is missing")))?;
        let adapter_limits = limits
            .iter()
            .find(|row| row.id == adapter.limits_id)
            .ok_or_else(|| ProfileError::Catalog(format!("WebGPU limits {} are missing", adapter.limits_id)))?;
        let device_limits = limits
            .iter()
            .find(|row| row.id == adapter.default_device_limits_id)
            .ok_or_else(|| {
                ProfileError::Catalog(format!(
                    "WebGPU limits {} are missing",
                    adapter.default_device_limits_id
                ))
            })?;
        expanded.insert(
            name.clone(),
            serde_json::json!({
                "info": adapter.info,
                "features": adapter.features,
                "limits": adapter_limits.values,
                "defaultDeviceLimits": device_limits.values,
            }),
        );
    }
    Ok(serde_json::json!({ "adapters": expanded }))
}

fn component_value_owned(component: CatalogComponent) -> Result<Value, ProfileError> {
    serde_json::to_value(component.data)
        .map_err(|error| ProfileError::Serialization(error.to_string()))
}

fn env_enabled(key: &str) -> bool {
    matches!(
        std::env::var(key)
            .ok()
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn embedded_catalog_is_valid_and_bounded() {
        let catalog = catalog().unwrap();
        assert_eq!(catalog.catalog_id, CATALOG_ID);
        assert!(CATALOG_GZIP.len() < 2 * 1024 * 1024);
        assert_eq!(catalog.base_profile_count(), 367);
        assert_eq!(catalog.screen_profile_count(), 226);
        assert_eq!(catalog.graphics_profile_count(), 427);
        let index: Value = serde_json::from_str(&catalog.index_json().unwrap()).unwrap();
        assert_eq!(index["defaultBrowserMajor"], 145);
        assert_eq!(index["graphicsApiBrowserMajor"], 145);
        let majors: std::collections::BTreeSet<u32> = catalog
            .base_profiles
            .iter()
            .map(|row| browser_major(&row.browser_version).unwrap())
            .collect();
        assert_eq!(majors, [143, 144, 145, 147, 148, 150].into_iter().collect());
    }

    #[test]
    fn fixed_default_is_stable() {
        let catalog = catalog().unwrap();
        let first = resolve_with_options(catalog, None, false, None).unwrap().0;
        let second = resolve_with_options(catalog, Some("0"), true, Some([255; 24])).unwrap().0;
        let fast = default_profile().unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(first.render_seed, second.render_seed);
        assert_eq!(first.runtime_json, second.runtime_json);
        assert_eq!(first.id, fast.id);
        assert_eq!(first.runtime_json, fast.runtime_json);

        let runtime: Value = serde_json::from_str(first.runtime_json()).unwrap();
        assert!(runtime["network"]["downlink"].as_f64().unwrap() > 0.0);
        assert!(runtime["network"]["rtt"].as_u64().unwrap() > 0);
        assert_eq!(runtime["network"]["effectiveType"], "4g");
        let ua_header = first.navigator.sec_ch_ua_header();
        for brand in &first.navigator.brands {
            assert!(ua_header.contains(&format!(
                "\"{}\";v=\"{}\"",
                brand.brand, brand.version
            )));
        }
        assert_eq!(
            first.navigator.sec_ch_ua_platform_header(),
            format!("\"{}\"", first.navigator.ua_platform)
        );
        let mut navigator = first.navigator.clone();
        navigator.languages = vec!["ru-RU", "en-US", "ru", "en"]
            .into_iter()
            .map(str::to_string)
            .collect();
        assert_eq!(
            navigator.accept_language_header(),
            "ru-RU,en-US;q=0.9,ru;q=0.8,en;q=0.7"
        );
    }

    #[test]
    fn numeric_seed_and_exact_pin_are_stable() {
        let catalog = catalog().unwrap();
        let seeded = resolve_with_options(catalog, Some("123456"), false, None).unwrap().0;
        let seeded_again = resolve_with_options(catalog, Some("123456"), false, None).unwrap().0;
        assert_eq!(seeded.id, seeded_again.id);
        let pinned = resolve_with_options(catalog, Some(&seeded.id), false, None).unwrap().0;
        assert_eq!(seeded.id, pinned.id);
    }

    #[test]
    fn catalog_index_lists_selectable_rows() {
        let index: Value = serde_json::from_str(&catalog().unwrap().index_json().unwrap()).unwrap();
        assert_eq!(index["catalogId"], CATALOG_ID);
        assert_eq!(index["baseProfiles"].as_array().unwrap().len(), 367);
        assert_eq!(index["graphicsProfiles"].as_array().unwrap().len(), 427);
        assert_eq!(index["screenProfiles"].as_array().unwrap().len(), 226);
        assert!(index["defaultProfileId"].as_str().unwrap().starts_with("c145w1:"));
        assert!(index.get("components").is_none());
    }

    #[test]
    fn exact_profile_lookup_rejects_unknown_ids() {
        let default_id: String = serde_json::from_str::<Value>(&catalog().unwrap().index_json().unwrap())
            .unwrap()["defaultProfileId"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(resolve_profile_id(&default_id).unwrap().id, default_id);
        assert!(matches!(
            resolve_profile_id("c145w1:bad:bad:bad"),
            Err(ProfileError::Selector(_))
        ));
    }

    #[test]
    fn every_captured_browser_major_is_selectable() {
        let catalog = catalog().unwrap();
        let screen_id = catalog.default_composition.screen_id.as_str();
        for major in [143, 144, 145, 147, 148, 150] {
            let base = catalog
                .base_profiles
                .iter()
                .find(|row| browser_major(&row.browser_version).unwrap() == major)
                .unwrap();
            let graphics = catalog
                .graphics_profiles
                .iter()
                .find(|row| graphics_major_weight(row, major) > 0)
                .unwrap();
            let id = composed_id(major, &base.id, &graphics.id, screen_id);
            let selected = resolve_profile_id(&id).unwrap();
            assert_eq!(selected.browser.major, major);
            assert_eq!(selected.id, id);
        }
    }

    #[test]
    fn invalid_selector_uses_default_and_marks_fallback() {
        let catalog = catalog().unwrap();
        let default = resolve_with_options(catalog, None, false, None).unwrap().0;
        let (fallback, used_fallback) =
            resolve_with_options(catalog, Some("c145w1:bad:bad:bad"), false, None).unwrap();
        assert!(used_fallback);
        assert_eq!(fallback.id, default.id);
    }

    #[test]
    fn rotation_draws_each_table_independently() {
        let catalog = catalog().unwrap();
        let low = resolve_with_options(catalog, None, true, Some([0; 24])).unwrap().0;
        let high = resolve_with_options(catalog, None, true, Some([255; 24])).unwrap().0;
        assert_ne!(low.id, high.id);
    }

    #[test]
    fn runtime_profile_is_registered_and_listed() {
        let id = "c150w1:11111111111111111111111111111111:22222222222222222222222222222222:33333333333333333333333333333333";
        let mut seed_hasher = Sha256::new();
        seed_hasher.update(b"graphics-render-v1");
        seed_hasher.update(id.as_bytes());
        let value = json!({
            "id": id,
            "catalogId": CATALOG_ID,
            "renderSeed": hex(&seed_hasher.finalize()),
            "browser": { "major": 150, "version": "150.0.1.1", "userAgent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/150.0.0.0 Safari/537.36" },
            "navigator": {
                "platform": "Win32", "uaPlatform": "Windows", "uaPlatformVersion": "19.0.0",
                "architecture": "x86", "bitness": "64", "brands": [], "fullVersionList": [],
                "languages": ["en-US"], "hardwareConcurrency": 8, "deviceMemory": 8.0, "maxTouchPoints": 0
            },
            "network": { "downlink": 1.7, "rtt": 75, "effectiveType": "4g", "saveData": false },
            "screen": {
                "id": "33333333333333333333333333333333", "width": 1920, "height": 1080,
                "availWidth": 1920, "availHeight": 1040, "availLeft": 0, "availTop": 0,
                "colorDepth": 24, "pixelDepth": 24, "devicePixelRatio": 1.0,
                "innerWidth": 1200, "innerHeight": 800, "outerWidth": 1200, "outerHeight": 900,
                "screenX": 0, "screenY": 0, "weight": 1
            },
            "graphics": {
                "id": "22222222222222222222222222222222", "maskedVendor": "WebKit", "maskedRenderer": "WebKit WebGL",
                "unmaskedVendor": "Google Inc. (NVIDIA)", "unmaskedRenderer": "ANGLE (NVIDIA, Test Direct3D11, D3D11)",
                "webgl1Id": "44444444444444444444444444444444", "webgl2Id": "55555555555555555555555555555555", "webgpuId": "66666666666666666666666666666666",
                "preferredCanvasFormat": "bgra8unorm", "wgslLanguageFeatures": [],
                "observationsByBrowserVersion": { "150.0.1.1": 1 }, "weight": 1,
                "webgl1": {}, "webgl2": {}, "webgpu": { "adapters": { "default": {} } }
            }
        });
        assert_eq!(register_runtime_profile(&value).unwrap(), id);
        assert_eq!(resolve_profile_id(id).unwrap().id, id);
        let index: Value = serde_json::from_str(&catalog().unwrap().index_json_with_runtime().unwrap()).unwrap();
        assert!(index["graphicsProfiles"].as_array().unwrap().iter().any(|row| row["id"] == "22222222222222222222222222222222"));
    }
}
