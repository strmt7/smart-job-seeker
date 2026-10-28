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
    /// Extract a bounded text sample from the current DOM for local review.
    /// Scripts, styles, templates and text inside form controls are excluded.
    pub fn page_text_bounded(&mut self, limit: usize) -> Result<(String, bool), DriverError> {
        if limit == 0 || limit > 32_000 {
            return Err(DriverError("Invalid page-text limit".into()));
        }
        let root = self.document()?;
        let mut text = String::new();
        let mut truncated = false;
        walk(&root, false, &mut |node, in_control| {
            let tag = node
                .get("nodeName")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            if matches!(tag.as_str(), "script" | "style" | "noscript" | "template") {
                return false;
            }
            if tag == "#text" && !in_control {
                if let Some(value) = node.get("nodeValue").and_then(Value::as_str) {
                    for c in value.chars() {
                        if text.len() + c.len_utf8() + 1 > limit {
                            truncated = true;
                            break;
                        }
                        text.push(c);
                    }
                    if text.len() < limit {
                        text.push(' ');
                    }
                }
            }
            true
        });
        Ok((
            text.split_whitespace().collect::<Vec<_>>().join(" "),
            truncated,
        ))
    }
    /// Read bounded JSON-LD script bodies from the current DOM without running scripts.
    pub fn structured_data_scripts(&mut self) -> Result<Vec<String>, DriverError> {
        let root = self.document()?;
        let mut scripts = Vec::new();
        let mut total = 0usize;
        walk(&root, false, &mut |node, _| {
            let tag = node.get("nodeName").and_then(Value::as_str).unwrap_or("");
            if tag.eq_ignore_ascii_case("script") {
                let attrs = attributes(node);
                if attrs
                    .get("type")
                    .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/ld+json"))
                {
                    let mut text = String::new();
                    if let Some(children) = node.get("children").and_then(Value::as_array) {
                        for child in children {
                            if child.get("nodeName").and_then(Value::as_str) == Some("#text") {
                                if let Some(value) = child.get("nodeValue").and_then(Value::as_str)
                                {
                                    text.push_str(value);
                                }
                            }
                        }
                    }
                    total = total.saturating_add(text.len());
                    if scripts.len() < 8 && total <= 1024 * 1024 {
                        scripts.push(text);
                    }
                }
                return false;
            }
            true
        });
        if total > 1024 * 1024 {
            return Err(DriverError("Structured data exceeds the size limit".into()));
        }
        Ok(scripts)
    }
    /// Return the current page URL after any redirects.
    pub fn current_url(&mut self) -> Result<String, DriverError> {
        let history = self
            .cdp
            .call("Page.getNavigationHistory", json!({}))
            .map_err(derr)?;
        let index = history
            .get("currentIndex")
            .and_then(Value::as_u64)
            .ok_or_else(|| DriverError("browser did not report the current page".into()))?
            as usize;
        history
            .get("entries")
            .and_then(Value::as_array)
            .and_then(|items| items.get(index))
            .and_then(|entry| entry.get("url"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| DriverError("browser did not report the current URL".into()))
    }

    /// Read application-looking anchor destinations without clicking or executing script.
    pub fn application_links(&mut self) -> Result<Vec<String>, DriverError> {
        Ok(application_links_from_document(&self.document()?))
    }
    pub fn launch(upload_dirs: Vec<PathBuf>) -> Result<Self, DriverError> {
        Self::launch_with_mode(upload_dirs, false)
    }

    pub fn launch_with_mode(upload_dirs: Vec<PathBuf>, visible: bool) -> Result<Self, DriverError> {
        let browser =
            BrowserProcess::launch_with_mode(Duration::from_secs(20), visible).map_err(derr)?;
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

    /// Capture the current viewport for the candidate's review.
    pub fn screenshot_base64(&mut self) -> Result<String, DriverError> {
        let result = self
            .cdp
            .call("Page.captureScreenshot", json!({"format":"png"}))
            .map_err(derr)?;
        let data = result
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("browser returned no screenshot".into()))?;
        if data.len() > 8 * 1024 * 1024 {
            return Err(DriverError("browser screenshot is too large".into()));
        }
        Ok(data.to_owned())
    }

    /// Attach a caller-selected local file to a known form control.
    pub fn attach_local_file(
        &mut self,
        selector: &str,
        path: &std::path::Path,
    ) -> Result<(), DriverError> {
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("File field not found".into()))?;
        self.cdp
            .call(
                "DOM.setFileInputFiles",
                json!({ "files": [path.to_string_lossy()], "nodeId": node }),
            )
            .map_err(derr)?;
        Ok(())
    }

    /// Return CSS page dimensions for bounded evidence capture.
    pub fn page_dimensions(&mut self) -> Result<(f64, f64), DriverError> {
        let result = self
            .cdp
            .call("Page.getLayoutMetrics", json!({}))
            .map_err(derr)?;
        let size = result
            .get("cssContentSize")
            .ok_or_else(|| DriverError("Page dimensions unavailable".into()))?;
        let width = size
            .get("width")
            .and_then(Value::as_f64)
            .ok_or_else(|| DriverError("Page width unavailable".into()))?;
        let height = size
            .get("height")
            .and_then(Value::as_f64)
            .ok_or_else(|| DriverError("Page height unavailable".into()))?;
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return Err(DriverError("Invalid page dimensions".into()));
        }
        Ok((width.ceil(), height.ceil()))
    }

    /// Capture a bounded page segment without navigating or clicking.
    pub fn screenshot_segment(
        &mut self,
        width: f64,
        y: f64,
        height: f64,
    ) -> Result<String, DriverError> {
        if !width.is_finite()
            || !y.is_finite()
            || !height.is_finite()
            || width <= 0.0
            || width > 2048.0
            || y < 0.0
            || height <= 0.0
            || height > 2000.0
        {
            return Err(DriverError(
                "Screenshot segment exceeds capture limits".into(),
            ));
        }
        let result = self.cdp.call("Page.captureScreenshot", json!({"format":"png","captureBeyondViewport":true,"clip":{"x":0,"y":y,"width":width,"height":height,"scale":1}})).map_err(derr)?;
        let data = result
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Browser returned no screenshot".into()))?;
        if data.len() > 8 * 1024 * 1024 {
            return Err(DriverError("Screenshot is too large".into()));
        }
        Ok(data.to_owned())
    }

    /// Read one DOM attribute for review or fixture verification.
    pub fn read_attribute(
        &mut self,
        selector: &str,
        name: &str,
    ) -> Result<Option<String>, DriverError> {
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError(format!("{selector} not found")))?;
        let result = self
            .cdp
            .call("DOM.getAttributes", json!({"nodeId":node}))
            .map_err(derr)?;
        let values = result
            .get("attributes")
            .and_then(Value::as_array)
            .ok_or_else(|| DriverError("missing DOM attributes".into()))?;
        Ok(values.as_chunks::<2>().0.iter().find_map(|pair| {
            (pair[0].as_str() == Some(name))
                .then(|| pair[1].as_str().map(str::to_owned))
                .flatten()
        }))
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
        let mut wrapping_labels: HashMap<u64, String> = HashMap::new();
        let mut radio_containers: HashMap<u64, (u64, String, Option<String>)> = HashMap::new();
        let mut wall_flags: Vec<String> = vec![];
        let mut text = String::new();
        let mut possible_next_step = false;
        walk(&root, false, &mut |node, in_control| {
            let tag = node
                .get("nodeName")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            // Captchas hide in ordinary markup (a div, an iframe), not only in
            // form controls, so every node is checked.
            let attrs = attributes(node);
            if (tag == "button"
                || (tag == "input"
                    && attrs.get("type").is_some_and(|kind| {
                        matches!(kind.to_ascii_lowercase().as_str(), "button" | "submit")
                    })))
                && !attrs.contains_key("disabled")
                && !attrs.contains_key("hidden")
            {
                let mut caption = String::new();
                collect_text(node, false, &mut caption);
                let caption = attrs
                    .get("aria-label")
                    .or_else(|| attrs.get("value"))
                    .map(String::as_str)
                    .unwrap_or(&caption);
                let caption = caption
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase();
                if matches!(
                    caption.as_str(),
                    "next" | "next step" | "continue" | "weiter" | "suivant" | "siguiente"
                ) {
                    possible_next_step = true;
                }
            }
            for key in ["class", "id", "name", "src", "title", "data-sitekey"] {
                if let Some(v) = attrs.get(key) {
                    let lv = v.to_lowercase();
                    if lv.contains("captcha")
                        || lv.contains("recaptcha")
                        || lv.contains("hcaptcha")
                        || lv.contains("cf-turnstile")
                        || lv.contains("challenges.cloudflare.com/turnstile")
                    {
                        wall_flags.push("captcha present".into());
                    }
                }
            }
            // Custom ARIA controls are not handled by the native-field driver.
            // Do not silently omit a required question on an otherwise readable form.
            if !matches!(tag.as_str(), "input" | "textarea" | "select")
                && attrs
                    .get("aria-required")
                    .is_some_and(|value| value.eq_ignore_ascii_case("true"))
                && attrs.get("role").is_some_and(|role| {
                    matches!(
                        role.to_ascii_lowercase().as_str(),
                        "combobox"
                            | "listbox"
                            | "textbox"
                            | "checkbox"
                            | "radio"
                            | "spinbutton"
                            | "slider"
                    )
                })
            {
                wall_flags.push("a required custom control needs manual review".into());
            }
            if tag == "label" {
                let mut label_text = String::new();
                collect_text(node, false, &mut label_text);
                let label_text = label_text.split_whitespace().collect::<Vec<_>>().join(" ");
                if let Some(for_id) = attrs.get("for") {
                    labels
                        .entry(for_id.clone())
                        .or_insert_with(|| label_text.clone());
                }
                if !label_text.is_empty() {
                    walk(node, false, &mut |child, _| {
                        let tag = child
                            .get("nodeName")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_ascii_lowercase();
                        if matches!(tag.as_str(), "input" | "textarea" | "select") {
                            if let Some(id) = child.get("nodeId").and_then(Value::as_u64) {
                                wrapping_labels
                                    .entry(id)
                                    .or_insert_with(|| label_text.clone());
                            }
                            return false;
                        }
                        true
                    });
                }
            }
            if tag == "fieldset"
                || attrs
                    .get("role")
                    .is_some_and(|role| role.eq_ignore_ascii_case("radiogroup"))
            {
                let legend = if tag == "fieldset" {
                    node.get("children")
                        .and_then(Value::as_array)
                        .and_then(|children| {
                            children.iter().find(|child| {
                                child
                                    .get("nodeName")
                                    .and_then(Value::as_str)
                                    .is_some_and(|name| name.eq_ignore_ascii_case("legend"))
                            })
                        })
                        .map(|node| {
                            let mut value = String::new();
                            collect_text(node, false, &mut value);
                            value.split_whitespace().collect::<Vec<_>>().join(" ")
                        })
                        .unwrap_or_default()
                } else {
                    attrs.get("aria-label").cloned().unwrap_or_default()
                };
                let group_ref = if tag == "fieldset" {
                    None
                } else {
                    attrs.get("aria-labelledby").cloned()
                };
                if let Some(fieldset_id) = node
                    .get("nodeId")
                    .and_then(Value::as_u64)
                    .filter(|_| !legend.is_empty() || group_ref.is_some())
                {
                    walk(node, false, &mut |child, _| {
                        if child
                            .get("nodeName")
                            .and_then(Value::as_str)
                            .is_some_and(|name| name.eq_ignore_ascii_case("input"))
                        {
                            if let Some(id) = child.get("nodeId").and_then(Value::as_u64) {
                                radio_containers
                                    .insert(id, (fieldset_id, legend.clone(), group_ref.clone()));
                            }
                        }
                        true
                    });
                }
            }
            if matches!(tag.as_str(), "script" | "style" | "noscript" | "template") {
                return false; // skip this subtree for text collection
            }
            if matches!(tag.as_str(), "input" | "textarea" | "select") && !in_control {
                if node.get("nodeId").and_then(|v| v.as_u64()).is_some() {
                    controls.push(Control {
                        node_id: node
                            .get("nodeId")
                            .and_then(Value::as_u64)
                            .unwrap_or_default(),
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

        // Resolve ID references after collecting the controls because labels can
        // occur anywhere in the document, including after the input itself.
        let mut wanted = std::collections::HashSet::new();
        for control in &controls {
            if let Some(ids) = control
                .attrs
                .get("aria-labelledby")
                .filter(|s| s.len() <= 512)
            {
                for id in ids.split_whitespace().take(16) {
                    if wanted.len() < 160 {
                        wanted.insert(id.to_owned());
                    }
                }
            }
        }
        for (_, _, group_ref) in radio_containers.values() {
            if let Some(ids) = group_ref.as_ref().filter(|s| s.len() <= 512) {
                for id in ids.split_whitespace().take(16) {
                    if wanted.len() < 160 {
                        wanted.insert(id.to_owned());
                    }
                }
            }
        }
        let mut referenced: HashMap<String, Option<String>> = HashMap::new();
        walk(&root, false, &mut |node, _| {
            let attrs = attributes(node);
            if let Some(id) = attrs.get("id").filter(|id| wanted.contains(*id)) {
                if referenced.contains_key(id) {
                    referenced.insert(id.clone(), None);
                } else {
                    let mut label = String::new();
                    collect_text(node, false, &mut label);
                    let label = label.split_whitespace().collect::<Vec<_>>().join(" ");
                    referenced.insert(
                        id.clone(),
                        (!label.is_empty() && label.len() <= 1000).then_some(label),
                    );
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

        let mut radio_groups: HashMap<(u64, String), usize> = HashMap::new();
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
            let referenced_label = control.attrs.get("aria-labelledby").map(|ids| {
                if ids.len() > 512 {
                    return None;
                }
                let ids: Vec<_> = ids.split_whitespace().collect();
                if ids.is_empty() || ids.len() > 16 {
                    return None;
                }
                let parts: Option<Vec<_>> = ids
                    .iter()
                    .map(|id| referenced.get(*id).and_then(Option::as_ref))
                    .collect();
                parts.map(|parts| {
                    parts
                        .into_iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
            });
            if referenced_label.as_ref().is_some_and(Option::is_none) {
                walls.push("a field has an unresolved or ambiguous accessible label".into());
            }
            let label = referenced_label
                .flatten()
                .or_else(|| control.attrs.get("aria-label").cloned())
                .or_else(|| labels.get(&id).cloned())
                .or_else(|| wrapping_labels.get(&control.node_id).cloned())
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
            if control.attrs.contains_key("disabled") || control.attrs.contains_key("readonly") {
                if required {
                    walls.push("a required field is disabled or read-only".into());
                }
                continue;
            }
            let sensitive = is_sensitive_label(&label) || input_type == "password";
            if input_type == "radio" {
                if let (Some((fieldset_id, legend, group_ref)), Some(name)) = (
                    radio_containers.get(&control.node_id),
                    control.attrs.get("name").filter(|name| !name.is_empty()),
                ) {
                    let group_label = if let Some(ids) = group_ref {
                        let ids: Vec<_> = ids.split_whitespace().collect();
                        let resolved: Option<Vec<_>> = if ids.is_empty()
                            || ids.len() > 16
                            || group_ref.as_ref().is_some_and(|s| s.len() > 512)
                        {
                            None
                        } else {
                            ids.iter()
                                .map(|id| referenced.get(*id).and_then(Option::as_ref))
                                .collect()
                        };
                        match resolved {
                            Some(parts) => parts
                                .into_iter()
                                .map(String::as_str)
                                .collect::<Vec<_>>()
                                .join(" "),
                            None => {
                                walls.push(
                                    "a radio group has an unresolved or ambiguous accessible label"
                                        .into(),
                                );
                                legend.clone()
                            }
                        }
                    } else {
                        legend.clone()
                    };
                    let key = (*fieldset_id, name.clone());
                    if let Some(index) = radio_groups.get(&key).copied() {
                        let field: &mut waypoint_applications::form_mapping::FormField =
                            &mut fields[index];
                        if let FormFieldType::Radio { options } = &mut field.field_type {
                            options.push(label.clone());
                        }
                        field.sensitive |= sensitive;
                        field.required |= required;
                    } else {
                        let index = fields.len();
                        radio_groups.insert(key, index);
                        fields.push(FormField {
                            selector: selector_for(&control),
                            label: group_label.clone(),
                            field_type: FormFieldType::Radio {
                                options: vec![label],
                            },
                            required,
                            sensitive: sensitive || is_sensitive_label(&group_label),
                            autocomplete: None,
                        });
                    }
                    continue;
                }
            }
            let field_type = match (control.tag.as_str(), input_type.as_str()) {
                (_, "file") => FormFieldType::File,
                (_, "checkbox") => FormFieldType::Checkbox,
                (_, "radio") => FormFieldType::Radio { options: vec![] },
                (_, "select") if control.attrs.contains_key("multiple") => {
                    FormFieldType::MultiSelect {
                        options: control.options.clone(),
                    }
                }
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
                autocomplete: control
                    .attrs
                    .get("autocomplete")
                    .filter(|value| value.len() <= 128)
                    .cloned(),
            });
        }

        let mut selectors = std::collections::HashSet::new();
        if fields
            .iter()
            .any(|field| !selectors.insert(field.selector.clone()))
        {
            walls.push("ambiguous form controls need manual review".into());
        }

        walls.sort();
        walls.dedup();
        let schema = waypoint_applications::form_mapping::FormSchema {
            origin: origin_of(url),
            fields,
            multi_step: possible_next_step,
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
                let resolved = self
                    .cdp
                    .call("DOM.resolveNode", json!({"nodeId":node}))
                    .map_err(derr)?;
                let object = resolved
                    .pointer("/object/objectId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| DriverError("Cannot inspect text field".into()))?;
                let check = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function() { return (this instanceof HTMLTextAreaElement || (this instanceof HTMLInputElement && ['text','email','tel','url','search','number'].includes(this.type))) && !this.matches(':disabled') && !this.readOnly; }","returnByValue":true})).map_err(derr)?;
                if check.get("exceptionDetails").is_some()
                    || check.pointer("/result/value").and_then(Value::as_bool) != Some(true)
                {
                    return Err(DriverError(
                        "Text field is unavailable or requires manual handling".into(),
                    ));
                }
                // Real focus + real typing: the page sees the same events a
                // human would produce. Replace existing content instead of
                // appending to a prefilled or browser-restored value.
                self.cdp
                    .call("DOM.focus", json!({ "nodeId": node }))
                    .map_err(derr)?;
                self.cdp.call("Input.dispatchKeyEvent", json!({"type":"rawKeyDown","key":"a","code":"KeyA","windowsVirtualKeyCode":65,"modifiers":2})).map_err(derr)?;
                self.cdp.call("Input.dispatchKeyEvent", json!({"type":"keyUp","key":"a","code":"KeyA","windowsVirtualKeyCode":65,"modifiers":2})).map_err(derr)?;
                self.cdp.call("Input.dispatchKeyEvent", json!({"type":"rawKeyDown","key":"Backspace","code":"Backspace","windowsVirtualKeyCode":8})).map_err(derr)?;
                self.cdp.call("Input.dispatchKeyEvent", json!({"type":"keyUp","key":"Backspace","code":"Backspace","windowsVirtualKeyCode":8})).map_err(derr)?;
                self.cdp
                    .call("Input.insertText", json!({ "text": t }))
                    .map_err(derr)?;
                let retained = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function(value) { return this.isConnected && !this.matches(':disabled') && !this.readOnly && this.value === value && this.validity.valid; }","arguments":[{"value":t}],"returnByValue":true})).map_err(derr)?;
                if retained.get("exceptionDetails").is_some()
                    || retained.pointer("/result/value").and_then(Value::as_bool) != Some(true)
                {
                    return Err(DriverError(
                        "Text was not retained exactly or the field is invalid; review it manually"
                            .into(),
                    ));
                }
                self.trace
                    .push(format!("typed {} chars into {selector}", t.chars().count()));
                Ok(())
            }
            PacketValue::Bool(b) => {
                let resolved = self
                    .cdp
                    .call("DOM.resolveNode", json!({"nodeId":node}))
                    .map_err(derr)?;
                let object = resolved
                    .pointer("/object/objectId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| DriverError("Cannot inspect checkbox".into()))?;
                let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function(value) { if (this.tagName !== 'INPUT' || this.type !== 'checkbox' || this.disabled) return false; if (this.checked !== value) { const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'checked').set; set.call(this, value); this.dispatchEvent(new Event('input', {bubbles:true})); this.dispatchEvent(new Event('change', {bubbles:true})); } return this.checked === value; }","arguments":[{"value":b}],"returnByValue":true})).map_err(derr)?;
                if result.get("exceptionDetails").is_some()
                    || result.pointer("/result/value").and_then(Value::as_bool) != Some(true)
                {
                    return Err(DriverError(
                        "Checkbox was unavailable or its value was not retained".into(),
                    ));
                }
                Ok(())
            }
            PacketValue::Date(d) => {
                if !waypoint_applications::form_mapping::valid_iso_date(d) {
                    return Err(DriverError("Invalid calendar date".into()));
                }
                let resolved = self
                    .cdp
                    .call("DOM.resolveNode", json!({"nodeId":node}))
                    .map_err(derr)?;
                let object = resolved
                    .pointer("/object/objectId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| DriverError("Cannot inspect date field".into()))?;
                let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function(value) { if (this.tagName !== 'INPUT' || this.type !== 'date' || this.disabled || this.readOnly) return false; const previous = this.value; const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set; set.call(this, value); if (this.value !== value || !this.validity.valid) { set.call(this, previous); return false; } this.dispatchEvent(new Event('input', {bubbles:true})); this.dispatchEvent(new Event('change', {bubbles:true})); return this.value === value; }","arguments":[{"value":d}],"returnByValue":true})).map_err(derr)?;
                if result.get("exceptionDetails").is_some()
                    || result.pointer("/result/value").and_then(Value::as_bool) != Some(true)
                {
                    return Err(DriverError(
                        "Date is unavailable, outside the field's limits, or was not retained"
                            .into(),
                    ));
                }
                Ok(())
            }
            PacketValue::TextList(_) => Err(DriverError("Use a multiple-choice command".into())),
            PacketValue::None => Ok(()),
        }
    }

    /// A fixed, narrow operation for native single-select controls. It rejects
    /// ambiguous or disabled labels and dispatches the usual form events.
    fn select_option(
        &mut self,
        selector: &str,
        label: &str,
        apply: bool,
    ) -> Result<(), DriverError> {
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Select field not found".into()))?;
        let result = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object_id = result
            .get("object")
            .and_then(|v| v.get("objectId"))
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Cannot inspect select field".into()))?;
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object_id,"functionDeclaration":"function(label, apply) { if (this.tagName !== 'SELECT' || this.multiple || this.disabled || !this.isConnected) return false; const matches = Array.from(this.options).filter(o => o.label.trim().replace(/\\s+/g, ' ').toLowerCase() === label.trim().replace(/\\s+/g, ' ').toLowerCase() && !o.disabled && !(o.parentElement && o.parentElement.tagName === 'OPTGROUP' && o.parentElement.disabled)); if (matches.length !== 1) return false; if (apply) { this.value = matches[0].value; this.dispatchEvent(new Event('input', {bubbles:true})); this.dispatchEvent(new Event('change', {bubbles:true})); } return this.selectedOptions.length === 1 && this.selectedOptions[0] === matches[0] && this.validity.valid; }","arguments":[{"value":label},{"value":apply}],"returnByValue":true})).map_err(derr)?;
        if result.get("exceptionDetails").is_some()
            || result
                .get("result")
                .and_then(|v| v.get("value"))
                .and_then(Value::as_bool)
                != Some(true)
        {
            return Err(DriverError(
                "Select option is unavailable, ambiguous, or was not retained".into(),
            ));
        }
        Ok(())
    }

    fn select_options(
        &mut self,
        selector: &str,
        labels: &[String],
        apply: bool,
    ) -> Result<(), DriverError> {
        if labels.is_empty() || labels.len() > 100 {
            return Err(DriverError("Choose between one and 100 options".into()));
        }
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Select field not found".into()))?;
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Cannot inspect select field".into()))?;
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function(labels, apply) {
            if (!(this instanceof HTMLSelectElement) || !this.multiple || this.matches(':disabled') || !this.isConnected) return false;
            const norm = s => s.trim().replace(/\s+/g, ' ').toLowerCase();
            const options = Array.from(this.options);
            const chosen = [];
            for (const label of labels) {
                const matches = options.filter(o => norm(o.label) === norm(label) && !o.disabled && !(o.parentElement && o.parentElement.tagName === 'OPTGROUP' && o.parentElement.disabled));
                if (matches.length !== 1 || chosen.includes(matches[0])) return false;
                chosen.push(matches[0]);
            }
            if (apply) {
                for (const option of options) option.selected = chosen.includes(option);
                this.dispatchEvent(new Event('input', {bubbles:true}));
                this.dispatchEvent(new Event('change', {bubbles:true}));
            }
            return this.isConnected && !this.matches(':disabled') && this.validity.valid && options.every(o => o.selected === chosen.includes(o));
        }"#,"arguments":[{"value":labels},{"value":apply}],"returnByValue":true})).map_err(derr)?;
        if result.get("exceptionDetails").is_some()
            || result.pointer("/result/value").and_then(Value::as_bool) != Some(true)
        {
            return Err(DriverError(
                "Multiple-choice options are unavailable, ambiguous, or were not retained".into(),
            ));
        }
        Ok(())
    }

    fn select_radio(
        &mut self,
        selector: &str,
        label: &str,
        apply: bool,
    ) -> Result<(), DriverError> {
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Radio group not found".into()))?;
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Cannot inspect radio group".into()))?;
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function(label, apply) {
          if (this.tagName !== 'INPUT' || this.type !== 'radio' || !this.name) return false;
          const selector = 'fieldset,[role="radiogroup"]';
          const group = this.closest(selector); if (!group) return false;
          const norm = s => s.trim().replace(/\s+/g, ' ').toLowerCase();
          const optionLabel = r => {
            const refs = r.getAttribute('aria-labelledby');
            if (refs !== null) {
              const ids = refs.trim().split(/\s+/).filter(Boolean);
              if (!ids.length || ids.length > 16 || refs.length > 512) return null;
              const parts = [];
              for (const id of ids) {
                const nodes = document.querySelectorAll('#' + CSS.escape(id));
                if (nodes.length !== 1 || !nodes[0].textContent.trim()) return null;
                parts.push(nodes[0].textContent);
              }
              return parts.join(' ');
            }
            return r.getAttribute('aria-label') || (r.labels && r.labels[0] && r.labels[0].textContent) || r.getAttribute('name') || '';
          };
          const radios = Array.from(group.querySelectorAll('input[type=radio]')).filter(r => r.closest(selector) === group && r.name === this.name && r.form === this.form && !r.matches(':disabled'));
          const allRadios = document.querySelectorAll('input[type=radio]'); if (allRadios.length > 5000) return false;
          const sameName = Array.from(allRadios).filter(r => r.name === this.name && r.form === this.form);
          if (sameName.some(r => r.closest(selector) !== group)) return false;
          const choices = radios.filter(r => { const text = optionLabel(r); return text !== null && norm(text) === norm(label); });
          if (choices.length !== 1) return false;
          const chosen = choices[0];
          if (apply && !chosen.checked) { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'checked').set.call(chosen, true); chosen.dispatchEvent(new Event('input', {bubbles:true})); chosen.dispatchEvent(new Event('change', {bubbles:true})); }
          return chosen.isConnected && chosen.checked && !chosen.matches(':disabled') && chosen.validity.valid;
        }"#,"arguments":[{"value":label},{"value":apply}],"returnByValue":true})).map_err(derr)?;
        if result.get("exceptionDetails").is_some()
            || result.pointer("/result/value").and_then(Value::as_bool) != Some(true)
        {
            return Err(DriverError(
                "Radio option is unavailable, ambiguous, or was not retained".into(),
            ));
        }
        Ok(())
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
    fn current_url(&mut self) -> Result<String, DriverError> {
        CdpDriver::current_url(self)
    }

    fn application_links(&mut self) -> Result<Vec<String>, DriverError> {
        CdpDriver::application_links(self)
    }

    fn open(&mut self, url: &str) -> Result<LiveForm, DriverError> {
        self.cdp.take_events();
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
        let result = self.inspect_current()?;
        self.trace.push(format!(
            "opened {url} ({} fields)",
            result.schema.fields.len()
        ));
        Ok(result)
    }

    fn inspect_current(&mut self) -> Result<LiveForm, DriverError> {
        let current_url = self.current_url()?;
        let (schema, walls) = self.read_form(&current_url)?;
        Ok(LiveForm {
            origin: schema.origin.clone(),
            schema,
            walls,
        })
    }

    fn verify_text_value(&mut self, selector: &str, expected: &str) -> Result<(), DriverError> {
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Previously filled field is no longer present".into()))?;
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Cannot inspect previously filled field".into()))?;
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function(value) { return this.isConnected && (this instanceof HTMLTextAreaElement || (this instanceof HTMLInputElement && ['text','email','tel','url','search','number','date'].includes(this.type))) && !this.matches(':disabled') && !this.readOnly && this.value === value && this.validity.valid; }","arguments":[{"value":expected}],"returnByValue":true})).map_err(derr)?;
        if result.get("exceptionDetails").is_some()
            || result.pointer("/result/value").and_then(Value::as_bool) != Some(true)
        {
            return Err(DriverError(
                "A previously filled field changed or became invalid".into(),
            ));
        }
        Ok(())
    }

    fn verify_choice(&mut self, command: &BrowserCommand) -> Result<(), DriverError> {
        match command {
            BrowserCommand::SelectOptions { selector, labels } => {
                self.select_options(selector, labels, false)
            }
            BrowserCommand::SelectOption { selector, label } => {
                self.select_option(selector, label, false)
            }
            BrowserCommand::SelectRadio { selector, label } => {
                self.select_radio(selector, label, false)
            }
            BrowserCommand::Fill {
                selector,
                value: PacketValue::Bool(expected),
            } => {
                let node = self
                    .node_id(selector)?
                    .ok_or_else(|| DriverError("Previously filled checkbox is missing".into()))?;
                let resolved = self
                    .cdp
                    .call("DOM.resolveNode", json!({"nodeId":node}))
                    .map_err(derr)?;
                let object = resolved
                    .pointer("/object/objectId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| DriverError("Cannot inspect checkbox".into()))?;
                let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function(value) { return this.isConnected && this instanceof HTMLInputElement && this.type === 'checkbox' && !this.matches(':disabled') && this.checked === value && this.validity.valid; }","arguments":[{"value":expected}],"returnByValue":true})).map_err(derr)?;
                if result.get("exceptionDetails").is_some()
                    || result.pointer("/result/value").and_then(Value::as_bool) != Some(true)
                {
                    return Err(DriverError("A checkbox changed or became invalid".into()));
                }
                Ok(())
            }
            _ => Err(DriverError("Unsupported choice verification".into())),
        }
    }

    fn execute(&mut self, command: &BrowserCommand) -> Result<(), DriverError> {
        match command {
            BrowserCommand::Navigate { url } => {
                self.open(url)?;
                Ok(())
            }
            BrowserCommand::Fill { selector, value } => self.fill(selector, value),
            BrowserCommand::SelectOption { selector, label } => {
                self.select_option(selector, label, true)
            }
            BrowserCommand::SelectOptions { selector, labels } => {
                self.select_options(selector, labels, true)
            }
            BrowserCommand::SelectRadio { selector, label } => {
                self.select_radio(selector, label, true)
            }
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
    node_id: u64,
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

fn application_links_from_document(root: &Value) -> Vec<String> {
    let mut wanted = std::collections::HashSet::new();
    walk(root, false, &mut |node, _| {
        if node
            .get("nodeName")
            .and_then(Value::as_str)
            .is_some_and(|tag| tag.eq_ignore_ascii_case("a"))
        {
            let attrs = attributes(node);
            if let Some(ids) = attrs.get("aria-labelledby").filter(|ids| ids.len() <= 512) {
                for id in ids.split_whitespace().take(16) {
                    if wanted.len() < 160 {
                        wanted.insert(id.to_owned());
                    }
                }
            }
        }
        true
    });
    let mut referenced: HashMap<String, Option<String>> = HashMap::new();
    walk(root, false, &mut |node, _| {
        let attrs = attributes(node);
        if let Some(id) = attrs.get("id").filter(|id| wanted.contains(*id)) {
            if referenced.contains_key(id) {
                referenced.insert(id.clone(), None);
            } else {
                let mut text = String::new();
                collect_text(node, false, &mut text);
                let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                referenced.insert(
                    id.clone(),
                    (!text.is_empty() && text.len() <= 1000).then_some(text),
                );
            }
        }
        true
    });
    let mut links = Vec::new();
    walk(root, false, &mut |node, _| {
        let tag = node.get("nodeName").and_then(Value::as_str).unwrap_or("");
        if tag.eq_ignore_ascii_case("a") {
            let attrs = attributes(node);
            let mut label = String::new();
            if let Some(ids) = attrs.get("aria-labelledby") {
                let parts: Vec<_> = ids.split_whitespace().collect();
                if ids.len() > 512 || parts.is_empty() || parts.len() > 16 {
                    return true;
                }
                let resolved: Option<Vec<_>> = parts
                    .iter()
                    .map(|id| referenced.get(*id).and_then(Option::as_ref))
                    .collect();
                let Some(resolved) = resolved else {
                    return true;
                };
                label = resolved
                    .into_iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(" ");
            } else if let Some(aria) = attrs
                .get("aria-label")
                .filter(|label| !label.trim().is_empty())
            {
                label.push_str(aria);
            } else {
                collect_text(node, false, &mut label);
            }
            let label = label
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            let candidate = [
                "apply",
                "apply now",
                "apply for this job",
                "apply for this position",
                "start application",
                "start your application",
            ]
            .contains(&label.as_str());
            if candidate {
                if let Some(href) = attrs.get("href").filter(|href| !href.trim().is_empty()) {
                    if !links.contains(href) {
                        links.push(href.clone());
                    }
                }
            }
        }
        !matches!(
            tag.to_ascii_lowercase().as_str(),
            "script" | "style" | "template" | "noscript"
        )
    });
    links
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
            let attrs = attributes(child);
            if attrs.contains_key("disabled") {
                continue;
            }
            if tag == "optgroup" {
                options.extend(options_of(child));
            } else if tag == "option" {
                let mut text = String::new();
                if let Some(label) = attrs.get("label") {
                    text.push_str(label);
                } else {
                    collect_text(child, false, &mut text);
                }
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
        let bytes = id.as_bytes();
        let simple = bytes
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')
            && (bytes[0].is_ascii_alphabetic()
                || bytes[0] == b'_'
                || (bytes[0] == b'-' && bytes.len() > 1 && !bytes[1].is_ascii_digit()));
        return if simple {
            format!("#{id}")
        } else {
            format!("[id=\"{}\"]", css_string(id))
        };
    }
    if let Some(name) = control.attrs.get("name").filter(|v| !v.is_empty()) {
        return format!("{}[name=\"{}\"]", control.tag, css_string(name));
    }
    if let Some(t) = control.attrs.get("type") {
        return format!("{}[type=\"{}\"]", control.tag, css_string(t));
    }
    control.tag.clone()
}

/// Escape a DOM attribute value inside a double-quoted CSS string.
fn css_string(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '\0' => escaped.push('\u{fffd}'),
            '"' | '\\' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                escaped.push_str(&format!("\\{:x} ", c as u32))
            }
            c => escaped.push(c),
        }
    }
    escaped
}

fn collect_text(node: &Value, skip: bool, out: &mut String) {
    if skip {
        return;
    }
    walk(node, false, &mut |current, _| {
        let tag = current
            .get("nodeName")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(tag.as_str(), "script" | "style" | "noscript" | "template") {
            return false;
        }
        if tag == "#text" {
            if let Some(value) = current.get("nodeValue").and_then(Value::as_str) {
                out.push_str(value);
                out.push(' ');
            }
        }
        true
    });
}

/// Depth-first walk; the callback returns false to skip a subtree.
fn walk<F: FnMut(&Value, bool) -> bool>(node: &Value, in_control: bool, cb: &mut F) {
    let mut pending = vec![(node, in_control)];
    while let Some((current, inside_control)) = pending.pop() {
        if !cb(current, inside_control) {
            continue;
        }
        let tag = current
            .get("nodeName")
            .and_then(Value::as_str)
            .unwrap_or("");
        let child_in_control = inside_control
            || ["input", "textarea", "select"]
                .iter()
                .any(|kind| tag.eq_ignore_ascii_case(kind));
        if let Some(children) = current.get("children").and_then(Value::as_array) {
            pending.extend(children.iter().rev().map(|child| (child, child_in_control)));
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

#[cfg(test)]
mod traversal_tests {
    use super::*;

    #[test]
    fn deeply_nested_nodes_keep_order_and_skip_non_content() {
        let mut nested = json!({"nodeName":"#text", "nodeValue":"deep"});
        for _ in 0..512 {
            nested = json!({"nodeName":"DIV", "children":[nested]});
        }
        let root = json!({"nodeName":"BODY", "children":[
            {"nodeName":"#text", "nodeValue":"first"},
            {"nodeName":"SCRIPT", "children":[{"nodeName":"#text", "nodeValue":"ignored"}]},
            nested,
            {"nodeName":"SELECT", "children":[{"nodeName":"OPTION", "children":[{"nodeName":"#text", "nodeValue":"choice"}]}]}
        ]});
        let mut text = String::new();
        collect_text(&root, false, &mut text);
        assert_eq!(text, "first deep choice ");
        let mut control_text = false;
        walk(&root, false, &mut |node, inside| {
            if node.get("nodeValue").and_then(Value::as_str) == Some("choice") {
                control_text = inside;
            }
            true
        });
        assert!(control_text);
    }
}
