//! Child frame realms.
//!
//! An iframe is a separate browsing context: its own JavaScript realm, its own
//! DOM tree, and its own origin. Before this existed, Obscura fetched a frame's
//! HTML and dropped the body into a detached `_IframeDocument` in the *parent's*
//! realm, so no script inside a frame ever ran.
//!
//! deno_core 0.350 exposes no multi-realm API (`JsRealm` is crate-private and
//! `create_realm` is gone), so a frame realm is a second [`ObscuraJsRuntime`]
//! with its own V8 isolate. That is stronger isolation than a second
//! `v8::Context` would give, and it carries a large practical benefit: the frame
//! gets its own `ObscuraState`, so every existing op keeps working inside a
//! frame with no changes at all.
//!
//! # Sharing one thread between two isolates
//!
//! deno_core never calls `Isolate::enter`/`exit`; it assumes one isolate per
//! thread. Creating a frame leaves the frame's isolate current, and the parent
//! then aborts inside V8 scope bookkeeping the next time it runs anything. A
//! frame therefore *parks* its isolate and only claims the thread for the
//! duration of its own work, through [`FrameRuntime::with_entered`].
//!
//! Because only one isolate may be current at a time, a frame must never call
//! back into its parent while entered. Cross-frame messaging is queued and
//! delivered after the frame has parked again, which also matches the
//! asynchronous semantics `postMessage` is specified to have.
//!
//! # Identity
//!
//! A frame must present the *same* browser identity as its parent: the same
//! fingerprint profile, user agent, platform, and geolocation. Anti-bot code
//! fingerprints inside the frame and compares it against the top document, so a
//! frame that resolved its own profile would be an immediate mismatch. The
//! caller supplies a `configure` closure for this, and callers are expected to
//! pass the same routine that configures the top-level document rather than a
//! second, drift-prone copy of the list.

use obscura_dom::parse_html;

use crate::runtime::ObscuraJsRuntime;

/// Claims a frame's isolate for the current thread and always gives it back,
/// including while a panic unwinds. An isolate left entered after a panic would
/// abort the process the next time the parent ran any JavaScript, so the
/// give-back lives in `Drop` rather than at the end of a function body.
struct IsolateGuard<'a> {
    runtime: &'a mut ObscuraJsRuntime,
}

impl<'a> IsolateGuard<'a> {
    fn enter(runtime: &'a mut ObscuraJsRuntime) -> Self {
        // SAFETY: paired with the `exit_isolate` in `Drop`, which cannot be
        // skipped, so entries and exits stay balanced and LIFO-ordered.
        unsafe { runtime.enter_isolate() };
        IsolateGuard { runtime }
    }
}

impl Drop for IsolateGuard<'_> {
    fn drop(&mut self) {
        // SAFETY: pairs with the `enter_isolate` in `enter`.
        unsafe { self.runtime.exit_isolate() };
    }
}

/// One child browsing context: a parked isolate plus the identity of the
/// document living in it.
pub struct FrameRuntime {
    runtime: ObscuraJsRuntime,
    frame_id: u32,
    url: String,
    origin: String,
}

impl FrameRuntime {
    /// Builds a frame realm around an already-fetched document.
    ///
    /// `configure` receives the frame's runtime before its document is
    /// installed and must apply the same identity the top-level document uses:
    /// fingerprint profile, user agent, platform, geolocation, and the shared
    /// cookie jar, storage, and HTTP client. Pass the same routine that
    /// configures the page runtime so the two can never drift apart.
    ///
    /// The isolate is parked before returning, so the caller's isolate stays
    /// current.
    pub fn new(
        frame_id: u32,
        url: &str,
        html: &str,
        configure: impl FnOnce(&mut ObscuraJsRuntime),
    ) -> Self {
        // ObscuraJsRuntime::new leaves its own isolate current, so everything
        // below already runs with the frame's isolate on the thread.
        let mut runtime = ObscuraJsRuntime::new();
        configure(&mut runtime);
        runtime.set_dom(parse_html(html));
        runtime.set_url(url);
        runtime.run_page_init();
        // SAFETY: balanced by the `enter_isolate` calls in `with_entered` and
        // `Drop`. Parking hands the thread back to whoever created this frame.
        unsafe { runtime.exit_isolate() };

        FrameRuntime {
            runtime,
            frame_id,
            url: url.to_string(),
            origin: origin_of(url),
        }
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

    /// Runs `work` with this frame's isolate current, then parks it again.
    ///
    /// Do not run another runtime's JavaScript inside `work`: only one isolate
    /// may hold the thread at a time.
    pub fn with_entered<R>(&mut self, work: impl FnOnce(&mut ObscuraJsRuntime) -> R) -> R {
        let guard = IsolateGuard::enter(&mut self.runtime);
        work(&mut *guard.runtime)
    }

    /// Evaluates an expression inside the frame.
    pub fn evaluate(&mut self, source: &str) -> Result<serde_json::Value, String> {
        self.with_entered(|runtime| runtime.evaluate(source))
    }

    /// Runs a script inside the frame, reporting a script error as `Err`.
    pub fn execute_script(&mut self, name: &str, source: &str) -> Result<(), String> {
        self.with_entered(|runtime| runtime.execute_script(name, source))
    }

    /// Runs the frame document's classic scripts, in document order.
    ///
    /// `load_external` resolves a `src=` script to its source text; returning
    /// `None` skips that script, which is what a failed subresource fetch looks
    /// like to the page. One script throwing does not stop the ones after it,
    /// matching how a browser treats separate classic scripts.
    ///
    /// Module scripts are skipped and reported: they need the frame's own module
    /// loader, which is not wired up yet.
    ///
    /// Returns one message per script that failed or was skipped.
    pub fn run_document_scripts(
        &mut self,
        load_external: impl Fn(&str) -> Option<String>,
    ) -> Vec<String> {
        let listed = self.evaluate(
            r#"JSON.stringify([...document.querySelectorAll('script')].map(node => ({
                src: node.getAttribute('src') || '',
                type: (node.getAttribute('type') || '').toLowerCase(),
                text: node.textContent || '',
            })))"#,
        );
        let scripts: Vec<DocumentScript> = match listed {
            Ok(value) => value
                .as_str()
                .and_then(|raw| serde_json::from_str(raw).ok())
                .unwrap_or_default(),
            Err(error) => return vec![format!("could not list frame scripts: {error}")],
        };

        let base = url::Url::parse(&self.url).ok();
        let mut problems = Vec::new();
        for (index, script) in scripts.iter().enumerate() {
            // Classic scripts only. An empty type, or a JavaScript MIME type, is
            // classic; anything else is data or a module.
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
                (format!("<frame:inline:{index}>"), script.text.clone())
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
            if let Err(error) = self.execute_script(&name, &source) {
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

impl Drop for FrameRuntime {
    fn drop(&mut self) {
        // deno_core built this isolate entered and tears it down expecting to
        // still own the thread, so claim it one last time. The runtime field is
        // dropped immediately after this body, while the isolate is current.
        // SAFETY: a frame is parked whenever it is not inside `with_entered`,
        // so this restores the state deno_core created the isolate in.
        unsafe { self.runtime.enter_isolate() };
    }
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

    fn page_runtime(url: &str, html: &str) -> ObscuraJsRuntime {
        let mut runtime = ObscuraJsRuntime::new();
        runtime.set_dom(parse_html(html));
        runtime.set_url(url);
        runtime.run_page_init();
        runtime
    }

    #[test]
    fn frame_has_its_own_realm_dom_and_origin() {
        let mut parent = page_runtime(
            "https://parent.example/page",
            "<html><body><h1>Parent</h1></body></html>",
        );
        parent
            .execute_script("p", "globalThis.marker = 'parent';")
            .unwrap();

        let mut frame = FrameRuntime::new(
            1,
            "https://child.example/frame",
            "<html><body><h1>Child</h1></body></html>",
            |_| {},
        );

        // The parent survives the frame's construction with its state intact.
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

        // The frame is a genuinely separate realm: own globals, own DOM, own URL.
        frame
            .execute_script("c", "globalThis.marker = 'child';")
            .unwrap();
        assert_eq!(
            frame
                .evaluate("document.querySelector('h1').textContent")
                .unwrap(),
            serde_json::json!("Child")
        );
        assert_eq!(
            frame.evaluate("globalThis.marker").unwrap(),
            serde_json::json!("child")
        );
        assert_eq!(
            frame.evaluate("location.href").unwrap(),
            serde_json::json!("https://child.example/frame")
        );

        // Interleaving still works after the frame has run.
        assert_eq!(
            parent.evaluate("globalThis.marker").unwrap(),
            serde_json::json!("parent")
        );
        assert_eq!(frame.origin(), "https://child.example");
        assert!(!frame.is_same_origin_as("https://parent.example"));
        assert!(frame.is_same_origin_as("https://child.example"));
    }

    /// The capability the frame realm exists for: a script that arrived with the
    /// frame's own document actually executes, against the frame's own DOM.
    #[test]
    fn frame_script_runs_against_the_frame_document() {
        let mut parent = page_runtime("https://parent.example/", "<html><body></body></html>");

        let mut frame = FrameRuntime::new(
            7,
            "https://child.example/f",
            "<html><body><div id=\"out\"></div></body></html>",
            |_| {},
        );
        frame
            .execute_script(
                "inline",
                "document.getElementById('out').textContent = 'ran:' + location.origin;",
            )
            .unwrap();

        assert_eq!(
            frame
                .evaluate("document.getElementById('out').textContent")
                .unwrap(),
            serde_json::json!("ran:https://child.example")
        );
        // The frame's document writes never touch the parent's DOM.
        assert_eq!(
            parent.evaluate("document.body.innerHTML").unwrap(),
            serde_json::json!("")
        );
    }

    /// A frame must not look like a different browser than its parent. Anti-bot
    /// code fingerprints inside the frame and compares it with the top document.
    #[test]
    fn frame_inherits_the_parent_browser_identity() {
        let user_agent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) TestAgent/150.0.0.0";
        let configure = |runtime: &mut ObscuraJsRuntime| {
            runtime.set_user_agent(user_agent);
            runtime.set_platform("Win32", "Windows", "19.0.0");
        };

        let mut parent = ObscuraJsRuntime::new();
        configure(&mut parent);
        parent.set_dom(parse_html("<html><body></body></html>"));
        parent.set_url("https://parent.example/");
        parent.run_page_init();

        let mut frame = FrameRuntime::new(
            1,
            "https://child.example/f",
            "<html><body></body></html>",
            configure,
        );

        for surface in [
            "navigator.userAgent",
            "navigator.platform",
            "navigator.userAgentData.platform",
        ] {
            assert_eq!(
                frame.evaluate(surface),
                parent.evaluate(surface),
                "frame and parent disagree on {surface}"
            );
        }
        assert_eq!(
            frame.evaluate("navigator.userAgent").unwrap(),
            serde_json::json!(user_agent)
        );
    }

    #[test]
    fn many_frames_can_be_alive_at_once() {
        let mut parent = page_runtime("https://parent.example/", "<html><body></body></html>");

        let mut frames: Vec<FrameRuntime> = (0..4)
            .map(|index| {
                FrameRuntime::new(
                    index,
                    &format!("https://f{index}.example/"),
                    "<html><body></body></html>",
                    |_| {},
                )
            })
            .collect();

        for (index, frame) in frames.iter_mut().enumerate() {
            frame
                .execute_script("set", &format!("globalThis.n = {index};"))
                .unwrap();
        }
        // Out-of-order access must be safe: parking makes entry order irrelevant.
        for (index, frame) in frames.iter_mut().enumerate().rev() {
            // V8 hands numbers back as doubles, so compare numerically rather
            // than against an integer-typed JSON literal.
            assert_eq!(
                frame.evaluate("globalThis.n").unwrap().as_f64(),
                Some(index as f64)
            );
        }
        assert_eq!(parent.evaluate("1 + 1").unwrap().as_f64(), Some(2.0));
    }

    /// The whole point of the frame realm: scripts that arrived with the frame's
    /// document run, in order, against the frame's own DOM.
    #[test]
    fn frame_runs_its_document_scripts_in_order() {
        let mut frame = FrameRuntime::new(
            1,
            "https://child.example/dir/page",
            r#"<html><body><div id="out"></div>
               <script>window.log = ['inline1'];</script>
               <script src="first.js"></script>
               <script src="/second.js"></script>
               <script>window.log.push('inline2');
                       document.getElementById('out').textContent = window.log.join(',');</script>
               </body></html>"#,
            |_| {},
        );

        let requested = std::cell::RefCell::new(Vec::new());
        let problems = frame.run_document_scripts(|url| {
            requested.borrow_mut().push(url.to_string());
            match url {
                "https://child.example/dir/first.js" => Some("window.log.push('ext1');".into()),
                "https://child.example/second.js" => Some("window.log.push('ext2');".into()),
                _ => None,
            }
        });

        assert!(problems.is_empty(), "unexpected problems: {problems:?}");
        // Relative and root-relative src both resolve against the frame's URL,
        // not the parent's.
        assert_eq!(
            requested.into_inner(),
            vec![
                "https://child.example/dir/first.js".to_string(),
                "https://child.example/second.js".to_string(),
            ]
        );
        assert_eq!(
            frame
                .evaluate("document.getElementById('out').textContent")
                .unwrap(),
            serde_json::json!("inline1,ext1,ext2,inline2")
        );
    }

    #[test]
    fn one_bad_frame_script_does_not_stop_the_rest() {
        let mut frame = FrameRuntime::new(
            1,
            "https://child.example/",
            r#"<html><body>
               <script>window.log = ['a'];</script>
               <script>throw new Error('boom');</script>
               <script src="missing.js"></script>
               <script type="module">window.log.push('module');</script>
               <script>window.log.push('b');</script>
               </body></html>"#,
            |_| {},
        );

        let problems = frame.run_document_scripts(|_| None);

        assert_eq!(
            frame.evaluate("window.log.join(',')").unwrap(),
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
    fn opaque_origin_frames_are_never_same_origin() {
        let frame = FrameRuntime::new(1, "about:blank", "<html><body></body></html>", |_| {});
        assert_eq!(frame.origin(), "null");
        assert!(!frame.is_same_origin_as("null"));
        assert!(!frame.is_same_origin_as("https://parent.example"));
    }
}
