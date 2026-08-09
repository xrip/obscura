use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::{json, Value};

fn selected_row(rows: &Value, id: &str, table: &str) -> Value {
    rows.as_array()
        .unwrap_or_else(|| panic!("{table} is not an array"))
        .iter()
        .find(|row| row.get("id").and_then(Value::as_str) == Some(id))
        .unwrap_or_else(|| panic!("{table} does not contain {id}"))
        .clone()
}

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let catalog_path = manifest_dir.join("data/chrome-windows-v1.json");
    println!("cargo:rerun-if-changed={}", catalog_path.display());

    let source = fs::read_to_string(&catalog_path).expect("read fingerprint catalog");
    assert!(source.len() <= 2 * 1024 * 1024 + 1, "fingerprint catalog exceeds 2 MiB");
    let catalog: Value = serde_json::from_str(&source).expect("parse fingerprint catalog");
    let composition = catalog.get("defaultComposition").expect("defaultComposition");
    let base_id = composition.get("baseId").and_then(Value::as_str).expect("default baseId");
    let graphics_id = composition.get("graphicsId").and_then(Value::as_str).expect("default graphicsId");
    let screen_id = composition.get("screenId").and_then(Value::as_str).expect("default screenId");
    let base = selected_row(&catalog["baseProfiles"], base_id, "baseProfiles");
    let graphics = selected_row(&catalog["graphicsProfiles"], graphics_id, "graphicsProfiles");
    let screen = selected_row(&catalog["screenProfiles"], screen_id, "screenProfiles");
    let webgl1_id = graphics.get("webgl1Id").and_then(Value::as_str).expect("webgl1Id");
    let webgl2_id = graphics.get("webgl2Id").and_then(Value::as_str).expect("webgl2Id");
    let webgpu_id = graphics.get("webgpuId").and_then(Value::as_str).expect("webgpuId");
    let webgpu = selected_row(&catalog["components"]["webgpu"], webgpu_id, "components.webgpu");
    let mut webgpu_adapters = Vec::new();
    let mut webgpu_limits = Vec::new();
    for adapter_id in webgpu["adapters"].as_object().expect("webgpu adapters").values() {
        let adapter_id = adapter_id.as_str().expect("webgpu adapter ID");
        let adapter = selected_row(
            &catalog["components"]["webgpuAdapters"],
            adapter_id,
            "components.webgpuAdapters",
        );
        for key in ["limitsId", "defaultDeviceLimitsId"] {
            let limits_id = adapter[key].as_str().expect("WebGPU limits ID");
            if !webgpu_limits.iter().any(|row: &Value| row["id"] == limits_id) {
                webgpu_limits.push(selected_row(
                    &catalog["components"]["webgpuLimits"],
                    limits_id,
                    "components.webgpuLimits",
                ));
            }
        }
        if !webgpu_adapters.iter().any(|row: &Value| row["id"] == adapter_id) {
            webgpu_adapters.push(adapter);
        }
    }
    let output = json!({
        "schemaVersion": catalog["schemaVersion"],
        "catalogId": catalog["catalogId"],
        "target": catalog["target"],
        "defaultComposition": composition,
        "baseProfile": base,
        "screenProfile": screen,
        "graphicsProfile": graphics,
        "webgl1": selected_row(&catalog["components"]["webgl1"], webgl1_id, "components.webgl1"),
        "webgl2": selected_row(&catalog["components"]["webgl2"], webgl2_id, "components.webgl2"),
        "webgpu": webgpu,
        "webgpuAdapters": webgpu_adapters,
        "webgpuLimits": webgpu_limits
    });
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("chrome-windows-v1.default.json");
    fs::write(out, serde_json::to_vec(&output).expect("serialize fixed profile"))
        .expect("write fixed profile");
    let compressed_path = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("chrome-windows-v1.json.gz");
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(source.as_bytes()).expect("compress fingerprint catalog");
    fs::write(compressed_path, encoder.finish().expect("finish fingerprint catalog compression"))
        .expect("write compressed fingerprint catalog");
}
