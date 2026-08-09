use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const INDEX_HTML: &str = include_str!("../../../webgl/capture/index.html");
const COLLECTOR_JS: &str = include_str!("../../../webgl/capture/collector.js");
const PROFILE_ID_JS: &str = include_str!("../../../webgl/capture/profile-id.js");
const ROUTE_PREFIX: &str = "/obscura/profiles";
const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_CAPTURE_BYTES: usize = 16 * 1024 * 1024;
const MAX_SOURCE_BYTES: u64 = 512 * 1024 * 1024;
const RUNTIME_DIR: &str = ".obscura-runtime";

#[derive(Clone)]
pub(crate) struct ProfileWorkbench {
    root: PathBuf,
}

impl ProfileWorkbench {
    pub(crate) fn new(root: PathBuf) -> anyhow::Result<Self> {
        let root = if root.is_absolute() {
            root
        } else {
            std::env::current_dir()?.join(root)
        };
        if root.exists() && !root.is_dir() {
            bail!(
                "profile workbench path is not a directory: {}",
                root.display()
            );
        }
        let runtime_dir = root.join(RUNTIME_DIR);
        match obscura_browser::profiles::load_runtime_profiles(&runtime_dir) {
            Ok(loaded) if loaded > 0 => tracing::info!(loaded, path = %runtime_dir.display(), "loaded saved runtime profiles"),
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, path = %runtime_dir.display(), "saved runtime profiles were not loaded"),
        }
        Ok(Self { root })
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
}

pub(crate) fn is_workbench_request(peek: &[u8]) -> bool {
    let line_end = peek
        .iter()
        .position(|byte| *byte == b'\n')
        .unwrap_or(peek.len());
    let line = String::from_utf8_lossy(&peek[..line_end]);
    line.split_whitespace()
        .nth(1)
        .map(|target| {
            let path = target.split('?').next().unwrap_or(target);
            path == ROUTE_PREFIX || path.starts_with(&format!("{ROUTE_PREFIX}/"))
        })
        .unwrap_or(false)
}

pub(crate) fn handle(
    mut stream: TcpStream,
    port: u16,
    peer: SocketAddr,
    workbench: Option<&ProfileWorkbench>,
) -> anyhow::Result<()> {
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(error) => return write_text(&mut stream, 400, "Bad Request", &error.to_string()),
    };
    let path = request.target.split('?').next().unwrap_or(&request.target);

    let Some(workbench) = workbench else {
        return write_text(
            &mut stream,
            404,
            "Not Found",
            "Profile workbench is off. Start serve with --profile-workbench-dir <path>.\n",
        );
    };

    match (request.method.as_str(), path) {
        ("GET", ROUTE_PREFIX) | ("GET", "/obscura/profiles/") => write_response(
            &mut stream,
            200,
            "OK",
            "text/html; charset=utf-8",
            INDEX_HTML.as_bytes(),
        ),
        ("GET", "/obscura/profiles/collector.js") => write_response(
            &mut stream,
            200,
            "OK",
            "text/javascript; charset=utf-8",
            COLLECTOR_JS.as_bytes(),
        ),
        ("GET", "/obscura/profiles/profile-id.js") => write_response(
            &mut stream,
            200,
            "OK",
            "text/javascript; charset=utf-8",
            PROFILE_ID_JS.as_bytes(),
        ),
        ("GET", "/obscura/profiles/catalog") => {
            let body = obscura_browser::profiles::catalog()?.index_json_with_runtime()?;
            write_response(
                &mut stream,
                200,
                "OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
            )
        }
        ("POST", "/obscura/profiles/capture") => {
            if let Err(error) = check_write_origin(&request.headers, peer, port) {
                return write_json_error(&mut stream, 403, "Forbidden", &error.to_string());
            }
            let body: Value = match serde_json::from_slice(&request.body) {
                Ok(body) => body,
                Err(error) => {
                    return write_json_error(
                        &mut stream,
                        400,
                        "Bad Request",
                        &format!("invalid JSON: {error}"),
                    );
                }
            };
            match save_capture(workbench.root(), &body) {
                Ok(saved) => {
                    let body = serde_json::to_vec(&json!({ "ok": true, "saved": saved }))?;
                    write_response(
                        &mut stream,
                        200,
                        "OK",
                        "application/json; charset=utf-8",
                        &body,
                    )
                }
                Err(error) => write_json_error(&mut stream, 400, "Bad Request", &error.to_string()),
            }
        }
        ("GET", _) | ("POST", _) => write_text(&mut stream, 404, "Not Found", "Not found.\n"),
        _ => write_text(
            &mut stream,
            405,
            "Method Not Allowed",
            "Method not allowed.\n",
        ),
    }
}

struct HttpRequest {
    method: String,
    target: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> anyhow::Result<HttpRequest> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut bytes = Vec::with_capacity(8192);
    let mut chunk = [0u8; 8192];
    let header_end;
    loop {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            bail!("request ended before its headers");
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(position) = find_bytes(&bytes, b"\r\n\r\n") {
            header_end = position + 4;
            if header_end > MAX_HEADER_BYTES {
                bail!("request headers are too large");
            }
            break;
        }
        if bytes.len() > MAX_HEADER_BYTES {
            bail!("request headers are too large");
        }
    }

    let header_text = std::str::from_utf8(&bytes[..header_end - 4])?;
    let mut lines = header_text.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split_whitespace();
    let method = first
        .next()
        .ok_or_else(|| anyhow!("request method is missing"))?
        .to_string();
    let target = first
        .next()
        .ok_or_else(|| anyhow!("request target is missing"))?
        .to_string();
    if first.next().is_none() {
        bail!("HTTP version is missing");
    }
    let mut headers = HashMap::new();
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| anyhow!("bad request header"))?;
        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
    }
    let content_length = match headers.get("content-length") {
        Some(value) => value.parse::<usize>().context("bad Content-Length")?,
        None => 0,
    };
    if content_length > MAX_CAPTURE_BYTES {
        bail!(
            "capture is too large; the limit is {} bytes",
            MAX_CAPTURE_BYTES
        );
    }
    while bytes.len() - header_end < content_length {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            bail!("request body ended early");
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    let body_end = header_end + content_length;
    Ok(HttpRequest {
        method,
        target,
        headers,
        body: bytes[header_end..body_end].to_vec(),
    })
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn check_write_origin(
    headers: &HashMap<String, String>,
    peer: SocketAddr,
    port: u16,
) -> anyhow::Result<()> {
    if !peer.ip().is_loopback() {
        bail!("capture save is allowed only from a loopback client");
    }
    let host = headers
        .get("host")
        .ok_or_else(|| anyhow!("Host header is missing"))?;
    if !is_loopback_host(host, port) {
        bail!("capture save needs a loopback Host on the serve port");
    }
    let origin = headers
        .get("origin")
        .ok_or_else(|| anyhow!("Origin header is missing"))?;
    if !origin.eq_ignore_ascii_case(&format!("http://{host}")) {
        bail!("capture save needs the workbench page origin");
    }
    let content_type = headers
        .get("content-type")
        .map(String::as_str)
        .unwrap_or("");
    if !content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .eq_ignore_ascii_case("application/json")
    {
        bail!("capture save needs application/json");
    }
    Ok(())
}

fn is_loopback_host(host: &str, port: u16) -> bool {
    let expected_port = port.to_string();
    if let Some(rest) = host.strip_prefix('[') {
        let Some((address, suffix)) = rest.split_once(']') else {
            return false;
        };
        return address == "::1" && suffix == format!(":{expected_port}");
    }
    let Some((name, found_port)) = host.rsplit_once(':') else {
        return false;
    };
    found_port == expected_port
        && (name.eq_ignore_ascii_case("localhost")
            || name
                .parse::<std::net::IpAddr>()
                .map(|ip| ip.is_loopback())
                .unwrap_or(false))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedCapture {
    profile: String,
    window_rows: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    profile_id: Option<String>,
    catalog_updated: bool,
}

fn save_capture(root: &Path, body: &Value) -> anyhow::Result<SavedCapture> {
    let profile = body
        .get("profile")
        .ok_or_else(|| anyhow!("profile is missing"))?;
    let capture_windows = body
        .get("windows")
        .ok_or_else(|| anyhow!("windows is missing"))?;
    check_capture(profile, capture_windows)?;

    fs::create_dir_all(root.join("profiles"))?;
    let windows_path = root.join("window.json");
    let mut windows = read_array(&windows_path)?;
    windows.extend(capture_windows.as_array().unwrap().iter().cloned());
    let window_rows = windows.len();

    let profile_bytes = pretty_json(profile)?;
    let profile_path = next_profile_path(&root.join("profiles"), &profile_bytes)?;
    let mut files = vec![PendingFile::new(
        windows_path,
        pretty_json(&Value::Array(windows))?,
    )];
    let runtime_id = body
        .get("runtime")
        .map(obscura_browser::profiles::register_runtime_profile)
        .transpose()
        .map_err(|error| anyhow!(error.to_string()))?;
    if let (Some(runtime), Some(profile_id)) = (body.get("runtime"), runtime_id.as_deref()) {
        let runtime_dir = root.join(RUNTIME_DIR);
        fs::create_dir_all(&runtime_dir)?;
        let runtime_path = runtime_dir.join(format!("runtime-{}.json", &hex_digest(profile_id)[..16]));
        let runtime_bytes = pretty_json(runtime)?;
        if runtime_path.exists() {
            if fs::read(&runtime_path)? != runtime_bytes {
                bail!("runtime profile ID already has different saved data: {profile_id}");
            }
        } else {
            files.push(PendingFile::new(runtime_path, runtime_bytes));
        }
    }
    for (index, file) in files.iter().enumerate() {
        if let Err(error) = file.prepare() {
            let _ = fs::remove_file(&file.next);
            for prepared in &files[..index] {
                let _ = fs::remove_file(&prepared.next);
            }
            return Err(error);
        }
    }
    if let Err(error) = write_new(&profile_path, &profile_bytes) {
        for file in &files {
            let _ = fs::remove_file(&file.next);
        }
        return Err(error);
    }
    if let Err(error) = replace_files(&files) {
        let _ = fs::remove_file(&profile_path);
        return Err(error);
    }

    Ok(SavedCapture {
        profile: profile_path
            .strip_prefix(root)
            .unwrap_or(&profile_path)
            .to_string_lossy()
            .replace('\\', "/"),
        window_rows,
        profile_id: runtime_id,
        catalog_updated: body.get("runtime").is_some(),
    })
}

fn hex_digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn check_capture(profile: &Value, windows: &Value) -> anyhow::Result<()> {
    if profile.get("profileVersion").and_then(Value::as_str) != Some("obscura-capture-v1") {
        bail!("profile is not an Obscura browser capture");
    }
    let fingerprints = profile
        .get("fingerprints")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("profile fingerprints are missing"))?;
    let browser = fingerprints
        .get("browser")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("profile browser data is missing"))?;
    let hardware = fingerprints
        .get("hardware")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("profile hardware data is missing"))?;
    let graphics = hardware
        .get("gpu")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("profile graphics data is missing"))?;
    graphics
        .get("unmaskedVendor")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("profile graphics vendor is missing"))?;
    graphics
        .get("unmaskedRenderer")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("profile graphics renderer is missing"))?;
    graphics
        .get("adapter")
        .ok_or_else(|| anyhow!("profile WebGPU data is missing"))?;
    graphics
        .get("preferredCanvasFormat")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("profile preferred canvas format is missing"))?;
    graphics
        .get("wgslLanguageFeatures")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("profile WGSL language features are missing"))?;
    browser
        .get("webglContext")
        .ok_or_else(|| anyhow!("profile WebGL 1 data is missing"))?;
    browser
        .get("webgl2Context")
        .ok_or_else(|| anyhow!("profile WebGL 2 data is missing"))?;

    let window_rows = windows
        .as_array()
        .filter(|rows| rows.len() == 1)
        .ok_or_else(|| anyhow!("windows must contain one observation"))?;
    let window_row = window_rows[0]
        .as_object()
        .ok_or_else(|| anyhow!("window observation is not an object"))?;
    let window_values = window_row
        .get("window")
        .and_then(Value::as_array)
        .filter(|rows| rows.len() == 1)
        .ok_or_else(|| anyhow!("window observation must have one window"))?;
    if window_row.get("total").and_then(Value::as_u64) != Some(1)
        || window_row.get("screen") != hardware.get("screen")
        || window_values.first() != browser.get("window")
    {
        bail!("profile and screen data are not from the same capture");
    }
    Ok(())
}

fn read_array(path: &Path) -> anyhow::Result<Vec<Value>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let size = fs::metadata(path)?.len();
    if size > MAX_SOURCE_BYTES {
        bail!("source file is too large: {}", path.display());
    }
    let file = File::open(path)?;
    serde_json::from_reader(file).with_context(|| format!("invalid JSON array: {}", path.display()))
}

fn pretty_json(value: &Value) -> anyhow::Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn next_profile_path(directory: &Path, profile_bytes: &[u8]) -> anyhow::Result<PathBuf> {
    let digest = format!("{:x}", Sha256::digest(profile_bytes));
    for number in 1..=999_999 {
        let path = directory.join(format!("capture-{}-{number:03}.json", &digest[..16]));
        if !path.exists() {
            return Ok(path);
        }
    }
    bail!("no free capture profile file name")
}

struct PendingFile {
    target: PathBuf,
    next: PathBuf,
    backup: PathBuf,
    bytes: Vec<u8>,
}

impl PendingFile {
    fn new(target: PathBuf, bytes: Vec<u8>) -> Self {
        let next = PathBuf::from(format!("{}.obscura-new", target.display()));
        let backup = PathBuf::from(format!("{}.obscura-backup", target.display()));
        Self {
            target,
            next,
            backup,
            bytes,
        }
    }

    fn prepare(&self) -> anyhow::Result<()> {
        if self.backup.exists() {
            bail!(
                "old backup needs manual recovery: {}",
                self.backup.display()
            );
        }
        if self.next.exists() {
            fs::remove_file(&self.next)?;
        }
        write_new(&self.next, &self.bytes)
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn replace_files(files: &[PendingFile]) -> anyhow::Result<()> {
    let mut changed = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let existed = file.target.exists();
        if existed {
            if let Err(error) = fs::rename(&file.target, &file.backup) {
                rollback(files, &changed);
                return Err(error.into());
            }
        }
        if let Err(error) = fs::rename(&file.next, &file.target) {
            if existed {
                let _ = fs::rename(&file.backup, &file.target);
            }
            rollback(files, &changed);
            return Err(error.into());
        }
        changed.push((index, existed));
    }
    for (index, existed) in changed {
        if existed {
            fs::remove_file(&files[index].backup)?;
        }
    }
    Ok(())
}

fn rollback(files: &[PendingFile], changed: &[(usize, bool)]) {
    for (index, existed) in changed.iter().rev() {
        let file = &files[*index];
        let _ = fs::remove_file(&file.target);
        if *existed {
            let _ = fs::rename(&file.backup, &file.target);
        }
    }
    for file in files {
        let _ = fs::remove_file(&file.next);
    }
}

fn write_json_error(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    message: &str,
) -> anyhow::Result<()> {
    let body = serde_json::to_vec(&json!({ "ok": false, "error": message }))?;
    write_response(
        stream,
        status,
        reason,
        "application/json; charset=utf-8",
        &body,
    )
}

fn write_text(stream: &mut TcpStream, status: u16, reason: &str, body: &str) -> anyhow::Result<()> {
    write_response(
        stream,
        status,
        reason,
        "text/plain; charset=utf-8",
        body.as_bytes(),
    )
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    body: &[u8],
) -> anyhow::Result<()> {
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len(),
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST: AtomicUsize = AtomicUsize::new(0);

    fn capture() -> Value {
        let screen = json!({ "width": 1920 });
        let window = json!({ "innerWidth": 1200 });
        let webgl1 = json!({ "version": "WebGL 1" });
        let webgl2 = json!({ "version": "WebGL 2" });
        let adapter = json!({ "default": { "features": [] } });
        let profile = json!({
            "profileVersion": "obscura-capture-v1",
            "fingerprints": {
                "browser": { "window": window, "webglContext": webgl1, "webgl2Context": webgl2 },
                "hardware": {
                    "screen": screen,
                    "gpu": {
                        "unmaskedVendor": "vendor", "unmaskedRenderer": "renderer",
                        "preferredCanvasFormat": "bgra8unorm", "wgslLanguageFeatures": ["feature"],
                        "adapter": adapter
                    }
                }
            }
        });
        json!({
            "profile": profile,
            "windows": [{ "total": 1, "window": [window], "screen": screen }]
        })
    }

    fn runtime() -> Value {
        let id = "c150w1:77777777777777777777777777777777:88888888888888888888888888888888:99999999999999999999999999999999";
        let mut seed_hasher = Sha256::new();
        seed_hasher.update(b"graphics-render-v1");
        seed_hasher.update(id.as_bytes());
        let render_seed = format!("{:x}", seed_hasher.finalize());
        json!({
            "id": id, "catalogId": "chrome-windows-v1", "renderSeed": render_seed,
            "browser": { "major": 150, "version": "150.0.1.1", "userAgent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/150.0.0.0 Safari/537.36" },
            "navigator": {
                "platform": "Win32", "uaPlatform": "Windows", "uaPlatformVersion": "19.0.0",
                "architecture": "x86", "bitness": "64", "brands": [], "fullVersionList": [],
                "languages": ["en-US"], "hardwareConcurrency": 8, "deviceMemory": 8.0, "maxTouchPoints": 0
            },
            "network": { "downlink": 1.7, "rtt": 75, "effectiveType": "4g", "saveData": false },
            "screen": {
                "id": "99999999999999999999999999999999", "width": 1920, "height": 1080,
                "availWidth": 1920, "availHeight": 1040, "availLeft": 0, "availTop": 0,
                "colorDepth": 24, "pixelDepth": 24, "devicePixelRatio": 1.0,
                "innerWidth": 1200, "innerHeight": 800, "outerWidth": 1200, "outerHeight": 900,
                "screenX": 0, "screenY": 0, "weight": 1
            },
            "graphics": {
                "id": "88888888888888888888888888888888", "maskedVendor": "WebKit", "maskedRenderer": "WebKit WebGL",
                "unmaskedVendor": "Google Inc. (NVIDIA)", "unmaskedRenderer": "ANGLE (NVIDIA, Test Direct3D11, D3D11)",
                "webgl1Id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "webgl2Id": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "webgpuId": "cccccccccccccccccccccccccccccccc",
                "preferredCanvasFormat": "bgra8unorm", "wgslLanguageFeatures": [],
                "observationsByBrowserVersion": { "150.0.1.1": 1 }, "weight": 1,
                "webgl1": {}, "webgl2": {}, "webgpu": { "adapters": { "default": {} } }
            }
        })
    }

    fn temp_root() -> PathBuf {
        let number = NEXT_TEST.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "obscura-profile-workbench-{}-{number}",
            std::process::id()
        ))
    }

    #[test]
    fn workbench_route_match_is_exact_to_the_prefix() {
        assert!(is_workbench_request(b"GET /obscura/profiles HTTP/1.1\r\n"));
        assert!(is_workbench_request(
            b"POST /obscura/profiles/capture HTTP/1.1\r\n"
        ));
        assert!(!is_workbench_request(b"GET /json/version HTTP/1.1\r\n"));
    }

    #[test]
    fn capture_parts_must_match() {
        let good = capture();
        check_capture(&good["profile"], &good["windows"]).unwrap();
        let mut bad = good.clone();
        bad["windows"][0]["screen"] = json!({ "width": 800 });
        assert!(check_capture(&bad["profile"], &bad["windows"]).is_err());
    }

    #[test]
    fn save_appends_rows_and_never_overwrites_a_profile() {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let mut body = capture();
        body["runtime"] = runtime();
        let first = save_capture(&root, &body).unwrap();
        let second = save_capture(&root, &body).unwrap();
        assert_eq!(second.window_rows, 2);
        assert_ne!(first.profile, second.profile);
        assert!(first.catalog_updated);
        assert_eq!(first.profile_id, second.profile_id);
        assert_eq!(fs::read_dir(root.join("profiles")).unwrap().count(), 2);
        assert_eq!(fs::read_dir(root.join(RUNTIME_DIR)).unwrap().count(), 1);
        assert_eq!(obscura_browser::profiles::load_runtime_profiles(&root.join(RUNTIME_DIR)).unwrap(), 1);
        assert!(!root.join("window.json.obscura-new").exists());
        assert!(!root.join("window.json.obscura-backup").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn write_origin_must_be_same_loopback_server() {
        let mut headers = HashMap::from([
            ("host".to_string(), "127.0.0.1:9222".to_string()),
            ("origin".to_string(), "http://127.0.0.1:9222".to_string()),
            ("content-type".to_string(), "application/json".to_string()),
        ]);
        let loopback = "127.0.0.1:40000".parse().unwrap();
        assert!(check_write_origin(&headers, loopback, 9222).is_ok());
        headers.insert("origin".to_string(), "https://bad.example".to_string());
        assert!(check_write_origin(&headers, loopback, 9222).is_err());
        let remote = "192.0.2.1:40000".parse().unwrap();
        assert!(check_write_origin(&headers, remote, 9222).is_err());
    }
}
