use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const CATALOG_ID: &str = "chrome-windows-v1";
const MAX_CATALOG_BYTES: usize = 2 * 1024 * 1024;
const MASKED_VENDOR: &str = "WebKit";
const MASKED_RENDERER: &str = "WebKit WebGL";
const DEFAULT_BROWSER_MAJOR: &str = "145";
const GRAPHICS_API_BROWSER_MAJOR: u32 = 145;
const GRAPHICS_API_REVISION: &str = "145.0.7632.75";
const WREQ_BROWSER_MAJORS: &[u32] = &[
    100, 101, 104, 105, 106, 107, 108, 109, 110, 114, 116, 117, 118, 119, 120,
    123, 124, 126, 127, 128, 129, 130, 131, 132, 133, 134, 135, 136, 137, 138,
    139, 140, 141, 142, 143, 144, 145, 146, 147, 148,
];

#[derive(Parser)]
#[command(name = "fingerprint-catalog")]
#[command(about = "Build the Obscura Chrome Windows fingerprint catalog")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Generate(GenerateArgs),
}

#[derive(Args, Clone)]
struct GenerateArgs {
    #[arg(long)]
    profiles: PathBuf,
    #[arg(long)]
    windows: PathBuf,
    #[arg(long)]
    out: PathBuf,
    #[arg(long)]
    schema: PathBuf,
    #[arg(long)]
    report: PathBuf,
    #[arg(long)]
    sources: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct SourceDigests {
    windows_sha256: String,
    windows_bytes: u64,
    profiles_sha256: String,
    profiles_files: u64,
    profiles_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Target {
    browser: String,
    default_browser_major: u32,
    graphics_api_browser_major: u32,
    graphics_api_revision: String,
    transport_browser_majors: Vec<u32>,
    os: String,
    graphics_backend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Composition {
    base_id: String,
    graphics_id: String,
    screen_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct BrandVersion {
    brand: String,
    version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct BaseContent {
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BaseProfile {
    id: String,
    #[serde(flatten)]
    content: BaseContent,
    weight: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ScreenContent {
    width: u32,
    height: u32,
    avail_width: u32,
    avail_height: u32,
    avail_left: i32,
    avail_top: i32,
    color_depth: u32,
    pixel_depth: u32,
    device_pixel_ratio: f64,
    inner_width: u32,
    inner_height: u32,
    outer_width: u32,
    outer_height: u32,
    screen_x: i32,
    screen_y: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScreenProfile {
    id: String,
    #[serde(flatten)]
    content: ScreenContent,
    weight: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ParameterValue {
    #[serde(rename = "type")]
    type_tag: String,
    value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ExtensionConstant {
    name: String,
    constant_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct PrecisionFormat {
    shader_type: u32,
    precision_type: u32,
    range_min: i32,
    range_max: i32,
    precision: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct WebGlContent {
    context_attributes: BTreeMap<String, Value>,
    parameters: BTreeMap<String, ParameterValue>,
    initial_state: BTreeMap<String, ParameterValue>,
    extensions: BTreeMap<String, ExtensionConstant>,
    supported_extensions: Vec<String>,
    shader_precision_formats: Vec<PrecisionFormat>,
    version: String,
    shading_language_version: String,
    max_anisotropy: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_draw_buffers_webgl: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebGlComponent {
    id: String,
    #[serde(flatten)]
    content: WebGlContent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct AdapterInfo {
    vendor: String,
    architecture: String,
    device: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    subgroup_min_size: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    subgroup_max_size: Option<u32>,
    is_fallback_adapter: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct WebGpuAdapterContent {
    info: AdapterInfo,
    features: Vec<String>,
    limits: BTreeMap<String, Value>,
    default_device_limits: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct WebGpuContent {
    adapters: BTreeMap<String, WebGpuAdapterContent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebGpuComponent {
    id: String,
    adapters: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebGpuAdapterComponent {
    id: String,
    info: AdapterInfo,
    features: Vec<String>,
    limits_id: String,
    default_device_limits_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebGpuLimitsComponent {
    id: String,
    values: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct GraphicsContent {
    masked_vendor: String,
    masked_renderer: String,
    unmasked_vendor: String,
    unmasked_renderer: String,
    webgl1_id: String,
    webgl2_id: String,
    webgpu_id: String,
    preferred_canvas_format: String,
    wgsl_language_features: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphicsProfile {
    id: String,
    #[serde(flatten)]
    content: GraphicsContent,
    observations_by_browser_version: BTreeMap<String, u64>,
    weight: u64,
}

#[derive(Debug)]
struct GraphicsAggregate {
    content: GraphicsContent,
    observations_by_browser_version: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Components {
    webgl1: Vec<WebGlComponent>,
    webgl2: Vec<WebGlComponent>,
    webgpu: Vec<WebGpuComponent>,
    webgpu_adapters: Vec<WebGpuAdapterComponent>,
    webgpu_limits: Vec<WebGpuLimitsComponent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FingerprintCatalog {
    schema_version: u32,
    catalog_id: String,
    target: Target,
    source_digests: SourceDigests,
    default_composition: Composition,
    base_profiles: Vec<BaseProfile>,
    screen_profiles: Vec<ScreenProfile>,
    graphics_profiles: Vec<GraphicsProfile>,
    components: Components,
}

#[derive(Default)]
struct CollisionGuard {
    values: HashMap<String, Vec<u8>>,
}

impl CollisionGuard {
    fn id<T: Serialize>(&mut self, value: &T) -> Result<String> {
        let bytes = serde_json::to_vec(value)?;
        let digest = Sha256::digest(&bytes);
        let id = hex(&digest[..16]);
        if let Some(old) = self.values.get(&id) {
            if old != &bytes {
                bail!("content ID collision for {id}");
            }
        } else {
            self.values.insert(id.clone(), bytes);
        }
        Ok(id)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Reject {
    source_index: usize,
    reason: String,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Generate(args) => generate(&args),
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn read_json(path: &Path) -> Result<(Vec<u8>, Value)> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let value = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse {}", path.display()))?;
    Ok((bytes, value))
}

fn value_at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a Value> {
    let mut current = value;
    for key in path {
        current = current
            .get(*key)
            .ok_or_else(|| anyhow!("missing {}", path.join(".")))?;
    }
    Ok(current)
}

fn string_at(value: &Value, path: &[&str]) -> Result<String> {
    value_at(value, path)?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("{} is not a non-empty string", path.join(".")))
}

fn u32_at(value: &Value, path: &[&str]) -> Result<u32> {
    let n = value_at(value, path)?
        .as_u64()
        .ok_or_else(|| anyhow!("{} is not an unsigned integer", path.join(".")))?;
    u32::try_from(n).map_err(|_| anyhow!("{} is out of range", path.join(".")))
}

fn i32_field(value: &Value, key: &str) -> Result<i32> {
    let n = value
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("{key} is not an integer"))?;
    i32::try_from(n).map_err(|_| anyhow!("{key} is out of range"))
}

fn u32_field(value: &Value, key: &str) -> Result<u32> {
    let n = value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("{key} is not an unsigned integer"))?;
    u32::try_from(n).map_err(|_| anyhow!("{key} is out of range"))
}

fn f64_at(value: &Value, path: &[&str]) -> Result<f64> {
    let n = value_at(value, path)?
        .as_f64()
        .ok_or_else(|| anyhow!("{} is not a number", path.join(".")))?;
    if !n.is_finite() {
        bail!("{} is not finite", path.join("."));
    }
    Ok(n)
}

fn f64_field(value: &Value, key: &str) -> Result<f64> {
    let n = value
        .get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| anyhow!("{key} is not a number"))?;
    if !n.is_finite() {
        bail!("{key} is not finite");
    }
    Ok(n)
}

fn string_list(value: &Value, path: &[&str]) -> Result<Vec<String>> {
    let list = value_at(value, path)?
        .as_array()
        .ok_or_else(|| anyhow!("{} is not an array", path.join(".")))?;
    let mut out = Vec::with_capacity(list.len());
    for item in list {
        let s = item
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("{} has an invalid string", path.join(".")))?;
        out.push(s.to_owned());
    }
    if out.is_empty() {
        bail!("{} is empty", path.join("."));
    }
    Ok(out)
}

fn string_list_allow_empty(value: &Value, path: &[&str]) -> Result<Vec<String>> {
    let list = value_at(value, path)?
        .as_array()
        .ok_or_else(|| anyhow!("{} is not an array", path.join(".")))?;
    list.iter()
        .map(|item| {
            item.as_str()
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| anyhow!("{} has an invalid string", path.join(".")))
        })
        .collect()
}

fn brand_list(value: &Value, path: &[&str]) -> Result<Vec<BrandVersion>> {
    let list = value_at(value, path)?
        .as_array()
        .ok_or_else(|| anyhow!("{} is not an array", path.join(".")))?;
    let mut out = Vec::with_capacity(list.len());
    for item in list {
        out.push(BrandVersion {
            brand: string_at(item, &["brand"])?,
            version: string_at(item, &["version"])?,
        });
    }
    if out.is_empty() {
        bail!("{} is empty", path.join("."));
    }
    Ok(out)
}

fn chrome_major(version: &str) -> Result<&str> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 4
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        bail!("browser version is not four numeric parts");
    }
    let major = parts[0];
    Ok(major)
}

fn parse_windows_browser_version(value: &Value) -> Result<String> {
    let browser_version = string_at(value, &["fingerprints", "browser", "version"])?;
    let major = chrome_major(&browser_version)?;
    let os_type = string_at(value, &["fingerprints", "system", "osType"])?;
    if os_type != "win" {
        bail!("operating system is not Windows");
    }
    let ua_data = value_at(value, &["fingerprints", "browser", "userAgentData"])?;
    if string_at(ua_data, &["platform"])? != "Windows" {
        bail!("UA data platform is not Windows");
    }
    if string_at(ua_data, &["uaFullVersion"])? != browser_version {
        bail!("browser and UA data full versions differ");
    }
    let user_agent = string_at(value, &["fingerprints", "browser", "userAgent"])?;
    let navigator_user_agent =
        string_at(value, &["fingerprints", "browser", "navigator", "userAgent"])?;
    if user_agent != navigator_user_agent {
        bail!("browser and navigator user agents differ");
    }
    let reduced_version = format!("Chrome/{major}.0.0.0");
    if !user_agent.contains("(Windows NT 10.0; Win64; x64)")
        || !user_agent.contains(&reduced_version)
    {
        bail!("user agent does not match the browser major and Windows x64");
    }
    let full_version_list = brand_list(ua_data, &["fullVersionList"])?;
    if !full_version_list.iter().any(|item| {
        (item.brand == "Google Chrome" || item.brand == "Chromium")
            && item.version == browser_version
    }) {
        bail!("UA data full version list does not match the browser version");
    }
    Ok(browser_version)
}

fn parse_base_profile(value: &Value) -> Result<BaseContent> {
    let browser_version = string_at(value, &["fingerprints", "browser", "version"])?;
    let browser_major = chrome_major(&browser_version)?;

    let user_agent = string_at(value, &["fingerprints", "browser", "userAgent"])?;
    let navigator_user_agent =
        string_at(value, &["fingerprints", "browser", "navigator", "userAgent"])?;
    if user_agent != navigator_user_agent {
        bail!("browser and navigator user agents differ");
    }
    if !user_agent.contains("(Windows NT 10.0; Win64; x64)")
        || !user_agent.contains(&format!("Chrome/{browser_major}.0.0.0"))
    {
        bail!("user agent is not reduced Chrome Windows x64 for the browser major");
    }

    let os_type = string_at(value, &["fingerprints", "system", "osType"])?;
    if os_type != "win" {
        bail!("operating system is not Windows");
    }

    let ua_data = value_at(value, &["fingerprints", "browser", "userAgentData"])?;
    let platform = string_at(ua_data, &["platform"])?;
    if platform != "Windows" {
        bail!("UA data platform is not Windows");
    }
    let platform_version = string_at(ua_data, &["platformVersion"])?;
    let captured_os_version = string_at(value, &["fingerprints", "system", "osVersion"])?;
    if platform_version != captured_os_version {
        bail!("UA data and system platform versions differ");
    }
    let architecture = string_at(ua_data, &["architecture"])?;
    let bitness = string_at(ua_data, &["bitness"])?;
    let cpu_arch = string_at(value, &["fingerprints", "hardware", "cpu", "arch"])?;
    let cpu_bitness = string_at(value, &["fingerprints", "hardware", "cpu", "bitness"])?;
    if architecture != cpu_arch || bitness != cpu_bitness {
        bail!("UA data and CPU architecture differ");
    }
    if architecture != "x86" || bitness != "64" {
        bail!("architecture is not x86-64");
    }

    let full_version = string_at(ua_data, &["uaFullVersion"])?;
    if full_version != browser_version {
        bail!("browser and UA data full versions differ");
    }
    let brands = brand_list(ua_data, &["brands"])?;
    let full_version_list = brand_list(ua_data, &["fullVersionList"])?;
    let brand_major_ok = brands.iter().any(|item| {
        (item.brand == "Google Chrome" || item.brand == "Chromium")
            && item.version == browser_major
    });
    let full_version_ok = full_version_list.iter().any(|item| {
        (item.brand == "Google Chrome" || item.brand == "Chromium")
            && item.version == browser_version
    });
    if !brand_major_ok || !full_version_ok {
        bail!("UA data brand versions do not match the browser version");
    }

    let languages = string_list(
        value,
        &["fingerprints", "browser", "navigator", "languages"],
    )?;
    let hardware_concurrency = u32_at(
        value,
        &["fingerprints", "browser", "navigator", "hardwareConcurrency"],
    )?;
    let device_memory = f64_at(
        value,
        &["fingerprints", "browser", "navigator", "deviceMemory"],
    )?;
    let max_touch_points = u32_at(
        value,
        &["fingerprints", "browser", "navigator", "maxTouchPoints"],
    )?;
    if hardware_concurrency == 0 || hardware_concurrency > 1024 {
        bail!("hardwareConcurrency is out of range");
    }
    if device_memory <= 0.0 || device_memory > 1024.0 {
        bail!("deviceMemory is out of range");
    }
    if max_touch_points > 1024 {
        bail!("maxTouchPoints is out of range");
    }

    Ok(BaseContent {
        browser_version,
        user_agent,
        brands,
        full_version_list,
        platform,
        platform_version,
        architecture,
        bitness,
        languages,
        hardware_concurrency,
        device_memory,
        max_touch_points,
    })
}

fn parse_screen_profile(screen: &Value, window: &Value) -> Result<ScreenContent> {
    let profile = ScreenContent {
        width: u32_field(screen, "width")?,
        height: u32_field(screen, "height")?,
        avail_width: u32_field(screen, "availWidth")?,
        avail_height: u32_field(screen, "availHeight")?,
        avail_left: i32_field(screen, "availLeft")?,
        avail_top: i32_field(screen, "availTop")?,
        color_depth: u32_field(screen, "colorDepth")?,
        pixel_depth: u32_field(screen, "pixelDepth")?,
        device_pixel_ratio: f64_field(window, "devicePixelRatio")?,
        inner_width: u32_field(window, "innerWidth")?,
        inner_height: u32_field(window, "innerHeight")?,
        outer_width: u32_field(window, "outerWidth")?,
        outer_height: u32_field(window, "outerHeight")?,
        screen_x: i32_field(window, "screenX")?,
        screen_y: i32_field(window, "screenY")?,
    };
    if profile.width == 0
        || profile.height == 0
        || profile.width > 100_000
        || profile.height > 100_000
        || profile.device_pixel_ratio <= 0.0
        || profile.device_pixel_ratio > 10.0
    {
        bail!("screen or window dimensions are out of range");
    }
    Ok(profile)
}

fn numeric_object_to_array(value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return value.clone();
    };
    if object.is_empty() || object.keys().any(|key| key.parse::<usize>().is_err()) {
        return value.clone();
    }
    let mut entries: Vec<(usize, Value)> = object
        .iter()
        .filter_map(|(key, value)| key.parse::<usize>().ok().map(|index| (index, value.clone())))
        .collect();
    entries.sort_by_key(|entry| entry.0);
    if entries.iter().enumerate().any(|(index, entry)| index != entry.0) {
        return value.clone();
    }
    Value::Array(entries.into_iter().map(|entry| entry.1).collect())
}

fn mutable_webgl_parameter(pname: u32) -> bool {
    matches!(
        pname,
        2849
            | 2884..=2886
            | 2928..=2932
            | 2960..=2968
            | 2978
            | 3024
            | 3042
            | 3074
            | 3088..=3089
            | 3106..=3107
            | 3314..=3317
            | 3330..=3333
            | 10752
            | 32773
            | 32777
            | 32823..=32824
            | 32877..=32878
            | 32926
            | 32928
            | 32938..=32939
            | 32968..=32971
            | 33170
            | 34016
            | 34816..=34819
            | 34853..=34860
            | 34877
            | 35723
            | 35977
            | 36003..=36005
            | 36387..=36388
            | 37440..=37441
            | 37443
    )
}

fn normalize_context_attributes(value: &Value) -> Result<BTreeMap<String, Value>> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("contextAttributes is not an object"))?;
    let allowed = [
        "alpha",
        "antialias",
        "depth",
        "desynchronized",
        "failIfMajorPerformanceCaveat",
        "powerPreference",
        "premultipliedAlpha",
        "preserveDrawingBuffer",
        "stencil",
        "xrCompatible",
    ];
    let mut out = BTreeMap::new();
    for key in allowed {
        if let Some(value) = object.get(key) {
            if !(value.is_boolean() || value.is_string()) {
                bail!("invalid context attribute {key}");
            }
            out.insert(key.to_owned(), value.clone());
        }
    }
    if out.len() < 9 {
        bail!("contextAttributes is missing a core field");
    }
    Ok(out)
}

fn normalize_webgl(value: &Value, webgl2: bool) -> Result<WebGlContent> {
    let context_attributes = normalize_context_attributes(value_at(value, &["contextAttributes"])?)?;
    let raw_parameters = value_at(value, &["parameters"])?
        .as_object()
        .ok_or_else(|| anyhow!("parameters is not an object"))?;
    let mut parameters = BTreeMap::new();
    let mut initial_state = BTreeMap::new();
    for (key, raw) in raw_parameters {
        let type_tag = raw.get("type").and_then(Value::as_str).unwrap_or("");
        if type_tag.is_empty() {
            continue;
        }
        let pname: u32 = key
            .parse()
            .with_context(|| format!("invalid WebGL parameter key {key}"))?;
        if !matches!(
            type_tag,
            "Array" | "Boolean" | "Float32Array" | "Int32Array" | "Number" | "String" | "Uint32Array"
        ) {
            bail!("unsupported WebGL parameter type {type_tag}");
        }
        let entry = ParameterValue {
            type_tag: type_tag.to_owned(),
            value: numeric_object_to_array(raw.get("value").unwrap_or(&Value::Null)),
        };
        if mutable_webgl_parameter(pname) {
            initial_state.insert(key.clone(), entry);
        } else {
            parameters.insert(key.clone(), entry);
        }
    }
    let expected = if webgl2 { 132 } else { 82 };
    if parameters.len() + initial_state.len() < expected {
        bail!("WebGL parameter block has too few valid parameters");
    }

    let raw_extensions = value_at(value, &["extensions"])?
        .as_object()
        .ok_or_else(|| anyhow!("extensions is not an object"))?;
    let mut extensions = BTreeMap::new();
    for (key, raw) in raw_extensions {
        let _: u32 = key
            .parse()
            .with_context(|| format!("invalid WebGL extension enum {key}"))?;
        extensions.insert(
            key.clone(),
            ExtensionConstant {
                name: string_at(raw, &["name"])?,
                constant_name: string_at(raw, &["constantName"])?,
            },
        );
    }

    let mut supported_extensions = string_list(value, &["supportedExtensions"])?;
    supported_extensions.sort();
    supported_extensions.dedup();

    let raw_precision = value_at(value, &["shaderPrecisionFormats"])?
        .as_array()
        .ok_or_else(|| anyhow!("shaderPrecisionFormats is not an array"))?;
    let mut shader_precision_formats = Vec::with_capacity(raw_precision.len());
    for raw in raw_precision {
        let format = value_at(raw, &["shaderPrecisionFormat"])?;
        shader_precision_formats.push(PrecisionFormat {
            shader_type: u32_at(raw, &["shaderType"])?,
            precision_type: u32_at(raw, &["precisionType"])?,
            range_min: i32_field(format, "rangeMin")?,
            range_max: i32_field(format, "rangeMax")?,
            precision: i32_field(format, "precision")?,
        });
    }
    shader_precision_formats.sort_by_key(|item| (item.shader_type, item.precision_type));
    if shader_precision_formats.len() != 12 {
        bail!("shaderPrecisionFormats does not have 12 entries");
    }

    Ok(WebGlContent {
        context_attributes,
        parameters,
        initial_state,
        extensions,
        supported_extensions,
        shader_precision_formats,
        version: string_at(value, &["version"])?,
        shading_language_version: string_at(value, &["shadingLanguageVersion"])?,
        max_anisotropy: f64_at(value, &["maxAnisotropy"])?,
        max_draw_buffers_webgl: value
            .get("maxDrawBuffersWebgl")
            .and_then(Value::as_u64)
            .map(u32::try_from)
            .transpose()
            .map_err(|_| anyhow!("maxDrawBuffersWebgl is out of range"))?,
    })
}

fn numeric_map(value: Option<&Value>, label: &str) -> Result<BTreeMap<String, Value>> {
    let object = value
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("{label} is not an object"))?;
    let mut out = BTreeMap::new();
    for (key, value) in object {
        let number = value
            .as_f64()
            .filter(|value| value.is_finite())
            .ok_or_else(|| anyhow!("{label}.{key} is not a finite number"))?;
        let _ = number;
        out.insert(key.clone(), value.clone());
    }
    if out.is_empty() {
        bail!("{label} is empty");
    }
    Ok(out)
}

fn optional_u32(value: Option<&Value>) -> Result<Option<u32>> {
    value
        .map(|value| {
            let n = value
                .as_u64()
                .ok_or_else(|| anyhow!("value is not an unsigned integer"))?;
            u32::try_from(n).map_err(|_| anyhow!("value is out of range"))
        })
        .transpose()
}

fn normalize_webgpu_adapter(value: &Value) -> Result<WebGpuAdapterContent> {
    let raw_info = value
        .get("info")
        .and_then(Value::as_object)
        .filter(|object| !object.is_empty())
        .or_else(|| value.get("adapterInfo").and_then(Value::as_object))
        .ok_or_else(|| anyhow!("adapter info is missing"))?;
    let info_value = Value::Object(raw_info.clone());
    let is_fallback_adapter = raw_info
        .get("isFallbackAdapter")
        .and_then(Value::as_bool)
        .or_else(|| value.get("isFallbackAdapter").and_then(Value::as_bool))
        .ok_or_else(|| anyhow!("isFallbackAdapter is missing"))?;
    let info = AdapterInfo {
        vendor: string_at(&info_value, &["vendor"])?,
        architecture: raw_info
            .get("architecture")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        device: raw_info
            .get("device")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        description: raw_info
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        subgroup_min_size: optional_u32(raw_info.get("subgroupMinSize"))?,
        subgroup_max_size: optional_u32(raw_info.get("subgroupMaxSize"))?,
        is_fallback_adapter,
    };

    let mut features = string_list(value, &["features"])?;
    let mut seen = HashSet::new();
    features.retain(|feature| seen.insert(feature.clone()));
    Ok(WebGpuAdapterContent {
        info,
        features,
        limits: numeric_map(value.get("limits"), "limits")?,
        default_device_limits: numeric_map(value.get("deviceLimits"), "deviceLimits")?,
    })
}

fn normalize_webgpu(value: &Value) -> Result<WebGpuContent> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("adapter is not an object"))?;
    let mut adapters = BTreeMap::new();
    for (source_name, output_name) in [
        ("default", "default"),
        ("low-power", "lowPower"),
        ("high-performance", "highPerformance"),
    ] {
        if let Some(raw) = object.get(source_name) {
            if !raw.is_null() {
                adapters.insert(output_name.to_owned(), normalize_webgpu_adapter(raw)?);
            }
        }
    }
    if !adapters.contains_key("default") {
        bail!("default WebGPU adapter is missing");
    }
    Ok(WebGpuContent { adapters })
}

fn parse_graphics_content(
    value: &Value,
    webgl1_components: &mut BTreeMap<String, WebGlComponent>,
    webgl2_components: &mut BTreeMap<String, WebGlComponent>,
    webgpu_components: &mut BTreeMap<String, WebGpuComponent>,
    webgpu_adapter_components: &mut BTreeMap<String, WebGpuAdapterComponent>,
    webgpu_limits_components: &mut BTreeMap<String, WebGpuLimitsComponent>,
    collisions: &mut CollisionGuard,
) -> Result<GraphicsContent> {
    let webgl1_source = value_at(value, &["fingerprints", "browser", "webglContext"])?;
    let webgl1 = normalize_webgl(webgl1_source, false)?;
    let webgl2_source = value_at(value, &["fingerprints", "browser", "webgl2Context"])?;
    let webgl2 = normalize_webgl(webgl2_source, true)?;
    let gpu = value_at(value, &["fingerprints", "hardware", "gpu"])?;
    let webgpu = normalize_webgpu(value_at(gpu, &["adapter"])?)?;
    let unmasked_vendor = string_at(gpu, &["unmaskedVendor"])?;
    let unmasked_renderer = string_at(gpu, &["unmaskedRenderer"])?;
    if !unmasked_renderer.contains("Direct3D11") || !unmasked_renderer.contains("D3D11") {
        bail!("graphics renderer is not ANGLE/D3D11");
    }
    let preferred_canvas_format = string_at(gpu, &["preferredCanvasFormat"])?;
    if preferred_canvas_format != "bgra8unorm" && preferred_canvas_format != "rgba8unorm" {
        bail!("preferred WebGPU canvas format is invalid");
    }
    let wgsl_language_features =
        string_list_allow_empty(gpu, &["wgslLanguageFeatures"])?;
    let mut seen_wgsl = HashSet::new();
    if wgsl_language_features
        .iter()
        .any(|feature| !seen_wgsl.insert(feature.as_str()))
    {
        bail!("WGSL language feature list has a duplicate");
    }

    let webgl1_id = add_component(
        webgl1,
        webgl1_components,
        collisions,
        |id, content| WebGlComponent { id, content },
    )?;
    let webgl2_id = add_component(
        webgl2,
        webgl2_components,
        collisions,
        |id, content| WebGlComponent { id, content },
    )?;
    let webgpu_id = add_webgpu_component(
        webgpu,
        webgpu_components,
        webgpu_adapter_components,
        webgpu_limits_components,
        collisions,
    )?;
    Ok(GraphicsContent {
        masked_vendor: MASKED_VENDOR.to_owned(),
        masked_renderer: MASKED_RENDERER.to_owned(),
        unmasked_vendor,
        unmasked_renderer,
        webgl1_id,
        webgl2_id,
        webgpu_id,
        preferred_canvas_format,
        wgsl_language_features,
    })
}

fn insert_graphics_observation(
    rows: &mut BTreeMap<Vec<u8>, GraphicsAggregate>,
    content: GraphicsContent,
    browser_version: &str,
) -> Result<()> {
    let key = serde_json::to_vec(&content)?;
    let row = rows.entry(key).or_insert_with(|| GraphicsAggregate {
        content,
        observations_by_browser_version: BTreeMap::new(),
    });
    let weight = row
        .observations_by_browser_version
        .entry(browser_version.to_owned())
        .or_default();
    *weight = weight
        .checked_add(1)
        .ok_or_else(|| anyhow!("graphics observation weight overflow"))?;
    Ok(())
}

fn browser_major_weight(weights: &BTreeMap<String, u64>, major: &str) -> Result<u64> {
    weights.iter().try_fold(0u64, |total, (version, weight)| {
        if chrome_major(version)? == major {
            total
                .checked_add(*weight)
                .ok_or_else(|| anyhow!("graphics browser-major weight overflow"))
        } else {
            Ok(total)
        }
    })
}

fn insert_weighted<T>(rows: &mut BTreeMap<Vec<u8>, (T, u64)>, content: T) -> Result<()>
where
    T: Serialize,
{
    let key = serde_json::to_vec(&content)?;
    if let Some((_, weight)) = rows.get_mut(&key) {
        *weight = weight
            .checked_add(1)
            .ok_or_else(|| anyhow!("observation weight overflow"))?;
    } else {
        rows.insert(key, (content, 1));
    }
    Ok(())
}

fn rank_default<'a, T, F>(items: &'a [T], weight: F) -> Result<&'a T>
where
    F: Fn(&T) -> (u64, &str),
{
    items
        .iter()
        .min_by(|left, right| {
            let (left_weight, left_id) = weight(left);
            let (right_weight, right_id) = weight(right);
            right_weight.cmp(&left_weight).then_with(|| left_id.cmp(right_id))
        })
        .ok_or_else(|| anyhow!("catalog table is empty"))
}

fn sorted_profile_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let mut directories = vec![directory.to_path_buf()];
    while let Some(current) = directories.pop() {
        for entry in fs::read_dir(&current)
            .with_context(|| format!("read profile directory {}", current.display()))?
        {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let path = entry.path();
            if file_type.is_dir() {
                directories.push(path);
            } else if file_type.is_file()
                && path.extension().and_then(|value| value.to_str()) == Some("json")
            {
                paths.push(path);
            }
        }
    }
    paths.sort();
    if paths.is_empty() {
        bail!("profile directory has no JSON files");
    }
    Ok(paths)
}

fn add_webgpu_limits_component(
    values: BTreeMap<String, Value>,
    components: &mut BTreeMap<String, WebGpuLimitsComponent>,
    collisions: &mut CollisionGuard,
) -> Result<String> {
    let id = collisions.id(&values)?;
    components
        .entry(id.clone())
        .or_insert_with(|| WebGpuLimitsComponent {
            id: id.clone(),
            values,
        });
    Ok(id)
}

fn add_webgpu_component(
    content: WebGpuContent,
    components: &mut BTreeMap<String, WebGpuComponent>,
    adapter_components: &mut BTreeMap<String, WebGpuAdapterComponent>,
    limits_components: &mut BTreeMap<String, WebGpuLimitsComponent>,
    collisions: &mut CollisionGuard,
) -> Result<String> {
    let id = collisions.id(&content)?;
    if components.contains_key(&id) {
        return Ok(id);
    }

    let mut adapters = BTreeMap::new();
    for (name, adapter) in content.adapters {
        let adapter_id = collisions.id(&adapter)?;
        if !adapter_components.contains_key(&adapter_id) {
            let limits_id = add_webgpu_limits_component(
                adapter.limits,
                limits_components,
                collisions,
            )?;
            let default_device_limits_id = add_webgpu_limits_component(
                adapter.default_device_limits,
                limits_components,
                collisions,
            )?;
            adapter_components.insert(
                adapter_id.clone(),
                WebGpuAdapterComponent {
                    id: adapter_id.clone(),
                    info: adapter.info,
                    features: adapter.features,
                    limits_id,
                    default_device_limits_id,
                },
            );
        }
        adapters.insert(name, adapter_id);
    }
    components.insert(id.clone(), WebGpuComponent { id: id.clone(), adapters });
    Ok(id)
}

fn add_component<T, O>(
    content: T,
    components: &mut BTreeMap<String, O>,
    collisions: &mut CollisionGuard,
    make: impl FnOnce(String, T) -> O,
) -> Result<String>
where
    T: Serialize,
{
    let id = collisions.id(&content)?;
    if !components.contains_key(&id) {
        components.insert(id.clone(), make(id.clone(), content));
    }
    Ok(id)
}

fn make_schema() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://obscura.dev/schema/chrome-windows-v1.schema.json",
        "title": "Obscura Chrome Windows fingerprint catalog",
        "type": "object",
        "additionalProperties": false,
        "required": [
            "schemaVersion", "catalogId", "target", "sourceDigests",
            "defaultComposition", "baseProfiles", "screenProfiles",
            "graphicsProfiles", "components"
        ],
        "properties": {
            "schemaVersion": { "const": 1 },
            "catalogId": { "const": CATALOG_ID },
            "target": { "$ref": "#/$defs/target" },
            "sourceDigests": { "$ref": "#/$defs/sourceDigests" },
            "defaultComposition": { "$ref": "#/$defs/composition" },
            "baseProfiles": { "type": "array", "minItems": 1, "items": { "$ref": "#/$defs/baseProfile" } },
            "screenProfiles": { "type": "array", "minItems": 1, "items": { "$ref": "#/$defs/screenProfile" } },
            "graphicsProfiles": { "type": "array", "minItems": 1, "items": { "$ref": "#/$defs/graphicsProfile" } },
            "components": { "$ref": "#/$defs/components" }
        },
        "$defs": {
            "id": { "type": "string", "pattern": "^[0-9a-f]{32}$" },
            "weight": { "type": "integer", "minimum": 1 },
            "brand": {
                "type": "object", "additionalProperties": false,
                "required": ["brand", "version"],
                "properties": { "brand": {"type":"string"}, "version": {"type":"string"} }
            },
            "target": {
                "type": "object", "additionalProperties": false,
                "required": ["browser","defaultBrowserMajor","graphicsApiBrowserMajor","graphicsApiRevision","transportBrowserMajors","os","graphicsBackend"],
                "properties": {
                    "browser":{"const":"Chrome"}, "defaultBrowserMajor":{"const":145},
                    "graphicsApiBrowserMajor":{"const":145},
                    "graphicsApiRevision":{"const":"145.0.7632.75"},
                    "transportBrowserMajors":{"type":"array","minItems":1,"items":{"type":"integer","minimum":1},"uniqueItems":true},
                    "os":{"const":"Windows"}, "graphicsBackend":{"const":"ANGLE/D3D11"}
                }
            },
            "sourceDigests": {
                "type":"object", "additionalProperties":false,
                "required":["windowsSha256","windowsBytes","profilesSha256","profilesFiles","profilesBytes"],
                "properties": {
                    "windowsSha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
                    "windowsBytes":{"type":"integer","minimum":1},
                    "profilesSha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
                    "profilesFiles":{"type":"integer","minimum":1},
                    "profilesBytes":{"type":"integer","minimum":1}
                }
            },
            "composition": {
                "type":"object", "additionalProperties":false,
                "required":["baseId","graphicsId","screenId"],
                "properties":{"baseId":{"$ref":"#/$defs/id"},"graphicsId":{"$ref":"#/$defs/id"},"screenId":{"$ref":"#/$defs/id"}}
            },
            "baseProfile": {
                "type":"object", "additionalProperties":false,
                "required":["id","browserVersion","userAgent","brands","fullVersionList","platform","platformVersion","architecture","bitness","languages","hardwareConcurrency","deviceMemory","maxTouchPoints","weight"],
                "properties": {
                    "id":{"$ref":"#/$defs/id"}, "browserVersion":{"type":"string","pattern":"^[0-9]+\\.[0-9]+\\.[0-9]+\\.[0-9]+$"},
                    "userAgent":{"type":"string"}, "brands":{"type":"array","items":{"$ref":"#/$defs/brand"}},
                    "fullVersionList":{"type":"array","items":{"$ref":"#/$defs/brand"}},
                    "platform":{"const":"Windows"}, "platformVersion":{"type":"string"},
                    "architecture":{"const":"x86"}, "bitness":{"const":"64"},
                    "languages":{"type":"array","minItems":1,"items":{"type":"string"}},
                    "hardwareConcurrency":{"type":"integer","minimum":1}, "deviceMemory":{"type":"number","exclusiveMinimum":0},
                    "maxTouchPoints":{"type":"integer","minimum":0}, "weight":{"$ref":"#/$defs/weight"}
                }
            },
            "screenProfile": {
                "type":"object", "additionalProperties":false,
                "required":["id","width","height","availWidth","availHeight","availLeft","availTop","colorDepth","pixelDepth","devicePixelRatio","innerWidth","innerHeight","outerWidth","outerHeight","screenX","screenY","weight"],
                "properties": {
                    "id":{"$ref":"#/$defs/id"}, "width":{"type":"integer","minimum":1}, "height":{"type":"integer","minimum":1},
                    "availWidth":{"type":"integer","minimum":0}, "availHeight":{"type":"integer","minimum":0}, "availLeft":{"type":"integer"}, "availTop":{"type":"integer"},
                    "colorDepth":{"type":"integer","minimum":1}, "pixelDepth":{"type":"integer","minimum":1}, "devicePixelRatio":{"type":"number","exclusiveMinimum":0},
                    "innerWidth":{"type":"integer","minimum":0}, "innerHeight":{"type":"integer","minimum":0}, "outerWidth":{"type":"integer","minimum":0}, "outerHeight":{"type":"integer","minimum":0},
                    "screenX":{"type":"integer"}, "screenY":{"type":"integer"}, "weight":{"$ref":"#/$defs/weight"}
                }
            },
            "graphicsProfile": {
                "type":"object", "additionalProperties":false,
                "required":["id","maskedVendor","maskedRenderer","unmaskedVendor","unmaskedRenderer","webgl1Id","webgl2Id","webgpuId","preferredCanvasFormat","wgslLanguageFeatures","observationsByBrowserVersion","weight"],
                "properties": {
                    "id":{"$ref":"#/$defs/id"}, "maskedVendor":{"type":"string"}, "maskedRenderer":{"type":"string"},
                    "unmaskedVendor":{"type":"string"}, "unmaskedRenderer":{"type":"string"},
                    "webgl1Id":{"$ref":"#/$defs/id"}, "webgl2Id":{"$ref":"#/$defs/id"}, "webgpuId":{"$ref":"#/$defs/id"},
                    "preferredCanvasFormat":{"enum":["bgra8unorm","rgba8unorm"]},
                    "wgslLanguageFeatures":{"type":"array","items":{"type":"string"},"uniqueItems":true},
                    "observationsByBrowserVersion":{"type":"object","minProperties":1,"propertyNames":{"pattern":"^[0-9]+\\.[0-9]+\\.[0-9]+\\.[0-9]+$"},"additionalProperties":{"$ref":"#/$defs/weight"}},
                    "weight":{"$ref":"#/$defs/weight"}
                }
            },
            "parameter": {
                "type":"object", "additionalProperties":false, "required":["type","value"],
                "properties":{"type":{"enum":["Array","Boolean","Float32Array","Int32Array","Number","String","Uint32Array"]},"value":{}}
            },
            "webgl": {
                "type":"object", "required":["id","contextAttributes","parameters","initialState","extensions","supportedExtensions","shaderPrecisionFormats","version","shadingLanguageVersion","maxAnisotropy"],
                "properties":{"id":{"$ref":"#/$defs/id"},"parameters":{"type":"object","additionalProperties":{"$ref":"#/$defs/parameter"}},"initialState":{"type":"object","additionalProperties":{"$ref":"#/$defs/parameter"}}}
            },
            "webgpu": {
                "type":"object","additionalProperties":false,"required":["id","adapters"],
                "properties":{
                    "id":{"$ref":"#/$defs/id"},
                    "adapters":{
                        "type":"object","additionalProperties":false,"required":["default"],
                        "properties":{
                            "default":{"$ref":"#/$defs/id"},
                            "lowPower":{"$ref":"#/$defs/id"},
                            "highPerformance":{"$ref":"#/$defs/id"}
                        }
                    }
                }
            },
            "webgpuAdapter": {"type":"object","additionalProperties":false,"required":["id","info","features","limitsId","defaultDeviceLimitsId"],"properties":{"id":{"$ref":"#/$defs/id"},"info":{"type":"object"},"features":{"type":"array","items":{"type":"string"},"uniqueItems":true},"limitsId":{"$ref":"#/$defs/id"},"defaultDeviceLimitsId":{"$ref":"#/$defs/id"}}},
            "webgpuLimits": {"type":"object","additionalProperties":false,"required":["id","values"],"properties":{"id":{"$ref":"#/$defs/id"},"values":{"type":"object"}}},
            "components": {
                "type":"object", "additionalProperties":false, "required":["webgl1","webgl2","webgpu","webgpuAdapters","webgpuLimits"],
                "properties": {
                    "webgl1":{"type":"array","minItems":1,"items":{"$ref":"#/$defs/webgl"}},
                    "webgl2":{"type":"array","minItems":1,"items":{"$ref":"#/$defs/webgl"}},
                    "webgpu":{"type":"array","minItems":1,"items":{"$ref":"#/$defs/webgpu"}},
                    "webgpuAdapters":{"type":"array","minItems":1,"items":{"$ref":"#/$defs/webgpuAdapter"}},
                    "webgpuLimits":{"type":"array","minItems":1,"items":{"$ref":"#/$defs/webgpuLimits"}}
                }
            }
        }
    })
}

fn write_json(path: &Path, value: &impl Serialize, compact: bool) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut bytes = if compact {
        serde_json::to_vec(value)?
    } else {
        serde_json::to_vec_pretty(value)?
    };
    bytes.push(b'\n');
    fs::write(path, bytes).with_context(|| format!("write {}", path.display()))
}

fn validate_catalog(catalog: &FingerprintCatalog) -> Result<()> {
    if catalog.schema_version != 1 || catalog.catalog_id != CATALOG_ID {
        bail!("catalog version or ID is invalid");
    }
    let bases: HashSet<&str> = catalog.base_profiles.iter().map(|row| row.id.as_str()).collect();
    let screens: HashSet<&str> = catalog.screen_profiles.iter().map(|row| row.id.as_str()).collect();
    let graphics: HashSet<&str> = catalog.graphics_profiles.iter().map(|row| row.id.as_str()).collect();
    let webgl1: HashSet<&str> = catalog.components.webgl1.iter().map(|row| row.id.as_str()).collect();
    let webgl2: HashSet<&str> = catalog.components.webgl2.iter().map(|row| row.id.as_str()).collect();
    let webgpu: HashSet<&str> = catalog.components.webgpu.iter().map(|row| row.id.as_str()).collect();
    let webgpu_adapters: HashSet<&str> = catalog
        .components
        .webgpu_adapters
        .iter()
        .map(|row| row.id.as_str())
        .collect();
    let webgpu_limits: HashSet<&str> = catalog
        .components
        .webgpu_limits
        .iter()
        .map(|row| row.id.as_str())
        .collect();
    if !bases.contains(catalog.default_composition.base_id.as_str())
        || !screens.contains(catalog.default_composition.screen_id.as_str())
        || !graphics.contains(catalog.default_composition.graphics_id.as_str())
    {
        bail!("default composition has an unknown ID");
    }
    for row in &catalog.graphics_profiles {
        if !webgl1.contains(row.content.webgl1_id.as_str())
            || !webgl2.contains(row.content.webgl2_id.as_str())
            || !webgpu.contains(row.content.webgpu_id.as_str())
        {
            bail!("graphics profile {} has an unknown component", row.id);
        }
        if row.observations_by_browser_version.is_empty()
            || row
                .observations_by_browser_version
                .values()
                .try_fold(0u64, |total, weight| total.checked_add(*weight))
                != Some(row.weight)
        {
            bail!("graphics profile {} has invalid browser-version weights", row.id);
        }
    }
    if catalog
        .components
        .webgpu
        .iter()
        .any(|row| {
            !row.adapters.contains_key("default")
                || row.adapters.keys().any(|name| {
                    !matches!(name.as_str(), "default" | "lowPower" | "highPerformance")
                })
                || row
                    .adapters
                    .values()
                    .any(|id| !webgpu_adapters.contains(id.as_str()))
        })
        || catalog.components.webgpu_adapters.iter().any(|row| {
            !webgpu_limits.contains(row.limits_id.as_str())
                || !webgpu_limits.contains(row.default_device_limits_id.as_str())
        })
    {
        bail!("WebGPU component has an unknown nested component");
    }
    if bases.len() != catalog.base_profiles.len()
        || screens.len() != catalog.screen_profiles.len()
        || graphics.len() != catalog.graphics_profiles.len()
        || webgl1.len() != catalog.components.webgl1.len()
        || webgl2.len() != catalog.components.webgl2.len()
        || webgpu.len() != catalog.components.webgpu.len()
        || webgpu_adapters.len() != catalog.components.webgpu_adapters.len()
        || webgpu_limits.len() != catalog.components.webgpu_limits.len()
    {
        bail!("catalog has a duplicate ID");
    }
    Ok(())
}

fn generate(args: &GenerateArgs) -> Result<()> {
    let mut collisions = CollisionGuard::default();
    let mut window_rejects = Vec::new();
    let mut profile_rejects = Vec::new();

    let (window_bytes, window_json) = read_json(&args.windows)?;
    let window_rows = window_json
        .as_array()
        .ok_or_else(|| anyhow!("windows input is not an array"))?;
    let mut screen_rows = BTreeMap::<Vec<u8>, (ScreenContent, u64)>::new();
    let mut window_observations = 0u64;
    for (source_index, row) in window_rows.iter().enumerate() {
        let parsed = (|| -> Result<()> {
            let screen = value_at(row, &["screen"])?;
            let windows = value_at(row, &["window"])?
                .as_array()
                .ok_or_else(|| anyhow!("window is not an array"))?;
            let total = u32_at(row, &["total"])? as usize;
            if total != windows.len() {
                bail!("total does not match window observation count");
            }
            for window in windows {
                insert_weighted(&mut screen_rows, parse_screen_profile(screen, window)?)?;
                window_observations += 1;
            }
            Ok(())
        })();
        if let Err(error) = parsed {
            window_rejects.push(Reject {
                source_index,
                reason: error.to_string(),
            });
        }
    }
    if screen_rows.is_empty() {
        bail!("no valid screen observations");
    }

    let profile_paths = sorted_profile_paths(&args.profiles)?;
    let mut profile_hashes = Vec::with_capacity(profile_paths.len());
    let mut profiles_bytes = 0u64;
    let mut profile_records = 0u64;
    let mut skipped_non_windows = 0u64;
    let mut graphics_observations = 0u64;
    let mut default_major_graphics_observations = 0u64;
    let mut browser_version_observations = BTreeMap::<String, u64>::new();
    let mut browser_major_observations = BTreeMap::<String, u64>::new();
    let mut base_rows = BTreeMap::<Vec<u8>, (BaseContent, u64)>::new();
    let mut graphics_rows = BTreeMap::<Vec<u8>, GraphicsAggregate>::new();
    let mut webgl1_components = BTreeMap::<String, WebGlComponent>::new();
    let mut webgl2_components = BTreeMap::<String, WebGlComponent>::new();
    let mut webgpu_components = BTreeMap::<String, WebGpuComponent>::new();
    let mut webgpu_adapter_components = BTreeMap::<String, WebGpuAdapterComponent>::new();
    let mut webgpu_limits_components = BTreeMap::<String, WebGpuLimitsComponent>::new();
    let screen_keys: HashSet<Vec<u8>> = screen_rows.keys().cloned().collect();
    let mut base_overlap = 0u64;
    let mut screen_overlap = 0u64;
    let mut full_overlap = 0u64;

    for (source_index, path) in profile_paths.iter().enumerate() {
        let bytes = fs::read(path).with_context(|| format!("read profile source {source_index}"))?;
        profiles_bytes = profiles_bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| anyhow!("profile byte count overflow"))?;
        profile_hashes.push(Sha256::digest(&bytes).to_vec());
        let source: Value = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(error) => {
                profile_rejects.push(Reject {
                    source_index,
                    reason: format!("invalid JSON: {error}"),
                });
                continue;
            }
        };
        let records: &[Value] = source
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_else(|| std::slice::from_ref(&source));
        for (record_index, value) in records.iter().enumerate() {
            profile_records += 1;
            let os_type = match string_at(value, &["fingerprints", "system", "osType"]) {
                Ok(value) => value,
                Err(error) => {
                    profile_rejects.push(Reject {
                        source_index,
                        reason: format!("record {record_index}: {error}"),
                    });
                    continue;
                }
            };
            if os_type != "win" {
                skipped_non_windows += 1;
                continue;
            }
            let browser_version = match parse_windows_browser_version(value) {
                Ok(value) => value,
                Err(error) => {
                    profile_rejects.push(Reject {
                        source_index,
                        reason: format!("record {record_index}: {error}"),
                    });
                    continue;
                }
            };
            let major = chrome_major(&browser_version)?.to_owned();
            *browser_version_observations
                .entry(browser_version.clone())
                .or_default() += 1;
            *browser_major_observations.entry(major.clone()).or_default() += 1;

            let base = match parse_base_profile(value) {
                Ok(value) => value,
                Err(error) => {
                    profile_rejects.push(Reject {
                        source_index,
                        reason: format!("record {record_index} base: {error}"),
                    });
                    continue;
                }
            };
            insert_weighted(&mut base_rows, base)?;
            base_overlap += 1;

            let has_graphics = match parse_graphics_content(
                value,
                &mut webgl1_components,
                &mut webgl2_components,
                &mut webgpu_components,
                &mut webgpu_adapter_components,
                &mut webgpu_limits_components,
                &mut collisions,
            ) {
                Ok(content) => {
                    insert_graphics_observation(
                        &mut graphics_rows,
                        content,
                        &browser_version,
                    )?;
                    graphics_observations += 1;
                    if major == DEFAULT_BROWSER_MAJOR {
                        default_major_graphics_observations += 1;
                    }
                    true
                }
                Err(error) => {
                    profile_rejects.push(Reject {
                        source_index,
                        reason: format!("record {record_index} graphics: {error}"),
                    });
                    false
                }
            };

            let screen = value_at(value, &["fingerprints", "hardware", "screen"])?;
            let window = value_at(value, &["fingerprints", "browser", "window"])?;
            let has_screen = parse_screen_profile(screen, window)
            .ok()
            .and_then(|content| serde_json::to_vec(&content).ok())
            .is_some_and(|key| screen_keys.contains(&key));
            if has_screen {
                screen_overlap += 1;
            }
            if has_screen && has_graphics {
                full_overlap += 1;
            }
        }
    }
    if base_rows.is_empty() {
        bail!("no valid Chrome Windows base profiles");
    }
    if default_major_graphics_observations == 0 {
        bail!("no valid default-major Chrome Windows graphics observations");
    }

    profile_hashes.sort();
    let mut profiles_hasher = Sha256::new();
    for hash in &profile_hashes {
        profiles_hasher.update(hash);
    }
    let source_digests = SourceDigests {
        windows_sha256: sha256(&window_bytes),
        windows_bytes: window_bytes.len() as u64,
        profiles_sha256: hex(&profiles_hasher.finalize()),
        profiles_files: profile_paths.len() as u64,
        profiles_bytes,
    };

    let mut base_profiles = Vec::with_capacity(base_rows.len());
    for (_, (content, weight)) in base_rows {
        base_profiles.push(BaseProfile {
            id: collisions.id(&content)?,
            content,
            weight,
        });
    }
    base_profiles.sort_by(|left, right| left.id.cmp(&right.id));

    let mut screen_profiles = Vec::with_capacity(screen_rows.len());
    for (_, (content, weight)) in screen_rows {
        screen_profiles.push(ScreenProfile {
            id: collisions.id(&content)?,
            content,
            weight,
        });
    }
    screen_profiles.sort_by(|left, right| left.id.cmp(&right.id));

    let mut graphics_profiles = Vec::new();
    for (_, row) in graphics_rows {
        let weight = row
            .observations_by_browser_version
            .values()
            .try_fold(0u64, |total, weight| total.checked_add(*weight))
            .ok_or_else(|| anyhow!("graphics observation weight overflow"))?;
        graphics_profiles.push(GraphicsProfile {
            id: collisions.id(&row.content)?,
            content: row.content,
            observations_by_browser_version: row.observations_by_browser_version,
            weight,
        });
    }
    graphics_profiles.sort_by(|left, right| left.id.cmp(&right.id));

    let used_webgl1: HashSet<&str> = graphics_profiles
        .iter()
        .map(|row| row.content.webgl1_id.as_str())
        .collect();
    let used_webgl2: HashSet<&str> = graphics_profiles
        .iter()
        .map(|row| row.content.webgl2_id.as_str())
        .collect();
    let used_webgpu: HashSet<&str> = graphics_profiles
        .iter()
        .map(|row| row.content.webgpu_id.as_str())
        .collect();
    webgl1_components.retain(|id, _| used_webgl1.contains(id.as_str()));
    webgl2_components.retain(|id, _| used_webgl2.contains(id.as_str()));
    webgpu_components.retain(|id, _| used_webgpu.contains(id.as_str()));

    let default_bases: Vec<&BaseProfile> = base_profiles
        .iter()
        .filter(|row| chrome_major(&row.content.browser_version).ok() == Some(DEFAULT_BROWSER_MAJOR))
        .collect();
    let default_base = *rank_default(&default_bases, |row| (row.weight, row.id.as_str()))?;
    let default_screen = rank_default(&screen_profiles, |row| (row.weight, row.id.as_str()))?;
    let default_graphics_rows: Vec<(&GraphicsProfile, u64)> = graphics_profiles
        .iter()
        .map(|row| {
            browser_major_weight(
                &row.observations_by_browser_version,
                DEFAULT_BROWSER_MAJOR,
            )
            .map(|weight| (row, weight))
        })
        .collect::<Result<_>>()?;
    let default_graphics_rows: Vec<(&GraphicsProfile, u64)> = default_graphics_rows
        .into_iter()
        .filter(|(_, weight)| *weight > 0)
        .collect();
    let default_graphics = rank_default(&default_graphics_rows, |(row, weight)| {
        (*weight, row.id.as_str())
    })?
    .0;

    let catalog = FingerprintCatalog {
        schema_version: 1,
        catalog_id: CATALOG_ID.to_owned(),
        target: Target {
            browser: "Chrome".to_owned(),
            default_browser_major: 145,
            graphics_api_browser_major: GRAPHICS_API_BROWSER_MAJOR,
            graphics_api_revision: GRAPHICS_API_REVISION.to_owned(),
            transport_browser_majors: WREQ_BROWSER_MAJORS.to_vec(),
            os: "Windows".to_owned(),
            graphics_backend: "ANGLE/D3D11".to_owned(),
        },
        source_digests: source_digests.clone(),
        default_composition: Composition {
            base_id: default_base.id.clone(),
            graphics_id: default_graphics.id.clone(),
            screen_id: default_screen.id.clone(),
        },
        base_profiles,
        screen_profiles,
        graphics_profiles,
        components: Components {
            webgl1: webgl1_components.into_values().collect(),
            webgl2: webgl2_components.into_values().collect(),
            webgpu: webgpu_components.into_values().collect(),
            webgpu_adapters: webgpu_adapter_components.into_values().collect(),
            webgpu_limits: webgpu_limits_components.into_values().collect(),
        },
    };
    validate_catalog(&catalog)?;

    let catalog_bytes = serde_json::to_vec(&catalog)?;
    if catalog_bytes.len() > MAX_CATALOG_BYTES {
        bail!(
            "compact catalog is {} bytes; limit is {} bytes (base {}, screen {}, graphics {}, webgl1 {}, webgl2 {}, webgpu {}, webgpu adapters {}, webgpu limits {})",
            catalog_bytes.len(),
            MAX_CATALOG_BYTES,
            serde_json::to_vec(&catalog.base_profiles)?.len(),
            serde_json::to_vec(&catalog.screen_profiles)?.len(),
            serde_json::to_vec(&catalog.graphics_profiles)?.len(),
            serde_json::to_vec(&catalog.components.webgl1)?.len(),
            serde_json::to_vec(&catalog.components.webgl2)?.len(),
            serde_json::to_vec(&catalog.components.webgpu)?.len(),
            serde_json::to_vec(&catalog.components.webgpu_adapters)?.len(),
            serde_json::to_vec(&catalog.components.webgpu_limits)?.len(),
        );
    }
    let banned_fields = [
        "clientName",
        "subscriptionExpires",
        "chargeMoney",
        "profileExpires",
        "textureHashs",
        "computeBenchmark",
        "raytraceBenchmark",
        "advancedFingerprint2025",
    ];
    let text = std::str::from_utf8(&catalog_bytes)?;
    let found_banned: Vec<&str> = banned_fields
        .iter()
        .copied()
        .filter(|field| text.contains(field))
        .collect();
    if !found_banned.is_empty() {
        bail!("catalog contains banned fields: {}", found_banned.join(", "));
    }

    let report = json!({
        "schemaVersion": 1,
        "catalogId": CATALOG_ID,
        "status": "valid",
        "schemaValidation": "passed",
        "catalogBytes": catalog_bytes.len(),
        "catalogLimitBytes": MAX_CATALOG_BYTES,
        "bannedFieldScan": { "passed": true, "fields": banned_fields },
        "input": {
            "windowGroups": window_rows.len(),
            "windowObservations": window_observations,
            "profileFiles": profile_paths.len(),
            "profileRecords": profile_records
        },
        "accepted": {
            "windowGroups": window_rows.len() - window_rejects.len(),
            "baseObservations": base_overlap,
            "graphicsObservations": graphics_observations,
            "defaultMajorGraphicsObservations": default_major_graphics_observations
        },
        "skipped": {
            "nonWindowsProfiles": skipped_non_windows
        },
        "rejected": {
            "windows": window_rejects,
            "profiles": profile_rejects
        },
        "browserVersionObservations": browser_version_observations,
        "browserMajorObservations": browser_major_observations,
        "unique": {
            "baseProfiles": catalog.base_profiles.len(),
            "screenProfiles": catalog.screen_profiles.len(),
            "graphicsProfiles": catalog.graphics_profiles.len(),
            "webgl1Components": catalog.components.webgl1.len(),
            "webgl2Components": catalog.components.webgl2.len(),
            "webgpuComponents": catalog.components.webgpu.len(),
            "webgpuAdapterComponents": catalog.components.webgpu_adapters.len(),
            "webgpuLimitsComponents": catalog.components.webgpu_limits.len()
        },
        "exactOverlap": {
            "baseProfiles": base_overlap,
            "screenProfiles": screen_overlap,
            "fullProfiles": full_overlap
        },
        "assumptions": [
            "Chrome 145 remains the fixed default browser major",
            "every valid Windows Chrome profile is selectable",
            "a browser major without an exact wreq transport uses the nearest available transport and emits a runtime warning",
            "the graphics API shape remains pinned to Chromium 145 and non-145 profiles emit a runtime warning"
        ]
    });

    write_json(&args.out, &catalog, true)?;
    write_json(&args.schema, &make_schema(), false)?;
    write_json(&args.report, &report, false)?;
    write_json(&args.sources, &source_digests, false)?;
    println!(
        "generated {} bytes: {} base, {} screen, {} graphics profiles",
        catalog_bytes.len(),
        catalog.base_profiles.len(),
        catalog.screen_profiles.len(),
        catalog.graphics_profiles.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;
    use tempfile::TempDir;

    fn fixture(name: &str) -> Value {
        serde_json::from_str(match name {
            "profile-v1" => include_str!("../tests/fixtures/profile-v1.json"),
            "profile-v2" => include_str!("../tests/fixtures/profile-v2.json"),
            "profile-invalid" => include_str!("../tests/fixtures/profile-invalid.json"),
            "adapter" => include_str!("../tests/fixtures/adapter-variants.json"),
            _ => panic!("unknown fixture"),
        })
        .unwrap()
    }

    fn complete_profile(name: &str) -> Value {
        let mut profile = fixture(name);
        let window = json!({
            "devicePixelRatio":1,"innerHeight":919,"innerWidth":1920,
            "outerHeight":1040,"outerWidth":1920,"screenX":0,"screenY":0
        });
        profile["fingerprints"]["browser"]["window"] = window;
        profile["fingerprints"]["browser"]["webglContext"] = test_webgl(82, false);
        profile["fingerprints"]["browser"]["webgl2Context"] = test_webgl(132, true);
        profile["fingerprints"]["hardware"]["screen"] = json!({
            "availHeight":1040,"availLeft":0,"availTop":0,"availWidth":1920,
            "colorDepth":24,"height":1080,"pixelDepth":24,"width":1920
        });
        profile["fingerprints"]["hardware"]["gpu"] = json!({
            "unmaskedVendor":"Google Inc. (Intel)",
            "unmaskedRenderer":"ANGLE (Intel, Test GPU Direct3D11, D3D11)",
            "preferredCanvasFormat":"bgra8unorm",
            "wgslLanguageFeatures":[
                "packed_4x8_integer_dot_product",
                "pointer_composite_access",
                "readonly_and_readwrite_storage_textures",
                "unrestricted_pointer_parameters"
            ],
            "adapter":{"default":fixture("adapter")}
        });
        profile
    }

    fn with_browser_version(mut profile: Value, version: &str) -> Value {
        let major = version.split('.').next().unwrap();
        let user_agent = format!(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{major}.0.0.0 Safari/537.36"
        );
        profile["fingerprints"]["browser"]["version"] = json!(version);
        profile["fingerprints"]["browser"]["userAgent"] = json!(user_agent);
        profile["fingerprints"]["browser"]["navigator"]["userAgent"] = json!(user_agent);
        profile["fingerprints"]["browser"]["userAgentData"]["uaFullVersion"] = json!(version);
        for item in profile["fingerprints"]["browser"]["userAgentData"]["brands"]
            .as_array_mut()
            .unwrap()
        {
            if matches!(item["brand"].as_str(), Some("Google Chrome" | "Chromium")) {
                item["version"] = json!(major);
            }
        }
        for item in profile["fingerprints"]["browser"]["userAgentData"]["fullVersionList"]
            .as_array_mut()
            .unwrap()
        {
            if matches!(item["brand"].as_str(), Some("Google Chrome" | "Chromium")) {
                item["version"] = json!(version);
            }
        }
        profile
    }

    fn test_webgl(parameter_count: usize, webgl2: bool) -> Value {
        let mut parameters = Map::new();
        for index in 0..parameter_count {
            parameters.insert(
                (50_000 + index).to_string(),
                json!({"name": format!("CAPABILITY_{index}"), "type":"Number", "value":index}),
            );
        }
        parameters.insert(
            "-1".to_owned(),
            json!({"name":"TIMEOUT_IGNORED", "type":"", "value":null}),
        );
        let mut precision = Vec::new();
        for shader_type in [35632, 35633] {
            for precision_type in [36336, 36337, 36338, 36339, 36340, 36341] {
                precision.push(json!({
                    "shaderType":shader_type,
                    "precisionType":precision_type,
                    "shaderPrecisionFormat":{"rangeMin":127,"rangeMax":127,"precision":23}
                }));
            }
        }
        let mut value = json!({
            "contextAttributes": {
                "alpha":true,"antialias":true,"depth":true,"desynchronized":false,
                "failIfMajorPerformanceCaveat":false,"powerPreference":"default",
                "premultipliedAlpha":true,"preserveDrawingBuffer":false,"stencil":false,
                "xrCompatible":false
            },
            "extensions": {
                "34046":{"name":"EXT_texture_filter_anisotropic","constantName":"MAX_TEXTURE_MAX_ANISOTROPY_EXT"}
            },
            "maxAnisotropy":16,
            "parameters":parameters,
            "shaderPrecisionFormats":precision,
            "shadingLanguageVersion": if webgl2 {"WebGL GLSL ES 3.00"} else {"WebGL GLSL ES 1.0"},
            "supportedExtensions":["EXT_texture_filter_anisotropic"],
            "version": if webgl2 {"WebGL 2.0"} else {"WebGL 1.0"}
        });
        if !webgl2 {
            value["maxDrawBuffersWebgl"] = json!(8);
        }
        value
    }

    #[test]
    fn base_schema_versions_normalize_to_the_same_private_free_row() {
        let first = parse_base_profile(&fixture("profile-v1")).unwrap();
        let second = parse_base_profile(&fixture("profile-v2")).unwrap();
        assert_eq!(first, second);
        let text = serde_json::to_string(&first).unwrap();
        assert!(!text.contains("clientName"));
        assert!(!text.contains("subscription"));
        assert!(!text.contains("chargeMoney"));
        assert!(parse_base_profile(&fixture("profile-invalid")).is_err());
    }

    #[test]
    fn adapter_info_fallback_is_normalized_and_probe_results_are_dropped() {
        let adapter = normalize_webgpu_adapter(&fixture("adapter")).unwrap();
        assert_eq!(adapter.info.vendor, "intel");
        assert_eq!(adapter.features, vec!["texture-compression-bc", "shader-f16"]);
        let text = serde_json::to_string(&adapter).unwrap();
        assert!(!text.contains("textureHashs"));
        assert!(!text.contains("bc1-rgba-unorm"));

        let mut with_info = fixture("adapter");
        with_info["info"] = json!({
            "vendor":"intel","architecture":"gen-12lp","device":"","description":"",
            "subgroupMinSize":8,"subgroupMaxSize":32,"isFallbackAdapter":false
        });
        let normalized = normalize_webgpu_adapter(&with_info).unwrap();
        assert_eq!(normalized.info.subgroup_min_size, Some(8));
        assert_eq!(normalized.info.subgroup_max_size, Some(32));

        let mut reordered = fixture("adapter");
        reordered["features"].as_array_mut().unwrap().reverse();
        let reordered = normalize_webgpu_adapter(&reordered).unwrap();
        let mut collisions = CollisionGuard::default();
        assert_ne!(
            collisions.id(&adapter).unwrap(),
            collisions.id(&reordered).unwrap()
        );

        let error = normalize_webgpu(&json!({ "low-power": fixture("adapter") }))
            .unwrap_err();
        assert!(error.to_string().contains("default WebGPU adapter"));
    }

    #[test]
    fn invalid_parameter_probes_and_captured_names_do_not_enter_components() {
        let webgl = normalize_webgl(&test_webgl(82, false), false).unwrap();
        assert_eq!(webgl.parameters.len(), 82);
        assert!(!webgl.parameters.contains_key("-1"));
        let text = serde_json::to_string(&webgl).unwrap();
        assert!(!text.contains("CAPABILITY_"));
        assert!(!text.contains("TIMEOUT_IGNORED"));
    }

    #[test]
    fn ids_and_weights_are_stable() {
        let first = parse_base_profile(&fixture("profile-v1")).unwrap();
        let second = parse_base_profile(&fixture("profile-v2")).unwrap();
        let mut collisions = CollisionGuard::default();
        assert_eq!(collisions.id(&first).unwrap(), collisions.id(&second).unwrap());

        let screen = ScreenContent {
            width: 1920,
            height: 1080,
            avail_width: 1920,
            avail_height: 1040,
            avail_left: 0,
            avail_top: 0,
            color_depth: 24,
            pixel_depth: 24,
            device_pixel_ratio: 1.0,
            inner_width: 1920,
            inner_height: 919,
            outer_width: 1920,
            outer_height: 1040,
            screen_x: 0,
            screen_y: 0,
        };
        let mut rows = BTreeMap::new();
        insert_weighted(&mut rows, screen.clone()).unwrap();
        insert_weighted(&mut rows, screen).unwrap();
        assert_eq!(rows.values().next().unwrap().1, 2);
    }

    fn write_test_inputs(root: &Path, reverse_profiles: bool) -> GenerateArgs {
        let profile_dir = root.join("profiles");
        fs::create_dir_all(&profile_dir).unwrap();
        let v1 = serde_json::to_vec_pretty(&complete_profile("profile-v1")).unwrap();
        let v2 = serde_json::to_vec_pretty(&complete_profile("profile-v2")).unwrap();
        let v148 = serde_json::to_vec_pretty(&with_browser_version(
            complete_profile("profile-v1"),
            "148.0.7778.169",
        ))
        .unwrap();
        if reverse_profiles {
            let nested = profile_dir.join("145");
            fs::create_dir_all(&nested).unwrap();
            let other = profile_dir.join("148");
            fs::create_dir_all(&other).unwrap();
            fs::write(profile_dir.join("a.json"), &v2).unwrap();
            fs::write(nested.join("z.json"), &v1).unwrap();
            fs::write(other.join("m.json"), &v148).unwrap();
        } else {
            fs::write(profile_dir.join("a.json"), &v1).unwrap();
            fs::write(profile_dir.join("z.json"), &v2).unwrap();
            let nested = profile_dir.join("148");
            fs::create_dir_all(&nested).unwrap();
            fs::write(nested.join("m.json"), &v148).unwrap();
        }
        let window = json!({
            "devicePixelRatio":1,"innerHeight":919,"innerWidth":1920,
            "outerHeight":1040,"outerWidth":1920,"screenX":0,"screenY":0
        });
        let windows = json!([{
            "total":2,
            "window":[window.clone(),window],
            "screen":{"availHeight":1040,"availLeft":0,"availTop":0,"availWidth":1920,"colorDepth":24,"height":1080,"pixelDepth":24,"width":1920}
        }]);
        fs::write(root.join("windows.json"), serde_json::to_vec_pretty(&windows).unwrap()).unwrap();

        GenerateArgs {
            profiles: profile_dir,
            windows: root.join("windows.json"),
            out: root.join("catalog.json"),
            schema: root.join("schema.json"),
            report: root.join("report.json"),
            sources: root.join("sources.json"),
        }
    }

    #[test]
    fn generation_is_stable_for_reordered_profile_files() {
        let first = TempDir::new().unwrap();
        let second = TempDir::new().unwrap();
        let first_args = write_test_inputs(first.path(), false);
        let second_args = write_test_inputs(second.path(), true);
        generate(&first_args).unwrap();
        generate(&second_args).unwrap();
        assert_eq!(fs::read(&first_args.out).unwrap(), fs::read(&second_args.out).unwrap());

        let catalog: FingerprintCatalog =
            serde_json::from_slice(&fs::read(&first_args.out).unwrap()).unwrap();
        assert_eq!(catalog.base_profiles.len(), 2);
        assert_eq!(catalog.base_profiles.iter().map(|row| row.weight).sum::<u64>(), 3);
        let default_base = catalog
            .base_profiles
            .iter()
            .find(|row| row.id == catalog.default_composition.base_id)
            .unwrap();
        assert_eq!(chrome_major(&default_base.content.browser_version).unwrap(), "145");
        assert_eq!(catalog.screen_profiles.len(), 1);
        assert_eq!(catalog.screen_profiles[0].weight, 2);
        assert_eq!(catalog.graphics_profiles.len(), 1);
        assert_eq!(catalog.graphics_profiles[0].weight, 3);
        assert_eq!(
            catalog.graphics_profiles[0].observations_by_browser_version,
            BTreeMap::from([
                ("145.0.7632.75".to_owned(), 2),
                ("148.0.7778.169".to_owned(), 1),
            ])
        );
        let text = fs::read_to_string(&first_args.out).unwrap();
        assert!(!text.contains("private-test-name"));
        assert!(fs::metadata(&first_args.out).unwrap().len() <= MAX_CATALOG_BYTES as u64 + 1);
    }

    #[test]
    fn hashes_and_schema_checks_change_with_input() {
        assert_ne!(sha256(b"one"), sha256(b"two"));
        let schema = make_schema();
        assert_eq!(schema["properties"]["schemaVersion"]["const"], 1);
        assert_eq!(schema["properties"]["catalogId"]["const"], CATALOG_ID);
    }
}
