//! Real browser driver over the Chrome DevTools Protocol (T029 runtime).
//!
//! This is the implementation of `waypoint_applications::driver::BrowserDriver`
//! that the submission engine was written against. It talks to a real
//! Chromium-family browser in a throwaway profile and acts on the page the way
//! a person would:
//!
//! - The form is described from the **document itself** (labels, `required`,
//!   types, options), never from a remembered template — so a changed form is
//!   detected rather than assumed.
//! - Values are entered with real input events (`Input.insertText`) instead of
//!   assigning `.value`, so the page's own listeners fire exactly as they do
//!   for a human, and a site cannot be fooled into accepting a value its own
//!   validation would reject.
//! - Files are attached through the file-input API, and clicks are synthesised
//!   from the element's box model.
//! - There is **no script execution surface**: the driver never evaluates
//!   arbitrary JavaScript on the page. The whole protocol surface it uses is
//!   DOM / Input / Page.
//! - Captchas and login walls are reported as walls. Nothing tries to climb
//!   them.

pub mod cdp;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use waypoint_applications::driver::{
    extract_application_ref, is_sensitive_label, BrowserDriver, DriverError, LiveForm, SiteAck,
};
use waypoint_applications::form_mapping::{BrowserCommand, FormField, FormFieldType, PacketValue};
use waypoint_applications::receipt::ReceiptKind;

use cdp::{BrowserProcess, Cdp, CdpError};

/// Turn a protocol failure into a driver failure, keeping the text.
fn derr(e: CdpError) -> DriverError {
    DriverError(e.0)
}

pub struct CdpDriver {
    browser: BrowserProcess,
    cdp: Cdp,
    /// Directories searched when a packet asks to upload a named file.
    upload_dirs: Vec<PathBuf>,
    root_node: Option<u64>,
    /// Human-readable trace of what the driver did, for the UI and for bug
    /// reports. Never contains field values that were typed.
    pub trace: Vec<String>,
    load_wait: Duration,
}

impl CdpDriver {
    pub fn launch(upload_dirs: Vec<PathBuf>) -> Result<Self, DriverError> {
        let browser = BrowserProcess::launch(Duration::from_secs(20)).map_err(derr)?;
        let ws = browser.connect().map_err(derr)?;
        let cdp = Cdp::new(ws).map_err(derr)?;
        let mut driver = Self {
            browser,
            cdp,
            upload_dirs,
            root_node: None,
            trace: vec![],
            load_wait: Duration::from_secs(10),
        };
        driver.cdp.call("Page.enable", json!({})).map_err(derr)?;
        driver.cdp.call("DOM.enable", json!({})).map_err(derr)?;
        Ok(driver)
    }

    pub fn browser_version(&mut self) -> Result<String, DriverError> {
        let v = self
            .cdp
            .call_browser("Browser.getVersion", json!({}))
            .map_err(derr)?;
        Ok(v.get("product")
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_string())
    }

    /* ------------------------------ page reading --------------------------- */

    fn document(&mut self) -> Result<Value, DriverError> {
        let doc = self
            .cdp
            .call("DOM.getDocument", json!({ "depth": -1, "pierce": false }))
            .map_err(derr)?;
        let root = doc.get("root").cloned().unwrap_or(Value::Null);
        self.root_node = root.get("nodeId").and_then(|v| v.as_u64());
        Ok(root)
    }

    fn node_id(&mut self, selector: &str) -> Result<Option<u64>, DriverError> {
        let root = self.document()?;
        let root_id = root
            .get("nodeId")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| DriverError("page has no root node".into()))?;
        let found = self
            .cdp
            .call(
                "DOM.querySelector",
                json!({ "nodeId": root_id, "selector": selector }),
            )
            .map_err(derr)?;
        Ok(found
            .get("nodeId")
            .and_then(|v| v.as_u64())
            .filter(|id| *id != 0))
    }

    /// Visible-ish text of the page: text nodes outside script/style/template.
    fn page_text(&mut self) -> Result<String, DriverError> {
        let root = self.document()?;
        let mut text = String::new();
        collect_text(&root, false, &mut text);
        Ok(text.split_whitespace().collect::<Vec<_>>().join(" "))
    }

    /// Read the form as it exists right now.
    fn read_form(
        &mut self,
        url: &str,
    ) -> Result<(waypoint_applications::form_mapping::FormSchema, Vec<String>), DriverError> {
        let root = self.document()?;
        let mut controls = vec![];
        let mut labels: HashMap<String, String> = HashMap::new();
        let mut wall_flags: Vec<String> = vec![];
        let mut text = String::new();
        walk(&root, false, &mut |node, in_control| {
            let tag = node
                .get("nodeName")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            // Captchas hide in ordinary markup (a div, an iframe), not only in
            // form controls, so every node is checked.
            let attrs = attributes(node);
            for key in ["class", "id", "name", "src", "title", "data-sitekey"] {
                if let Some(v) = attrs.get(key) {
                    let lv = v.to_lowercase();
                    if lv.contains("captcha") || lv.contains("recaptcha") || lv.contains("hcaptcha")
                    {
                        wall_flags.push("captcha present".into());
                    }
                }
            }
            if tag == "label" {
                let for_id = attrs.get("for").cloned();
                if let Some(for_id) = for_id {
                    let mut label_text = String::new();
                    collect_text(node, false, &mut label_text);
                    labels.entry(for_id).or_insert_with(|| {
                        label_text.split_whitespace().collect::<Vec<_>>().join(" ")
                    });
                }
            }
            if matches!(tag.as_str(), "script" | "style" | "noscript" | "template") {
                return false; // skip this subtree for text collection
            }
            if matches!(tag.as_str(), "input" | "textarea" | "select") && !in_control {
                if node.get("nodeId").and_then(|v| v.as_u64()).is_some() {
                    controls.push(Control {
                        tag: tag.clone(),
                        attrs: attrs.clone(),
                        options: options_of(node),
                    });
                }
                // Continue walking: a control may contain option children.
                return true;
            }
            if tag == "#text" {
                if let Some(value) = node.get("nodeValue").and_then(|v| v.as_str()) {
                    text.push_str(value);
                    text.push(' ');
                }
            }
            true
        });

        let mut fields = vec![];
        let mut walls = wall_flags;
        let page_text_joined = text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        if page_text_joined.contains("verify you are human")
            || page_text_joined.contains("i'm not a robot")
        {
            walls.push("captcha present".to_string());
        }

        for control in controls {
            let input_type = control
                .attrs
                .get("type")
                .cloned()
                .unwrap_or_else(|| {
                    if control.tag == "select" {
                        "select".into()
                    } else {
                        "text".into()
                    }
                })
                .to_ascii_lowercase();
            if matches!(
                input_type.as_str(),
                "hidden" | "submit" | "button" | "reset" | "image"
            ) {
                continue;
            }
            let id = control.attrs.get("id").cloned().unwrap_or_default();
            let label = control
                .attrs
                .get("aria-label")
                .cloned()
                .or_else(|| labels.get(&id).cloned())
                .or_else(|| control.attrs.get("placeholder").cloned())
                .or_else(|| control.attrs.get("name").cloned())
                .or_else(|| {
                    if id.is_empty() {
                        None
                    } else {
                        Some(id.clone())
                    }
                })
                .unwrap_or_else(|| "unlabelled field".into());

            if input_type == "password" {
                walls.push("login required (page asks for a password)".into());
            }

            let required = control.attrs.contains_key("required")
                || control
                    .attrs
                    .get("aria-required")
                    .is_some_and(|v| v == "true");
            let sensitive = is_sensitive_label(&label) || input_type == "password";
            let field_type = match (control.tag.as_str(), input_type.as_str()) {
                (_, "file") => FormFieldType::File,
                (_, "checkbox") => FormFieldType::Checkbox,
                (_, "radio") => FormFieldType::Radio { options: vec![] },
                (_, "select") => FormFieldType::Select {
                    options: control.options.clone(),
                },
                (_, "email") => FormFieldType::Email,
                (_, "date") => FormFieldType::Date,
                ("textarea", _) => FormFieldType::TextArea,
                _ => FormFieldType::Text,
            };
            fields.push(FormField {
                selector: selector_for(&control),
                label,
                field_type,
                required,
                sensitive,
            });
        }

        walls.sort();
        walls.dedup();
        let schema = waypoint_applications::form_mapping::FormSchema {
            origin: origin_of(url),
            fields,
            multi_step: false,
            steps: 1,
        };
        Ok((schema, walls))
    }

    /* ------------------------------- actions -------------------------------- */

    fn fill(&mut self, selector: &str, value: &PacketValue) -> Result<(), DriverError> {
        let Some(node) = self.node_id(selector)? else {
            return Err(DriverError(format!("field {selector} not found")));
        };
        match value {
            PacketValue::FileRef { name } => {
                let path = self.resolve_upload(name)?;
                self.cdp
                    .call(
                        "DOM.setFileInputFiles",
                        json!({ "files": [path.to_string_lossy()], "nodeId": node }),
                    )
                    .map_err(derr)?;
                self.trace.push(format!("attached {name} to {selector}"));
                Ok(())
            }
            PacketValue::Text(t) => {
                // Real focus + real typing: the page sees the same events a
                // human would produce.
                self.cdp
                    .call("DOM.focus", json!({ "nodeId": node }))
                    .map_err(derr)?;
                self.cdp
                    .call("Input.insertText", json!({ "text": t }))
                    .map_err(derr)?;
                self.trace
                    .push(format!("typed {} chars into {selector}", t.chars().count()));
                Ok(())
            }
            PacketValue::Bool(b) => {
                if *b {
                    self.click_node(node, selector)?;
                }
                Ok(())
            }
            PacketValue::Date(d) => {
                self.cdp
                    .call("DOM.focus", json!({ "nodeId": node }))
                    .map_err(derr)?;
                self.cdp
                    .call("Input.insertText", json!({ "text": d }))
                    .map_err(derr)?;
                Ok(())
            }
            PacketValue::None => Ok(()),
        }
    }

    fn resolve_upload(&self, name: &str) -> Result<PathBuf, DriverError> {
        for dir in &self.upload_dirs {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        Err(DriverError(format!(
            "the file to upload ({name}) was not found in any configured folder"
        )))
    }

    fn click_node(&mut self, node: u64, selector: &str) -> Result<(), DriverError> {
        let model = self
            .cdp
            .call("DOM.getBoxModel", json!({ "nodeId": node }))
            .map_err(derr)?;
        let quad = model
            .pointer("/model/content")
            .and_then(|v| v.as_array())
            .ok_or_else(|| DriverError(format!("{selector} has no layout box")))?;
        let nums: Vec<f64> = quad.iter().filter_map(|v| v.as_f64()).collect();
        if nums.len() < 8 {
            return Err(DriverError(format!("{selector} has a degenerate box")));
        }
        let x = (nums[0] + nums[2] + nums[4] + nums[6]) / 4.0;
        let y = (nums[1] + nums[3] + nums[5] + nums[7]) / 4.0;
        self.cdp
            .call(
                "Input.dispatchMouseEvent",
                json!({ "type": "mouseMoved", "x": x, "y": y }),
            )
            .map_err(derr)?;
        self.cdp
            .call(
                "Input.dispatchMouseEvent",
                json!({ "type": "mousePressed", "x": x, "y": y, "button": "left", "clickCount": 1 }),
            )
            .map_err(derr)?;
        self.cdp
            .call(
                "Input.dispatchMouseEvent",
                json!({ "type": "mouseReleased", "x": x, "y": y, "button": "left", "clickCount": 1 }),
            )
            .map_err(derr)?;
        self.trace.push(format!("clicked {selector}"));
        Ok(())
    }

    fn submit_control(&mut self) -> Result<Option<u64>, DriverError> {
        for selector in [
            "button[type=submit]",
            "input[type=submit]",
            "#submit",
            "button.submit",
            "button",
        ] {
            if let Some(id) = self.node_id(selector)? {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    fn wait_for_text_change(
        &mut self,
        before: &str,
        timeout: Duration,
    ) -> Result<String, DriverError> {
        let deadline = Instant::now() + timeout;
        loop {
            self.cdp.pump(Duration::from_millis(350));
            let now = self.page_text()?;
            if now != before && !now.is_empty() {
                return Ok(now);
            }
            if Instant::now() >= deadline {
                return Ok(now);
            }
        }
    }
}

impl BrowserDriver for CdpDriver {
    fn open(&mut self, url: &str) -> Result<LiveForm, DriverError> {
        self.cdp
            .call("Page.navigate", json!({ "url": url }))
            .map_err(derr)?;
        // Wait for the load event, then for the document to be readable.
        let deadline = Instant::now() + self.load_wait;
        loop {
            self.cdp.pump(Duration::from_millis(200));
            if self.cdp.saw_event("Page.loadEventFired") {
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
        }
        let (schema, walls) = self.read_form(url)?;
        self.trace
            .push(format!("opened {url} ({} fields)", schema.fields.len()));
        Ok(LiveForm {
            origin: schema.origin.clone(),
            schema,
            walls,
        })
    }

    fn execute(&mut self, command: &BrowserCommand) -> Result<(), DriverError> {
        match command {
            BrowserCommand::Navigate { url } => {
                self.open(url)?;
                Ok(())
            }
            BrowserCommand::Fill { selector, value } => self.fill(selector, value),
            BrowserCommand::UploadFile {
                selector,
                file_name,
            } => self.fill(
                selector,
                &PacketValue::FileRef {
                    name: file_name.clone(),
                },
            ),
            BrowserCommand::Click { selector } => {
                let Some(node) = self.node_id(selector)? else {
                    return Err(DriverError(format!("{selector} not found")));
                };
                self.click_node(node, selector)
            }
            BrowserCommand::ReadText { selector } => {
                let Some(node) = self.node_id(selector)? else {
                    return Err(DriverError(format!("{selector} not found")));
                };
                let html = self
                    .cdp
                    .call("DOM.getOuterHTML", json!({ "nodeId": node }))
                    .map_err(derr)?;
                let _ = html;
                Ok(())
            }
            BrowserCommand::Close => {
                self.browser.shutdown();
                Ok(())
            }
        }
    }

    fn commit(&mut self) -> Result<SiteAck, DriverError> {
        let before = self.page_text()?;
        let Some(node) = self.submit_control()? else {
            return Err(DriverError(
                "no submit control was found on the page".into(),
            ));
        };
        self.click_node(node, "submit")?;
        let after = self.wait_for_text_change(&before, Duration::from_secs(8))?;
        self.trace.push("submitted".into());

        if let Some(reference) = extract_application_ref(&after) {
            return Ok(SiteAck {
                receipt_kind: ReceiptKind::ConfirmationPageWithId,
                payload: reference,
            });
        }
        // No stable identifier: report the page's own words and let the policy
        // layer decide they are not enough.
        let lower = after.to_lowercase();
        let looks_like_success = [
            "thank you for applying",
            "thanks for applying",
            "application received",
            "we have received",
            "successfully submitted",
            "your application has been",
        ]
        .iter()
        .any(|phrase| lower.contains(phrase));
        if looks_like_success {
            return Ok(SiteAck {
                receipt_kind: ReceiptKind::GenericSuccessText,
                payload: after.chars().take(500).collect(),
            });
        }
        Err(DriverError(
            "no confirmation of any kind appeared after submitting".into(),
        ))
    }
}

impl Drop for CdpDriver {
    fn drop(&mut self) {
        self.browser.shutdown();
    }
}

/* ------------------------------- DOM helpers ------------------------------- */

#[derive(Debug, Clone)]
struct Control {
    tag: String,
    attrs: HashMap<String, String>,
    options: Vec<String>,
}

fn attributes(node: &Value) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let Some(attrs) = node.get("attributes").and_then(|v| v.as_array()) {
        let mut it = attrs.iter();
        while let (Some(k), Some(v)) = (it.next(), it.next()) {
            if let (Some(k), Some(v)) = (k.as_str(), v.as_str()) {
                out.insert(k.to_ascii_lowercase(), v.to_string());
            }
        }
    }
    out
}

fn options_of(node: &Value) -> Vec<String> {
    let mut options = vec![];
    if let Some(children) = node.get("children").and_then(|v| v.as_array()) {
        for child in children {
            let tag = child
                .get("nodeName")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if tag == "option" {
                let mut text = String::new();
                collect_text(child, false, &mut text);
                let cleaned = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if !cleaned.is_empty() {
                    options.push(cleaned);
                }
            }
        }
    }
    options
}

/// Build a stable selector from the control's own markup. Preferred order:
/// id, then name, then tag[type=...] — the same knobs a site actually exposes.
fn selector_for(control: &Control) -> String {
    if let Some(id) = control.attrs.get("id").filter(|v| !v.is_empty()) {
        return format!("#{id}");
    }
    if let Some(name) = control.attrs.get("name").filter(|v| !v.is_empty()) {
        return format!("{}[name=\"{name}\"]", control.tag);
    }
    if let Some(t) = control.attrs.get("type") {
        return format!("{}[type={t}]", control.tag);
    }
    control.tag.clone()
}

fn collect_text(node: &Value, skip: bool, out: &mut String) {
    let tag = node
        .get("nodeName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let skip_here = skip || matches!(tag.as_str(), "script" | "style" | "noscript" | "template");
    if !skip_here && tag == "#text" {
        if let Some(value) = node.get("nodeValue").and_then(|v| v.as_str()) {
            out.push_str(value);
            out.push(' ');
        }
    }
    if let Some(children) = node.get("children").and_then(|v| v.as_array()) {
        for child in children {
            collect_text(child, skip_here, out);
        }
    }
}

/// Depth-first walk; the callback returns false to skip a subtree.
fn walk<F: FnMut(&Value, bool) -> bool>(node: &Value, in_control: bool, cb: &mut F) {
    let proceed = cb(node, in_control);
    let tag = node
        .get("nodeName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let child_in_control = in_control || matches!(tag.as_str(), "input" | "textarea" | "select");
    if proceed {
        if let Some(children) = node.get("children").and_then(|v| v.as_array()) {
            for child in children {
                walk(child, child_in_control, cb);
            }
        }
    }
}

fn origin_of(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, rest)) => {
            let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
            format!("{scheme}://{host}")
        }
        None => url.to_string(),
    }
}
