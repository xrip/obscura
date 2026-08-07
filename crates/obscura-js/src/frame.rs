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
//! - Each realm registers its document in `RealmStates`, and an op looks up the
//!   realm that called it. Making a realm current around the host's calls into
//!   it is not enough, because a frame's timers and settled promises re-enter
//!   JavaScript straight from the event loop.
//!
//! Staying in one isolate is what lets same-origin frames share objects with
//! their parent, the way `iframe.contentWindow.document` does in a browser. A
//! second isolate could never do that.

use std::cell::RefCell;
use std::rc::Rc;

use obscura_dom::parse_html;

use crate::ops::{ObscuraState, RealmStates};
use crate::runtime::ObscuraJsRuntime;

/// One child browsing context: its own realm, document and origin, living in
/// the page's isolate.
pub struct FrameRealm {
    context: deno_core::v8::Global<deno_core::v8::Context>,
    /// Held so the frame's entry can be taken out again when the frame dies.
    realms: Rc<RefCell<RealmStates>>,
    frame_id: u32,
    parent_frame_id: u32,
    url: String,
    origin: String,
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
        let context = parent.create_realm_context()?;
        if !parent.share_ops_with_realm(&context) {
            return None;
        }
        parent.copy_identity_to_realm(&context);

        let mut state = ObscuraState::new();
        state.dom = Some(parse_html(html));
        state.url = url.to_string();
        state.frame_id = frame_id;
        parent.share_resources_with(&mut state);

        let realms = parent.realm_states();
        realms
            .borrow_mut()
            .register(context.clone(), Rc::new(RefCell::new(state)));

        let realm = FrameRealm {
            context,
            realms,
            frame_id,
            parent_frame_id,
            url: url.to_string(),
            origin: origin_of(url),
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
                     globalThis.__obscura_init();"
                ),
            )
            .ok()?;
        Some(realm)
    }

    /// Fires the frame document's lifecycle events, in spec order.
    ///
    /// The page's own document gets these; a frame's did not, and a document
    /// that is never told it finished loading will not run any of the work
    /// scripts defer until then. That is most of what a widget does — a frame
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

    /// Sets the frame document's viewport before any of its scripts run.
    pub fn set_viewport(
        &self,
        parent: &mut ObscuraJsRuntime,
        width: f64,
        height: f64,
    ) -> Result<(), String> {
        let width = if width.is_finite() && width > 0.0 { width } else { 300.0 };
        let height = if height.is_finite() && height > 0.0 { height } else { 150.0 };
        self.execute_script(
            parent,
            &format!(
                "globalThis.innerWidth={width};globalThis.innerHeight={height};\
                 if(globalThis.visualViewport){{\
                   globalThis.visualViewport.width={width};\
                   globalThis.visualViewport.height={height};\
                 }}"
            ),
        )
    }

    pub fn parent_frame_id(&self) -> u32 {
        self.parent_frame_id
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

        let base = url::Url::parse(&self.url).ok();
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

    /// Absolute URLs of the frame's `src=` classic scripts, in document order.
    ///
    /// A caller that fetches over the network needs the list before running
    /// anything, because `run_document_scripts` resolves sources synchronously.
    pub fn external_script_urls(&self, parent: &mut ObscuraJsRuntime) -> Vec<String> {
        let base = url::Url::parse(&self.url).ok();
        self.list_scripts(parent)
            .unwrap_or_default()
            .iter()
            .filter(|script| script.is_classic() && !script.src.is_empty())
            .map(|script| {
                base.as_ref()
                    .and_then(|base| base.join(&script.src).ok())
                    .map(|url| url.to_string())
                    .unwrap_or_else(|| script.src.clone())
            })
            .collect()
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

#[allow(dead_code)]
fn trace_vm_property_call(mut source: String) -> String {
    fn is_ident(byte: u8) -> bool {
        byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
    }

    fn closing_bracket(bytes: &[u8], open: usize) -> Option<usize> {
        let mut depth = 0usize;
        for (index, byte) in bytes.iter().enumerate().skip(open) {
            match byte {
                b'[' => depth += 1,
                b']' => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 { return Some(index) }
                }
                _ => {}
            }
        }
        None
    }

    let bytes = source.as_bytes();
    let mut property_reads = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let start = cursor;
        let first = bytes[start];
        if !(first.is_ascii_alphabetic() || first == b'_' || first == b'$')
            || (start > 0 && is_ident(bytes[start - 1]))
        {
            cursor += 1;
            continue;
        }
        let mut ident_end = start + 1;
        while ident_end < bytes.len() && is_ident(bytes[ident_end]) { ident_end += 1 }
        if bytes.get(ident_end) != Some(&b'[') {
            cursor = ident_end;
            continue;
        }
        let Some(lhs_close) = closing_bracket(bytes, ident_end) else { break };
        if bytes.get(lhs_close + 1) != Some(&b'=') {
            cursor = lhs_close + 1;
            continue;
        }
        let rhs = lhs_close + 2;
        let ident = &source[start..ident_end];
        if !source[rhs..].starts_with(ident) || bytes.get(rhs + ident.len()) != Some(&b'[') {
            cursor = rhs;
            continue;
        }
        let owner_open = rhs + ident.len();
        let Some(owner_close) = closing_bracket(bytes, owner_open) else { break };
        let property_open = owner_close + 1;
        if bytes.get(property_open) != Some(&b'[') {
            cursor = property_open;
            continue;
        }
        let Some(property_close) = closing_bracket(bytes, property_open) else { break };
        if bytes.get(property_close + 1).is_some_and(|byte| !matches!(byte, b',' | b';' | b')' | b'}' | b':')) {
            cursor = property_close + 1;
            continue;
        }
        let value = &source[start..=lhs_close];
        let owner = &source[rhs..=owner_close];
        let property = &source[property_open + 1..property_close];
        property_reads.push((
            property_close + 1,
            format!(
                ",((a)=>(a.length<5000&&a.push({{property:typeof {property}==='string'?{property}:typeof {property},ownerType:typeof {owner},ownerTag:Object.prototype.toString.call({owner}),valueType:typeof {value}}})))(globalThis.__vmReads||(globalThis.__vmReads=[]))"
            ),
        ));
        cursor = property_close + 1;
    }
    for (position, insertion) in property_reads.into_iter().rev() {
        source.insert_str(position, &insertion);
    }

    let bytes = source.as_bytes();
    let marker = b"===void 0?";
    let mut search = 0;
    let mut insertions = Vec::new();
    while search + marker.len() <= bytes.len() {
        let Some(offset) = bytes[search..]
            .windows(marker.len())
            .position(|window| window == marker)
        else {
            break;
        };
        let marker_start = search + offset;
        let mut owner_start = marker_start;
        while owner_start > 0 && is_ident(bytes[owner_start - 1]) {
            owner_start -= 1;
        }
        let owner = &source[owner_start..marker_start];

        let mut value_end = owner_start.saturating_sub(1);
        if bytes.get(value_end) != Some(&b'=') || owner.is_empty() {
            search = marker_start + marker.len();
            continue;
        }
        while value_end > 0 && bytes[value_end - 1].is_ascii_whitespace() {
            value_end -= 1;
        }
        let mut value_start = value_end;
        while value_start > 0 && is_ident(bytes[value_start - 1]) {
            value_start -= 1;
        }
        let value = &source[value_start..value_end];

        let property_start = marker_start + marker.len();
        let mut property_end = property_start;
        while property_end < bytes.len() && is_ident(bytes[property_end]) {
            property_end += 1;
        }
        let property = &source[property_start..property_end];
        let expected = format!(":{owner}[{property}]");
        if value.is_empty() || property.is_empty() || !source[property_end..].starts_with(&expected) {
            search = marker_start + marker.len();
            continue;
        }
        let comma = property_end + expected.len();
        if bytes.get(comma) != Some(&b',') {
            search = marker_start + marker.len();
            continue;
        }
        insertions.push((
            comma + 1,
            format!(
                "globalThis.__vmTrace={{property:{property},ownerType:typeof {owner},valueType:typeof {value},ownerIsWindow:{owner}===globalThis,ownerIsDocument:{owner}===globalThis.document,ownerIsNavigator:{owner}===globalThis.navigator}},typeof {value}==='function'||(globalThis.__vmMissing||(globalThis.__vmMissing=[])).length>=200||globalThis.__vmMissing.push(globalThis.__vmTrace),"
            ),
        ));
        search = marker_start + marker.len();
    }
    for (position, insertion) in insertions.into_iter().rev() {
        source.insert_str(position, &insertion);
    }
    if let Some(start) = source.find("this[`g`]=") {
        if let Some(relative_end) = source[start..].find(",this[`h`]=") {
            let position = start + relative_end + 1;
            source.insert_str(
                position,
                "this[`g`]=new Proxy(this[`g`],{get(t,p,r){const v=Reflect.get(t,p,r);if(v===void 0&&Number.isInteger(+p)){const a=globalThis.__vmUndefined||(globalThis.__vmUndefined=[]);if(a.length<200)a.push({kind:'get',slot:String(p),stack:String(new Error().stack||'')})}return v},set(t,p,v,r){if(v===void 0&&Number.isInteger(+p)){const a=globalThis.__vmUndefined||(globalThis.__vmUndefined=[]);if(a.length<200)a.push({kind:'set',slot:String(p),stack:String(new Error().stack||'')})}return Reflect.set(t,p,v,r)}}),",
            );
        }
    }
    for (read, value, property, owner) in [
        ("rP[rR],", "rB", "rR", "rP"),
        ("yZ[yn],", "yd", "yn", "yZ"),
    ] {
        if let Some(offset) = source.find(read) {
            let position = offset + read.len();
            source.insert_str(
                position,
                &format!(
                    "globalThis.__vmTrace={{property:{property},ownerType:typeof {owner},valueType:typeof {value},ownerIsWindow:{owner}===globalThis,ownerIsDocument:{owner}===globalThis.document,ownerIsNavigator:{owner}===globalThis.navigator}},typeof {value}==='function'||(globalThis.__vmMissing||(globalThis.__vmMissing=[])).push(globalThis.__vmTrace),"
                ),
            );
        }
    }
    for (read, value, property, owner) in [
        ("j[rz]=j[rR][j[rM]]", "j[rz]", "j[rM]", "j[rR]"),
        ("j[rD]=j[rR][j[rl]]", "j[rD]", "j[rl]", "j[rR]"),
        ("R[yn]=R[yl][R[Y]]", "R[yn]", "R[Y]", "R[yl]"),
        ("R[yd]=R[yl][R[yS]]", "R[yd]", "R[yS]", "R[yl]"),
    ] {
        if let Some(offset) = source.find(read) {
            let position = offset + read.len();
            source.insert_str(
                position,
                &format!(
                    ",(globalThis.__vmGets||(globalThis.__vmGets=[])).push({{property:typeof {property}==='string'?{property}:typeof {property},ownerType:typeof {owner},valueType:typeof {value},ownerIsWindow:{owner}===globalThis,ownerIsDocument:{owner}===globalThis.document,ownerIsNavigator:{owner}===globalThis.navigator}})"
                ),
            );
        }
    }
    for (read, value, property, owner) in [
        ("P[i]=P[yl][yN]", "P[i]", "yN", "P[yl]"),
        ("j[rD]=j[rR][j[rl]]", "j[rD]", "j[rl]", "j[rR]"),
        ("R[Y]=R[yn][yl]", "R[Y]", "yl", "R[yn]"),
        ("R[yl]=R[Y][R[yd]]", "R[yl]", "R[yd]", "R[Y]"),
        ("R[yl]=R[yd][R[yn]]", "R[yl]", "R[yn]", "R[yd]"),
    ] {
        if let Some(offset) = source.find(read) {
            let position = offset + read.len();
            source.insert_str(
                position,
                &format!(
                    ",((a)=>(a.length<5000&&a.push({{property:{property},ownerType:typeof {owner},ownerTag:Object.prototype.toString.call({owner}),valueType:typeof {value}}})))(globalThis.__vmReads||(globalThis.__vmReads=[]))"
                ),
            );
        }
    }
    source
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

        frame.set_viewport(&mut parent, 300.0, 65.0).unwrap();
        assert_eq!(
            frame
                .evaluate(
                    &mut parent,
                    "[innerWidth,innerHeight,visualViewport.width,visualViewport.height]",
                )
                .unwrap(),
            serde_json::json!([300, 65, 300, 65]),
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
                    index,
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

    #[test]
    fn opaque_origin_frames_are_never_same_origin() {
        let mut parent = page("https://parent.example/", "<html><body></body></html>");
        let frame = FrameRealm::new(
                    &mut parent,
                    1,
                    0,
                    "about:blank", "<html><body></body></html>")
            .expect("frame realm");
        assert_eq!(frame.origin(), "null");
        assert!(!frame.is_same_origin_as("null"));
        assert!(!frame.is_same_origin_as("https://parent.example"));
    }
}
