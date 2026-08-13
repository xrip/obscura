//! Child frame realms.
//!
//! An iframe is a separate browsing context: its own JavaScript realm, its own
//! DOM tree, and its own origin. Without this, a frame's HTML is fetched and its
//! body dropped into a detached document in the *parent's* realm, so no script
//! inside a frame ever runs (issue #600).
//!
//! A frame realm is a second `v8::Context` in the page's isolate. Three things
//! make that practical:
//!
//! - The startup snapshot already contains the whole bootstrap, so a restored
//!   context arrives with every DOM class installed. A realm costs a context
//!   restore, not a re-parse.
//! - The realm's op table is filled from the page realm's, so every shim in the
//!   frame can call ops.
//! - Each realm registers its document in `RealmStates`, and an op looks up the
//!   realm that called it. Making a realm current around the host's calls into
//!   it is not enough, because a frame's timers and settled promises re-enter
//!   JavaScript straight from the event loop.
//!
//! Staying in one isolate is what lets same-origin frames share objects with
//! their parent, the way `iframe.contentWindow.document` does in a browser. A
//! second isolate could never do that.

use std::cell::Cell;
use std::rc::Rc;

use obscura_dom::parse_html;

use crate::ops::{ObscuraState, RealmStates};
use crate::runtime::ObscuraJsRuntime;

/// One child browsing context: its own realm, document and origin, living in
/// the page's isolate.
pub struct FrameRealm {
    context: deno_core::v8::Global<deno_core::v8::Context>,
    /// Held so the frame's entry can be taken out again when the frame dies.
    realms: Rc<std::cell::RefCell<RealmStates>>,
    frame_id: u32,
    parent_frame_id: u32,
    owner_nid: u32,
    url: String,
    origin: String,
    screen_position: Cell<Option<(f64, f64)>>,
    input_screen_origin: Cell<Option<(f64, f64)>>,
    viewport_sync_pending: Cell<bool>,
}

impl Drop for FrameRealm {
    fn drop(&mut self) {
        self.realms.borrow_mut().forget(&self.context);
    }
}

impl FrameRealm {
    /// Builds a frame realm around an already-fetched document.
    ///
    /// The frame inherits the page's browser identity and its shared resources,
    /// by copying them from the parent rather than by being told them, so the
    /// two cannot drift apart.
    pub fn new(
        parent: &mut ObscuraJsRuntime,
        frame_id: u32,
        parent_frame_id: u32,
        url: &str,
        html: &str,
    ) -> Option<Self> {
        Self::new_with_owner(parent, frame_id, parent_frame_id, 0, url, html)
    }

    /// Builds a frame realm and keeps only the owner's node id, not the owner
    /// DOM node. The host needs the id when a child browsing context navigates
    /// itself and must measure the same iframe again.
    pub fn new_with_owner(
        parent: &mut ObscuraJsRuntime,
        frame_id: u32,
        parent_frame_id: u32,
        owner_nid: u32,
        url: &str,
        html: &str,
    ) -> Option<Self> {
        let realms = parent.realm_states();
        let parent_context = realms
            .borrow()
            .context_by_frame_id(parent_frame_id);
        let parent_origin = if parent_frame_id == 0 {
            parent.page_origin()
        } else {
            realms
                .borrow()
                .by_frame_id(parent_frame_id)
                .map(|state| origin_of(&state.borrow().url))
                .unwrap_or_else(|| parent.page_origin())
        };
        let context = parent.create_realm_context()?;
        if !parent.share_ops_with_realm(&context) {
            return None;
        }
        parent.copy_identity_to_realm(&context);
        parent.copy_fingerprint_profile_to_realm(&context);

        // Only a same-origin frame is reachable from the page. Cross-origin
        // keeps its own security token, so V8 answers `undefined` for any
        // property the page tries to read out of it, and nothing about it is
        // published below.
        // An about:blank document inherits the creator's origin. This is the
        // default document of every newly inserted iframe, so it must remain
        // reachable by its parent until it navigates elsewhere.
        let origin = if matches!(url, "about:blank" | "about:srcdoc") {
            parent_origin.clone()
        } else {
            origin_of(url)
        };
        let same_origin = origin != "null" && origin == parent_origin;
        if same_origin {
            if let Some(parent_context) = parent_context.as_ref() {
                parent.share_security_token_from_realm(parent_context, &context);
            } else {
                parent.share_security_token_with_realm(&context);
            }
        }

        let mut state = ObscuraState::new();
        state.dom = Some(parse_html(html));
        state.url = url.to_string();
        state.frame_id = frame_id;
        parent.share_resources_with(&mut state);

        realms.borrow_mut().register(
            context.clone(),
            frame_id,
            Rc::new(std::cell::RefCell::new(state)),
        );

        let realm = FrameRealm {
            context,
            realms,
            frame_id,
            parent_frame_id,
            owner_nid,
            url: url.to_string(),
            origin,
            screen_position: Cell::new(None),
            input_screen_origin: Cell::new(None),
            viewport_sync_pending: Cell::new(false),
        };
        // Both ids before init, not after: init is what installs `parent` and
        // `top`, and a document that runs even one script believing it is
        // top-level has already taken the wrong branch.
        realm
            .run(
                parent,
                &format!(
                    "globalThis.__obscura_frameId = {frame_id};\
                     globalThis.__obscura_parentFrameId = {parent_frame_id};\
                     globalThis.__obscura_init();\
                     globalThis.__documentReadyState__ = 'loading';"
                ),
            )
            .ok()?;
        // Only after init, so the document the page reaches through
        // `contentDocument` is the initialized one.
        if same_origin {
            if let Some(parent_context) = parent_context.as_ref() {
                parent.publish_realm_objects_in_context(parent_context, &realm.context, frame_id);
            } else {
                parent.publish_realm_objects(&realm.context, frame_id);
            }
        }
        Some(realm)
    }

    /// Fires the frame document's lifecycle events, in spec order.
    ///
    /// The page's own document gets these; a frame's did not, and a document
    /// that is never told it finished loading will not run any of the work
    /// scripts defer until then. That is most of what a widget does: a frame
    /// can talk to its parent perfectly and still never build its interface,
    /// which looks like a rendering problem and is a lifecycle one.
    pub fn dispatch_load_events(&self, parent: &mut ObscuraJsRuntime) -> Result<(), String> {
        self.execute_script(
            parent,
            "globalThis.__documentReadyState__ = 'interactive';\
             try { document.dispatchEvent(new Event('DOMContentLoaded', \
                 { bubbles: false, cancelable: false })); } catch (_) {}\
             try { window.dispatchEvent(new Event('DOMContentLoaded', \
                 { bubbles: false, cancelable: false })); } catch (_) {}\
             globalThis.__documentReadyState__ = 'complete';\
             try { document.dispatchEvent(new Event('readystatechange')); } catch (_) {}\
             if (typeof window.onload === 'function') { try { window.onload(); } catch (_) {} }\
             try { window.dispatchEvent(new Event('load', \
                 { bubbles: false, cancelable: false })); } catch (_) {}",
        )
    }

    /// Cancels host work owned by this realm before its context is released.
    pub fn dispose_pending_tasks(&self, parent: &mut ObscuraJsRuntime) -> Result<(), String> {
        self.execute_script(
            parent,
            "globalThis.__obscura_dispose_realm_tasks?.();",
        )
    }

    /// Delivers a `postMessage` that another realm sent to this one.
    pub fn deliver_message(
        &self,
        parent: &mut ObscuraJsRuntime,
        data_json: &str,
        origin: &str,
        source_frame_id: u32,
    ) -> Result<(), String> {
        self.execute_script(
            parent,
            &format!(
                "globalThis.__obscura_deliverMessage({}, {}, {source_frame_id});",
                encode_json_argument(data_json),
                encode_json_argument(origin),
            ),
        )
    }

    pub fn frame_id(&self) -> u32 {
        self.frame_id
    }

    pub fn viewport(&self) -> Option<(f64, f64)> {
        self.realms
            .borrow()
            .by_frame_id(self.frame_id)
            .map(|state| {
                let state = state.borrow();
                (state.viewport.0 as f64, state.viewport.1 as f64)
            })
    }

    pub fn mark_viewport_sync_pending(&self) {
        self.viewport_sync_pending.set(true);
    }

    pub fn viewport_sync_pending(&self) -> bool {
        self.viewport_sync_pending.get()
    }

    pub fn mark_viewport_sync_complete(&self) {
        self.viewport_sync_pending.set(false);
    }

    /// Sets the frame document's viewport before any of its scripts run.
    pub fn set_viewport(
        &self,
        parent: &mut ObscuraJsRuntime,
        width: f64,
        height: f64,
    ) -> Result<(), String> {
        let width = if width.is_finite() && width > 0.0 {
            width
        } else {
            300.0
        };
        let height = if height.is_finite() && height > 0.0 {
            height
        } else {
            150.0
        };
        #[cfg(feature = "render")]
        if let Some(state) = self.realms.borrow().by_frame_id(self.frame_id) {
            let mut state = state.borrow_mut();
            let viewport = (width as f32, height as f32);
            if state.viewport != viewport {
                state.viewport = viewport;
                state.prepared_render = None;
                state.pending_style_mutations.clear();
                state.resolved_scroll = None;
            }
        }
        // A child window keeps the containing browser's chrome inset, but its
        // own outer dimensions are based on the child viewport. Reporting the
        // parent's full outer window here exposes the top-level screen shape
        // inside every iframe, unlike Chrome.
        let parent_outer_height = parent
            .evaluate("Number(outerHeight)")
            .ok()
            .and_then(|value| value.as_f64())
            .unwrap_or(height);
        let parent_inner_height = parent
            .evaluate("Number(innerHeight)")
            .ok()
            .and_then(|value| value.as_f64())
            .unwrap_or(height);
        let chrome_height = (parent_outer_height - parent_inner_height).max(0.0);
        let child_outer_width = width.max(1.0);
        let child_outer_height = (height + chrome_height).max(height);
        self.execute_script(
            parent,
            &format!(
                "globalThis.innerWidth={width};globalThis.innerHeight={height};\
                 globalThis.outerWidth={child_outer_width};globalThis.outerHeight={child_outer_height};\
                 if(typeof globalThis.__obscura_set_visual_viewport_size==='function'){{\
                   globalThis.__obscura_set_visual_viewport_size({width},{height});\
                 }}else if(globalThis.visualViewport){{\
                   globalThis.visualViewport.width={width};\
                   globalThis.visualViewport.height={height};\
                 }}"
            ),
        )?;

        // Keep the parent-side cross-origin contentWindow shim in step with
        // the real child realm. The shim is what parent JavaScript can see.
        let sync_window = format!(
            "if(globalThis.__obscura_frameWindows && \
                 globalThis.__obscura_frameWindows[{frame_id}]){{\
               const w=globalThis.__obscura_frameWindows[{frame_id}];\
               w.innerWidth={width};w.innerHeight={height};\
               w.outerWidth={child_outer_width};w.outerHeight={child_outer_height};\
             }}"
            , frame_id = self.frame_id
        );
        if self.parent_frame_id == 0 {
            parent.execute_script("sync iframe window metrics", &sync_window)
        } else if let Some(context) = self
            .realms
            .borrow()
            .context_by_frame_id(self.parent_frame_id)
        {
            parent.eval_in_realm(&context, &sync_window).map(|_| ())
        } else {
            Ok(())
        }
    }

    /// Set the child browsing context's screen position from its embedding
    /// box. Input events in a real iframe use this screen origin.
    pub fn set_screen_position(&self, parent: &mut ObscuraJsRuntime, x: f64, y: f64) -> bool {
        if !x.is_finite() || !y.is_finite() || self.screen_position.get() == Some((x, y)) {
            return false;
        }
        if self
            .execute_script(
                parent,
                &format!(
                    "globalThis.screenX={x};globalThis.screenY={y};\
                     globalThis.screenLeft={x};globalThis.screenTop={y};"
                ),
            )
            .is_err()
        {
            return false;
        }
        self.screen_position.set(Some((x, y)));
        true
    }

    /// Set the absolute screen origin of the frame's viewport for input
    /// events. This is separate from window.screenX/screenY: a child window
    /// reports the containing browser window origin, while event screen
    /// coordinates include the iframe's embedding offset.
    pub fn set_input_screen_origin(
        &self,
        parent: &mut ObscuraJsRuntime,
        x: f64,
        y: f64,
    ) -> bool {
        if !x.is_finite() || !y.is_finite() || self.input_screen_origin.get() == Some((x, y)) {
            return false;
        }
        if self
            .execute_script(
                parent,
                &format!(
                    "Object.defineProperty(globalThis,'__obscura_inputScreenX',{{value:{x},writable:true,configurable:true}});\
                     Object.defineProperty(globalThis,'__obscura_inputScreenY',{{value:{y},writable:true,configurable:true}});"
                ),
            )
            .is_err()
        {
            return false;
        }
        self.input_screen_origin.set(Some((x, y)));
        true
    }

    pub fn parent_frame_id(&self) -> u32 {
        self.parent_frame_id
    }

    pub fn owner_nid(&self) -> u32 {
        self.owner_nid
    }

    pub fn take_pending_navigation(&self) -> Option<(String, String, String)> {
        self.realms
            .borrow()
            .by_frame_id(self.frame_id)
            .and_then(|state| state.borrow_mut().pending_navigation.take())
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// The frame's origin, or `"null"` for a document with an opaque origin.
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// Whether script from `other_origin` may reach into this frame's DOM. Two
    /// opaque origins are never same-origin, which is why `"null"` never
    /// matches.
    pub fn is_same_origin_as(&self, other_origin: &str) -> bool {
        self.origin != "null" && self.origin == other_origin
    }

    /// Runs `source` in the frame's realm. Ops called from it find the frame's
    /// document by looking up the realm they were called from.
    fn run(&self, parent: &mut ObscuraJsRuntime, source: &str) -> Result<String, String> {
        parent.eval_in_realm(&self.context, source)
    }

    /// Runs a script inside the frame, reporting a script error as `Err`.
    pub fn execute_script(
        &self,
        parent: &mut ObscuraJsRuntime,
        source: &str,
    ) -> Result<(), String> {
        self.run(parent, source).map(|_| ())
    }

    /// Evaluates an expression inside the frame and decodes it as JSON.
    pub fn evaluate(
        &self,
        parent: &mut ObscuraJsRuntime,
        expression: &str,
    ) -> Result<serde_json::Value, String> {
        let json = self.run(parent, &format!("JSON.stringify({expression})"))?;
        serde_json::from_str(&json).map_err(|error| error.to_string())
    }

    /// Runs the frame document's classic scripts, in document order.
    ///
    /// `load_external` resolves a `src=` script to its source text; returning
    /// `None` skips it, which is what a failed subresource fetch looks like to
    /// the page. One script throwing does not stop the ones after it, matching
    /// how a browser treats separate classic scripts.
    ///
    /// Module scripts are skipped and reported: they need the frame's own module
    /// loader, which is not wired up yet.
    ///
    /// Returns one message per script that failed or was skipped.
    pub fn run_document_scripts(
        &self,
        parent: &mut ObscuraJsRuntime,
        load_external: impl Fn(&str) -> Option<String>,
    ) -> Vec<String> {
        let scripts = match self.list_scripts(parent) {
            Ok(scripts) => scripts,
            Err(error) => return vec![error],
        };

        let mut problems = Vec::new();
        for (index, script) in scripts.iter().enumerate() {
            if !script.is_classic() {
                if script.type_attribute == "module" {
                    problems.push(format!("frame module script {index} skipped: not supported"));
                }
                continue;
            }

            let (name, source) = if script.src.is_empty() {
                (format!("inline {index}"), script.text.clone())
            } else {
                let resolved = self.resolve(&script.src);
                match load_external(&resolved) {
                    Some(source) => (resolved, source),
                    None => {
                        problems.push(format!("frame script {resolved} could not be loaded"));
                        continue;
                    }
                }
            };
            if source.trim().is_empty() {
                continue;
            }
            if let Err(error) = self.execute_script(parent, &source) {
                problems.push(format!("frame script {name} failed: {error}"));
            }
        }
        problems
    }

    /// Absolute URLs of the frame's `src=` classic scripts, in document order.
    ///
    /// A caller that fetches over the network needs the list before running
    /// anything, because `run_document_scripts` resolves sources synchronously.
    pub fn external_script_urls(&self, parent: &mut ObscuraJsRuntime) -> Vec<String> {
        self.list_scripts(parent)
            .unwrap_or_default()
            .iter()
            .filter(|script| script.is_classic() && !script.src.is_empty())
            .map(|script| self.resolve(&script.src))
            .collect()
    }

    /// Resolves a subresource URL against the frame's own document URL, not the
    /// parent's. A relative `src` in a frame is relative to the frame.
    fn resolve(&self, src: &str) -> String {
        url::Url::parse(&self.url)
            .and_then(|base| base.join(src))
            .map(|url| url.to_string())
            .unwrap_or_else(|_| src.to_string())
    }

    fn list_scripts(&self, parent: &mut ObscuraJsRuntime) -> Result<Vec<DocumentScript>, String> {
        let listed = self.evaluate(
            parent,
            r#"[...document.querySelectorAll('script')].map(node => ({
                src: node.getAttribute('src') || '',
                type: (node.getAttribute('type') || '').toLowerCase(),
                text: node.textContent || '',
            }))"#,
        );
        match listed {
            Ok(value) => Ok(serde_json::from_value(value).unwrap_or_default()),
            Err(error) => Err(format!("could not list frame scripts: {error}")),
        }
    }
}

#[derive(serde::Deserialize)]
struct DocumentScript {
    src: String,
    #[serde(rename = "type")]
    type_attribute: String,
    text: String,
}

impl DocumentScript {
    /// An empty type, or a JavaScript MIME type, is a classic script. Anything
    /// else is data or a module.
    fn is_classic(&self) -> bool {
        self.type_attribute.is_empty()
            || matches!(
                self.type_attribute.as_str(),
                "text/javascript" | "application/javascript" | "text/ecmascript"
            )
    }
}

/// Embeds a string in JavaScript source as a literal, so a payload holding
/// quotes or newlines cannot end the literal and be read as code.
fn encode_json_argument(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

/// Serializes an origin the way `location.origin` does, using `"null"` for
/// schemes that have no tuple origin.
fn origin_of(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(parsed) => {
            let origin = parsed.origin();
            if origin.is_tuple() {
                origin.ascii_serialization()
            } else {
                "null".to_string()
            }
        }
        Err(_) => "null".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn page(url: &str, html: &str) -> ObscuraJsRuntime {
        let mut runtime = ObscuraJsRuntime::new();
        runtime.set_dom(parse_html(html));
        runtime.set_url(url);
        runtime.run_page_init();
        runtime
    }

    #[test]
    fn frame_has_its_own_realm_dom_and_origin() {
        let mut parent = page(
            "https://parent.example/page",
            "<html><body><h1>Parent</h1></body></html>",
        );
        parent
            .execute_script("p", "globalThis.marker = 'parent';")
            .unwrap();

        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://child.example/frame",
            "<html><body><h1>Child</h1></body></html>",
        )
        .expect("frame realm");

        frame
            .execute_script(&mut parent, "globalThis.marker = 'child';")
            .unwrap();

        // Separate realm: own globals, own DOM, own URL.
        assert_eq!(
            frame
                .evaluate(&mut parent, "document.querySelector('h1').textContent")
                .unwrap(),
            serde_json::json!("Child")
        );
        assert_eq!(
            frame.evaluate(&mut parent, "globalThis.marker").unwrap(),
            serde_json::json!("child")
        );
        assert_eq!(
            frame.evaluate(&mut parent, "location.href").unwrap(),
            serde_json::json!("https://child.example/frame")
        );

        // The parent keeps its own document and globals throughout.
        assert_eq!(
            parent
                .evaluate("document.querySelector('h1').textContent")
                .unwrap(),
            serde_json::json!("Parent")
        );
        assert_eq!(
            parent.evaluate("globalThis.marker").unwrap(),
            serde_json::json!("parent")
        );

        assert_eq!(frame.origin(), "https://child.example");
        assert_eq!(frame.frame_id(), 1);
        assert!(!frame.is_same_origin_as("https://parent.example"));
        assert!(frame.is_same_origin_as("https://child.example"));
    }

    #[test]
    fn nested_same_origin_frame_is_published_into_its_parent_realm() {
        let mut parent = page(
            "https://parent.example/page",
            "<html><body><iframe></iframe></body></html>",
        );
        let outer = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://parent.example/outer",
            "<html><body><iframe></iframe></body></html>",
        )
        .expect("outer frame realm");
        let nested = FrameRealm::new(
            &mut parent,
            2,
            1,
            "about:blank",
            "<html><body><p>Nested</p></body></html>",
        )
        .expect("nested frame realm");

        assert_eq!(nested.origin(), "https://parent.example");
        assert_eq!(
            outer
                .evaluate(
                    &mut parent,
                    "Boolean(globalThis.__obscura_frameObjects[2].document)"
                )
                .unwrap(),
            serde_json::json!(true)
        );
        assert_eq!(
            outer
                .evaluate(
                    &mut parent,
                    "globalThis.__obscura_frameObjects[2].document.querySelector('p').textContent"
                )
                .unwrap(),
            serde_json::json!("Nested")
        );
    }

    #[test]
    fn iframe_document_inner_html_updates_its_pending_document() {
        let mut parent = page(
            "https://parent.example/page",
            "<html><body></body></html>",
        );
        parent
            .execute_script(
                "iframe document write",
                "(function(){const f=document.createElement('iframe');document.body.appendChild(f);f.contentDocument.body.innerHTML='<p>Nested</p>';})()",
            )
            .unwrap();

        let pending = parent.take_pending_frames();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].html.contains("<p>Nested</p>"));
    }

    #[test]
    fn nested_iframe_document_inner_html_updates_its_pending_document() {
        let mut parent = page(
            "https://parent.example/page",
            "<html><body></body></html>",
        );
        let outer = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://parent.example/outer",
            "<html><body></body></html>",
        )
        .expect("outer frame realm");
        outer
            .execute_script(
                &mut parent,
                "(function(){const f=document.createElement('iframe');document.body.appendChild(f);f.contentDocument.body.innerHTML='<p>Nested</p>';})()",
            )
            .unwrap();

        let pending = parent.take_pending_frames();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].parent_frame_id, 1);
        assert!(pending[0].html.contains("<p>Nested</p>"));
    }

    #[test]
    fn iframe_srcdoc_starts_a_pending_document() {
        let mut parent = page(
            "https://parent.example/page",
            "<html><body></body></html>",
        );
        parent
            .execute_script(
                "iframe srcdoc",
                "(function(){const f=document.createElement('iframe');f.srcdoc='<p>Srcdoc</p>';document.body.appendChild(f);})()",
            )
            .unwrap();

        let pending = parent.take_pending_frames();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].url, "about:srcdoc");
        assert!(pending[0].html.contains("<p>Srcdoc</p>"));
    }

    #[test]
    fn srcdoc_html_without_body_keeps_head_scripts_visible() {
        let mut parent = page("https://parent.example/page", "<html><body></body></html>");
        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "about:srcdoc",
            r#"<html><meta http-equiv="content-security-policy" content="script-src 'unsafe-inline'"><script>globalThis.srcdocRan = true;</script></html>"#,
        )
        .expect("srcdoc frame realm");

        let problems = frame.run_document_scripts(&mut parent, |_| None);
        assert!(problems.is_empty(), "unexpected problems: {problems:?}");
        assert_eq!(
            frame
                .evaluate(
                    &mut parent,
                    "[document.querySelectorAll('script').length, globalThis.srcdocRan === true]",
                )
                .unwrap(),
            serde_json::json!([1, true])
        );
    }

    #[test]
    fn a_new_frame_starts_loading_until_load_events() {
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://child.example/frame",
            "<html><body></body></html>",
        )
        .expect("frame realm");

        assert_eq!(
            frame.evaluate(&mut parent, "document.readyState").unwrap(),
            serde_json::json!("loading"),
        );

        frame.dispatch_load_events(&mut parent).unwrap();
        assert_eq!(
            frame.evaluate(&mut parent, "document.readyState").unwrap(),
            serde_json::json!("complete"),
        );
    }

    #[test]
    fn a_frame_inherits_the_selected_graphics_profile() {
        let mut parent = ObscuraJsRuntime::new();
        parent.set_fingerprint_profile(
            r#"{"renderSeed":"deadbeef","graphics":{"webgpu":{"adapters":{"default":{"features":[],"limits":{},"info":{}}}}}}"#,
        );
        parent.set_dom(parse_html("<html><body></body></html>"));
        parent.set_url("https://parent.example/");
        parent.run_page_init();

        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://child.example/frame",
            "<html><body></body></html>",
        )
        .expect("frame realm");

        assert_eq!(
            parent.evaluate("typeof navigator.gpu").unwrap(),
            serde_json::json!("object"),
        );
        assert_eq!(
            frame.evaluate(&mut parent, "typeof navigator.gpu").unwrap(),
            serde_json::json!("object"),
        );
        assert_eq!(
            frame
                .evaluate(&mut parent, "[typeof crossOriginIsolated, crossOriginIsolated]")
                .unwrap(),
            serde_json::json!(["boolean", false]),
        );
    }

    #[test]
    fn frame_shadow_roots_use_the_frame_dom_not_the_parent_dom() {
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://child.example/frame",
            "<html><body></body></html>",
        )
        .expect("frame realm");

        frame
            .execute_script(
                &mut parent,
                "document.body.attachShadow({ mode: 'open' });",
            )
            .unwrap();

        assert_eq!(
            frame.evaluate(&mut parent, "document.body.shadowRoot !== null")
                .unwrap(),
            serde_json::json!(true),
        );
        assert_eq!(
            parent.evaluate("document.body.shadowRoot === null").unwrap(),
            serde_json::json!(true),
        );
    }

    #[cfg(feature = "render")]
    #[test]
    fn frame_shadow_elements_have_layout_geometry() {
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://child.example/frame",
            "<html><body></body></html>",
        )
        .expect("frame realm");
        frame.set_viewport(&mut parent, 300.0, 65.0).unwrap();

        let result = frame
            .evaluate(
                &mut parent,
                r#"(() => {
                    const root = document.body.attachShadow({ mode: 'closed' });
                    root.innerHTML = '<style>div { width: 100px; height: 20px }</style><div id="inside"></div>';
                    const inside = root.querySelector('#inside');
                    const rect = inside.getBoundingClientRect();
                    const metrics = JSON.parse(Deno.core.ops.op_layout_metrics());
                    return [innerWidth, metrics.clientWidth, metrics.clientHeight,
                        root.childNodes.length, rect.width, rect.height,
                        document.elementFromPoint(10, 10) === document.body];
                })()"#,
            )
            .unwrap();
        assert_eq!(result, serde_json::json!([300, 300, 65, 2, 100, 20, true]));
    }

    #[test]
    fn frame_uses_its_embedding_viewport() {
        let mut parent = page(
            "https://parent.example/page",
            "<html><body><iframe style='width:300px;height:65px'></iframe></body></html>",
        );
        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://child.example/frame",
            "<html><body></body></html>",
        )
        .expect("frame realm");

        parent
            .execute_script(
                "install iframe window test shim",
                "__obscura_frameWindows[1]={innerWidth:300,innerHeight:150,outerWidth:300,outerHeight:150};",
            )
            .unwrap();
        frame.set_viewport(&mut parent, 300.0, 65.0).unwrap();
        let parent_metrics = parent
            .evaluate("[innerWidth,innerHeight,outerWidth,outerHeight]")
            .unwrap();
        let parent_inner_height = parent_metrics[1].as_f64().unwrap();
        let parent_outer_height = parent_metrics[3].as_f64().unwrap();
        let child_outer_height = 65.0 + (parent_outer_height - parent_inner_height).max(0.0);
        let parent_window = parent
            .evaluate(
                "[__obscura_frameWindows[1].innerWidth,\
                 __obscura_frameWindows[1].innerHeight,\
                 __obscura_frameWindows[1].outerWidth,\
                 __obscura_frameWindows[1].outerHeight]",
            )
            .unwrap();
        let child_viewport = frame
            .evaluate(
                &mut parent,
                "[innerWidth,innerHeight,outerWidth,outerHeight,visualViewport.width,visualViewport.height]",
            )
            .unwrap();
        assert_eq!(
            child_viewport,
            serde_json::json!([300, 65, 300, child_outer_height as u64, 300, 65]),
        );
        assert_eq!(
            parent_window,
            serde_json::json!([300, 65, 300, child_outer_height as u64]),
        );
    }

    #[test]
    fn dynamic_inner_html_starts_iframe_navigation() {
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://child.example/frame",
            "<html><body></body></html>",
        )
        .expect("frame realm");

        let state = frame
            .evaluate(
                &mut parent,
                r#"(function() {
                    const oldFetch = globalThis.fetch;
                    globalThis.fetch = () => Promise.resolve({ ok: false, type: 'basic' });
                    const shadow = document.body.attachShadow({ mode: 'open' });
                    shadow.innerHTML = '<iframe></iframe>';
                    const iframe = shadow.querySelector('iframe');
                    globalThis.fetch = oldFetch;
                    return [iframe.src, iframe._iframeLoadingUrl, iframe._frameId];
                })()"#,
            )
            .unwrap();

        assert_eq!(
            state,
            serde_json::json!(["", "about:blank", 1])
        );
    }

    #[test]
    fn iframe_window_location_starts_navigation() {
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://child.example/frame",
            "<html><body></body></html>",
        )
        .expect("frame realm");

        let state = frame
            .evaluate(
                &mut parent,
                r#"(function() {
                    const oldFetch = globalThis.fetch;
                    globalThis.fetch = () => Promise.resolve({ ok: false, type: 'basic' });
                    const iframe = document.createElement('iframe');
                    document.body.appendChild(iframe);
                    iframe.contentWindow.location.replace('https://nested.example/challenge');
                    const result = [iframe._frameId, iframe._iframeLoadingUrl];
                    globalThis.fetch = oldFetch;
                    return result;
                })()"#,
            )
            .unwrap();

        assert_eq!(
            state,
            serde_json::json!([0, "https://nested.example/challenge"])
        );
    }

    /// A frame must not look like a different browser than its parent. Anti-bot
    /// code fingerprints inside the frame and compares it with the top document.
    #[test]
    fn frame_inherits_the_parent_browser_identity() {
        let user_agent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) TestAgent/150.0.0.0";
        let mut parent = ObscuraJsRuntime::new();
        parent.set_user_agent(user_agent);
        parent.set_platform("Win32", "Windows", "19.0.0");
        parent.set_dom(parse_html("<html><body></body></html>"));
        parent.set_url("https://parent.example/");
        parent.run_page_init();

        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://child.example/f",
            "<html><body></body></html>",
        )
        .expect("frame realm");

        for surface in [
            "navigator.userAgent",
            "navigator.platform",
            "navigator.userAgentData.platform",
        ] {
            assert_eq!(
                frame.evaluate(&mut parent, surface).unwrap(),
                parent.evaluate(surface).unwrap(),
                "frame and parent disagree on {surface}"
            );
        }
        assert_eq!(
            frame.evaluate(&mut parent, "navigator.userAgent").unwrap(),
            serde_json::json!(user_agent)
        );
    }

    /// The capability the frame realm exists for: scripts that arrived with the
    /// frame's document run, in order, against the frame's own DOM.
    #[test]
    fn frame_runs_its_document_scripts_in_order() {
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://child.example/dir/page",
            r#"<html><body><div id="out"></div>
               <script>window.log = ['inline1'];</script>
               <script src="first.js"></script>
               <script src="/second.js"></script>
               <script>window.log.push('inline2');
                       document.getElementById('out').textContent = window.log.join(',');</script>
               </body></html>"#,
        )
        .expect("frame realm");

        let requested = RefCell::new(Vec::new());
        let problems = frame.run_document_scripts(&mut parent, |url| {
            requested.borrow_mut().push(url.to_string());
            match url {
                "https://child.example/dir/first.js" => Some("window.log.push('ext1');".into()),
                "https://child.example/second.js" => Some("window.log.push('ext2');".into()),
                _ => None,
            }
        });

        assert!(problems.is_empty(), "unexpected problems: {problems:?}");
        // Relative and root-relative src resolve against the frame's URL, not
        // the parent's.
        assert_eq!(
            requested.into_inner(),
            vec![
                "https://child.example/dir/first.js".to_string(),
                "https://child.example/second.js".to_string(),
            ]
        );
        assert_eq!(
            frame
                .evaluate(&mut parent, "document.getElementById('out').textContent")
                .unwrap(),
            serde_json::json!("inline1,ext1,ext2,inline2")
        );
        // The frame's document writes never touch the parent's DOM.
        assert_eq!(
            parent.evaluate("document.body.innerHTML").unwrap(),
            serde_json::json!("")
        );
    }

    #[test]
    fn one_bad_frame_script_does_not_stop_the_rest() {
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://child.example/",
            r#"<html><body>
               <script>window.log = ['a'];</script>
               <script>throw new Error('boom');</script>
               <script src="missing.js"></script>
               <script type="module">window.log.push('module');</script>
               <script>window.log.push('b');</script>
               </body></html>"#,
        )
        .expect("frame realm");

        let problems = frame.run_document_scripts(&mut parent, |_| None);

        assert_eq!(
            frame.evaluate(&mut parent, "window.log.join(',')").unwrap(),
            serde_json::json!("a,b")
        );
        assert_eq!(problems.len(), 3, "problems: {problems:?}");
        assert!(problems.iter().any(|p| p.contains("boom")), "{problems:?}");
        assert!(
            problems.iter().any(|p| p.contains("missing.js")),
            "{problems:?}"
        );
        assert!(problems.iter().any(|p| p.contains("module")), "{problems:?}");
    }

    #[test]
    fn many_frames_can_be_alive_at_once() {
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        let frames: Vec<FrameRealm> = (0..4)
            .map(|index| {
                FrameRealm::new(
                    &mut parent,
                    // Frame ids start at 1: 0 names the page itself, which is
                    // what a DOM call from an unframed realm reports.
                    index + 1,
                    0,
                    &format!("https://f{index}.example/"),
                    &format!("<html><body><h1>{index}</h1></body></html>"),
                )
                .expect("frame realm")
            })
            .collect();

        for (index, frame) in frames.iter().enumerate() {
            frame
                .execute_script(&mut parent, &format!("globalThis.n = {index};"))
                .unwrap();
        }
        // Out-of-order access must be safe: each frame carries its own state.
        for (index, frame) in frames.iter().enumerate().rev() {
            assert_eq!(
                frame.evaluate(&mut parent, "globalThis.n").unwrap().as_f64(),
                Some(index as f64)
            );
            assert_eq!(
                frame
                    .evaluate(&mut parent, "document.querySelector('h1').textContent")
                    .unwrap(),
                serde_json::json!(index.to_string())
            );
        }
    }

    /// The hard case. A frame's deferred work re-enters JavaScript from the
    /// event loop, long after the host last called into the frame, so nothing
    /// can have made the frame "current" for it. It has to find its own
    /// document anyway.
    #[tokio::test(flavor = "current_thread")]
    async fn a_frames_deferred_work_still_sees_the_frames_document() {
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://child.example/",
            "<html><body></body></html>",
        )
        .expect("frame realm");

        // 50ms, not 0: a zero delay drains as a microtask while the host is
        // still inside the frame, which would hide the bug this guards.
        frame
            .execute_script(
                &mut parent,
                "setTimeout(() => { document.body.setAttribute('data-who', location.href); }, 50);",
            )
            .unwrap();
        parent.run_event_loop_bounded(300).await.unwrap();

        assert_eq!(
            frame
                .evaluate(&mut parent, "document.body.getAttribute('data-who')")
                .unwrap(),
            serde_json::json!("https://child.example/"),
            "the frame's timer did not write to the frame's own document"
        );
        assert_eq!(
            parent
                .evaluate("document.body.getAttribute('data-who')")
                .unwrap(),
            serde_json::Value::Null,
            "the frame's timer wrote to the parent's document"
        );
    }

    /// A frame's timers cannot go through deno_core's queue: `op_timer_queue`
    /// reads per-context state that only a deno_core-created context carries,
    /// and queueing from a snapshot realm dereferences uninitialized memory,
    /// which aborts the process rather than failing a test. This is the guard
    /// against that path ever being restored.
    #[tokio::test(flavor = "current_thread")]
    async fn a_frame_timer_fires_without_deno_cores_timer_queue() {
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://child.example/",
            "<html><body></body></html>",
        )
        .expect("frame realm");
        frame
            .execute_script(&mut parent, "setTimeout(() => { globalThis.fired = 1; }, 50);")
            .unwrap();
        parent.run_event_loop_bounded(300).await.unwrap();
        assert_eq!(
            frame.evaluate(&mut parent, "globalThis.fired || 0").unwrap(),
            serde_json::json!(1),
            "the frame's timer callback never ran"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn disposing_a_frame_stops_its_interval() {
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://child.example/frame",
            "<html><body></body></html>",
        )
        .expect("frame realm");

        frame
            .execute_script(
                &mut parent,
                "globalThis.__ticks=0;setInterval(()=>__ticks++,1);",
            )
            .unwrap();
        parent.run_event_loop_bounded(20).await.unwrap();
        let before = frame.evaluate(&mut parent, "__ticks").unwrap();

        frame.dispose_pending_tasks(&mut parent).unwrap();
        parent.run_event_loop_bounded(30).await.unwrap();

        assert_eq!(frame.evaluate(&mut parent, "__ticks").unwrap(), before);
    }

    /// Frame timers run on a separate queue from the page's, so cancelling one
    /// has its own path and its own way to go wrong.
    #[tokio::test(flavor = "current_thread")]
    async fn clear_timeout_cancels_a_frame_timer() {
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://child.example/",
            "<html><body></body></html>",
        )
        .expect("frame realm");
        frame
            .execute_script(
                &mut parent,
                "globalThis.kept = 0;\
                 const cancelled = setTimeout(() => { globalThis.kept = 1; }, 50);\
                 setTimeout(() => { globalThis.kept = 2; }, 50);\
                 clearTimeout(cancelled);",
            )
            .unwrap();
        parent.run_event_loop_bounded(300).await.unwrap();
        assert_eq!(
            frame.evaluate(&mut parent, "globalThis.kept").unwrap(),
            serde_json::json!(2),
            "clearTimeout did not cancel exactly the frame timer it was given"
        );
    }

    /// V8 reports the frame as the microtask context, so a promise continuation
    /// resolves ops against the frame without any help from the host.
    #[tokio::test(flavor = "current_thread")]
    async fn a_frames_promise_continuation_sees_the_frames_document() {
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://child.example/",
            "<html><body></body></html>",
        )
        .expect("frame realm");
        frame
            .execute_script(
                &mut parent,
                "Promise.resolve().then(() => { \
                   document.body.setAttribute('data-who', location.href); });",
            )
            .unwrap();
        parent.run_event_loop_bounded(300).await.unwrap();
        assert_eq!(
            frame
                .evaluate(&mut parent, "document.body.getAttribute('data-who')")
                .unwrap(),
            serde_json::json!("https://child.example/"),
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_frames_rejected_promise_does_not_crash_the_isolate() {
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        let frame = FrameRealm::new(
            &mut parent,
            2,
            1,
            "https://child.example/",
            "<html><body></body></html>",
        )
        .expect("frame realm");

        frame
            .execute_script(
                &mut parent,
                "Promise.reject(new Error('child rejection'));",
            )
            .unwrap();

        let error = parent
            .run_event_loop_bounded(100)
            .await
            .expect_err("an unhandled rejection must still be reported");
        assert!(error.contains("child rejection"), "unexpected error: {error}");
        assert_eq!(parent.evaluate("1 + 1").unwrap(), serde_json::json!(2.0));
    }

    /// A frame posting to `parent` must reach the page, arrive trusted, and
    /// carry the frame's origin. Turnstile and every widget like it drop an
    /// untrusted message silently, so an untrusted delivery is not a cosmetic
    /// difference, it is the widget hanging forever.
    #[test]
    fn a_frame_posts_to_its_parent_as_a_trusted_message() {
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        parent
            .execute_script(
                "p",
                "globalThis.got = [];\
                 addEventListener('message', (e) => globalThis.got.push(\
                   [e.data, e.origin, e.isTrusted]));",
            )
            .unwrap();
        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://child.example/f",
            "<html><body></body></html>",
        )
        .expect("frame realm");

        frame
            .execute_script(&mut parent, "parent.postMessage({token: 'ok'}, '*');")
            .unwrap();
        // The host is the transport, exactly as `Page` does between turns.
        let queued = parent.take_pending_frame_messages();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].target_frame_id, 0);
        assert_eq!(queued[0].source_frame_id, 1);
        let script = format!(
            "globalThis.__obscura_deliverMessage({}, {}, {});",
            serde_json::to_string(&queued[0].data_json).unwrap(),
            serde_json::to_string(&queued[0].origin).unwrap(),
            queued[0].source_frame_id,
        );
        parent.execute_script("<frame-message>", &script).unwrap();

        assert_eq!(
            parent.evaluate("globalThis.got").unwrap(),
            serde_json::json!([[{"token": "ok"}, "https://child.example", true]]),
        );
    }

    /// `parent === window` is how a document decides it is top-level, so a
    /// framed realm must not see itself as the top.
    #[test]
    fn a_framed_realm_does_not_look_top_level() {
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        let frame = FrameRealm::new(
            &mut parent,
            2,
            0,
            "https://child.example/f",
            "<html><body></body></html>",
        )
        .expect("frame realm");
        assert_eq!(
            frame
                .evaluate(&mut parent, "[parent === window, top === window]")
                .unwrap(),
            serde_json::json!([false, false]),
        );
        // The page itself really is the top and must still say so.
        assert_eq!(
            parent.evaluate("[parent === window, top === window]").unwrap(),
            serde_json::json!([true, true]),
        );
    }

    #[test]
    fn a_framed_realm_uses_the_process_timezone() {
        // SAFETY: nextest gives this test its own process and no runtime or
        // worker thread exists before this point.
        unsafe { std::env::set_var("TZ", "Europe/Istanbul"); }
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://child.example/f",
            "<html><body></body></html>",
        )
        .expect("frame realm");

        assert_eq!(
            frame
                .evaluate(
                    &mut parent,
                    "({zone:Intl.DateTimeFormat().resolvedOptions().timeZone,offset:new Date().getTimezoneOffset()})",
                )
                .unwrap(),
            serde_json::json!({"zone": "Europe/Istanbul", "offset": -180}),
        );
    }

    /// Script can post in a synchronous loop while the host only drains between
    /// event loop turns, and this queue is on the process heap rather than
    /// V8's, where the heap-limit guard would never see it.
    #[test]
    fn a_flood_of_messages_cannot_grow_the_queue_without_bound() {
        std::env::set_var("OBSCURA_FRAME_MESSAGE_QUEUE_ENTRIES", "64");
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://child.example/f",
            "<html><body></body></html>",
        )
        .expect("frame realm");

        frame
            .execute_script(
                &mut parent,
                "for (let i = 0; i < 5000; i++) parent.postMessage(i, '*');",
            )
            .unwrap();

        let queued = parent.take_pending_frame_messages();
        assert_eq!(queued.len(), 64, "the queue was not capped");
        // The messages kept are the earliest, which is the half of a handshake
        // that matters.
        assert_eq!(queued[0].data_json, r#"{"v":0}"#);
        std::env::remove_var("OBSCURA_FRAME_MESSAGE_QUEUE_ENTRIES");
    }

    /// The page realm holds the frame's window and document, so a discarded
    /// frame leaves the page naming objects from a context the host no longer
    /// holds. Reading one must be safe. A regression here is an access
    /// violation that takes the process down, not a failed assertion.
    ///
    /// It must also not read as anything: V8 severs a global proxy when its
    /// context goes, which is the same thing a browser does to a WindowProxy
    /// when it discards a browsing context.
    #[test]
    fn a_discarded_realm_leaves_the_page_safe_to_run() {
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        {
            let frame = FrameRealm::new(
                &mut parent,
                1,
                0,
                "https://parent.example/child",
                "<html><body><h1>Child</h1></body></html>",
            )
            .expect("frame realm");
            frame
                .execute_script(&mut parent, "globalThis.marker = 'child';")
                .unwrap();
            // Reachable from the page while the frame is alive.
            assert_eq!(
                parent
                    .evaluate("globalThis.__obscura_frameObjects[1].window.marker")
                    .unwrap(),
                serde_json::json!("child"),
            );
        }

        // Dropping the realm does not free it, and must not make touching it
        // unsafe: the page still names its window, so V8 keeps the context
        // alive and the read still answers. This is exactly why a discarded
        // frame has to have its entry removed rather than merely dropped, and
        // what `Page::release_detached_frames` is for.
        assert_eq!(
            parent
                .evaluate("globalThis.__obscura_frameObjects[1].window.marker")
                .unwrap(),
            serde_json::json!("child"),
        );
        // The page's own DOM work still resolves against the page.
        assert_eq!(
            parent.evaluate("document.body.innerHTML").unwrap(),
            serde_json::json!(""),
        );
        // Dropping the page's reference is what lets the frame be collected.
        parent
            .execute_script("p", "delete globalThis.__obscura_frameObjects[1];")
            .unwrap();
        assert_eq!(
            parent
                .evaluate("globalThis.__obscura_frameObjects[1] === undefined")
                .unwrap(),
            serde_json::json!(true),
        );
    }

    /// A DOM call names the realm it belongs to, so the page reading the
    /// frame's document gets the frame's document. Resolving from the running
    /// context instead would silently answer with the page's own.
    #[test]
    fn the_page_reads_the_frames_document_through_its_own_object() {
        let mut parent = page(
            "https://parent.example/",
            "<html><head><title>parent</title></head><body></body></html>",
        );
        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://parent.example/child",
            "<html><head><title>BEFORE</title></head><body><p>child</p></body></html>",
        )
        .expect("frame realm");
        frame
            .execute_script(&mut parent, "document.title = 'RAN-IN-CHILD';")
            .unwrap();

        // Read the frame's document from the *page's* realm.
        assert_eq!(
            parent
                .evaluate("globalThis.__obscura_frameObjects[1].document.title")
                .unwrap(),
            serde_json::json!("RAN-IN-CHILD"),
        );
        assert_eq!(
            parent
                .evaluate("globalThis.__obscura_frameObjects[1].document.querySelector('p').textContent")
                .unwrap(),
            serde_json::json!("child"),
        );
        // The page's own title is untouched by any of that.
        assert_eq!(
            parent.evaluate("document.title").unwrap(),
            serde_json::json!("parent"),
        );
    }

    /// A cross-origin frame must stay opaque. Nothing about it is published to
    /// the page, and V8's own access check answers `undefined` for anything the
    /// page reaches for, because the two realms keep different security tokens.
    #[test]
    fn a_cross_origin_frame_is_not_reachable_from_the_page() {
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "https://other.example/f",
            "<html><body></body></html>",
        )
        .expect("frame realm");
        frame
            .execute_script(&mut parent, "globalThis.secret = 'do-not-leak';")
            .unwrap();

        assert_eq!(
            parent
                .evaluate("globalThis.__obscura_frameObjects[1] === undefined")
                .unwrap(),
            serde_json::json!(true),
            "a cross-origin frame was published to the page"
        );
        // The frame still works on its own side.
        assert_eq!(
            frame.evaluate(&mut parent, "globalThis.secret").unwrap(),
            serde_json::json!("do-not-leak"),
        );
    }

    #[test]
    fn about_blank_frames_inherit_the_parent_origin() {
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        let frame = FrameRealm::new(
            &mut parent,
            1,
            0,
            "about:blank",
            "<html><body></body></html>",
        )
        .expect("frame realm");
        assert_eq!(frame.origin(), "https://parent.example");
        assert!(frame.is_same_origin_as("https://parent.example"));
    }
}
