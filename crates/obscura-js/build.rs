use std::path::PathBuf;

/// Fork: the graphics identity layer lives in its own js/ files and is spliced
/// into bootstrap.js at marker comments, so bootstrap.js carries two comment
/// lines for the whole layer instead of ~1400 lines of fork code.
///
/// Splicing is textual on purpose: the modules declare bindings that must land
/// inside bootstrap's IIFE for its own code to resolve them lexically.
///
/// A missing marker is a hard error. Silently dropping the splice would leave
/// the engine advertising upstream's empty WebGL stubs under a profile that
/// promises a real GPU, which is a worse fingerprint than either alone.
fn splice_fork_modules(bootstrap: &str) -> String {
    const MODULES: &[(&str, &[&str])] = &[
        (
            "/* __OBSCURA_FORK_LATE_MODULE__ */",
            &[
                include_str!("js/graphics_shim.js"),
                include_str!("js/graphics_api_v145.js"),
                include_str!("js/graphics.js"),
                include_str!("js/fork_event_target.js"),
                include_str!("js/fork_interfaces.js"),
                include_str!("js/fork_media_codecs.js"),
                include_str!("js/fork_rtc_capabilities.js"),
                include_str!("js/fork_console.js"),
                include_str!("js/fork_viewport_battery.js"),
            ],
        ),
        (
            "/* __OBSCURA_GRAPHICS_PAGE_INIT__ */",
            &[include_str!("js/graphics_page_init.js")],
        ),
        (
            "/* __OBSCURA_FORK_STORAGE__ */",
            &[include_str!("js/fork_storage.js")],
        ),
        // Last statement of __obscura_init, after upstream's own hide-list loop.
        (
            "/* __OBSCURA_FORK_PAGE_INIT_END__ */",
            &[
                include_str!("js/fork_browser_shape.js"),
                include_str!("js/fork_navigator_chrome151.js"),
                include_str!("js/fork_audio_memory.js"),
                include_str!("js/fork_hide_globals.js"),
            ],
        ),
        // Top level, before upstream's `performance = performance || {...}`
        // and `btoa = btoa || ...` fallbacks, so both short-circuit onto ours.
        (
            "/* __OBSCURA_FORK_EARLY_MODULE__ */",
            &[
                include_str!("js/fork_binary_encoding.js"),
                include_str!("js/fork_performance.js"),
            ],
        ),
    ];
    let mut spliced = bootstrap.to_string();
    for (marker, sources) in MODULES {
        assert!(
            spliced.contains(marker),
            "bootstrap.js is missing the fork marker {marker}. An upstream merge \
             probably dropped it; put it back rather than inlining the module."
        );
        spliced = spliced.replace(marker, &sources.join("\n"));
    }
    spliced
}

fn main() {
    println!("cargo:rerun-if-changed=js/bootstrap.js");
    println!("cargo:rerun-if-changed=js/graphics_shim.js");
    println!("cargo:rerun-if-changed=js/graphics_api_v145.js");
    println!("cargo:rerun-if-changed=js/graphics.js");
    println!("cargo:rerun-if-changed=js/graphics_page_init.js");
    println!("cargo:rerun-if-changed=js/fork_binary_encoding.js");
    println!("cargo:rerun-if-changed=js/fork_storage.js");
    println!("cargo:rerun-if-changed=js/fork_interfaces.js");
    println!("cargo:rerun-if-changed=js/fork_media_codecs.js");
    println!("cargo:rerun-if-changed=js/fork_rtc_capabilities.js");
    println!("cargo:rerun-if-changed=js/fork_console.js");
    println!("cargo:rerun-if-changed=js/fork_event_target.js");
    println!("cargo:rerun-if-changed=js/fork_viewport_battery.js");
    println!("cargo:rerun-if-changed=js/fork_browser_shape.js");
    println!("cargo:rerun-if-changed=js/fork_navigator_chrome151.js");
    println!("cargo:rerun-if-changed=js/fork_audio_memory.js");
    println!("cargo:rerun-if-changed=js/fork_hide_globals.js");
    println!("cargo:rerun-if-changed=js/fork_performance.js");
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let snapshot_path = out_dir.join("OBSCURA_SNAPSHOT.bin");

    let bootstrap_js = splice_fork_modules(include_str!("js/bootstrap.js"));

    let output = deno_core::snapshot::create_snapshot(
        deno_core::snapshot::CreateSnapshotOptions {
            cargo_manifest_dir: env!("CARGO_MANIFEST_DIR"),
            startup_snapshot: None,
            skip_op_registration: true,
            extensions: vec![],
            extension_transpiler: None,
            with_runtime_cb: Some(Box::new(move |runtime| {
                runtime
                    .execute_script("<obscura:bootstrap>", bootstrap_js.to_string())
                    .expect("bootstrap.js should not fail during snapshot creation");
            })),
        },
        None,
    )
    .expect("Failed to create V8 snapshot");

    std::fs::write(&snapshot_path, &*output.output).expect("Failed to write snapshot");
    println!(
        "cargo:rustc-env=OBSCURA_SNAPSHOT_PATH={}",
        snapshot_path.display()
    );

    for file in &output.files_loaded_during_snapshot {
        println!("cargo:rerun-if-changed={}", file.display());
    }
}
