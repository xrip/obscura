use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::sync::{Arc, OnceLock};

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

const CATALOG_GZIP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/chrome-145-windows-v1.json.gz"));
const DEFAULT_CATALOG_JSON: &str = include_str!(concat!(env!("OUT_DIR"), "/chrome-145-windows-v1.default.json"));
const CATALOG_ID: &str = "chrome-145-windows-v1";
const COMPOSED_PREFIX: &str = "c145w1";

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
    browser_major: u32,
    browser_revision: String,
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
    pub weight: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct CatalogComponent {
    id: String,
    #[serde(flatten)]
    data: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct CatalogComponents {
    webgl1: Vec<CatalogComponent>,
    webgl2: Vec<CatalogComponent>,
    webgpu: Vec<CatalogComponent>,
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
    webgpu: CatalogComponent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserIdentity {
    pub version: String,
    pub user_agent: String,
}

#[derive(Debug, Clone, Serialize)]
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
    screen: &'a ScreenWindowProfile,
    graphics: RuntimeGraphics<'a>,
}

#[derive(Debug, Clone)]
pub struct ResolvedFingerprintProfile {
    pub id: String,
    pub browser: BrowserIdentity,
    pub navigator: NavigatorIdentity,
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
            default_profile_id: String,
            base_profiles: &'a [BaseCatalogProfile],
            graphics_profiles: &'a [GraphicsProfile],
            screen_profiles: &'a [ScreenWindowProfile],
        }

        let ids = &self.default_composition;
        serde_json::to_string_pretty(&CatalogIndex {
            schema_version: self.schema_version,
            catalog_id: &self.catalog_id,
            default_profile_id: format!(
                "{COMPOSED_PREFIX}:{}:{}:{}",
                ids.base_id, ids.graphics_id, ids.screen_id
            ),
            base_profiles: &self.base_profiles,
            graphics_profiles: &self.graphics_profiles,
            screen_profiles: &self.screen_profiles,
        })
        .map_err(|error| ProfileError::Serialization(error.to_string()))
    }
}

static CATALOG: OnceLock<Result<FingerprintCatalog, ProfileError>> = OnceLock::new();
static DEFAULT_PROFILE: OnceLock<Result<Arc<ResolvedFingerprintProfile>, ProfileError>> = OnceLock::new();
static WARNED_INVALID_SELECTOR: OnceLock<()> = OnceLock::new();

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
    if !bases.contains(catalog.default_composition.base_id.as_str())
        || !screens.contains(catalog.default_composition.screen_id.as_str())
        || !graphics.contains(catalog.default_composition.graphics_id.as_str())
    {
        return Err(ProfileError::Catalog("default composition has an unknown ID".to_string()));
    }
    for row in &catalog.graphics_profiles {
        if row.weight == 0
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
    if catalog.base_profiles.iter().any(|row| row.weight == 0)
        || catalog.screen_profiles.iter().any(|row| row.weight == 0)
    {
        return Err(ProfileError::Catalog("a profile has zero weight".to_string()));
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
        || target.browser_major != 145
        || target.browser_revision != "145.0.7632.75"
        || target.os != "Windows"
        || target.graphics_backend != "ANGLE/D3D11"
    {
        return Err(ProfileError::Catalog("target is not Chrome 145 Windows ANGLE/D3D11".to_string()));
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
    let webgpu = component_value_owned(selected.webgpu)?;
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
        } else if selector.starts_with("c145w1:") {
            match parse_composed_selector(catalog, selector) {
                Some(ids) => (ids, false),
                None => (default, true),
            }
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
            (
                (
                    weighted_pick(&catalog.base_profiles, base_draw, |row| row.weight, |row| row.id.as_str()),
                    weighted_pick(&catalog.graphics_profiles, graphics_draw, |row| row.weight, |row| row.id.as_str()),
                    weighted_pick(&catalog.screen_profiles, screen_draw, |row| row.weight, |row| row.id.as_str()),
                ),
                false,
            )
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
    if parts.next()? != COMPOSED_PREFIX {
        return None;
    }
    let base = parts.next()?;
    let graphics = parts.next()?;
    let screen = parts.next()?;
    if parts.next().is_some()
        || !catalog.base_profiles.iter().any(|row| row.id == base)
        || !catalog.graphics_profiles.iter().any(|row| row.id == graphics)
        || !catalog.screen_profiles.iter().any(|row| row.id == screen)
    {
        return None;
    }
    Some((
        catalog.base_profiles.iter().find(|row| row.id == base)?.id.as_str(),
        catalog.graphics_profiles.iter().find(|row| row.id == graphics)?.id.as_str(),
        catalog.screen_profiles.iter().find(|row| row.id == screen)?.id.as_str(),
    ))
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
    (
        weighted_pick(&catalog.base_profiles, draw("base"), |row| row.weight, |row| row.id.as_str()),
        weighted_pick(&catalog.graphics_profiles, draw("graphics"), |row| row.weight, |row| row.id.as_str()),
        weighted_pick(&catalog.screen_profiles, draw("screen"), |row| row.weight, |row| row.id.as_str()),
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
    let webgpu = component_value(&catalog.components.webgpu, &graphics.webgpu_id)?;
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
    let id = format!("{COMPOSED_PREFIX}:{base_id}:{graphics_id}:{screen_id}");
    let mut seed_hasher = Sha256::new();
    seed_hasher.update(b"graphics-render-v1");
    seed_hasher.update(id.as_bytes());
    let render_seed = hex(&seed_hasher.finalize());
    let browser = BrowserIdentity {
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
    let runtime = RuntimeFingerprint {
        id: &id,
        catalog_id,
        render_seed: &render_seed,
        browser: &browser,
        navigator: &navigator,
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
        screen: screen.clone(),
        graphics: graphics.clone(),
        webgl1,
        webgl2,
        webgpu,
        render_seed,
        runtime_json,
    })
}

fn component_value(components: &[CatalogComponent], id: &str) -> Result<Value, ProfileError> {
    let component = components
        .iter()
        .find(|component| component.id == id)
        .ok_or_else(|| ProfileError::Catalog(format!("component {id} is missing")))?;
    component_value_owned(component.clone())
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

    #[test]
    fn embedded_catalog_is_valid_and_bounded() {
        let catalog = catalog().unwrap();
        assert_eq!(catalog.catalog_id, CATALOG_ID);
        assert!(CATALOG_GZIP.len() < 2 * 1024 * 1024);
        assert_eq!(catalog.base_profile_count(), 77);
        assert_eq!(catalog.screen_profile_count(), 225);
        assert_eq!(catalog.graphics_profile_count(), 298);
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
        assert_eq!(index["baseProfiles"].as_array().unwrap().len(), 77);
        assert_eq!(index["graphicsProfiles"].as_array().unwrap().len(), 298);
        assert_eq!(index["screenProfiles"].as_array().unwrap().len(), 225);
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
}
