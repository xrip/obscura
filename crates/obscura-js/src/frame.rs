//! Child frame realms.
//!
//! An iframe is a separate browsing context: its own JavaScript realm, its own
//! DOM tree, and its own origin. Before this existed, Obscura fetched a frame's
//! HTML and dropped the body into a detached `_IframeDocument` in the *parent's*
//! realm, so no script inside a frame ever ran.
//!
//! A frame realm is a second `v8::Context` in the page's isolate. Three things
//! make that practical:
//!
//! - The startup snapshot already contains the whole bootstrap, so a restored
//!   context arrives with every DOM class installed. A realm costs a context
//!   restore, not a re-parse.
//! - The realm's op table is filled from the page realm's, so every shim in the
//!   frame can call ops.
//! - Ops read one isolate-wide `ObscuraState`. Only one realm runs at a time, so
//!   a frame is made current by swapping that state around its work. No op
//!   signature changes.
//!
//! Staying in one isolate is what lets same-origin frames share objects with
//! their parent, the way `iframe.contentWindow.document` does in a browser. A
//! second isolate could never do that.

use std::cell::RefCell;
use std::rc::Rc;

use obscura_dom::parse_html;

use crate::ops::ObscuraState;
use crate::runtime::ObscuraJsRuntime;

/// One child browsing context: its own realm, document and origin, living in
/// the page's isolate.
pub struct FrameRealm {
    context: deno_core::v8::Global<deno_core::v8::Context>,
    state: Rc<RefCell<ObscuraState>>,
    frame_id: u32,
    url: String,
    origin: String,
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
        url: &str,
        html: &str,
    ) -> Option<Self> {
        let context = parent.create_realm_context()?;
        if !parent.share_ops_with_realm(&context) {
            return None;
        }
        parent.copy_identity_to_realm(&context);

        let mut state = ObscuraState::new();
        state.dom = Some(parse_html(html));
        state.url = url.to_string();
        parent.share_resources_with(&mut state);

        let realm = FrameRealm {
            context,
            state: Rc::new(RefCell::new(state)),
            frame_id,
            url: url.to_string(),
            origin: origin_of(url),
        };
        // Build the frame's document against the frame's own state.
        realm.run(parent, "globalThis.__obscura_init();").ok()?;
        Some(realm)
    }

    pub fn frame_id(&self) -> u32 {
        self.frame_id
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

    /// Runs `source` in the frame's realm with the frame's document current.
    fn run(&self, parent: &mut ObscuraJsRuntime, source: &str) -> Result<String, String> {
        let previous = parent.swap_op_state(self.state.clone());
        let result = parent.eval_in_realm(&self.context, source);
        parent.swap_op_state(previous);
        result
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
        let listed = self.evaluate(
            parent,
            r#"[...document.querySelectorAll('script')].map(node => ({
                src: node.getAttribute('src') || '',
                type: (node.getAttribute('type') || '').toLowerCase(),
                text: node.textContent || '',
            }))"#,
        );
        let scripts: Vec<DocumentScript> = match listed {
            Ok(value) => serde_json::from_value(value).unwrap_or_default(),
            Err(error) => return vec![format!("could not list frame scripts: {error}")],
        };

        let base = url::Url::parse(&self.url).ok();
        let mut problems = Vec::new();
        for (index, script) in scripts.iter().enumerate() {
            // An empty type, or a JavaScript MIME type, is a classic script.
            // Anything else is data or a module.
            let classic = script.type_attribute.is_empty()
                || matches!(
                    script.type_attribute.as_str(),
                    "text/javascript" | "application/javascript" | "text/ecmascript"
                );
            if !classic {
                if script.type_attribute == "module" {
                    problems.push(format!("frame module script {index} skipped: not supported"));
                }
                continue;
            }

            let (name, source) = if script.src.is_empty() {
                (format!("inline {index}"), script.text.clone())
            } else {
                let resolved = base
                    .as_ref()
                    .and_then(|base| base.join(&script.src).ok())
                    .map(|url| url.to_string())
                    .unwrap_or_else(|| script.src.clone());
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
}

#[derive(serde::Deserialize)]
struct DocumentScript {
    src: String,
    #[serde(rename = "type")]
    type_attribute: String,
    text: String,
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
                    index,
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

    #[test]
    fn opaque_origin_frames_are_never_same_origin() {
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        let frame = FrameRealm::new(&mut parent, 1, "about:blank", "<html><body></body></html>")
            .expect("frame realm");
        assert_eq!(frame.origin(), "null");
        assert!(!frame.is_same_origin_as("null"));
        assert!(!frame.is_same_origin_as("https://parent.example"));
    }
}
