//! Fork-only. No `__obscura_*` global may be enumerable on `window`.
//!
//! `Object.keys(window).filter(k => k.includes('obscura'))` is a one-line
//! detection, and it returned four names: __obscura_viewport_w,
//! __obscura_viewport_h, __obscura_screen_emulated and __obscura_click_target.
//!
//! The cause is a timing one. Upstream hides internals with
//! `__obscura_hide_list`, computed ONCE from `Object.getOwnPropertyNames` while
//! build.rs runs bootstrap.js for the V8 snapshot, and the fork's own
//! `_forkHideGlobals` sweep runs at the end of `__obscura_init`. Both can only
//! see names that already exist. These four are created later -- by a host
//! `evaluate` or by a DOM method call -- so a plain assignment creates a fresh
//! enumerable:true property that neither pass ever looked at.
//!
//! Pre-declaring the name in `_preHideInternals` fixes it for good: assigning to
//! an existing writable+configurable property only updates the value and leaves
//! the descriptor alone, no matter how late the assignment happens.

use std::sync::Arc;

use obscura_browser::{BrowserContext, Page};

/// Every `__obscura_*` name our own code assigns after snapshot time, with the
/// place that assigns it. A name that is only ever declared at bootstrap top
/// level is already covered by the snapshot hide list and is not listed here.
const LATE_ASSIGNED_GLOBALS: &[&str] = &[
    "__obscura_viewport_w",           // obscura-js/src/runtime.rs, set_viewport
    "__obscura_viewport_h",           // obscura-js/src/runtime.rs, set_viewport
    "__obscura_screen_w",             // bootstrap.js, _setScreenValues
    "__obscura_screen_h",             // bootstrap.js, _setScreenValues
    "__obscura_screen_emulated",      // bootstrap.js, _setScreenValues
    "__obscura_click_target",         // obscura-cdp/src/domains/input.rs, and click()/focus()
    "__obscura_mouse_down",           // obscura-cdp/src/domains/input.rs
    "__obscura_focused",              // bootstrap.js, focus()/blur()
    "__obscura_css",                  // obscura-browser/src/page.rs, injected stylesheet
    "__obscura_clone_hooks",          // bootstrap.js, lazily built inside a function
    "__obscura_fingerprint_profile",  // obscura-js/src/graphics.rs
    "__obscura_geo_lat",              // obscura-js/src/runtime.rs, geolocation override
    "__obscura_geo_lon",              // obscura-js/src/runtime.rs, geolocation override
    "__obscura_await_meta",           // obscura-js/src/runtime.rs, evaluate-with-await
    "__obscura_await_rejected",       // obscura-js/src/runtime.rs, evaluate-with-await
];

/// Every `__obscura_*` name the engine's own source mentions on `globalThis`.
///
/// Read out of the tree rather than listed by hand, so a global added later is
/// picked up without anyone remembering to update this file. Only `src/` and
/// `js/` are scanned: a test fixture may assign whatever it likes.
fn obscura_globals_in_source() -> Vec<String> {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ is the parent of this crate")
        .to_path_buf();

    let mut sources = Vec::new();
    collect_sources(&crates, &mut sources);

    let mut names = Vec::new();
    for source in sources {
        let text = std::fs::read_to_string(&source).unwrap_or_default();
        for (offset, _) in text.match_indices("globalThis.__obscura_") {
            let name: String = text[offset + "globalThis.".len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            // Rust format! templates build some names at runtime
            // (`__obscura_binding__{}`), and those are page-chosen, not ours.
            if text[offset..].starts_with(&format!("globalThis.{name}{{")) {
                continue;
            }
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names.sort();
    names
}

fn collect_sources(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            // Descend into each crate, but only into its shipped source.
            if name == "target" || name == "tests" || name == "benches" {
                continue;
            }
            collect_sources(&path, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rs") | Some("js")
        ) {
            out.push(path);
        }
    }
}

async fn page() -> Page {
    let context = Arc::new(BrowserContext::new("fork-hidden-globals".to_string()));
    let mut page = Page::new("fork-hidden-globals-page".to_string(), context);
    page.navigate("data:text/html,<button id=b>x</button>")
        .await
        .expect("the fixture page must load");
    page
}

/// Every way a page script can see a global, deduplicated. Object.keys is the
/// obvious one, but a fingerprinter reaching for unusual window properties uses
/// getOwnPropertyNames or Reflect.ownKeys, which report non-enumerable names
/// too, and for-in reads the enumerable flag and cannot be intercepted at all.
const LEAKED: &str = r#"
    (() => {
        const internal = k => k.includes('obscura') || k.includes('Obscura');
        const seen = new Set();
        for (const k of Object.keys(globalThis)) if (internal(k)) seen.add('keys:' + k);
        for (const k of Object.getOwnPropertyNames(globalThis)) if (internal(k)) seen.add('names:' + k);
        for (const k of Reflect.ownKeys(globalThis)) if (typeof k === 'string' && internal(k)) seen.add('ownKeys:' + k);
        for (const k of Object.keys(Object.getOwnPropertyDescriptors(globalThis))) if (internal(k)) seen.add('descriptors:' + k);
        for (const k in globalThis) if (internal(k)) seen.add('forin:' + k);
        return [...seen].sort();
    })()
"#;

#[tokio::test(flavor = "current_thread")]
async fn the_host_setters_do_not_add_enumerable_globals() {
    let mut page = page().await;

    // The real paths that were leaking, driven through the real API rather than
    // simulated, so the test breaks if a setter stops going through bootstrap.
    page.set_viewport((1280.0, 720.0));
    page.set_screen_size_override(Some((1920.0, 1080.0)), true);
    page.evaluate("document.getElementById('b').focus(); document.getElementById('b').click()");

    let leaked = page.evaluate(LEAKED);
    assert_eq!(leaked, serde_json::json!([]), "enumerable internals on window");
}

#[tokio::test(flavor = "current_thread")]
async fn every_obscura_global_in_the_tree_is_on_the_hide_list() {
    // The guard against the next one of these. __obscura_hide_list is what the
    // reflection filter matches on, and what __obscura_init re-hides per
    // navigation, so a name missing from it is a name a page can see. Names that
    // exist at snapshot time land on the list on their own; anything created
    // later has to be added by the fork block in js/fork_hide_globals.js.
    let names = obscura_globals_in_source();
    assert!(
        names.len() > 20,
        "the source scan found only {names:?} -- the scan itself is broken"
    );

    let mut page = page().await;
    let script = format!(
        "(() => {{ const list = new Set(globalThis.__obscura_hide_list || []); \
         return {names:?}.filter(n => !list.has(n)); }})()"
    );
    let missing = page.evaluate(&script);

    assert_eq!(
        missing,
        serde_json::json!([]),
        "these globals are visible to a page script; add them to the \
         lateAssigned list in crates/obscura-js/js/fork_hide_globals.js"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_late_assignment_cannot_make_an_internal_enumerable() {
    let mut page = page().await;

    // Covers the names whose host path needs CDP or a live navigation to reach.
    // Assigning from script is the same operation the host performs, so if the
    // descriptor survives this it survives the host too.
    // evaluate() takes an expression, so the assignments need an IIFE around
    // them. Without it the whole script is a parse error, evaluate quietly
    // returns null, and the test passes for the wrong reason.
    let assignments = LATE_ASSIGNED_GLOBALS
        .iter()
        .map(|name| format!("globalThis.{name} = 1;"))
        .collect::<String>();
    let assigned = page.evaluate(&format!("(() => {{ {assignments} return 'ok'; }})()"));
    assert_eq!(assigned, serde_json::json!("ok"), "the assignments must run");

    let leaked = page.evaluate(LEAKED);
    assert_eq!(leaked, serde_json::json!([]), "enumerable internals on window");
}
