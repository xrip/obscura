//! Fork-only: hand the selected fingerprint profile to the page realm.
//!
//! Upstream's bootstrap only learns a user agent and a platform string. The
//! graphics identity layer (js/graphics.js) needs the whole composed profile:
//! GPU vendor and renderer, WebGL and WebGL2 parameter tables, WebGPU adapter
//! limits and features, screen metrics.
//!
//! This lives outside runtime.rs so an upstream rewrite of that file, which
//! happens often, does not touch it. Rust allows an inherent impl in any module
//! of the defining crate, so the method still reads as
//! `runtime.set_fingerprint_profile(..)` at the call site.

use crate::runtime::ObscuraJsRuntime;

impl ObscuraJsRuntime {
    /// Publish the profile for the next `__obscura_init`.
    ///
    /// `profile_json` must be a JSON object literal; it comes from
    /// `ResolvedFingerprintProfile::runtime_json`, which is built by
    /// obscura-browser and never from page input. The page never observes the
    /// global: js/graphics_page_init.js reads and deletes it inside
    /// `__obscura_init`, before any page script runs.
    pub fn set_fingerprint_profile(&mut self, profile_json: &str) {
        self.fingerprint_profile_json = Some(profile_json.to_string());
        let _ = self.runtime.execute_script(
            "<set-fingerprint-profile>",
            format!("globalThis.__obscura_fingerprint_profile={profile_json};"),
        );
    }

    /// Put the selected profile into a child before its `__obscura_init`.
    ///
    /// The main realm consumes and deletes its injected global during init, so
    /// copying that global later would leave frames without the WebGL/WebGPU
    /// identity the page already selected. The JSON is host-owned profile data,
    /// never page input.
    pub(crate) fn copy_fingerprint_profile_to_realm(
        &mut self,
        realm: &deno_core::v8::Global<deno_core::v8::Context>,
    ) {
        let Some(profile_json) = self.fingerprint_profile_json.clone() else {
            return;
        };
        let _ = self.eval_in_realm(
            realm,
            &format!("globalThis.__obscura_fingerprint_profile={profile_json};"),
        );
    }
}
