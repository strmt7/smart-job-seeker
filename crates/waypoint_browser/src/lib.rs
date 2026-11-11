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
//! - The driver exposes no model-generated script execution. Trusted, fixed
//!   Runtime functions support DOM inspection and value checks alongside the
//!   DOM, Input, Page, Network, CSS and Browser protocol methods.
//! - Captchas and login walls are reported as walls. Nothing tries to climb
//!   them.

mod captured_frame;
pub use captured_frame::{CapturedFrame, CapturedFrames, RecordedFieldCapture, RecordedFieldKind};
pub mod capture_discovery;
pub mod capture_privacy;
pub mod capture_session;
pub mod cdp;
pub mod rehearsal;
pub mod rehearsal_capture;

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

fn is_stale_inspection_node(error: &CdpError) -> bool {
    is_disappeared_node_error(&error.0)
}

fn is_disappeared_node_error(message: &str) -> bool {
    message.contains("DOM.resolveNode: protocol error")
        && message.contains("No node with given id found")
}

fn history_requires_top_level(error: &CdpError) -> bool {
    error
        .0
        .strip_prefix("Page.getNavigationHistory: protocol error ")
        .and_then(|body| serde_json::from_str::<Value>(body).ok())
        .is_some_and(|reply| {
            reply["code"] == -32000
                && reply["message"] == "Command can only be executed on top-level targets"
        })
}

fn embedded_frame_url(tree: &Value) -> Result<String, DriverError> {
    let invalid =
        || DriverError("Embedded document URL could not be verified; review it manually".into());
    let frame = tree.pointer("/frameTree/frame").ok_or_else(invalid)?;
    let id = frame["id"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 512)
        .ok_or_else(invalid)?;
    let parent = frame["parentId"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 512)
        .ok_or_else(invalid)?;
    if id == parent {
        return Err(invalid());
    }
    let raw = frame["url"]
        .as_str()
        .filter(|v| {
            !v.is_empty()
                && v.len() <= 4096
                && !v.chars().any(|c| c.is_control() || c.is_whitespace())
        })
        .ok_or_else(invalid)?;
    let parsed = url::Url::parse(raw).map_err(|_| invalid())?;
    let security = frame["securityOrigin"]
        .as_str()
        .and_then(|v| url::Url::parse(v).ok())
        .ok_or_else(invalid)?;
    if !matches!(parsed.scheme(), "https" | "http")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || !security.username().is_empty()
        || security.password().is_some()
        || security.path() != "/"
        || security.query().is_some()
        || security.fragment().is_some()
        || parsed.origin() != security.origin()
    {
        return Err(invalid());
    }
    let fragment = match frame.get("urlFragment") {
        None => "",
        Some(Value::String(value))
            if value.len() <= 4096 && (value.is_empty() || value.starts_with('#')) =>
        {
            value.as_str()
        }
        _ => return Err(invalid()),
    };
    if fragment
        .chars()
        .any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(invalid());
    }
    let result = if fragment.is_empty() {
        raw.to_string()
    } else if let Some(at) = raw.find('#') {
        if &raw[at..] != fragment {
            return Err(invalid());
        }
        raw.to_string()
    } else {
        format!("{raw}{fragment}")
    };
    if result.len() > 4096 {
        return Err(invalid());
    }
    Ok(result)
}

pub const CHANGED_DURING_INSPECTION: &str =
    "A page element disappeared during inspection; the scan was retried. Review the form manually";

// Spend the 720-pixel capture width on nearby context without excluding the
// target's right edge. A wide target may leave little room for context.
fn context_left_edge(
    field_left: f64,
    field_right: f64,
    viewport_left: f64,
    page_width: f64,
) -> f64 {
    (field_left - 240.0)
        .max(field_right - 720.0)
        .max(viewport_left)
        .max(0.0)
        .min(page_width)
}

fn retry_changed_inspection(
    mut result: Result<LiveForm, DriverError>,
    mut retry: impl FnMut(usize) -> Result<LiveForm, DriverError>,
) -> Result<LiveForm, DriverError> {
    let mut changed = false;
    for attempt in 0..3 {
        match result {
            Err(ref error) if is_disappeared_node_error(&error.0) => {
                changed = true;
                result = retry(attempt);
            }
            _ => break,
        }
    }
    let mut form = result.map_err(|error| {
        if is_disappeared_node_error(&error.0) {
            DriverError("The page kept changing during inspection; review it manually".into())
        } else {
            error
        }
    })?;
    if changed
        && !form
            .walls
            .iter()
            .any(|wall| wall == CHANGED_DURING_INSPECTION)
    {
        form.walls.push(CHANGED_DURING_INSPECTION.into());
    }
    Ok(form)
}

/// Frame contents are outside the current form reader's inspection boundary.
pub const UNINSPECTED_FRAME_REVIEW: &str =
    "An embedded frame may contain uninspected application content; review it manually";

/// A visible frame outside the page's access boundary appears to contain form controls.
pub const INACCESSIBLE_FRAME_FORM_REVIEW: &str =
    "An inaccessible embedded frame may contain form controls; review it manually";

/// The browser did not provide a stable child target for an inaccessible frame.
pub const ISOLATED_FRAME_INSPECTION_UNAVAILABLE: &str =
    "An inaccessible embedded frame could not be inspected; review it manually";

/// A rendered web component contains form content that needs review.
pub const SHADOW_FORM_REVIEW: &str =
    "A web component may contain form controls; review it manually";

pub const CLOSED_SHADOW_FORM_REVIEW: &str =
    "A closed web component may contain form controls; review it manually";

pub const EMBEDDED_SCOPE_LIMIT_REVIEW: &str =
    "Some embedded scopes could not be inspected within the limit; review them manually";

pub const NATIVE_DATALIST_REVIEW: &str =
    "A suggested datalist value needs manual review on the application page";

/// A visible field is explicitly marked invalid by the site.
pub const INVALID_FIELD_REVIEW_PREFIX: &str = "The application marks a field invalid; review it: ";

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct PageControl {
    pub label: String,
    pub disabled: bool,
    pub selector: String,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct PageControls {
    pub controls: Vec<PageControl>,
    pub incomplete: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CustomChoice {
    pub label: String,
    pub selector: String,
    pub expanded: bool,
    pub options: Vec<String>,
    pub options_incomplete: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CustomChoices {
    pub choices: Vec<CustomChoice>,
    pub incomplete: bool,
}

/// A read-only snapshot of a custom ARIA radio group. State is reported by
/// the page and must not be treated as independently verified.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CustomRadioGroup {
    pub selector: String,
    pub label: String,
    pub options: Vec<CustomRadioOption>,
    pub incomplete: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CustomRadioOption {
    pub label: String,
    pub checked: Option<bool>,
    pub disabled: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CustomRadioGroups {
    pub groups: Vec<CustomRadioGroup>,
    pub incomplete: bool,
}

// All referenced IDs must resolve uniquely and exactly one must identify a
// listbox. The same fixed resolver is used for observation and actions.
const CHOICE_LIST_RESOLVER: &str = r#"const resolveChoiceList = owner => {
    const raw = owner.getAttribute('aria-controls') || owner.getAttribute('aria-owns') || '';
    if (!raw || raw.length > 256) return null;
    const ids = raw.trim().split(/\s+/).filter(Boolean);
    if (!ids.length || ids.length > 8 || new Set(ids).size !== ids.length) return null;
    const root = owner.getRootNode(); let list = null;
    for (const id of ids) {
        const nodes = root.querySelectorAll('#' + CSS.escape(id));
        if (nodes.length !== 1) return null;
        if ((nodes[0].getAttribute('role') || '').toLowerCase() === 'listbox') {
            if (list) return null;
            list = nodes[0];
        }
    }
    return list;
};"#;

enum ClickGuard<'a> {
    Ordinary,
    NonSubmit,
    ClosedCombobox,
    RadioOption {
        label: &'a str,
        group_selector: &'a str,
    },
    ChoiceOption {
        label: &'a str,
        list_id: &'a str,
        owner_selector: &'a str,
    },
}

/// Turn a protocol failure into a driver failure, keeping the text.
fn derr(e: CdpError) -> DriverError {
    DriverError(e.0)
}

/// US keyboard metadata for printable ASCII. Unicode and other text use the
/// browser's text insertion path instead of guessing a physical key layout.
fn ascii_key(character: char) -> Option<(String, u32, u32, char)> {
    if character == ' ' {
        return Some(("Space".into(), 32, 0, ' '));
    }
    if character.is_ascii_alphabetic() {
        let upper = character.to_ascii_uppercase();
        return Some((
            format!("Key{upper}"),
            upper as u32,
            if character.is_ascii_uppercase() { 8 } else { 0 },
            character.to_ascii_lowercase(),
        ));
    }
    const DIGITS: &str = "0123456789";
    const SHIFTED_DIGITS: &str = ")!@#$%^&*(";
    if let Some(index) = DIGITS.find(character) {
        return Some((
            format!("Digit{index}"),
            b'0' as u32 + index as u32,
            0,
            character,
        ));
    }
    if let Some(index) = SHIFTED_DIGITS.find(character) {
        return Some((
            format!("Digit{index}"),
            b'0' as u32 + index as u32,
            8,
            (b'0' + index as u8) as char,
        ));
    }
    for (plain, shifted, code, key_code) in [
        ('-', '_', "Minus", 189),
        ('=', '+', "Equal", 187),
        ('[', '{', "BracketLeft", 219),
        (']', '}', "BracketRight", 221),
        ('\\', '|', "Backslash", 220),
        (';', ':', "Semicolon", 186),
        ('\'', '"', "Quote", 222),
        (',', '<', "Comma", 188),
        ('.', '>', "Period", 190),
        ('/', '?', "Slash", 191),
        ('`', '~', "Backquote", 192),
    ] {
        if character == plain || character == shifted {
            return Some((
                code.into(),
                key_code,
                if character == shifted { 8 } else { 0 },
                plain,
            ));
        }
    }
    None
}

/// Split long text at spaces without splitting Unicode characters or words.
/// Bound the number of browser round trips and leave long unbroken text intact.
fn text_segments(text: &str) -> Vec<&str> {
    const TARGET_BYTES: usize = 64;
    const MAX_SEGMENTS: usize = 8;
    let mut segments = Vec::new();
    let mut start = 0;
    for (index, character) in text.char_indices() {
        let end = index + character.len_utf8();
        if character == ' '
            && end - start >= TARGET_BYTES
            && segments.len() < MAX_SEGMENTS - 1
            && end < text.len()
        {
            segments.push(&text[start..end]);
            start = end;
        }
    }
    if start < text.len() {
        segments.push(&text[start..]);
    }
    segments
}

/// This is only a hint to request manual navigation, never authorization to click.
fn possible_next_label(label: &str) -> bool {
    let normalized = label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    ["next", "continue", "weiter", "suivant", "siguiente"]
        .iter()
        .any(|prefix| {
            normalized.strip_prefix(prefix).is_some_and(|rest| {
                rest.is_empty()
                    || rest
                        .chars()
                        .next()
                        .is_some_and(|ch| ch.is_whitespace() || matches!(ch, ':' | '→' | '»' | '>'))
            })
        })
}

/// Interpret the browser's `accept` hint for common document formats. This is
/// not proof the server accepts a file; unknown MIME mappings need review.
fn check_file_accept(accept: &str, filename: &str) -> Result<(), DriverError> {
    let accept = accept.trim();
    if accept.is_empty() {
        return Ok(());
    }
    let extension = std::path::Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = match extension.as_str() {
        "pdf" => Some("application/pdf"),
        "docx" => Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
        "doc" => Some("application/msword"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "txt" => Some("text/plain"),
        _ => None,
    };
    let mut uncertain = false;
    for part in accept
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let part = part.to_ascii_lowercase();
        if let Some(expected) = part.strip_prefix('.') {
            if !extension.is_empty() && expected == extension {
                return Ok(());
            }
        } else if part == "*/*" {
            return Ok(());
        } else if let Some(mime) = mime {
            if part == mime
                || part
                    .strip_suffix("/*")
                    .is_some_and(|prefix| mime.starts_with(&format!("{prefix}/")))
            {
                return Ok(());
            }
            if !part.contains('/') {
                uncertain = true;
            }
        } else {
            uncertain = true;
        }
    }
    if uncertain {
        Err(DriverError(
            "Cannot verify the file field's declared formats; review the attachment manually"
                .into(),
        ))
    } else {
        Err(DriverError(
            "The file field does not list this file format; review the attachment manually".into(),
        ))
    }
}

enum BrowserOwner {
    Standard(BrowserProcess),
    #[cfg(windows)]
    Rehearsal(rehearsal::RehearsalOwner),
}

impl BrowserOwner {
    fn shutdown(&mut self) -> Result<(), cdp::CdpError> {
        match self {
            Self::Standard(browser) => {
                browser.shutdown();
                Ok(())
            }
            #[cfg(windows)]
            Self::Rehearsal(browser) => browser.shutdown(),
        }
    }
}

pub struct CdpDriver {
    browser: BrowserOwner,
    cdp: Cdp,
    /// Directories searched when a packet asks to upload a named file.
    upload_dirs: Vec<PathBuf>,
    root_node: Option<u64>,
    /// Human-readable trace of what the driver did, for the UI and for bug
    /// reports. Never contains field values that were typed.
    pub trace: Vec<String>,
    load_wait: Duration,
    pointer_position: Option<(f64, f64)>,
    // Status belongs to one browser frame and loader, never to an origin alone.
    document_status: Option<(String, String, u64)>,
    // One owned inspector sheet per main-frame document, empty outside captures.
    capture_sheet: Option<(String, String, String)>,
    captured_scope: Option<captured_frame::CapturedScope>,
}

impl CdpDriver {
    /// Read a unique semantic section whose DOM id is the requested fragment.
    /// An absent, duplicated, or non-section id is left unclassified.
    pub fn page_section_text_bounded(
        &mut self,
        id: &str,
        limit: usize,
    ) -> Result<Option<(String, bool)>, DriverError> {
        if id.is_empty()
            || id.len() > 200
            || id.chars().any(char::is_control)
            || limit == 0
            || limit > 32_000
        {
            return Err(DriverError("Invalid section-text request".into()));
        }
        Ok(unique_section_text(&self.document()?, id, limit))
    }

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
    /// Extract the end of the current DOM text without retaining the preceding text.
    pub fn page_text_tail_bounded(&mut self, limit: usize) -> Result<(String, bool), DriverError> {
        if limit == 0 || limit > 32_000 {
            return Err(DriverError("Invalid page-text limit".into()));
        }
        Ok(tail_text(&self.document()?, limit))
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
        let history = match self.cdp.call("Page.getNavigationHistory", json!({})) {
            Ok(history) => history,
            Err(error) if history_requires_top_level(&error) => {
                let tree = self
                    .cdp
                    .call("Page.getFrameTree", json!({}))
                    .map_err(derr)?;
                return embedded_frame_url(&tree);
            }
            Err(error) => return Err(derr(error)),
        };
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
    /// Read bounded anchor destinations without clicking or executing script.
    pub fn page_links(
        &mut self,
    ) -> Result<Vec<waypoint_applications::driver::PageLink>, DriverError> {
        let root = self.document()?;
        let referenced = anchor_references(&root);
        let mut links = Vec::new();
        walk_with_visibility(&root, false, false, &mut |node, _, hidden| {
            if !hidden
                && node
                    .get("nodeName")
                    .and_then(Value::as_str)
                    .is_some_and(|tag| tag.eq_ignore_ascii_case("a"))
            {
                let attrs = attributes(node);
                if let Some(href) = attrs
                    .get("href")
                    .filter(|href| !href.trim().is_empty() && href.len() <= 2048)
                {
                    if links.len() < waypoint_applications::driver::PAGE_LINK_LIMIT {
                        let label: String = anchor_label(node, &attrs, &referenced)
                            .unwrap_or_else(|| "Label unavailable".into())
                            .chars()
                            .take(200)
                            .collect();
                        if !links
                            .iter()
                            .any(|link: &waypoint_applications::driver::PageLink| {
                                link.url == *href && link.label == label
                            })
                        {
                            links.push(waypoint_applications::driver::PageLink {
                                url: href.clone(),
                                label,
                            });
                        }
                    }
                }
            }
            true
        });
        Ok(links)
    }
    /// Inventory a bounded set of rendered button-like controls for advisory
    /// review. This does not expose a click operation or determine side effects.
    pub fn page_controls(&mut self) -> Result<PageControls, DriverError> {
        self.refresh_captured_form()?;
        let root = self.document()?;
        let references = button_references(&root);
        let mut candidates = Vec::new();
        let mut incomplete = false;
        walk_with_visibility(&root, false, false, &mut |node, _, hidden| {
            let tag = node
                .get("nodeName")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            let attrs = attributes(node);
            let button = tag == "button"
                || attrs
                    .get("role")
                    .is_some_and(|role| role.eq_ignore_ascii_case("button"))
                || (tag == "input"
                    && attrs.get("type").is_some_and(|kind| {
                        matches!(
                            kind.to_ascii_lowercase().as_str(),
                            "button" | "submit" | "image"
                        )
                    }));
            if button && !hidden {
                // Hidden DOM buttons can precede the useful controls. Scan a
                // bounded larger set, while retaining at most 30 visible ones.
                if candidates.len() >= 120 {
                    incomplete = true;
                    return true;
                }
                let Some(id) = node.get("nodeId").and_then(Value::as_u64) else {
                    incomplete = true;
                    return true;
                };
                let label = if tag == "input"
                    && !attrs.contains_key("aria-label")
                    && !attrs.contains_key("aria-labelledby")
                {
                    attrs.get("value").cloned().unwrap_or_default()
                } else {
                    anchor_label(node, &attrs, &references).unwrap_or_default()
                };
                let label_incomplete = label.trim().is_empty() || label.chars().count() > 200;
                let label = if label.trim().is_empty() {
                    "Unlabelled control".to_owned()
                } else {
                    label.chars().take(200).collect()
                };
                let disabled = attrs.contains_key("disabled")
                    || attrs
                        .get("aria-disabled")
                        .is_some_and(|v| v.eq_ignore_ascii_case("true"));
                candidates.push((id, label, disabled, label_incomplete));
            }
            true
        });
        let started = Instant::now();
        let mut controls = Vec::new();
        for (id, label, disabled, label_incomplete) in candidates {
            if started.elapsed() >= Duration::from_secs(4) {
                incomplete = true;
                break;
            }
            match self.with_review_timeout(|driver| driver.control_visibility_and_path(id)) {
                Ok((true, selector, available, _, _)) => {
                    if controls.len() >= 30 {
                        incomplete = true;
                        break;
                    }
                    incomplete |= label_incomplete;
                    controls.push(PageControl {
                        label,
                        disabled: disabled || !available,
                        selector: self.captured_selector(&selector),
                    });
                }
                Ok(_) => (),
                Err(_) => {
                    incomplete = true;
                    break;
                }
            }
        }
        Ok(PageControls {
            controls,
            incomplete,
        })
    }

    /// Inspect custom radio groups without focusing or activating them.
    pub fn page_custom_radio_groups(&mut self) -> Result<CustomRadioGroups, DriverError> {
        self.with_review_timeout(|driver| driver.page_custom_radio_groups_inner())
    }

    fn page_custom_radio_groups_inner(&mut self) -> Result<CustomRadioGroups, DriverError> {
        let root = self.document()?;
        let mut nodes = Vec::new();
        let mut incomplete = false;
        walk_with_visibility(&root, false, false, &mut |node, _, hidden| {
            let attrs = attributes(node);
            let tag = node.get("nodeName").and_then(Value::as_str).unwrap_or("");
            if !hidden
                && (tag.eq_ignore_ascii_case("iframe")
                    || node
                        .get("shadowRoots")
                        .and_then(Value::as_array)
                        .is_some_and(|r| !r.is_empty()))
            {
                incomplete = true;
            }
            if !hidden
                && attrs
                    .get("role")
                    .is_some_and(|r| r.eq_ignore_ascii_case("radiogroup"))
            {
                if nodes.len() >= 30 {
                    incomplete = true;
                    return true;
                }
                if let Some(id) = node.get("nodeId").and_then(Value::as_u64) {
                    nodes.push(id);
                } else {
                    incomplete = true;
                }
            }
            true
        });
        let mut groups = Vec::new();
        let started = Instant::now();
        for node in nodes {
            if started.elapsed() > Duration::from_secs(4) {
                incomplete = true;
                break;
            }
            let (visible, selector) = match self.control_visibility_and_path(node) {
                Ok((v, selector, _, _, _)) => (v, selector),
                Err(_) => {
                    incomplete = true;
                    continue;
                }
            };
            if !visible {
                continue;
            }
            let resolved = match self.cdp.call("DOM.resolveNode", json!({"nodeId":node})) {
                Ok(v) => v,
                Err(_) => {
                    incomplete = true;
                    continue;
                }
            };
            let Some(object) = resolved.pointer("/object/objectId").and_then(Value::as_str) else {
                incomplete = true;
                continue;
            };
            let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function() {
                if (!this.isConnected || (this.getAttribute('role') || '').toLowerCase() !== 'radiogroup') return null;
                const root=this.getRootNode();
                const name = element => {
                    const refs=element.getAttribute('aria-labelledby');
                    if (refs !== null) {
                        const ids=refs.trim().split(/\s+/).filter(Boolean); if (!ids.length || ids.length>16 || refs.length>512) return '';
                        const parts=[]; for (const id of ids) { const found=root.querySelectorAll('#'+CSS.escape(id)); if(found.length!==1) return ''; parts.push(found[0].textContent || ''); } return parts.join(' ').trim().replace(/\s+/g,' ');
                    }
                    return (element.hasAttribute('aria-label') ? element.getAttribute('aria-label') : (element.innerText || '')).trim().replace(/\s+/g,' ');
                };
                const optionName = item => {
                    const full=name(item), groupRefs=(this.getAttribute('aria-labelledby') || '').trim().split(/\s+/).filter(Boolean), refs=(item.getAttribute('aria-labelledby') || '').trim().split(/\s+/).filter(Boolean);
                    if (groupRefs.length && refs.length>groupRefs.length && groupRefs.every((id,index)=>refs[index]===id)) { const prefix=name(this); if(prefix && full.startsWith(prefix+' ')) return full.slice(prefix.length).trim(); }
                    return full;
                };
                const all=this.querySelectorAll('[role="radio"]'); if (!all.length) return null;
                let incomplete=all.length>30; const options=[];
                for (const item of Array.from(all).slice(0,30)) {
                    if (item.closest('[role="radiogroup"]') !== this || item.tagName === 'INPUT') { incomplete=true; continue; }
                    const label=optionName(item), state=(item.getAttribute('aria-checked') || '').toLowerCase();
                    const visible=item.getClientRects().length>0 && getComputedStyle(item).visibility==='visible' && !item.closest('[hidden], [aria-hidden="true" i]');
                    if (!visible || !label || label.length>200) { incomplete=true; continue; }
                    if (state!=='true' && state!=='false') incomplete=true;
                    options.push({label,checked:state==='true'?true:state==='false'?false:null,disabled:!!item.closest('[aria-disabled="true" i]')});
                }
                if (options.filter(option => option.checked === true).length > 1 || new Set(options.map(option => option.label.toLowerCase())).size !== options.length) incomplete=true;
                let label=(this.hasAttribute('aria-label') || this.hasAttribute('aria-labelledby')) ? name(this) : ''; if(!label || label.length>200) {label='Unlabelled radio group'; incomplete=true;}
                return {label,options,incomplete};
            }"#,"returnByValue":true}));
            let released = self
                .cdp
                .call("Runtime.releaseObject", json!({"objectId":object}));
            let result = result.map_err(derr)?;
            released.map_err(derr)?;
            if result.get("exceptionDetails").is_some() {
                incomplete = true;
                continue;
            }
            let Some(value) = result
                .pointer("/result/value")
                .filter(|value| value.is_object())
            else {
                continue;
            };
            let mut value = value.clone();
            value["selector"] = json!(selector);
            match serde_json::from_value::<CustomRadioGroup>(value) {
                Ok(group) => groups.push(group),
                Err(_) => incomplete = true,
            }
        }
        Ok(CustomRadioGroups { groups, incomplete })
    }

    /// Select an unselected custom radio option. Callers must independently bind
    /// the action to fresh visual review, user intent and a stable page snapshot.
    /// The application does not currently expose this driver operation.
    pub fn select_custom_radio_option(
        &mut self,
        selector: &str,
        label: &str,
    ) -> Result<(), DriverError> {
        let node = self.custom_radio_option_node(selector, label)?;
        self.click_node_guarded(
            node,
            "reviewed custom radio option",
            ClickGuard::RadioOption {
                label,
                group_selector: selector,
            },
        )?;
        for _ in 0..4 {
            self.cdp.pump(Duration::from_millis(200));
            if self.verify_custom_radio_value(selector, label).is_ok() {
                return Ok(());
            }
        }
        Err(DriverError(
            "Radio choice may have changed but its state could not be verified; review it manually"
                .into(),
        ))
    }

    /// Capture uniquely referenced question labels in one bounded visible region.
    /// Otherwise capture the bounded group context for manual/visual review.
    pub fn screenshot_custom_radio_question(
        &mut self,
        selector: &str,
    ) -> Result<String, DriverError> {
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Radio group disappeared".into()))?;
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Radio group disappeared".into()))?;
        let found = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function() {
            if (!this.isConnected || (this.getAttribute('role') || '').toLowerCase() !== 'radiogroup') return null;
            const refs=this.getAttribute('aria-labelledby'); if (refs===null) return {group:true};
            const ids=refs.trim().split(/\s+/).filter(Boolean); if(!ids.length || ids.length>16 || refs.length>512 || new Set(ids).size!==ids.length) return null;
            const root=this.getRootNode(), labels=[];
            for(const id of ids) { const matches=root.querySelectorAll('#'+CSS.escape(id)); if(matches.length!==1) return null; labels.push(matches[0]); }
            let left=Infinity, top=Infinity, right=-Infinity, bottom=-Infinity;
            for(const label of labels) {
                const rect=label.getBoundingClientRect(), style=getComputedStyle(label);
                if(!rect.width || !rect.height || style.visibility!=='visible' || style.display==='none' || Number(style.opacity)===0) return null;
                let parent=label.parentElement, depth=0;
                while(parent && depth++<128) {
                    const s=getComputedStyle(parent), r=parent.getBoundingClientRect();
                    if(s.display==='none' || s.visibility!=='visible' || Number(s.opacity)===0) return null;
                    if(/^(hidden|clip|scroll|auto)$/.test(s.overflowX) && (rect.left<r.left || rect.right>r.right)) return null;
                    if(/^(hidden|clip|scroll|auto)$/.test(s.overflowY) && (rect.top<r.top || rect.bottom>r.bottom)) return null;
                    parent=parent.parentElement;
                }
                if(parent) return null;
                left=Math.min(left,rect.left); top=Math.min(top,rect.top); right=Math.max(right,rect.right); bottom=Math.max(bottom,rect.bottom);
            }
            return {bounds:[left,top,right,top,right,bottom,left,bottom]};
        }"#,"returnByValue":true}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let found = found.map_err(derr)?;
        released.map_err(derr)?;
        let value = &found["result"]["value"];
        if value["group"] == true {
            return self.screenshot_node(node, true, false);
        }
        let bounds = value["bounds"].as_array().ok_or_else(|| {
            DriverError("Radio question label is unavailable or ambiguous".into())
        })?;
        let numbers: Vec<f64> = bounds.iter().filter_map(Value::as_f64).collect();
        self.screenshot_bounds(&numbers, false, true)
    }

    /// Capture the option before a caller seeks independent visual review.
    pub fn screenshot_custom_radio_option(
        &mut self,
        selector: &str,
        label: &str,
    ) -> Result<String, DriverError> {
        let node = self.custom_radio_option_node(selector, label)?;
        self.screenshot_node(node, false, true)
    }

    fn custom_radio_option_node(
        &mut self,
        selector: &str,
        label: &str,
    ) -> Result<u64, DriverError> {
        if label.is_empty() || label.len() > 200 || label.trim() != label {
            return Err(DriverError("Invalid radio option label".into()));
        }
        let inventory = self.page_custom_radio_groups()?;
        let matches: Vec<_> = inventory
            .groups
            .iter()
            .filter(|group| group.selector == selector)
            .collect();
        if inventory.incomplete
            || matches.len() != 1
            || matches[0].incomplete
            || matches[0]
                .options
                .iter()
                .any(|option| option.checked != Some(false))
            || matches[0]
                .options
                .iter()
                .filter(|option| option.label == label && !option.disabled)
                .count()
                != 1
        {
            return Err(DriverError(
                "Radio group is incomplete, already selected, or ambiguous; review it manually"
                    .into(),
            ));
        }
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Radio group disappeared".into()))?;
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Radio group disappeared".into()))?;
        let found = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function(label) {
            if (!this.isConnected || (this.getAttribute('role') || '').toLowerCase() !== 'radiogroup') return null;
            const root=this.getRootNode(), options=this.querySelectorAll('[role="radio"]'); if(!options.length || options.length>30)return null;
            const name=item=>{const refs=item.getAttribute('aria-labelledby');if(refs!==null){const ids=refs.trim().split(/\s+/).filter(Boolean);if(!ids.length || ids.length>16 || refs.length>512)return '';const parts=[];for(const id of ids){const found=root.querySelectorAll('#'+CSS.escape(id));if(found.length!==1)return '';parts.push(found[0].textContent || '');}return parts.join(' ').trim().replace(/\s+/g,' ');}return (item.hasAttribute('aria-label')?item.getAttribute('aria-label'):(item.innerText || '')).trim().replace(/\s+/g,' ');};
            const optionName=item=>{const full=name(item),groupRefs=(this.getAttribute('aria-labelledby')||'').trim().split(/\s+/).filter(Boolean),refs=(item.getAttribute('aria-labelledby')||'').trim().split(/\s+/).filter(Boolean);if(groupRefs.length && refs.length>groupRefs.length && groupRefs.every((id,index)=>refs[index]===id)){const prefix=name(this);if(prefix && full.startsWith(prefix+' '))return full.slice(prefix.length).trim();}return full;};
            const matches=Array.from(options).filter(item=>item.closest('[role="radiogroup"]')===this && optionName(item)===label);
            return matches.length===1 ? matches[0] : null;
        }"#,"arguments":[{"value":label}],"returnByValue":false}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let found = found.map_err(derr)?;
        released.map_err(derr)?;
        let option = found
            .pointer("/result/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Radio option disappeared or became ambiguous".into()))?;
        let requested = self.cdp.call("DOM.requestNode", json!({"objectId":option}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":option}));
        let requested = requested.map_err(derr)?;
        released.map_err(derr)?;
        requested
            .get("nodeId")
            .and_then(Value::as_u64)
            .filter(|id| *id != 0)
            .ok_or_else(|| DriverError("Radio option disappeared".into()))
    }

    /// Check the page-reported state after a radio action; visual verification
    /// remains the caller's responsibility.
    pub fn verify_custom_radio_value(
        &mut self,
        selector: &str,
        label: &str,
    ) -> Result<(), DriverError> {
        let inventory = self.page_custom_radio_groups()?;
        let matches: Vec<_> = inventory
            .groups
            .iter()
            .filter(|group| group.selector == selector)
            .collect();
        if inventory.incomplete
            || matches.len() != 1
            || matches[0].incomplete
            || matches[0].options.iter().any(|option| {
                option.checked.is_none()
                    || (option.label == label && (option.checked != Some(true) || option.disabled))
                    || (option.label != label && option.checked != Some(false))
            })
            || matches[0]
                .options
                .iter()
                .filter(|option| option.label == label)
                .count()
                != 1
        {
            return Err(DriverError(
                "Radio value could not be verified; review it manually".into(),
            ));
        }
        Ok(())
    }

    /// Read rendered ARIA comboboxes and their currently rendered options.
    /// This never opens a control or selects an option. Embedded scopes and
    /// virtualized or collapsed lists are explicitly incomplete.
    pub fn page_custom_choices(&mut self) -> Result<CustomChoices, DriverError> {
        self.with_review_timeout(|driver| driver.page_custom_choices_inner())
    }

    fn page_custom_choices_inner(&mut self) -> Result<CustomChoices, DriverError> {
        let root = self.document()?;
        let references = button_references(&root);
        let mut candidates = Vec::new();
        let mut incomplete = false;
        walk_with_visibility(&root, false, false, &mut |node, _, hidden| {
            let tag = node.get("nodeName").and_then(Value::as_str).unwrap_or("");
            let attrs = attributes(node);
            if !hidden
                && (tag.eq_ignore_ascii_case("iframe")
                    || node
                        .get("shadowRoots")
                        .and_then(Value::as_array)
                        .is_some_and(|roots| !roots.is_empty()))
            {
                incomplete = true;
            }
            if !hidden
                && attrs
                    .get("role")
                    .is_some_and(|role| role.eq_ignore_ascii_case("combobox"))
                && !tag.eq_ignore_ascii_case("select")
            {
                // Keep the walk bounded while allowing the review interface
                // to page beyond its first 15 controls on larger forms.
                if candidates.len() >= 45 {
                    incomplete = true;
                    return true;
                }
                let Some(id) = node.get("nodeId").and_then(Value::as_u64) else {
                    incomplete = true;
                    return true;
                };
                let label = anchor_label(node, &attrs, &references)
                    .filter(|label| !label.is_empty() && label.chars().count() <= 512)
                    .unwrap_or_else(|| {
                        incomplete = true;
                        "Unlabelled choice".into()
                    });
                candidates.push((id, label));
            }
            true
        });
        let started = Instant::now();
        let mut choices = Vec::new();
        for (node, label) in candidates {
            if started.elapsed() > Duration::from_secs(4) {
                incomplete = true;
                break;
            }
            let (visible, selector, _, _, _) =
                match self.with_review_timeout(|driver| driver.control_visibility_and_path(node)) {
                    Ok(value) => value,
                    Err(_) => {
                        incomplete = true;
                        continue;
                    }
                };
            if !visible {
                // The original DOM snapshot contained a visible candidate;
                // a later visibility change means the inventory is incomplete.
                incomplete = true;
                continue;
            }
            let resolved = match self.cdp.call("DOM.resolveNode", json!({"nodeId":node})) {
                Ok(value) => value,
                Err(error) if is_stale_inspection_node(&error) => {
                    incomplete = true;
                    continue;
                }
                Err(error) => return Err(derr(error)),
            };
            let Some(object) = resolved.pointer("/object/objectId").and_then(Value::as_str) else {
                incomplete = true;
                continue;
            };
            let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function() {
                const expanded = (this.getAttribute('aria-expanded') || '').toLowerCase() === 'true';
                if (!this.isConnected || !expanded) return {expanded:false,options:[],incomplete:true};
                /*CHOICE_LIST_RESOLVER*/
                const root = this.getRootNode(), list = resolveChoiceList(this);
                if (!list) return {expanded:true,options:[],incomplete:true};
                const nodes = list.querySelectorAll('[role="option"]');
                // A virtualized list can omit options even when every rendered
                // item fits inside our cap, so completeness is never assumed.
                const options = []; let incomplete = true;
                const listBounds = list.getBoundingClientRect(), listStyle = getComputedStyle(list);
                for (let i = 0; i < Math.min(nodes.length, 500) && options.length < 30; i++) {
                    const node = nodes[i];
                    let visible = node.getClientRects().length > 0;
                    const box = node.getBoundingClientRect();
                    let left = Math.max(0, box.left), right = Math.min(innerWidth, box.right);
                    let top = Math.max(0, box.top), bottom = Math.min(innerHeight, box.bottom);
                    if (['hidden','clip','auto','scroll'].includes(listStyle.overflowX)) { left = Math.max(left, listBounds.left + list.clientLeft); right = Math.min(right, listBounds.left + list.clientLeft + list.clientWidth); }
                    if (['hidden','clip','auto','scroll'].includes(listStyle.overflowY)) { top = Math.max(top, listBounds.top + list.clientTop); bottom = Math.min(bottom, listBounds.top + list.clientTop + list.clientHeight); }
                    if (!visible || right <= left || bottom <= top) continue;
                    for (let parent = node, depth = 0; parent; parent = parent.parentElement) {
                        if (++depth > 64) { visible = false; break; }
                        const style = getComputedStyle(parent);
                        if (style.visibility === 'hidden' || style.visibility === 'collapse' || Number(style.opacity) === 0 || (parent.getAttribute('aria-hidden') || '').toLowerCase() === 'true') { visible = false; break; }
                        if (parent !== node) {
                            const bounds = parent.getBoundingClientRect();
                            if (['hidden','clip','auto','scroll'].includes(style.overflowX)) { left = Math.max(left, bounds.left + parent.clientLeft); right = Math.min(right, bounds.left + parent.clientLeft + parent.clientWidth); }
                            if (['hidden','clip','auto','scroll'].includes(style.overflowY)) { top = Math.max(top, bounds.top + parent.clientTop); bottom = Math.min(bottom, bounds.top + parent.clientTop + parent.clientHeight); }
                        }
                    }
                    visible &&= right > left && bottom > top;
                    const text = (node.getAttribute('aria-label') || node.innerText || '').trim().replace(/\s+/g,' ');
                    if (!visible || (node.getAttribute('aria-disabled') || '').toLowerCase() === 'true' || !text || text.length > 200) continue;
                    options.push(text);
                }
                return {expanded:true,options,incomplete};
            }"#.replace("/*CHOICE_LIST_RESOLVER*/", CHOICE_LIST_RESOLVER), "returnByValue":true}));
            let released = self
                .cdp
                .call("Runtime.releaseObject", json!({"objectId":object}));
            let result = result.map_err(derr)?;
            released.map_err(derr)?;
            let Some(value) = result.pointer("/result/value") else {
                incomplete = true;
                continue;
            };
            let expanded = value
                .get("expanded")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let options = value
                .get("options")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            let options_incomplete = value
                .get("incomplete")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            choices.push(CustomChoice {
                label,
                selector,
                expanded,
                options,
                options_incomplete,
            });
        }
        Ok(CustomChoices {
            choices,
            incomplete,
        })
    }

    /// Open one already identified, closed ARIA combobox. The caller is
    /// responsible for policy, explicit review, and recording the action.
    pub fn open_custom_choice(&mut self, selector: &str) -> Result<CustomChoice, DriverError> {
        let inventory = self.page_custom_choices()?;
        let matching: Vec<_> = inventory
            .choices
            .iter()
            .filter(|choice| choice.selector == selector)
            .collect();
        if matching.len() != 1 || matching[0].expanded {
            return Err(DriverError(
                "Choice is unavailable, ambiguous, or already open; review it manually".into(),
            ));
        }
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Choice disappeared; review it manually".into()))?;
        self.click_node_guarded(node, selector, ClickGuard::ClosedCombobox)?;
        for _ in 0..4 {
            self.cdp.pump(Duration::from_millis(200));
            let fresh = self.page_custom_choices()?;
            let mut matching = fresh
                .choices
                .into_iter()
                .filter(|choice| choice.selector == selector);
            if let Some(choice) = matching.next() {
                if matching.next().is_none() && choice.expanded {
                    return Ok(choice);
                }
            }
        }
        Err(DriverError(
            "Choice did not open or changed; review it manually".into(),
        ))
    }

    /// Move one bounded wheel step within a uniquely identified open list.
    /// This does not click an option; page scroll handlers can still run.
    pub fn custom_choice_scroll_available(
        &mut self,
        selector: &str,
        down: bool,
    ) -> Result<bool, DriverError> {
        let (_, _, position, maximum, _) = self.custom_choice_scroll_target(selector)?;
        Ok(if down {
            position < maximum - 0.5
        } else {
            position > 0.5
        })
    }

    pub fn scroll_custom_choice_options(
        &mut self,
        selector: &str,
        down: bool,
    ) -> Result<CustomChoice, DriverError> {
        let before_url = self.current_url()?;
        let (node, identity, before, maximum, height) =
            self.custom_choice_scroll_target(selector)?;
        if (down && before >= maximum - 0.5) || (!down && before <= 0.5) {
            return Err(DriverError("Choice list is already at its boundary".into()));
        }
        let model = self
            .cdp
            .call("DOM.getBoxModel", json!({"nodeId":node}))
            .map_err(derr)?;
        let quad = model
            .pointer("/model/content")
            .and_then(Value::as_array)
            .ok_or_else(|| DriverError("Choice list has no visible bounds".into()))?;
        let values: Vec<_> = quad.iter().filter_map(Value::as_f64).collect();
        if values.len() != 8 || values.iter().any(|value| !value.is_finite()) {
            return Err(DriverError("Invalid choice list bounds".into()));
        }
        let x = (values[0] + values[2] + values[4] + values[6]) / 4.0;
        let y = (values[1] + values[3] + values[5] + values[7]) / 4.0;
        if x < 0.0 || y < 0.0 {
            return Err(DriverError("Choice list is outside the viewport".into()));
        }
        let delta = (height * 0.7).clamp(20.0, 300.0) * if down { 1.0 } else { -1.0 };
        self.cdp
            .call(
                "Input.dispatchMouseEvent",
                json!({"type":"mouseWheel","x":x,"y":y,"deltaX":0,"deltaY":delta}),
            )
            .map_err(derr)?;
        self.cdp.pump(Duration::from_millis(200));
        let (_, after_identity, after, _, _) = self.custom_choice_scroll_target(selector)?;
        if before_url != self.current_url()? {
            return Err(DriverError(
                "The page changed while scrolling the choice list".into(),
            ));
        }
        if identity != after_identity {
            return Err(DriverError(
                "The choice list node changed while scrolling".into(),
            ));
        }
        if (down && after <= before) || (!down && after >= before) {
            return Err(DriverError(
                "Choice list did not scroll; inspect it manually".into(),
            ));
        }
        let choices = self.page_custom_choices()?;
        let mut matches = choices
            .choices
            .into_iter()
            .filter(|choice| choice.selector == selector && choice.expanded);
        let choice = matches
            .next()
            .ok_or_else(|| DriverError("Choice changed while scrolling".into()))?;
        if matches.next().is_some() {
            return Err(DriverError(
                "Choice became ambiguous while scrolling".into(),
            ));
        }
        self.trace.push(format!("scrolled choice {selector}"));
        Ok(choice)
    }

    fn custom_choice_scroll_target(
        &mut self,
        selector: &str,
    ) -> Result<(u64, u64, f64, f64, f64), DriverError> {
        let owner = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Choice disappeared".into()))?;
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":owner}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Choice disappeared".into()))?;
        let found = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function() {
            if (!this.isConnected || (this.getAttribute('role') || '').toLowerCase() !== 'combobox' || (this.getAttribute('aria-expanded') || '').toLowerCase() !== 'true') return null;
            /*CHOICE_LIST_RESOLVER*/
            const root = this.getRootNode(), list = resolveChoiceList(this);
            if (!list) return null;
            const style = getComputedStyle(list), rect = list.getBoundingClientRect();
            if (!['auto','scroll'].includes(style.overflowY) || list.scrollHeight <= list.clientHeight || rect.width < 8 || rect.height < 8 || rect.left < 0 || rect.top < 0 || rect.right > innerWidth || rect.bottom > innerHeight) return null;
            const hit = root.elementFromPoint((rect.left + rect.right)/2, (rect.top + rect.bottom)/2);
            if (!hit || !(hit === list || list.contains(hit))) return null;
            return list;
        }"#.replace("/*CHOICE_LIST_RESOLVER*/", CHOICE_LIST_RESOLVER), "returnByValue":false}));
        let release_owner = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let found = found.map_err(derr)?;
        release_owner.map_err(derr)?;
        let list = found
            .pointer("/result/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                DriverError("Choice list cannot be safely scrolled; inspect it manually".into())
            })?;
        let state = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":list,"functionDeclaration":"function() { return {top:this.scrollTop, maximum:this.scrollHeight-this.clientHeight, height:this.clientHeight}; }","returnByValue":true}));
        let requested = self.cdp.call("DOM.requestNode", json!({"objectId":list}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":list}));
        let state = state.map_err(derr)?;
        let requested = requested.map_err(derr)?;
        released.map_err(derr)?;
        let number = |key: &str| {
            state
                .pointer(&format!("/result/value/{key}"))
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite())
                .ok_or_else(|| DriverError("Choice scroll state unavailable".into()))
        };
        let node = requested
            .get("nodeId")
            .and_then(Value::as_u64)
            .filter(|id| *id != 0)
            .ok_or_else(|| DriverError("Choice list disappeared".into()))?;
        let described = self
            .cdp
            .call("DOM.describeNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let identity = described
            .pointer("/node/backendNodeId")
            .and_then(Value::as_u64)
            .filter(|id| *id != 0)
            .ok_or_else(|| DriverError("Choice list identity unavailable".into()))?;
        Ok((
            node,
            identity,
            number("top")?,
            number("maximum")?,
            number("height")?,
        ))
    }

    /// Select one uniquely identified rendered option and verify the displayed
    /// value. The caller must bind this action to explicit user review.
    pub fn select_custom_choice_option(
        &mut self,
        selector: &str,
        label: &str,
    ) -> Result<(), DriverError> {
        let (option_node, list_id) = self.custom_choice_option_node(selector, label)?;
        self.click_node_guarded(
            option_node,
            "reviewed custom option",
            ClickGuard::ChoiceOption {
                label,
                list_id: &list_id,
                owner_selector: selector,
            },
        )?;
        for _ in 0..4 {
            self.cdp.pump(Duration::from_millis(200));
            if self.verify_custom_choice_value(selector, label).is_ok() {
                return Ok(());
            }
        }
        Err(DriverError(
            "Choice may have changed but its displayed value was not verified; review it manually"
                .into(),
        ))
    }

    fn custom_choice_option_node(
        &mut self,
        selector: &str,
        label: &str,
    ) -> Result<(u64, String), DriverError> {
        if label.trim().is_empty() || label.len() > 200 || label != label.trim() {
            return Err(DriverError("Invalid choice label".into()));
        }
        let inventory = self.page_custom_choices()?;
        let matching: Vec<_> = inventory
            .choices
            .iter()
            .filter(|choice| choice.selector == selector)
            .collect();
        if matching.len() != 1
            || !matching[0].expanded
            || matching[0]
                .options
                .iter()
                .filter(|option| option.as_str() == label)
                .count()
                != 1
        {
            return Err(DriverError(
                "Choice option is unavailable or ambiguous; review it manually".into(),
            ));
        }
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Choice disappeared; review it manually".into()))?;
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Choice disappeared; review it manually".into()))?;
        let list_id_result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function() { /*CHOICE_LIST_RESOLVER*/ const list=resolveChoiceList(this); return list ? list.id : null; }"#.replace("/*CHOICE_LIST_RESOLVER*/", CHOICE_LIST_RESOLVER),"returnByValue":true}));
        let list_id = match list_id_result {
            Ok(value) => match value
                .pointer("/result/value")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
            {
                Some(id) => id.to_owned(),
                None => {
                    let _ = self
                        .cdp
                        .call("Runtime.releaseObject", json!({"objectId":object}));
                    return Err(DriverError(
                        "Choice list cannot be identified; review it manually".into(),
                    ));
                }
            },
            Err(error) => {
                let _ = self
                    .cdp
                    .call("Runtime.releaseObject", json!({"objectId":object}));
                return Err(derr(error));
            }
        };
        let found = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function(label,id) {
            /*CHOICE_LIST_RESOLVER*/
            if (!this.isConnected || (this.getAttribute('role') || '').toLowerCase() !== 'combobox' || (this.getAttribute('aria-expanded') || '').toLowerCase() !== 'true') return null;
            const list = resolveChoiceList(this);
            if (!list || list.id !== id) return null;
            const nodes = list.querySelectorAll('[role="option"]');
            if (nodes.length > 500) return null;
            let result = null, count = 0;
            for (const option of nodes) {
                const text = (option.getAttribute('aria-label') || option.innerText || '').trim().replace(/\s+/g,' ');
                if (text === label) { count++; result = option; }
            }
            return count === 1 ? result : null;
        }"#.replace("/*CHOICE_LIST_RESOLVER*/", CHOICE_LIST_RESOLVER), "arguments":[{"value":label},{"value":list_id}],"returnByValue":false}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let found = found.map_err(derr)?;
        released.map_err(derr)?;
        let option_object = found
            .pointer("/result/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                DriverError("Choice option is unavailable or ambiguous; review it manually".into())
            })?;
        let requested = self
            .cdp
            .call("DOM.requestNode", json!({"objectId":option_object}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":option_object}));
        let requested = requested.map_err(derr)?;
        released.map_err(derr)?;
        let option_node = requested
            .get("nodeId")
            .and_then(Value::as_u64)
            .filter(|id| *id != 0)
            .ok_or_else(|| DriverError("Choice option disappeared; review it manually".into()))?;
        Ok((option_node, list_id))
    }

    /// Verify a previously selected custom choice without modifying the page.
    pub fn verify_custom_choice_value(
        &mut self,
        selector: &str,
        label: &str,
    ) -> Result<(), DriverError> {
        self.verify_custom_choice_values(&[(selector, label)])
    }

    /// Recheck a bounded group against a single control inventory.
    pub fn verify_custom_choice_values(
        &mut self,
        values: &[(&str, &str)],
    ) -> Result<(), DriverError> {
        if values.is_empty() {
            return Ok(());
        }
        if values.len() > 30 {
            return Err(DriverError("Too many custom choices to verify".into()));
        }
        let inventory = self.page_custom_choices()?;
        for (selector, label) in values {
            if inventory
                .choices
                .iter()
                .filter(|choice| choice.selector == *selector)
                .count()
                != 1
            {
                return Err(DriverError(
                    "Previously selected choice is unavailable or ambiguous".into(),
                ));
            }
            let node = self
                .node_id(selector)?
                .ok_or_else(|| DriverError("Previously selected choice disappeared".into()))?;
            let resolved = self
                .cdp
                .call("DOM.resolveNode", json!({"nodeId":node}))
                .map_err(derr)?;
            let object = resolved
                .pointer("/object/objectId")
                .and_then(Value::as_str)
                .ok_or_else(|| DriverError("Previously selected choice disappeared".into()))?;
            let checked = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function(label) {
            if (!this.isConnected || (this.getAttribute('role') || '').toLowerCase() !== 'combobox' || this.matches(':disabled') || this.closest('[aria-disabled="true" i], [aria-readonly="true" i]')) return false;
            const text = this instanceof HTMLInputElement ? this.value : this.innerText;
            return typeof text === 'string' && text.trim().replace(/\s+/g,' ') === label;
        }"#, "arguments":[{"value":label}],"returnByValue":true}));
            let released = self
                .cdp
                .call("Runtime.releaseObject", json!({"objectId":object}));
            let checked = checked.map_err(derr)?;
            released.map_err(derr)?;
            if checked.pointer("/result/value").and_then(Value::as_bool) != Some(true) {
                return Err(DriverError(
                    "Previously selected choice changed or could not be verified".into(),
                ));
            }
        }
        Ok(())
    }
    pub fn launch(upload_dirs: Vec<PathBuf>) -> Result<Self, DriverError> {
        Self::launch_with_mode(upload_dirs, false)
    }

    pub fn launch_with_mode(upload_dirs: Vec<PathBuf>, visible: bool) -> Result<Self, DriverError> {
        let browser =
            BrowserProcess::launch_with_mode(Duration::from_secs(20), visible).map_err(derr)?;
        let ws = browser.connect().map_err(derr)?;
        let cdp = Cdp::new(ws).map_err(derr)?;
        Self::initialize(BrowserOwner::Standard(browser), cdp, upload_dirs)
    }

    /// Launch a private browser chosen by the application, preserving user overrides.
    pub fn launch_with_binary(
        upload_dirs: Vec<PathBuf>,
        visible: bool,
        binary: &std::path::Path,
    ) -> Result<Self, DriverError> {
        let browser = BrowserProcess::launch_with_binary(binary, Duration::from_secs(20), visible)
            .map_err(derr)?;
        let cdp = Cdp::new(browser.connect().map_err(derr)?).map_err(derr)?;
        Self::initialize(BrowserOwner::Standard(browser), cdp, upload_dirs)
    }

    /// Experimental local-fixture rehearsal. This is not yet qualified for
    /// interacting with real employer applications or active captured sites.
    #[cfg(windows)]
    pub fn launch_rehearsal(
        python: &std::path::Path,
        config: rehearsal::RehearsalConfig,
    ) -> Result<(Self, rehearsal::RehearsalReady), DriverError> {
        if !matches!(config.schema, 1 | 2) {
            return Err(DriverError(
                "Resource discovery cannot open a candidate-entry driver".into(),
            ));
        }
        let relative_upload_dirs: std::collections::BTreeSet<_> = config
            .content_files
            .keys()
            .filter(|name| name.to_ascii_lowercase().ends_with(".pdf"))
            .filter_map(|name| {
                std::path::Path::new(name)
                    .parent()
                    .map(std::path::Path::to_owned)
            })
            .collect();
        let (owner, pipe, evidence) =
            rehearsal::RehearsalOwner::start(python, config).map_err(derr)?;
        let cdp = Cdp::new_rehearsal(pipe).map_err(derr)?;
        let mut uploads = vec![evidence.content_root.clone()];
        uploads.extend(
            relative_upload_dirs
                .into_iter()
                .map(|path| evidence.content_root.join(path)),
        );
        let mut driver = Self::initialize(BrowserOwner::Rehearsal(owner), cdp, uploads)?;
        driver
            .cdp
            .call_browser("Browser.setDownloadBehavior", json!({"behavior":"deny"}))
            .map_err(derr)?;
        driver
            .trace
            .push("experimental local rehearsal; not qualified for real applications".into());
        Ok((driver, evidence))
    }

    /// Experimental replay accounting; never a delivery or server-validation receipt.
    #[cfg(windows)]
    pub fn rehearsal_replay_report(&mut self) -> Result<Value, DriverError> {
        if !matches!(self.browser, BrowserOwner::Rehearsal(_)) {
            return Err(DriverError(
                "Replay accounting requires an isolated session".into(),
            ));
        }
        self.cdp
            .call_browser("SmartJobseeker.replayReport", json!({}))
            .map_err(derr)
    }

    fn initialize(
        browser: BrowserOwner,
        cdp: Cdp,
        upload_dirs: Vec<PathBuf>,
    ) -> Result<Self, DriverError> {
        let mut driver = Self {
            browser,
            cdp,
            upload_dirs,
            root_node: None,
            trace: vec![],
            load_wait: Duration::from_secs(10),
            pointer_position: None,
            document_status: None,
            capture_sheet: None,
            captured_scope: None,
        };
        driver.cdp.call("Page.enable", json!({})).map_err(derr)?;
        driver.cdp.call("DOM.enable", json!({})).map_err(derr)?;
        driver
            .cdp
            .call(
                "Network.enable",
                json!({"maxTotalBufferSize":0,"maxResourceBufferSize":0,"maxPostDataSize":0}),
            )
            .map_err(derr)?;
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

    /// A partial observation only: other targets, transports and late events
    /// may not be represented. No request URL, headers or body are retained.
    pub fn observed_non_read_requests(&mut self) -> Result<u64, DriverError> {
        self.cdp
            .call("Page.getFrameTree", json!({}))
            .map_err(derr)?;
        Ok(self.cdp.observed_non_read_requests())
    }

    fn stop_if_non_read_request_observed(&self, before: u64) -> Result<(), DriverError> {
        if self.cdp.observed_non_read_requests() > before {
            return Err(DriverError("A non-GET/HEAD request or outgoing WebSocket frame was observed during filling; text entry stopped. Its purpose and delivery are unknown".into()));
        }
        Ok(())
    }

    /// Stabilize the editing caret only. No focus, field value, DOM node or
    /// animation is changed, and all pixels are still compared exactly. The
    /// inspector sheet is owned by this driver; restoration is checked after every attempt.
    /// A caret inside a separately styled shadow root or another frame can
    /// remain visible; freshness then stays conservative rather than masking it.
    fn prepare_capture_styles(&mut self) -> Result<String, DriverError> {
        let tree = self
            .cdp
            .call("Page.getFrameTree", json!({}))
            .map_err(derr)?;
        let frame = tree
            .pointer("/frameTree/frame")
            .ok_or_else(|| DriverError("Capture document unavailable".into()))?;
        let frame_id = frame["id"]
            .as_str()
            .ok_or_else(|| DriverError("Capture frame unavailable".into()))?
            .to_owned();
        let loader = frame["loaderId"].as_str().unwrap_or("").to_owned();
        if self
            .capture_sheet
            .as_ref()
            .is_none_or(|(id, document, _)| *id != frame_id || *document != loader)
        {
            self.cdp.call("CSS.enable", json!({})).map_err(derr)?;
            let sheet = self
                .cdp
                .call(
                    "CSS.createStyleSheet",
                    json!({"frameId":frame_id,"force":true}),
                )
                .map_err(derr)?;
            let id = sheet["styleSheetId"]
                .as_str()
                .ok_or_else(|| DriverError("Capture stylesheet unavailable".into()))?
                .to_owned();
            self.capture_sheet = Some((frame_id, loader, id));
        }
        let sheet = self
            .capture_sheet
            .as_ref()
            .ok_or_else(|| DriverError("Capture stylesheet unavailable".into()))?
            .2
            .clone();
        Ok(sheet)
    }

    fn with_capture_styles<T>(
        &mut self,
        capture: impl FnOnce(&mut Self) -> Result<T, DriverError>,
    ) -> Result<T, DriverError> {
        let sheet = self.prepare_capture_styles()?;
        let applied = self
            .cdp
            .call(
                "CSS.setStyleSheetText",
                json!({"styleSheetId":sheet,"text":"* { caret-color: transparent !important; }"}),
            )
            .map_err(derr);
        let captured = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            applied.and_then(|_| capture(self))
        }));
        let restored = self
            .cdp
            .call(
                "CSS.setStyleSheetText",
                json!({"styleSheetId":sheet,"text":""}),
            )
            .map_err(derr);
        if let Err(error) = restored {
            return Err(DriverError(format!(
                "Capture caret rendering could not be restored: {error}"
            )));
        }
        match captured {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    fn capture_png(&mut self, parameters: Value) -> Result<Value, DriverError> {
        self.with_capture_styles(|driver| {
            driver
                .cdp
                .call("Page.captureScreenshot", parameters)
                .map_err(derr)
        })
    }

    /// Capture the current viewport for the candidate's review.
    pub fn screenshot_base64(&mut self) -> Result<String, DriverError> {
        if self.captured_scope.is_some() {
            return self.screenshot_captured_frame();
        }
        let result = self.capture_png(json!({"format":"png"}))?;
        let data = result
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("browser returned no screenshot".into()))?;
        if data.len() > 8 * 1024 * 1024 {
            return Err(DriverError("browser screenshot is too large".into()));
        }
        Ok(data.to_owned())
    }

    /// Temporarily bound individual browser protocol calls during a review operation.
    pub fn with_review_timeout<T>(&mut self, operation: impl FnOnce(&mut Self) -> T) -> T {
        let previous = self.cdp.timeout;
        self.cdp.timeout = Duration::from_secs(1);
        let result = operation(self);
        self.cdp.timeout = previous;
        result
    }

    /// Scroll a known field into view without focusing, changing or submitting it.
    pub fn scroll_field_into_view(&mut self, selector: &str) -> Result<(), DriverError> {
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Field is no longer available for visual review".into()))?;
        self.cdp
            .call("DOM.scrollIntoViewIfNeeded", json!({"nodeId":node}))
            .map_err(derr)?;
        // Allow a short bounded interval for immediate site scroll handlers.
        self.cdp.pump(Duration::from_millis(150));
        // The document or frame may have changed while the scroll was running.
        if self.node_id(selector)?.is_none() {
            return Err(DriverError(
                "Field changed during visual review; inspect again".into(),
            ));
        }
        Ok(())
    }

    /// Center a choice before visual review to leave room for its popup.
    /// This scrolls only; callers must recheck the page and prior answers.
    pub fn center_field_for_review(&mut self, selector: &str) -> Result<(), DriverError> {
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Field is no longer available for visual review".into()))?;
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Field is no longer available for visual review".into()))?
            .to_owned();
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function() { if (!this.isConnected) return false; this.scrollIntoView({block:'center',inline:'nearest',behavior:'instant'}); return this.isConnected; }","returnByValue":true}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let result = result.map_err(derr)?;
        released.map_err(derr)?;
        if result.get("exceptionDetails").is_some()
            || result.pointer("/result/value").and_then(Value::as_bool) != Some(true)
        {
            return Err(DriverError(
                "Could not position the field for visual review".into(),
            ));
        }
        self.cdp.pump(Duration::from_millis(150));
        if self.node_id(selector)?.is_none() {
            return Err(DriverError(
                "Field changed during visual review; inspect again".into(),
            ));
        }
        Ok(())
    }

    fn section_control_nodes(
        &mut self,
        section: &str,
        deadline: Instant,
    ) -> Result<(usize, Vec<(u64, u64)>), DriverError> {
        let root = self.document_with_frames(false)?;
        let root_id = root
            .get("nodeId")
            .and_then(Value::as_u64)
            .ok_or_else(|| DriverError("Document root unavailable".into()))?;
        let sections = self
            .cdp
            .call(
                "DOM.querySelectorAll",
                json!({"nodeId":root_id,"selector":section}),
            )
            .map_err(derr)?;
        let ids = sections
            .get("nodeIds")
            .and_then(Value::as_array)
            .ok_or_else(|| DriverError("Section lookup unavailable".into()))?;
        if ids.len() != 1 {
            return Err(DriverError(
                "Application section is missing or ambiguous".into(),
            ));
        }
        let section_id = ids[0]
            .as_u64()
            .ok_or_else(|| DriverError("Invalid section node".into()))?;
        let controls = self.cdp.call("DOM.querySelectorAll", json!({"nodeId":section_id,"selector":"input:not([type=hidden]):not([type=file]):not([type=submit]):not([type=button]):not([type=image]), textarea, select"})).map_err(derr)?;
        let nodes = controls
            .get("nodeIds")
            .and_then(Value::as_array)
            .ok_or_else(|| DriverError("Section controls unavailable".into()))?;
        let total = nodes.len();
        let mut result = Vec::new();
        for node in nodes.iter().take(12) {
            if Instant::now() >= deadline {
                return Err(DriverError("Form discovery time limit reached".into()));
            }
            let id = node
                .as_u64()
                .ok_or_else(|| DriverError("Invalid control node".into()))?;
            let description = self
                .cdp
                .call("DOM.describeNode", json!({"nodeId":id}))
                .map_err(derr)?;
            let backend = description
                .pointer("/node/backendNodeId")
                .and_then(Value::as_u64)
                .ok_or_else(|| DriverError("Control identity unavailable".into()))?;
            result.push((id, backend));
        }
        Ok((total, result))
    }

    /// Reveal a bounded number of ordinary controls inside one identified section.
    /// This scrolls only; it never focuses, types, attaches or activates controls.
    pub fn reveal_section_controls(&mut self, section: &str) -> Result<(usize, bool), DriverError> {
        let before = self.current_url()?;
        let deadline = Instant::now() + Duration::from_secs(12);
        // Only the explicit custom-control advisory is compatible with this
        // optional scroll. Unknown walls stop the sweep as well.
        let has_wall = |live: &LiveForm| {
            live.walls.iter().any(|wall| {
                wall != "a custom control needs manual review"
                    && !wall.starts_with("a custom control needs manual review: ")
            })
        };
        if has_wall(&self.inspect_current()?) {
            return Err(DriverError(
                "The page requires manual review before form discovery".into(),
            ));
        }
        let (total, mut nodes) = self.section_control_nodes(section, deadline)?;
        let identities: Vec<_> = nodes.iter().map(|(_, backend)| *backend).collect();
        let truncated = total > 12;
        let mut scrolled = 0;
        while scrolled < total.min(12) {
            if Instant::now() >= deadline {
                return Err(DriverError("Form discovery time limit reached".into()));
            }
            let node = nodes[scrolled].0;
            self.cdp
                .call("DOM.scrollIntoViewIfNeeded", json!({"nodeId":node}))
                .map_err(derr)?;
            self.cdp.pump(Duration::from_millis(120));
            if self.current_url()? != before {
                return Err(DriverError("The page changed during form discovery".into()));
            }
            scrolled += 1;
            if scrolled % 4 == 0 {
                let live = self.inspect_current()?;
                if has_wall(&live) {
                    return Err(DriverError(
                        "The page requires manual review during form discovery".into(),
                    ));
                }
                if scrolled < total.min(12) {
                    // DOM inspection can invalidate frontend node IDs. Requery
                    // before continuing and refuse a changed control inventory.
                    let (current_total, current_nodes) =
                        self.section_control_nodes(section, deadline)?;
                    if current_total != total
                        || current_nodes
                            .iter()
                            .map(|(_, backend)| *backend)
                            .ne(identities.iter().copied())
                    {
                        return Err(DriverError("Form controls changed during discovery".into()));
                    }
                    nodes = current_nodes;
                }
            }
        }
        let (current_total, current_nodes) = self.section_control_nodes(section, deadline)?;
        if current_total != total
            || current_nodes
                .iter()
                .map(|(_, backend)| *backend)
                .ne(identities.iter().copied())
        {
            return Err(DriverError("Form controls changed during discovery".into()));
        }
        Ok((scrolled, truncated))
    }

    /// Capture a bounded area around a known field after it has been scrolled
    /// into view. The crop may omit a distant label or obscured content.
    pub fn screenshot_field_context(&mut self, selector: &str) -> Result<String, DriverError> {
        self.screenshot_element(selector, true)
    }

    /// Capture only a control's bounding box, reducing unrelated nearby text.
    /// Overlays and content clipped by an ancestor can still obscure the control.
    pub fn screenshot_control(&mut self, selector: &str) -> Result<String, DriverError> {
        self.screenshot_element(selector, false)
    }

    /// Capture one uniquely identified rendered option without scrolling or clicking.
    pub fn screenshot_custom_choice_option(
        &mut self,
        selector: &str,
        label: &str,
    ) -> Result<String, DriverError> {
        let (node, _) = self.custom_choice_option_node(selector, label)?;
        self.screenshot_node(node, false, true)
    }

    fn screenshot_element(&mut self, selector: &str, context: bool) -> Result<String, DriverError> {
        if self.captured_scope.is_some() {
            return self.screenshot_captured_field(selector, context);
        }
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Field is unavailable for visual capture".into()))?;
        self.screenshot_node(node, context, false)
    }

    fn screenshot_node(
        &mut self,
        node: u64,
        context: bool,
        allow_larger_element: bool,
    ) -> Result<String, DriverError> {
        let model = self
            .cdp
            .call("DOM.getBoxModel", json!({"nodeId":node}))
            .map_err(derr)?;
        let quad = model
            .pointer("/model/border")
            .and_then(Value::as_array)
            .ok_or_else(|| DriverError("Field has no capture bounds".into()))?;
        let numbers: Vec<f64> = quad.iter().filter_map(Value::as_f64).collect();
        self.screenshot_bounds(&numbers, context, allow_larger_element)
    }

    fn screenshot_bounds(
        &mut self,
        numbers: &[f64],
        context: bool,
        allow_larger_element: bool,
    ) -> Result<String, DriverError> {
        if numbers.len() != 8 || numbers.iter().any(|number| !number.is_finite()) {
            return Err(DriverError("Invalid field capture bounds".into()));
        }
        let x = numbers
            .iter()
            .step_by(2)
            .copied()
            .fold(f64::INFINITY, f64::min);
        let y = numbers
            .iter()
            .skip(1)
            .step_by(2)
            .copied()
            .fold(f64::INFINITY, f64::min);
        let metrics = self
            .cdp
            .call("Page.getLayoutMetrics", json!({}))
            .map_err(derr)?;
        let viewport = metrics
            .get("cssVisualViewport")
            .ok_or_else(|| DriverError("Viewport unavailable for visual capture".into()))?;
        let page_x = viewport
            .get("pageX")
            .and_then(Value::as_f64)
            .ok_or_else(|| DriverError("Viewport position unavailable".into()))?;
        let page_y = viewport
            .get("pageY")
            .and_then(Value::as_f64)
            .ok_or_else(|| DriverError("Viewport position unavailable".into()))?;
        let viewport_width = viewport
            .get("clientWidth")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && *value > 0.0)
            .ok_or_else(|| DriverError("Viewport width unavailable".into()))?;
        let viewport_height = viewport
            .get("clientHeight")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && *value > 0.0)
            .ok_or_else(|| DriverError("Viewport height unavailable".into()))?;
        let viewport_right = page_x + viewport_width;
        let viewport_bottom = page_y + viewport_height;
        let (page_width, page_height) = self.page_dimensions()?;
        let right = numbers
            .iter()
            .step_by(2)
            .copied()
            .fold(f64::NEG_INFINITY, f64::max)
            + page_x;
        let bottom = numbers
            .iter()
            .skip(1)
            .step_by(2)
            .copied()
            .fold(f64::NEG_INFINITY, f64::max)
            + page_y;
        if !page_x.is_finite()
            || !page_y.is_finite()
            || x < 0.0
            || y < 0.0
            || right > viewport_right
            || bottom > viewport_bottom
            || right <= x + page_x
            || bottom <= y + page_y
        {
            return Err(DriverError("Control is outside the visible viewport; scroll it into view before visual capture".into()));
        }
        let (left, top, width, height, scale) = if context {
            let left = context_left_edge(x + page_x, right, page_x, page_width);
            // Include more section context when the field is short, while
            // preserving the previous 70-pixel margin for tall controls.
            let field_height = bottom - (y + page_y);
            let headroom = (400.0 - field_height - 60.0).clamp(70.0, 160.0);
            let top = (y + page_y - headroom)
                .max(page_y)
                .max(0.0)
                .min(page_height);
            let width = (right + 60.0 - left)
                .clamp(220.0, 720.0)
                .min(page_width - left)
                .min(viewport_right - left);
            let height = (bottom + 60.0 - top)
                .clamp(80.0, 400.0)
                .min(page_height - top)
                .min(viewport_bottom - top);
            (left, top, width, height, 1)
        } else {
            let left = x + page_x;
            let top = y + page_y;
            (
                left,
                top,
                (right - left).min(if allow_larger_element { 1600.0 } else { 720.0 }),
                (bottom - top).min(if allow_larger_element { 1080.0 } else { 400.0 }),
                if allow_larger_element && (right - left > 720.0 || bottom - top > 400.0) {
                    1
                } else {
                    2
                },
            )
        };
        if left > x + page_x || top > y + page_y || right > left + width || bottom > top + height {
            return Err(DriverError(
                "Field is too large for one visual capture".into(),
            ));
        }
        if !left.is_finite() || !top.is_finite() || width <= 0.0 || height <= 0.0 {
            return Err(DriverError("Field capture is outside the page".into()));
        }
        let captured = self.capture_png(json!({"format":"png","captureBeyondViewport":true,"clip":{"x":left,"y":top,"width":width,"height":height,"scale":scale}}))?;
        let data = captured
            .get("data")
            .and_then(Value::as_str)
            .filter(|data| data.len() <= 4 * 1024 * 1024)
            .ok_or_else(|| DriverError("Field screenshot unavailable or too large".into()))?;
        Ok(data.to_owned())
    }

    /// Attach a caller-selected local file to a known form control.
    pub fn attach_local_file(
        &mut self,
        selector: &str,
        path: &std::path::Path,
    ) -> Result<(), DriverError> {
        self.attach_local_files(selector, &[path.to_owned()])
    }

    /// Read whether a current file input permits more than one file.
    pub fn file_input_allows_multiple(&mut self, selector: &str) -> Result<bool, DriverError> {
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("File field not found".into()))?;
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Cannot inspect file field".into()))?;
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function() { return this.isConnected && this instanceof HTMLInputElement && this.type === 'file' && !this.matches(':disabled') && !this.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]') ? this.multiple : null; }","returnByValue":true})).map_err(derr)?;
        if result.get("exceptionDetails").is_some() {
            return Err(DriverError("Cannot inspect file field".into()));
        }
        result
            .pointer("/result/value")
            .and_then(Value::as_bool)
            .ok_or_else(|| DriverError("File field is unavailable; review it manually".into()))
    }

    /// Attach a bounded, explicitly reviewed set of files to one input.
    pub fn attach_local_files(
        &mut self,
        selector: &str,
        paths: &[std::path::PathBuf],
    ) -> Result<(), DriverError> {
        self.attach_local_files_inner(selector, paths, None)
    }

    /// Refuse to overwrite a selection that differs from the reviewed prior set.
    pub fn attach_local_files_if_unchanged(
        &mut self,
        selector: &str,
        paths: &[std::path::PathBuf],
        previous: &[std::path::PathBuf],
    ) -> Result<(), DriverError> {
        self.attach_local_files_inner(selector, paths, Some(previous))
    }

    fn attach_local_files_inner(
        &mut self,
        selector: &str,
        paths: &[std::path::PathBuf],
        previous: Option<&[std::path::PathBuf]>,
    ) -> Result<(), DriverError> {
        if paths.is_empty() || paths.len() > 5 {
            return Err(DriverError("Choose one to five files".into()));
        }
        if paths.len() > 1 && !self.file_input_allows_multiple(selector)? {
            return Err(DriverError(
                "This file field does not allow multiple files".into(),
            ));
        }
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("File field not found".into()))?;
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Cannot inspect file field".into()))?;
        let available = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function() { return (this.isConnected && !this.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]')) && this instanceof HTMLInputElement && this.type === 'file' && !this.matches(':disabled'); }","returnByValue":true})).map_err(derr)?;
        if available.get("exceptionDetails").is_some()
            || available.pointer("/result/value").and_then(Value::as_bool) != Some(true)
        {
            return Err(DriverError(
                "File field is unavailable; review it manually".into(),
            ));
        }
        let accept = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function() { return (this.isConnected && !this.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]')) && !this.matches(':disabled') ? this.accept : null; }","returnByValue":true})).map_err(derr)?;
        let accept = accept
            .pointer("/result/value")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                DriverError("Cannot inspect the file field's declared formats".into())
            })?;
        let mut names = std::collections::HashSet::new();
        for path in paths {
            let metadata = std::fs::metadata(path)
                .map_err(|_| DriverError("Cannot read an attachment".into()))?;
            if !metadata.is_file() {
                return Err(DriverError("Attachment is not a regular file".into()));
            }
            let filename = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| DriverError("Cannot inspect the attachment filename".into()))?;
            if !names.insert(filename.to_lowercase()) {
                return Err(DriverError(
                    "Choose files with distinct attachment names".into(),
                ));
            }
            check_file_accept(accept, filename)?;
        }
        if let Some(previous) = previous {
            if previous.is_empty() {
                let empty = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function() { return this.isConnected && this instanceof HTMLInputElement && this.type === 'file' && !this.matches(':disabled') && !this.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]') && this.files && this.files.length === 0; }","returnByValue":true})).map_err(derr)?;
                if empty.get("exceptionDetails").is_some()
                    || empty.pointer("/result/value").and_then(Value::as_bool) != Some(true)
                {
                    return Err(DriverError("The file field already contains an unreviewed selection; inspect it manually".into()));
                }
            } else {
                self.verify_local_files(selector, previous).map_err(|_| {
                    DriverError("The file selection changed; inspect it manually".into())
                })?;
            }
        }
        // Verification obtains a fresh document and invalidates earlier CDP node IDs.
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("File field not found".into()))?;
        self.cdp
            .call(
                "DOM.setFileInputFiles",
                json!({ "files": paths.iter().map(|path| path.to_string_lossy()).collect::<Vec<_>>(), "nodeId": node }),
            )
            .map_err(derr)?;
        self.verify_local_files(selector, paths)
    }

    /// Check whether the selected input still holds the staged local file.
    pub fn verify_local_file(
        &mut self,
        selector: &str,
        path: &std::path::Path,
    ) -> Result<(), DriverError> {
        self.verify_local_files(selector, &[path.to_owned()])
    }

    pub fn verify_local_files(
        &mut self,
        selector: &str,
        paths: &[std::path::PathBuf],
    ) -> Result<(), DriverError> {
        if paths.is_empty() || paths.len() > 5 {
            return Err(DriverError("Invalid attachment set".into()));
        }
        let mut expected = Vec::with_capacity(paths.len());
        for path in paths {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| {
                    DriverError("File name cannot be represented in the browser".into())
                })?;
            let size = std::fs::metadata(path)
                .map_err(|e| DriverError(format!("Cannot read attachment: {e}")))?
                .len();
            // Bound memory in both the host and the browser when hashing selected files.
            if size > 10 * 1024 * 1024 {
                return Err(DriverError("Attachment is too large to verify".into()));
            }
            use std::io::Read;
            let file = std::fs::File::open(path)
                .map_err(|_| DriverError("Cannot read attachment".into()))?;
            let mut bytes = Vec::new();
            file.take(10 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| DriverError("Cannot read attachment".into()))?;
            if bytes.len() as u64 != size {
                return Err(DriverError("Attachment changed during verification".into()));
            }
            expected.push(
                json!({"name":name,"size":size,"sha256":waypoint_domain::sha256_hex(&bytes)}),
            );
        }
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("File field not found".into()))?;
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Cannot inspect file field".into()))?;
        let retained = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"async function(expected) { const matches = () => this.isConnected && !this.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]') && this instanceof HTMLInputElement && this.type === 'file' && !this.matches(':disabled') && (expected.length === 1 || this.multiple) && this.files && this.files.length === expected.length && expected.every((file, i) => this.files[i].name === file.name && this.files[i].size === file.size); if (!matches() || !globalThis.crypto?.subtle) return false; try { const files = Array.from(this.files); for (let i = 0; i < files.length; i++) { const digest = await crypto.subtle.digest('SHA-256', await files[i].arrayBuffer()); const hex = Array.from(new Uint8Array(digest), byte => byte.toString(16).padStart(2, '0')).join(''); if (hex !== expected[i].sha256) return false; } return matches() && files.every((file, i) => this.files[i] === file); } catch (_) { return false; } }","arguments":[{"value":expected}],"awaitPromise":true,"returnByValue":true})).map_err(derr)?;
        if retained.get("exceptionDetails").is_some()
            || retained.pointer("/result/value").and_then(Value::as_bool) != Some(true)
        {
            return Err(DriverError(
                "File was not retained by the field; review the attachment manually".into(),
            ));
        }
        let accept = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function() { return (this.isConnected && !this.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]')) && !this.matches(':disabled') ? this.accept : null; }","returnByValue":true})).map_err(derr)?;
        let accept = accept
            .pointer("/result/value")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                DriverError("Cannot inspect the file field's declared formats".into())
            })?;
        for path in paths {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| DriverError("Cannot inspect the attachment filename".into()))?;
            check_file_accept(accept, name)?;
        }
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
        self.screenshot_tile(0.0, y, width, height)
    }

    /// Capture a bounded two-dimensional page tile without navigating or clicking.
    pub fn screenshot_tile(
        &mut self,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    ) -> Result<String, DriverError> {
        if self.captured_scope.is_some() {
            return Err(DriverError("Review embedded fields individually; full document capture is unavailable in this view".into()));
        }
        if !x.is_finite()
            || x < 0.0
            || x > 4096.0
            || !width.is_finite()
            || !y.is_finite()
            || !height.is_finite()
            || width <= 0.0
            || width > 2048.0
            || x + width > 4096.0
            || y < 0.0
            || height <= 0.0
            || height > 2000.0
        {
            return Err(DriverError(
                "Screenshot segment exceeds capture limits".into(),
            ));
        }
        let result = self.capture_png(json!({"format":"png","captureBeyondViewport":true,"clip":{"x":x,"y":y,"width":width,"height":height,"scale":1}}))?;
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
        self.document_with_frames(false)
    }

    fn document_with_frames(&mut self, include_frames: bool) -> Result<Value, DriverError> {
        let doc = self
            .cdp
            .call(
                "DOM.getDocument",
                json!({ "depth": -1, "pierce": include_frames }),
            )
            .map_err(derr)?;
        let root = doc.get("root").cloned().unwrap_or(Value::Null);
        self.root_node = root.get("nodeId").and_then(|v| v.as_u64());
        Ok(root)
    }

    fn node_id(&mut self, selector: &str) -> Result<Option<u64>, DriverError> {
        let selector = self.decode_captured_selector(selector)?;
        let selector = selector.as_ref();
        let root = self.document_with_frames(
            selector.starts_with("waypoint-frame:") || selector.starts_with("waypoint-shadow:"),
        )?;
        let root_id = root
            .get("nodeId")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| DriverError("page has no root node".into()))?;
        let mut document = root;
        let mut root_id = root_id;
        let mut inner = selector;
        let mut depth = 0;
        loop {
            let (scope, shadow) = if let Some(scope) = inner.strip_prefix("waypoint-frame:") {
                (scope, false)
            } else if let Some(scope) = inner.strip_prefix("waypoint-shadow:") {
                (scope, true)
            } else {
                break;
            };
            depth += 1;
            if depth > 4 {
                return Err(DriverError(
                    "Embedded field exceeds the scope depth limit".into(),
                ));
            }
            let mut parts = scope.splitn(3, ':');
            let owner = parts.next().and_then(|s| s.parse::<u64>().ok());
            let expected = parts.next().and_then(|s| s.parse::<u64>().ok());
            let next = parts.next().filter(|s| !s.is_empty());
            let (Some(owner), Some(expected), Some(next)) = (owner, expected, next) else {
                return Err(DriverError("Invalid embedded field selector".into()));
            };
            let mut candidate = None;
            walk(&document, false, &mut |node, _| {
                if node.get("backendNodeId").and_then(Value::as_u64) == Some(owner) {
                    candidate = Some(node.clone());
                }
                true
            });
            let Some(host) = candidate else {
                return Ok(None);
            };
            let Some(host_id) = host.get("nodeId").and_then(Value::as_u64) else {
                return Ok(None);
            };
            let child = if shadow {
                if !self.open_visible_shadow_host(host_id)? {
                    return Ok(None);
                }
                host.get("shadowRoots")
                    .and_then(Value::as_array)
                    .and_then(|roots| {
                        roots.iter().find(|root| {
                            root.get("shadowRootType").and_then(Value::as_str) == Some("open")
                                && root.get("backendNodeId").and_then(Value::as_u64)
                                    == Some(expected)
                        })
                    })
            } else {
                if !self.accessible_visible_frame(host_id)? {
                    return Ok(None);
                }
                host.get("contentDocument").filter(|child| {
                    child.get("backendNodeId").and_then(Value::as_u64) == Some(expected)
                })
            };
            let Some(child) = child else { return Ok(None) };
            let Some(child_id) = child.get("nodeId").and_then(Value::as_u64) else {
                return Ok(None);
            };
            root_id = child_id;
            document = child.clone();
            inner = next;
        }
        let selector = inner;
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

    /// Retry bounded read-only observations if the page replaced a node while it was
    /// being inspected. The failed observation may have missed a review wall.
    fn inspect_with_retry(&mut self) -> Result<LiveForm, DriverError> {
        let first = self.inspect_current_once();
        retry_changed_inspection(first, |attempt| {
            self.cdp
                .pump(Duration::from_millis([100, 250, 500][attempt]));
            self.inspect_current_once()
        })
    }

    fn inspect_current_once(&mut self) -> Result<LiveForm, DriverError> {
        let current_url = self.current_url()?;
        let (schema, mut walls) = self.read_form(&current_url)?;
        if current_url.starts_with("https://") || current_url.starts_with("http://") {
            let frame_tree = self.cdp.call("Page.getFrameTree", json!({})).ok();
            let frame = frame_tree
                .as_ref()
                .and_then(|tree| tree.pointer("/frameTree/frame/id"))
                .and_then(Value::as_str);
            let loader = frame_tree
                .as_ref()
                .and_then(|tree| tree.pointer("/frameTree/frame/loaderId"))
                .and_then(Value::as_str);
            let status = frame.zip(loader).and_then(|(frame, loader)| {
                let observed = self.cdp.events.iter().rev().find_map(|(name, event)| {
                    (name == "Network.responseReceived"
                        && event.pointer("/params/frameId").and_then(Value::as_str) == Some(frame)
                        && event.pointer("/params/loaderId").and_then(Value::as_str)
                            == Some(loader))
                    .then(|| event.pointer("/params/status").and_then(Value::as_u64))
                    .flatten()
                });
                if let Some(code) = observed {
                    self.document_status = Some((frame.to_owned(), loader.to_owned(), code));
                    Some(code)
                } else {
                    self.document_status
                        .as_ref()
                        .filter(|(saved_frame, saved_loader, _)| {
                            saved_frame == frame && saved_loader == loader
                        })
                        .map(|(_, _, code)| *code)
                }
            });
            match status {
                Some(code @ 400..=599) => walls.push(format!("The page returned HTTP {code}; manual review is required")),
                Some(200..=299) => (),
                _ => walls.push("The main page response status could not be verified; manual review is required".into()),
            }
        }
        Ok(LiveForm {
            origin: schema.origin.clone(),
            schema,
            walls,
        })
    }

    /// Inspect rendered visibility and derive a structural selector without changing the page.
    fn control_visibility_and_path(
        &mut self,
        node: u64,
    ) -> Result<(bool, String, bool, bool, bool), DriverError> {
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                DriverError("Form field disappeared during visibility inspection".into())
            })?
            .to_owned();
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function() {
            if (!this.isConnected) return null;
            const visibleNode = this instanceof HTMLInputElement && this.type === 'file' ? this.parentElement : this;
            if (!visibleNode) return null;
            const style = getComputedStyle(visibleNode);
            let needsReview = false, outsideScrollArea = false, dialogObscured = false;
            const rect = visibleNode.getBoundingClientRect();
            let left = rect.left, right = rect.right, top = rect.top, bottom = rect.bottom;
            for (let node = visibleNode, depth = 0; node; node = node.parentElement) {
                if (++depth > 64) return null;
                const ancestorStyle = getComputedStyle(node);
                if (Number(ancestorStyle.opacity) === 0) needsReview = true;
                if (node !== visibleNode) {
                    const box = node.getBoundingClientRect();
                    const centerX = (rect.left + rect.right) / 2, centerY = (rect.top + rect.bottom) / 2;
                    if (['auto', 'scroll'].includes(ancestorStyle.overflowX) && (centerX < box.left + node.clientLeft || centerX >= box.left + node.clientLeft + node.clientWidth)) outsideScrollArea = true;
                    if (['auto', 'scroll'].includes(ancestorStyle.overflowY) && (centerY < box.top + node.clientTop || centerY >= box.top + node.clientTop + node.clientHeight)) outsideScrollArea = true;
                    if (['hidden', 'clip'].includes(ancestorStyle.overflowX)) {
                        left = Math.max(left, box.left + node.clientLeft);
                        right = Math.min(right, box.left + node.clientLeft + node.clientWidth);
                    }
                    if (['hidden', 'clip'].includes(ancestorStyle.overflowY)) {
                        top = Math.max(top, box.top + node.clientTop);
                        bottom = Math.min(bottom, box.top + node.clientTop + node.clientHeight);
                    }
                }
            }
            // A partly hidden answer cannot be visually reviewed in full.
            // Allow subpixel layout rounding without treating it as clipping.
            if (rect.width > 0 && rect.height > 0 && (right <= left || bottom <= top || left > rect.left + 0.5 || right < rect.right - 0.5 || top > rect.top + 0.5 || bottom < rect.bottom - 0.5)) needsReview = true;
            // Hit-test only an in-viewport center. Offscreen and unscrolled controls
            // are checked again after focusing or scrolling.
            const x = (rect.left + rect.right) / 2, y = (rect.top + rect.bottom) / 2;
            if (!needsReview && !outsideScrollArea && (this instanceof HTMLTextAreaElement || this instanceof HTMLSelectElement || (this instanceof HTMLInputElement && this.type !== 'file') || this.isContentEditable) && rect.width > 0 && rect.height > 0 && x >= 0 && y >= 0 && x < innerWidth && y < innerHeight) {
                const root = visibleNode.getRootNode();
                // Chromium rounds fractional hit-test coordinates. A center less
                // than half a pixel from the viewport edge can round outside it.
                // Sample an interior viewport pixel, still inside this control.
                const hitX = Math.min(x, innerWidth - 1), hitY = Math.min(y, innerHeight - 1);
                const hit = hitX >= rect.left && hitX < rect.right && hitY >= rect.top && hitY < rect.bottom && typeof root.elementFromPoint === 'function' ? root.elementFromPoint(hitX, hitY) : null;
                if (!hit || (hit !== visibleNode && !visibleNode.contains(hit))) { needsReview = true; dialogObscured = !!hit?.closest('[role="dialog" i], dialog, [aria-modal="true" i]'); }
            }
            const visible = visibleNode.getClientRects().length > 0 && style.visibility !== 'hidden' && style.visibility !== 'collapse' && !needsReview;
            const parts = []; let current = this;
            while (current && current.nodeType === 1) {
                if (parts.length >= 64 || !current.localName) return null;
                let position = 1, sibling = current.previousElementSibling;
                while (sibling) { if (++position > 5000) return null; sibling = sibling.previousElementSibling; }
                parts.unshift(CSS.escape(current.localName) + ':nth-child(' + position + ')');
                current = current.parentElement;
            }
            const path = parts.join(' > ');
            const available = !this.matches(':disabled') && this.readOnly !== true && !this.closest('[aria-disabled="true" i], [aria-readonly="true" i]');
            return path.length <= 4096 ? {visible, path, available, needsReview, dialogObscured} : null;
        }"#,"returnByValue":true}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let result = result.map_err(derr)?;
        released.map_err(derr)?;
        let value = result.pointer("/result/value").ok_or_else(|| {
            DriverError("Could not inspect field visibility; manual review is needed".into())
        })?;
        let visible = value
            .get("visible")
            .and_then(Value::as_bool)
            .ok_or_else(|| {
                DriverError("Field visibility is unavailable; manual review is needed".into())
            })?;
        let path = value
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty())
            .ok_or_else(|| {
                DriverError("Field selector is unavailable; manual review is needed".into())
            })?;
        let available = value
            .get("available")
            .and_then(Value::as_bool)
            .ok_or_else(|| {
                DriverError("Field availability is unavailable; manual review is needed".into())
            })?;
        let needs_review = value
            .get("needsReview")
            .and_then(Value::as_bool)
            .ok_or_else(|| {
                DriverError("Field visibility is unavailable; manual review is needed".into())
            })?;
        let dialog_obscured = value
            .get("dialogObscured")
            .and_then(Value::as_bool)
            .ok_or_else(|| {
                DriverError("Field visibility is unavailable; manual review is needed".into())
            })?;
        Ok((
            visible,
            path.to_owned(),
            available,
            needs_review,
            dialog_obscured,
        ))
    }

    /// Boxless text containers have no client rectangle; reject clipping and
    /// hidden ancestors before considering their rendered descendants.
    fn boxless_text_root_visible(&mut self, node: u64) -> Result<bool, DriverError> {
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Text container disappeared".into()))?;
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function() {
            if (!this.isConnected || getComputedStyle(this).display !== 'contents') return false;
            let node = this;
            for (let depth = 0; node; depth++, node = node.parentElement) {
                if (depth > 64) return false;
                const style = getComputedStyle(node);
                if (style.visibility === 'hidden' || style.visibility === 'collapse' || Number(style.opacity) === 0 ||
                    ['hidden','clip'].includes(style.overflowX) || ['hidden','clip'].includes(style.overflowY) ||
                    (style.display !== 'contents' && !node.getClientRects().length)) return false;
            }
            return true;
        }"#, "returnByValue":true}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let result = result.map_err(derr)?;
        released.map_err(derr)?;
        Ok(result
            .pointer("/result/value")
            .and_then(Value::as_bool)
            .unwrap_or(false))
    }

    /// Read only rendered heading text; DOM text can include hidden descendants.
    fn rendered_section_heading(&mut self, node: u64) -> Result<Option<String>, DriverError> {
        if !self.control_visibility_and_path(node)?.0 && !self.boxless_text_root_visible(node)? {
            return Ok(None);
        }
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Section heading disappeared".into()))?
            .to_owned();
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function() {
            if (!this.isConnected) return null;
            const boxless = getComputedStyle(this).display === 'contents';
            const walker = document.createTreeWalker(this, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT), parts = [];
            let text = walker.nextNode(), count = 0, size = 0, previousBlock = null, haveText = false;
            while (text) {
                if (++count > 512) return null;
                const isBreak = text.nodeType === Node.ELEMENT_NODE && text.tagName === 'BR';
                if (text.nodeType !== Node.TEXT_NODE && !isBreak) { text = walker.nextNode(); continue; }
                let parent = isBreak ? text : text.parentElement, visible = !!parent, block = null;
                for (let depth = 0; visible && parent; depth++, parent = parent.parentElement) {
                    if (depth > 64 || ['SCRIPT','STYLE','TEMPLATE','NOSCRIPT'].includes(parent.tagName)) { visible = false; break; }
                    const style = getComputedStyle(parent);
                    if (style.visibility === 'hidden' || style.visibility === 'collapse' || Number(style.opacity) === 0 || (style.display !== 'contents' && !parent.getClientRects().length)) { visible = false; break; }
                    if (boxless && (['hidden','clip'].includes(style.overflowX) || ['hidden','clip'].includes(style.overflowY))) return null;
                    if (parent === this) break;
                    if (!block && ['block','flow-root','flex','grid','list-item','table','table-row','table-cell'].includes(style.display)) block = parent;
                }
                if (visible) {
                    const value = isBreak ? ' ' : (text.nodeValue || '');
                    if (haveText && block !== previousBlock && (block || previousBlock)) { parts.push(' '); size++; }
                    size += value.length; if (size > 2000) return null;
                    parts.push(value); previousBlock = block; haveText = true;
                }
                text = walker.nextNode();
            }
            return parts.join('');
        }"#,"returnByValue":true}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let result = result.map_err(derr)?;
        released.map_err(derr)?;
        let Some(text) = result
            .pointer("/result/value")
            .and_then(Value::as_str)
            .filter(|text| text.len() <= 2000)
        else {
            return Ok(None);
        };
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        Ok((!text.is_empty() && text.len() <= 500).then_some(text))
    }

    /// Detect a visible required marker near one uniquely named placeholder field.
    fn nearby_required_marker(&mut self, node: u64) -> Result<Option<bool>, DriverError> {
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Field disappeared".into()))?;
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function() {
            if (!this.isConnected) return null;
            const name = (this.getAttribute('placeholder') || '').trim().replace(/\s+/g,' ').toLowerCase();
            if (!name || name.length > 200) return false;
            let parent = this.parentElement;
            for (let depth = 0; parent && depth < 4; depth++, parent = parent.parentElement) {
                const controls = parent.querySelectorAll('input:not([type=hidden]),select,textarea');
                if (controls.length > 1) {
                    const labels = parent.querySelectorAll('label');
                    if (labels.length > 16) return null;
                    for (const label of labels) {
                        if (!label.getClientRects().length || label.closest('[hidden], [aria-hidden="true" i]')) continue;
                        const style = getComputedStyle(label);
                        if (style.visibility !== 'visible' || Number(style.opacity) === 0) continue;
                        const text = (label.innerText || '').trim().replace(/\s+/g,' ');
                        if (text.endsWith('*') && text.slice(0,-1).trim().replace(/[:：]\s*$/,'').trim().toLowerCase() === name) return null;
                    }
                    break;
                }
                if (controls.length !== 1 || controls[0] !== this) continue;
                const labels = parent.querySelectorAll('label');
                if (labels.length > 16) return null;
                let matches = 0;
                for (const label of labels) {
                    const target = label.getAttribute('for');
                    if (target && target !== this.id) continue;
                    if (!label.getClientRects().length || label.closest('[hidden], [aria-hidden="true" i]')) continue;
                    const style = getComputedStyle(label);
                    if (style.visibility !== 'visible' || Number(style.opacity) === 0) continue;
                    const text = (label.innerText || '').trim().replace(/\s+/g,' ');
                    if (!text.endsWith('*')) continue;
                    const normalized = text.slice(0,-1).trim().replace(/[:：]\s*$/,'').trim().toLowerCase();
                    if (normalized === name) matches++;
                }
                if (matches > 1) return null;
                if (matches === 1) return true;
            }
            return false;
        }"#, "returnByValue":true}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let result = result.map_err(derr)?;
        released.map_err(derr)?;
        Ok(result.pointer("/result/value").and_then(Value::as_bool))
    }

    /// Read rendered label text, excluding controls and hidden descendants.
    fn rendered_form_label(&mut self, node: u64) -> Result<Option<String>, DriverError> {
        if !self.control_visibility_and_path(node)?.0 && !self.boxless_text_root_visible(node)? {
            return Ok(None);
        }
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Label disappeared".into()))?;
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function() {
            if (!this.isConnected || this.tagName !== 'LABEL') return null;
            const boxless = getComputedStyle(this).display === 'contents';
            const walker = document.createTreeWalker(this, NodeFilter.SHOW_TEXT), parts = [];
            let text = walker.nextNode(), count = 0, size = 0;
            while (text) {
                if (++count > 512) return null;
                let parent = text.parentElement, visible = !!parent;
                for (let depth = 0; visible && parent; depth++, parent = parent.parentElement) {
                    if (depth > 64 || ['INPUT','TEXTAREA','SELECT','BUTTON','SCRIPT','STYLE','TEMPLATE','NOSCRIPT'].includes(parent.tagName) || (parent.getAttribute('aria-hidden') || '').toLowerCase() === 'true') { visible = false; break; }
                    const style = getComputedStyle(parent);
                    if (style.visibility === 'hidden' || style.visibility === 'collapse' || Number(style.opacity) === 0 || (style.display !== 'contents' && !parent.getClientRects().length)) { visible = false; break; }
                    if (boxless && (['hidden','clip'].includes(style.overflowX) || ['hidden','clip'].includes(style.overflowY))) return null;
                    if (parent === this) break;
                }
                if (visible) { const value = text.nodeValue || ''; size += value.length; if (size > 2000) return null; parts.push(value); }
                text = walker.nextNode();
            }
            const value = parts.join(' ').trim().replace(/\s+/g,' ');
            return value && value.length <= 500 ? value : null;
        }"#, "returnByValue":true}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let result = result.map_err(derr)?;
        released.map_err(derr)?;
        Ok(result
            .pointer("/result/value")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty() && text.len() <= 500)
            .map(str::to_owned))
    }

    /// Detect a common unannotated choice widget: one editable input, a hidden
    /// companion and a nearby list-like container. This is a conservative hint.
    fn unannotated_choice_hint(&mut self, node: u64) -> Result<Option<bool>, DriverError> {
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Field disappeared".into()))?;
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function() {
            if (!this.isConnected || !(this instanceof HTMLInputElement)) return null;
            let parent = this.parentElement;
            for (let depth = 0; parent && depth < 2; depth++, parent = parent.parentElement) {
                if (['FORM','BODY','FIELDSET'].includes(parent.tagName)) break;
                const editable = parent.querySelectorAll('input:not([type="hidden" i]),textarea,select');
                if (editable.length !== 1 || editable[0] !== this) break;
                if (!parent.querySelector('input[type="hidden" i]')) continue;
                const nodes = parent.querySelectorAll('[role],[class],[id]');
                if (nodes.length > 100) return null;
                for (const node of nodes) {
                    if (node === this || node.tagName === 'INPUT') continue;
                    const role = (node.getAttribute('role') || '').toLowerCase();
                    if (role === 'listbox' || role === 'option') return true;
                    const tokens = ((node.getAttribute('class') || '') + ' ' + (node.getAttribute('id') || '')).toLowerCase().split(/[^a-z0-9]+/);
                    if (tokens.some(token => ['dropdown','autocomplete','suggestion','suggestions','listbox'].includes(token))) return true;
                }
            }
            return false;
        }"#, "returnByValue":true}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let result = result.map_err(derr)?;
        released.map_err(derr)?;
        Ok(result.pointer("/result/value").and_then(Value::as_bool))
    }

    /// Find nearby rendered text for an otherwise unnamed textarea. This is
    /// advisory context, not an accessible association or permission to fill.
    fn rendered_unassociated_question(
        &mut self,
        node: u64,
        checkbox: bool,
    ) -> Result<Option<String>, DriverError> {
        if !self.control_visibility_and_path(node)?.0 {
            return Ok(None);
        }
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Field disappeared".into()))?;
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function() {
            if (!this.isConnected || !(this instanceof HTMLTextAreaElement || (arguments[0] === true && this instanceof HTMLInputElement && this.type === 'checkbox'))) return null;
            let branch = this;
            for (let depth = 0; depth < 4; depth++) {
                const parent = branch.parentElement;
                if (!parent || ['FORM','FIELDSET','SECTION','BODY'].includes(parent.tagName) || parent.children.length > 20) return null;
                const controls = parent.querySelectorAll('input:not([type="hidden" i]),textarea,select,button,[contenteditable="true"]');
                if (controls.length !== 1 || controls[0] !== this) return null;
                const candidates = [];
                for (const sibling of parent.children) {
                    if (sibling === branch || !sibling.getClientRects().length) continue;
                    const style = getComputedStyle(sibling);
                    if (style.visibility === 'hidden' || style.visibility === 'collapse' || Number(style.opacity) === 0 || sibling.getAttribute('aria-hidden') === 'true') continue;
                    const value = (sibling.innerText || '').trim().replace(/\s+/g,' ');
                    if (value) candidates.push(value);
                }
                if (candidates.length === 1 && candidates[0].length <= 500) return candidates[0];
                if (candidates.length > 1) return null;
                branch = parent;
            }
            return null;
        }"#, "arguments":[{"value":checkbox}], "returnByValue":true}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let result = result.map_err(derr)?;
        released.map_err(derr)?;
        Ok(result
            .pointer("/result/value")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty() && text.len() <= 500)
            .map(str::to_owned))
    }

    /// Read nearby visible context for an unnamed native choice group. None
    /// means no group was established; an empty string means a group was found
    /// but its question was unavailable or ambiguous. Both remain advisory.
    fn rendered_unassociated_choice_question(
        &mut self,
        node: u64,
        kind: &str,
    ) -> Result<Option<String>, DriverError> {
        if !self.control_visibility_and_path(node)?.0 {
            return Ok(None);
        }
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Choice disappeared".into()))?;
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function(kind) {
            if (!this.isConnected || !(this instanceof HTMLInputElement) || this.type !== kind || !['radio','checkbox'].includes(kind) || (!this.name && kind !== 'checkbox') || !this.form) return null;
            let branch = this, foundGroup = false;
            for (let depth = 0; depth < 8; depth++) {
                const parent = branch.parentElement;
                if (!parent || ['FORM','FIELDSET','SECTION','BODY'].includes(parent.tagName) || parent.children.length > 20) return foundGroup ? '' : null;
                let controls = Array.from(parent.querySelectorAll('input:not([type="hidden" i]),textarea,select,button,[contenteditable="true"]'));
                if (!this.name) {
                    controls = controls.filter(control => control === this || (control.getClientRects().length && getComputedStyle(control).visibility === 'visible'));
                    if (controls.length > 20 || controls.some(control => !(control instanceof HTMLInputElement) || control.type !== kind || control.form !== this.form)) return null;
                    if (controls.length >= 3) {
                        const others = controls.filter(control => control !== this);
                        const names = new Set(others.map(control => control.name));
                        if (names.size === 1 && !names.has('')) return '';
                        return null;
                    }
                    branch = parent;
                    continue;
                }
                if (controls.length > 20 || Array.from(controls).some(control => !(control instanceof HTMLInputElement) || control.type !== kind || control.name !== this.name || control.form !== this.form)) return foundGroup ? '' : null;
                if (controls.length >= 2) {
                    foundGroup = true;
                    const candidates = [];
                    for (const sibling of parent.children) {
                        if (sibling === branch || sibling.querySelector('input,textarea,select,button,[contenteditable="true"]') || !sibling.getClientRects().length) continue;
                        const style = getComputedStyle(sibling);
                        if (style.visibility === 'hidden' || style.visibility === 'collapse' || Number(style.opacity) === 0 || sibling.getAttribute('aria-hidden') === 'true') continue;
                        const value = (sibling.innerText || '').trim().replace(/\s+/g,' ');
                        if (value) candidates.push(value);
                    }
                    if (candidates.length === 1 && candidates[0].length <= 500) return candidates[0];
                    if (candidates.length > 1) return '';
                }
                branch = parent;
            }
            return foundGroup ? '' : null;
        }"#, "arguments":[{"value":kind}], "returnByValue":true}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let result = result.map_err(derr)?;
        released.map_err(derr)?;
        Ok(result
            .pointer("/result/value")
            .and_then(Value::as_str)
            .filter(|text| text.len() <= 500)
            .map(str::to_owned))
    }

    fn open_visible_shadow_host(&mut self, node: u64) -> Result<bool, DriverError> {
        if !self.control_visibility_and_path(node)?.0 {
            return Ok(false);
        }
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                DriverError("Web component disappeared; manual review is needed".into())
            })?
            .to_owned();
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function() { return this.isConnected && this.shadowRoot !== null; }","returnByValue":true}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let result = result.map_err(derr)?;
        released.map_err(derr)?;
        Ok(result.pointer("/result/value").and_then(Value::as_bool) == Some(true))
    }

    /// None means hidden; otherwise report whether the embedding document can access it.
    fn frame_access(&mut self, node: u64) -> Result<Option<bool>, DriverError> {
        if !self.control_visibility_and_path(node)?.0 {
            return Ok(None);
        }
        let model = self
            .cdp
            .call("DOM.getBoxModel", json!({"nodeId":node}))
            .map_err(derr)?;
        if model
            .pointer("/model/width")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            <= 0.0
            || model
                .pointer("/model/height")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                <= 0.0
        {
            return Ok(None);
        }
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                DriverError("Embedded frame disappeared; manual review is needed".into())
            })?
            .to_owned();
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function() { try { return this.isConnected && this.contentDocument !== null; } catch (_) { return false; } }","returnByValue":true}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let result = result.map_err(derr)?;
        released.map_err(derr)?;
        Ok(Some(
            result.pointer("/result/value").and_then(Value::as_bool) == Some(true),
        ))
    }

    fn accessible_visible_frame(&mut self, node: u64) -> Result<bool, DriverError> {
        Ok(self.frame_access(node)? == Some(true))
    }

    /// Inspect only a frame target identified by the current page's DOM. No
    /// fields, labels, values or URLs leave this call. None means unavailable.
    fn isolated_frame_has_controls(&mut self, frame: &str) -> Result<Option<bool>, DriverError> {
        let targets = self
            .cdp
            .call_browser("Target.getTargets", json!({}))
            .map_err(derr)?;
        let present = targets
            .get("targetInfos")
            .and_then(Value::as_array)
            .is_some_and(|targets| {
                targets.iter().any(|target| {
                    target.get("type").and_then(Value::as_str) == Some("iframe")
                        && target.get("targetId").and_then(Value::as_str) == Some(frame)
                })
            });
        if !present {
            return Ok(None);
        }
        let attached = self
            .cdp
            .call_browser(
                "Target.attachToTarget",
                json!({"targetId":frame,"flatten":true}),
            )
            .map_err(derr)?;
        let session = attached
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Embedded target session unavailable".into()))?
            .to_owned();
        let inspected = (|| -> Result<Option<bool>, DriverError> {
            self.cdp
                .call_session(&session, "DOM.enable", json!({}))
                .map_err(derr)?;
            let before = self
                .cdp
                .call_session(&session, "Page.getFrameTree", json!({}))
                .map_err(derr)?;
            let identity = |tree: &Value| {
                tree.pointer("/frameTree/frame/id")
                    .and_then(Value::as_str)
                    .zip(
                        tree.pointer("/frameTree/frame/loaderId")
                            .and_then(Value::as_str),
                    )
                    .map(|(id, loader)| (id.to_owned(), loader.to_owned()))
            };
            let Some(first) = identity(&before).filter(|(id, _)| id == frame) else {
                return Ok(None);
            };
            let document = self
                .cdp
                .call_session(
                    &session,
                    "DOM.getDocument",
                    json!({"depth":-1,"pierce":false}),
                )
                .map_err(derr)?;
            let Some(root) = document.get("root") else {
                return Ok(None);
            };
            let found = document_has_form_controls(root);
            let after = self
                .cdp
                .call_session(&session, "Page.getFrameTree", json!({}))
                .map_err(derr)?;
            if identity(&after).as_ref() != Some(&first) {
                return Ok(None);
            }
            Ok(Some(found))
        })();
        let detached = self
            .cdp
            .call_browser("Target.detachFromTarget", json!({"sessionId":session}));
        detached.map_err(derr)?;
        inspected
    }

    /// Read the main document and a bounded number of accessible embedded documents.
    fn read_form(
        &mut self,
        url: &str,
    ) -> Result<(waypoint_applications::form_mapping::FormSchema, Vec<String>), DriverError> {
        let root = self.document_with_frames(true)?;
        let (mut schema, mut walls) = self.read_form_document(url, &root)?;
        // The queue owns only the root and the few documents actually inspected.
        // Each selector binds every frame and document along its path.
        let started = Instant::now();
        let previous_timeout = self.cdp.timeout;
        self.cdp.timeout = Duration::from_secs(1);
        let inspected = (|| -> Result<(), DriverError> {
            let mut queue = std::collections::VecDeque::from([(root, String::new(), 0usize)]);
            let mut inspected_count = 0;
            let mut isolated_inspections = 0usize;
            while let Some((document, prefix, depth)) = queue.pop_front() {
                if depth >= 4 || inspected_count >= 4 {
                    if document_has_embedded_scope(&document)
                        && !walls.iter().any(|wall| wall == EMBEDDED_SCOPE_LIMIT_REVIEW)
                    {
                        walls.push(EMBEDDED_SCOPE_LIMIT_REVIEW.into());
                    }
                    continue;
                }
                let mut shadow_hosts = Vec::new();
                walk_with_visibility(&document, false, false, &mut |node, _, hidden| {
                    if !hidden {
                        if let (Some(id), Some(backend), Some(roots)) = (
                            node.get("nodeId").and_then(Value::as_u64),
                            node.get("backendNodeId").and_then(Value::as_u64),
                            node.get("shadowRoots").and_then(Value::as_array),
                        ) {
                            for root in roots {
                                if root.get("shadowRootType").and_then(Value::as_str)
                                    == Some("open")
                                    && document_may_contain_form_or_scope(root)
                                {
                                    if let Some(root_backend) =
                                        root.get("backendNodeId").and_then(Value::as_u64)
                                    {
                                        shadow_hosts.push((id, backend, root_backend));
                                    }
                                }
                            }
                        }
                    }
                    true
                });
                for (host_id, host_backend, root_backend) in shadow_hosts {
                    if inspected_count >= 4 {
                        if !walls.iter().any(|wall| wall == EMBEDDED_SCOPE_LIMIT_REVIEW) {
                            walls.push(EMBEDDED_SCOPE_LIMIT_REVIEW.into());
                        }
                        break;
                    }
                    if started.elapsed() > Duration::from_secs(8) {
                        return Err(DriverError("Embedded form inspection exceeded its time budget; manual review is needed".into()));
                    }
                    if !self.open_visible_shadow_host(host_id)? {
                        continue;
                    }
                    let mut child = None;
                    walk(&document, false, &mut |node, _| {
                        if node.get("backendNodeId").and_then(Value::as_u64) == Some(host_backend) {
                            child = node
                                .get("shadowRoots")
                                .and_then(Value::as_array)
                                .and_then(|roots| {
                                    roots.iter().find(|root| {
                                        root.get("shadowRootType").and_then(Value::as_str)
                                            == Some("open")
                                            && root.get("backendNodeId").and_then(Value::as_u64)
                                                == Some(root_backend)
                                    })
                                })
                                .cloned();
                        }
                        true
                    });
                    let Some(child) = child else { continue };
                    inspected_count += 1;
                    let (mut embedded, mut embedded_walls) =
                        self.read_form_document(url, &child)?;
                    if schema.fields.len().saturating_add(embedded.fields.len()) > 160 {
                        return Err(DriverError("Form has too many controls for one inspection; manual review is needed".into()));
                    }
                    let child_prefix =
                        format!("{prefix}waypoint-shadow:{host_backend}:{root_backend}:");
                    for field in &mut embedded.fields {
                        field.selector = format!("{child_prefix}{}", field.selector);
                        for section in &mut field.context.sections {
                            section.id = format!("{child_prefix}{}", section.id);
                        }
                    }
                    schema.fields.extend(embedded.fields);
                    schema.multi_step |= embedded.multi_step;
                    walls.append(&mut embedded_walls);
                    queue.push_back((child, child_prefix, depth + 1));
                }
                let mut frames = Vec::new();
                walk_with_visibility(&document, false, false, &mut |node, _, hidden| {
                    let tag = node.get("nodeName").and_then(Value::as_str).unwrap_or("");
                    if !hidden
                        && (tag.eq_ignore_ascii_case("iframe") || tag.eq_ignore_ascii_case("frame"))
                    {
                        if let (Some(id), Some(backend)) = (
                            node.get("nodeId").and_then(Value::as_u64),
                            node.get("backendNodeId").and_then(Value::as_u64),
                        ) {
                            frames.push((id, backend));
                        }
                    }
                    true
                });
                for (frame_id, frame_backend) in frames {
                    if inspected_count >= 4 {
                        if !walls.iter().any(|wall| wall == EMBEDDED_SCOPE_LIMIT_REVIEW) {
                            walls.push(EMBEDDED_SCOPE_LIMIT_REVIEW.into());
                        }
                        break;
                    }
                    if started.elapsed() > Duration::from_secs(8) {
                        return Err(DriverError("Embedded form inspection exceeded its time budget; manual review is needed".into()));
                    }
                    let access = self.frame_access(frame_id)?;
                    if access == Some(false) {
                        // Bound opaque-document inspection regardless of whether
                        // Chromium supplies it inline or through another target.
                        if isolated_inspections >= 4 {
                            if !walls
                                .iter()
                                .any(|wall| wall == ISOLATED_FRAME_INSPECTION_UNAVAILABLE)
                            {
                                walls.push(ISOLATED_FRAME_INSPECTION_UNAVAILABLE.into());
                            }
                            continue;
                        }
                        isolated_inspections += 1;
                        let mut has_controls = false;
                        let mut isolated_target = None;
                        let mut child_present = false;
                        walk(&document, false, &mut |node, _| {
                            if node.get("backendNodeId").and_then(Value::as_u64)
                                == Some(frame_backend)
                            {
                                isolated_target = node
                                    .get("frameId")
                                    .and_then(Value::as_str)
                                    .map(str::to_owned);
                                if let Some(child) = node.get("contentDocument") {
                                    child_present = true;
                                    has_controls = document_has_form_controls(child);
                                }
                            }
                            true
                        });
                        if !child_present {
                            let inspected = if let Some(target) = isolated_target {
                                self.isolated_frame_has_controls(&target).ok().flatten()
                            } else {
                                None
                            };
                            match inspected {
                                Some(found) => has_controls = found,
                                None => {
                                    if !walls
                                        .iter()
                                        .any(|wall| wall == ISOLATED_FRAME_INSPECTION_UNAVAILABLE)
                                    {
                                        walls.push(ISOLATED_FRAME_INSPECTION_UNAVAILABLE.into());
                                    }
                                }
                            }
                        }
                        if has_controls
                            && !walls
                                .iter()
                                .any(|wall| wall == INACCESSIBLE_FRAME_FORM_REVIEW)
                        {
                            walls.push(INACCESSIBLE_FRAME_FORM_REVIEW.into());
                        }
                        continue;
                    }
                    if access != Some(true) {
                        continue;
                    }
                    let mut child = None;
                    walk(&document, false, &mut |node, _| {
                        if node.get("backendNodeId").and_then(Value::as_u64) == Some(frame_backend)
                        {
                            child = node.get("contentDocument").cloned();
                        }
                        true
                    });
                    let Some(child) = child else { continue };
                    let Some(document_backend) = child.get("backendNodeId").and_then(Value::as_u64)
                    else {
                        continue;
                    };
                    inspected_count += 1;
                    let (mut embedded, mut embedded_walls) =
                        self.read_form_document(url, &child)?;
                    if schema.fields.len().saturating_add(embedded.fields.len()) > 160 {
                        return Err(DriverError("Form has too many controls for one inspection; manual review is needed".into()));
                    }
                    let child_prefix =
                        format!("{prefix}waypoint-frame:{frame_backend}:{document_backend}:");
                    for field in &mut embedded.fields {
                        field.selector = format!("{child_prefix}{}", field.selector);
                        for section in &mut field.context.sections {
                            section.id = format!("{child_prefix}{}", section.id);
                        }
                    }
                    schema.fields.extend(embedded.fields);
                    schema.multi_step |= embedded.multi_step;
                    walls.append(&mut embedded_walls);
                    queue.push_back((child, child_prefix, depth + 1));
                }
            }
            Ok(())
        })();
        self.cdp.timeout = previous_timeout;
        inspected?;
        walls.sort();
        walls.dedup();
        Ok((schema, walls))
    }

    fn read_form_document(
        &mut self,
        url: &str,
        root: &Value,
    ) -> Result<(waypoint_applications::form_mapping::FormSchema, Vec<String>), DriverError> {
        let datalists = static_datalists(root);
        let mut controls = vec![];
        let mut hidden_selectors = std::collections::HashSet::new();
        let mut possible_choice_nodes = std::collections::HashSet::new();
        let mut labels: HashMap<String, String> = HashMap::new();
        let mut duplicate_labels = std::collections::HashSet::new();
        let mut wrapping_labels: HashMap<u64, String> = HashMap::new();
        let mut label_nodes: HashMap<String, u64> = HashMap::new();
        let mut wrapping_label_nodes: HashMap<u64, u64> = HashMap::new();
        let mut radio_containers: HashMap<u64, (u64, String, Option<String>)> = HashMap::new();
        let mut radio_form_owners: HashMap<u64, u64> = HashMap::new();
        let mut radio_group_required: HashMap<u64, bool> = HashMap::new();
        type GroupContext = (u64, String, Option<String>, Option<u64>, bool, bool);
        let mut field_containers: HashMap<u64, Vec<GroupContext>> = HashMap::new();
        let mut missing_section_identity = false;
        let mut group_scan_budget = 50_000usize;
        let mut group_scan_incomplete = false;
        let mut wall_flags: Vec<String> = vec![];
        let mut custom_nodes = Vec::new();
        let mut next_nodes = Vec::new();
        let mut next_references = Vec::new();
        let mut frame_nodes = Vec::new();
        let mut shadow_nodes = Vec::new();
        let mut text = String::new();
        let mut possible_next_step = false;
        walk_with_visibility(root, false, false, &mut |node, in_control, hidden| {
            let tag = node
                .get("nodeName")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            // Captchas hide in ordinary markup (a div, an iframe), not only in
            // form controls, so every node is checked.
            let attrs = attributes(node);
            if let Some(children) = node
                .get("children")
                .and_then(Value::as_array)
                .filter(|children| (3..=20).contains(&children.len()))
            {
                let mut editable = Vec::new();
                let mut has_hidden = false;
                let mut has_list_hint = false;
                for child in children {
                    let tag = child
                        .get("nodeName")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    let attributes = attributes(child);
                    if tag == "input" {
                        let kind = attributes
                            .get("type")
                            .map(String::as_str)
                            .unwrap_or("text")
                            .to_ascii_lowercase();
                        if kind == "hidden" {
                            has_hidden = true;
                        } else if matches!(
                            kind.as_str(),
                            "text" | "search" | "email" | "tel" | "url"
                        ) {
                            if let Some(id) = child.get("nodeId").and_then(Value::as_u64) {
                                editable.push(id);
                            }
                        }
                    } else {
                        let role = attributes
                            .get("role")
                            .map(|value| value.to_ascii_lowercase())
                            .unwrap_or_default();
                        let tokens = format!(
                            "{} {}",
                            attributes.get("class").map(String::as_str).unwrap_or(""),
                            attributes.get("id").map(String::as_str).unwrap_or("")
                        )
                        .to_ascii_lowercase();
                        has_list_hint |= matches!(role.as_str(), "listbox" | "option")
                            || tokens
                                .split(|ch: char| !ch.is_ascii_alphanumeric())
                                .any(|token| {
                                    matches!(
                                        token,
                                        "dropdown"
                                            | "autocomplete"
                                            | "suggestion"
                                            | "suggestions"
                                            | "listbox"
                                    )
                                });
                    }
                }
                if has_hidden && has_list_hint && editable.len() == 1 {
                    possible_choice_nodes.insert(editable[0]);
                }
            }
            if !hidden {
                if let Some(roots) = node.get("shadowRoots").and_then(Value::as_array) {
                    let relevant: Vec<_> = roots
                        .iter()
                        .filter(|root| {
                            matches!(
                                root.get("shadowRootType").and_then(Value::as_str),
                                Some("open" | "closed")
                            )
                        })
                        .collect();
                    if !relevant.is_empty() {
                        if let Some(id) = node.get("nodeId").and_then(Value::as_u64) {
                            let closed_form = relevant.iter().any(|root| {
                                root.get("shadowRootType").and_then(Value::as_str) == Some("closed")
                                    && document_has_form_controls(root)
                            });
                            shadow_nodes.push((id, closed_form));
                        } else {
                            wall_flags.push(SHADOW_FORM_REVIEW.into());
                        }
                    }
                }
            }
            if matches!(tag.as_str(), "iframe" | "frame") && !hidden {
                if let Some(id) = node.get("nodeId").and_then(Value::as_u64) {
                    frame_nodes.push(id);
                } else {
                    wall_flags.push(UNINSPECTED_FRAME_REVIEW.into());
                }
            }
            // An accessible current-step indicator is evidence of a staged
            // workflow independent of the language used by its buttons.
            if !hidden
                && attrs
                    .get("aria-current")
                    .is_some_and(|value| value.eq_ignore_ascii_case("step"))
            {
                if let Some(id) = node.get("nodeId").and_then(Value::as_u64) {
                    next_nodes.push(id);
                } else {
                    possible_next_step = true;
                }
            }
            if (tag == "button"
                || attrs
                    .get("role")
                    .is_some_and(|role| role.eq_ignore_ascii_case("button"))
                || (tag == "input"
                    && attrs.get("type").is_some_and(|kind| {
                        matches!(kind.to_ascii_lowercase().as_str(), "button" | "submit")
                    })))
                && !attrs.contains_key("disabled")
                && !attrs
                    .get("aria-disabled")
                    .is_some_and(|value| value.eq_ignore_ascii_case("true"))
                && !hidden
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
                if let Some(ids) = attrs
                    .get("aria-labelledby")
                    .filter(|ids| !ids.is_empty() && ids.len() <= 512)
                {
                    if let Some(id) = node.get("nodeId").and_then(Value::as_u64) {
                        next_references.push((id, ids.clone()));
                    } else {
                        possible_next_step = true;
                    }
                } else if possible_next_label(&caption) {
                    if let Some(id) = node.get("nodeId").and_then(Value::as_u64) {
                        next_nodes.push(id);
                    } else {
                        possible_next_step = true;
                    }
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
            if hidden && !matches!(tag.as_str(), "input" | "textarea" | "select") {
                return !matches!(tag.as_str(), "script" | "style" | "noscript" | "template");
            }
            let plain_editor = supported_editing_host(&tag, &attrs);
            // Typing into an editable choice widget does not establish that a
            // site-side option was selected, even if the text is retained.
            let dynamic_choice = !(tag == "input"
                && attrs
                    .get("type")
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("password")))
                && ((tag != "select"
                    && attrs
                        .get("role")
                        .is_some_and(|role| role.eq_ignore_ascii_case("combobox")))
                    || (tag == "input"
                        && attrs.contains_key("list")
                        && native_datalist_options(&attrs, &datalists).is_none())
                    || (matches!(tag.as_str(), "input" | "textarea")
                        && attrs.get("aria-autocomplete").is_some_and(|value| {
                            matches!(
                                value.to_ascii_lowercase().as_str(),
                                "list" | "both" | "inline"
                            )
                        })));
            let required_custom = attrs.contains_key("required")
                || attrs
                    .get("aria-required")
                    .is_some_and(|value| value.eq_ignore_ascii_case("true"));
            if required_custom
                && matches!(tag.as_str(), "input" | "textarea" | "select")
                && hidden
                && !attrs
                    .get("type")
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("hidden"))
            {
                wall_flags.push("a required hidden control needs manual review".into());
            }
            // ARIA puts the required state on the radiogroup, not normally
            // on each option. Native radio inputs have their own mapping;
            // custom radio descendants still require explicit review.
            let required_custom_radio_group = required_custom
                && attrs
                    .get("role")
                    .is_some_and(|role| role.eq_ignore_ascii_case("radiogroup"))
                && {
                    let mut custom_radio = false;
                    walk(node, false, &mut |child, _| {
                        let child_attrs = attributes(child);
                        let child_tag = child.get("nodeName").and_then(Value::as_str).unwrap_or("");
                        if child_attrs
                            .get("role")
                            .is_some_and(|role| role.eq_ignore_ascii_case("radio"))
                            && !(child_tag.eq_ignore_ascii_case("input")
                                && child_attrs
                                    .get("type")
                                    .is_some_and(|kind| kind.eq_ignore_ascii_case("radio")))
                        {
                            custom_radio = true;
                        }
                        true
                    });
                    custom_radio
                };
            // Custom ARIA controls are not handled by the native-field driver.
            // Do not silently omit a required question on an otherwise readable form.
            if !hidden
                && (dynamic_choice
                    || required_custom_radio_group
                    || required_custom
                        && !plain_editor
                        && (attrs.get("contenteditable").is_some_and(|value| {
                            matches!(
                                value.to_ascii_lowercase().as_str(),
                                "" | "true" | "plaintext-only"
                            ) && !matches!(tag.as_str(), "input" | "textarea" | "select")
                        }) || attrs.get("role").is_some_and(|role| {
                            match role.to_ascii_lowercase().as_str() {
                                "combobox" | "listbox" => tag != "select",
                                "spinbutton" => {
                                    !(tag == "input"
                                        && attrs.get("type").is_some_and(|kind| {
                                            kind.eq_ignore_ascii_case("number")
                                        }))
                                }
                                "slider" | "switch" => true,
                                "textbox" | "searchbox" | "checkbox" | "radio" => {
                                    !matches!(tag.as_str(), "input" | "textarea" | "select")
                                }
                                _ => false,
                            }
                        })))
            {
                if let Some(id) = node.get("nodeId").and_then(Value::as_u64) {
                    let label = attrs
                        .get("aria-label")
                        .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
                        .filter(|value| !value.is_empty() && value.len() <= 160);
                    let references = attrs.get("aria-labelledby").cloned();
                    custom_nodes.push((id, label, references, required_custom));
                } else {
                    wall_flags.push(
                        if required_custom {
                            "a required custom control needs manual review"
                        } else {
                            "a custom control needs manual review"
                        }
                        .into(),
                    );
                }
            }
            if tag == "label" {
                let mut label_text = String::new();
                walk(node, false, &mut |child, inside_control| {
                    let tag = child.get("nodeName").and_then(Value::as_str).unwrap_or("");
                    if ["script", "style", "template", "noscript"]
                        .iter()
                        .any(|name| tag.eq_ignore_ascii_case(name))
                    {
                        return false;
                    }
                    if tag == "#text" && !inside_control {
                        if let Some(text) = child.get("nodeValue").and_then(Value::as_str) {
                            label_text.push_str(text);
                            label_text.push(' ');
                        }
                    }
                    true
                });
                let label_text = label_text.split_whitespace().collect::<Vec<_>>().join(" ");
                if let Some(for_id) = attrs.get("for").filter(|value| !value.is_empty()) {
                    if labels.contains_key(for_id) {
                        duplicate_labels.insert(for_id.clone());
                    }
                    labels
                        .entry(for_id.clone())
                        .or_insert_with(|| label_text.clone());
                    if let Some(label_node) = node.get("nodeId").and_then(Value::as_u64) {
                        label_nodes.entry(for_id.clone()).or_insert(label_node);
                    }
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
                                if let Some(label_node) = node.get("nodeId").and_then(Value::as_u64)
                                {
                                    wrapping_label_nodes.entry(id).or_insert(label_node);
                                }
                            }
                            return false;
                        }
                        true
                    });
                }
            }
            if tag == "form" {
                if let Some(form_id) = node.get("nodeId").and_then(Value::as_u64) {
                    walk(node, false, &mut |child, _| {
                        let child_tag = child.get("nodeName").and_then(Value::as_str).unwrap_or("");
                        if child_tag.eq_ignore_ascii_case("form")
                            && child.get("nodeId").and_then(Value::as_u64) != Some(form_id)
                        {
                            return false;
                        }
                        if child_tag.eq_ignore_ascii_case("input") {
                            let attrs = attributes(child);
                            if attrs
                                .get("type")
                                .is_some_and(|value| value.eq_ignore_ascii_case("radio"))
                                && !attrs.contains_key("form")
                            {
                                if let Some(id) = child.get("nodeId").and_then(Value::as_u64) {
                                    radio_form_owners.insert(id, form_id);
                                }
                            }
                        }
                        true
                    });
                }
            }
            // A plain container with one direct heading can describe a form
            // section even without a semantic section/fieldset element. The
            // heading is still checked for rendered visibility below.
            let headed_div = tag == "div"
                && attrs.get("role").is_none_or(|role| {
                    role.eq_ignore_ascii_case("group") || role.eq_ignore_ascii_case("region")
                })
                && node
                    .get("children")
                    .and_then(Value::as_array)
                    .is_some_and(|children| {
                        children
                            .iter()
                            .filter(|child| {
                                child
                                    .get("nodeName")
                                    .and_then(Value::as_str)
                                    .is_some_and(|name| {
                                        matches!(
                                            name.to_ascii_lowercase().as_str(),
                                            "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                                        )
                                    })
                            })
                            .take(2)
                            .count()
                            == 1
                    });
            let named_field_group = headed_div
                || tag == "fieldset"
                || (tag == "section"
                    && attrs
                        .get("role")
                        .is_none_or(|role| role.eq_ignore_ascii_case("region")))
                || attrs.get("role").is_some_and(|role| {
                    role.eq_ignore_ascii_case("group") || role.eq_ignore_ascii_case("region")
                });
            let radio_group = attrs
                .get("role")
                .is_some_and(|role| role.eq_ignore_ascii_case("radiogroup"));
            if radio_group {
                let required = attrs
                    .get("aria-required")
                    .is_some_and(|value| value.eq_ignore_ascii_case("true"));
                // Outer groups are visited before inner ones, so the nearest
                // radiogroup determines the requirement for nested controls.
                walk(node, false, &mut |child, _| {
                    let child_attrs = attributes(child);
                    if child
                        .get("nodeName")
                        .and_then(Value::as_str)
                        .is_some_and(|tag| tag.eq_ignore_ascii_case("input"))
                        && child_attrs
                            .get("type")
                            .is_some_and(|kind| kind.eq_ignore_ascii_case("radio"))
                    {
                        if let Some(id) = child.get("nodeId").and_then(Value::as_u64) {
                            radio_group_required.insert(id, required);
                        }
                    }
                    true
                });
            }
            if named_field_group || radio_group {
                let mut implicit_heading = None;
                let mut ambiguous_heading = false;
                let mut legend = if tag == "fieldset" {
                    attrs
                        .get("aria-label")
                        .filter(|label| !label.trim().is_empty())
                        .cloned()
                        .or_else(|| {
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
                        })
                        .unwrap_or_default()
                } else {
                    attrs.get("aria-label").cloned().unwrap_or_default()
                };
                let group_ref = attrs.get("aria-labelledby").cloned();
                if (tag == "section" || headed_div)
                    && legend.trim().is_empty()
                    && group_ref.is_none()
                {
                    let headings: Vec<_> = node
                        .get("children")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter(|child| {
                            child
                                .get("nodeName")
                                .and_then(Value::as_str)
                                .is_some_and(|name| {
                                    matches!(
                                        name.to_ascii_lowercase().as_str(),
                                        "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                                    )
                                })
                        })
                        .take(2)
                        .collect();
                    if let [heading] = headings.as_slice() {
                        let mut text = String::new();
                        collect_text(heading, false, &mut text);
                        legend = text.split_whitespace().collect::<Vec<_>>().join(" ");
                        implicit_heading = heading.get("nodeId").and_then(Value::as_u64);
                        ambiguous_heading = implicit_heading.is_none() || legend.trim().is_empty();
                    } else if headings.len() > 1 {
                        ambiguous_heading = true;
                    }
                }
                if let Some(fieldset_id) = node
                    .get("nodeId")
                    .and_then(Value::as_u64)
                    .filter(|_| !legend.is_empty() || group_ref.is_some() || ambiguous_heading)
                {
                    // Frontend node IDs change on getDocument; backend IDs name
                    // the same live node across inspections and change on replacement.
                    let section_backend = node
                        .get("backendNodeId")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    missing_section_identity |= section_backend == 0;
                    let completed =
                        walk_group_bounded(node, &mut group_scan_budget, &mut |child| {
                            let child_tag = child
                                .get("nodeName")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_ascii_lowercase();
                            if matches!(
                                child_tag.as_str(),
                                "script" | "style" | "noscript" | "template"
                            ) {
                                return false;
                            }
                            if named_field_group
                                && (matches!(child_tag.as_str(), "input" | "textarea" | "select")
                                    || supported_editing_host(&child_tag, &attributes(child)))
                            {
                                if let Some(id) = child.get("nodeId").and_then(Value::as_u64) {
                                    let groups = field_containers.entry(id).or_default();
                                    if groups.len() < 9 {
                                        groups.push((
                                            section_backend,
                                            legend.clone(),
                                            group_ref.clone(),
                                            implicit_heading,
                                            ambiguous_heading,
                                            headed_div && implicit_heading.is_some(),
                                        ));
                                    }
                                }
                            }
                            if child_tag == "input" && (tag == "fieldset" || radio_group) {
                                if let Some(id) = child.get("nodeId").and_then(Value::as_u64) {
                                    radio_containers.insert(
                                        id,
                                        (fieldset_id, legend.clone(), group_ref.clone()),
                                    );
                                }
                            }
                            true
                        });
                    group_scan_incomplete |= !completed;
                }
            }
            if matches!(tag.as_str(), "script" | "style" | "noscript" | "template") {
                return false; // skip this subtree for text collection
            }
            if dynamic_choice {
                return true;
            }
            if (matches!(tag.as_str(), "input" | "textarea" | "select") || plain_editor)
                && !in_control
            {
                if node.get("nodeId").and_then(|v| v.as_u64()).is_some() {
                    let control = Control {
                        node_id: node
                            .get("nodeId")
                            .and_then(Value::as_u64)
                            .unwrap_or_default(),
                        tag: tag.clone(),
                        attrs: attrs.clone(),
                        options: if tag == "input" {
                            native_datalist_options(&attrs, &datalists)
                                .cloned()
                                .unwrap_or_default()
                        } else {
                            options_of(node)
                        },
                    };
                    if hidden {
                        add_hidden_selectors(&mut hidden_selectors, &control);
                    } else {
                        controls.push(control);
                    }
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

        if missing_section_identity {
            return Err(DriverError(
                "A form section has no stable identity; inspect again or review manually".into(),
            ));
        }

        // Accessible names may be provided by nodes elsewhere in the page.
        // Resolve only bounded, unambiguous references; unresolved names need
        // review rather than silently dropping a possible navigation control.
        if next_references.len().saturating_add(custom_nodes.len()) > 160 {
            return Err(DriverError(
                "Form has too many controls for one inspection; manual review is needed".into(),
            ));
        }
        let mut next_labels: HashMap<String, Option<String>> = HashMap::new();
        let mut wanted_next = std::collections::HashSet::new();
        for (_, ids) in &next_references {
            for id in ids.split_whitespace().take(16) {
                wanted_next.insert(id.to_owned());
            }
        }
        for (_, _, references, _) in &custom_nodes {
            if let Some(ids) = references.as_ref().filter(|ids| ids.len() <= 512) {
                for id in ids.split_whitespace().take(16) {
                    wanted_next.insert(id.to_owned());
                }
            }
        }
        walk(root, false, &mut |node, _| {
            let attrs = attributes(node);
            if let Some(id) = attrs.get("id").filter(|id| wanted_next.contains(*id)) {
                if next_labels.contains_key(id) {
                    next_labels.insert(id.clone(), None);
                } else {
                    let mut label = String::new();
                    collect_text(node, false, &mut label);
                    let label = label.split_whitespace().collect::<Vec<_>>().join(" ");
                    next_labels.insert(
                        id.clone(),
                        (!label.is_empty() && label.len() <= 1000).then_some(label),
                    );
                }
            }
            true
        });
        for (node, ids) in next_references {
            let parts: Option<Vec<_>> = if ids.split_whitespace().count() > 16 {
                None
            } else {
                ids.split_whitespace()
                    .map(|id| next_labels.get(id).and_then(Option::as_ref))
                    .collect()
            };
            match parts {
                Some(parts)
                    if possible_next_label(
                        &parts
                            .iter()
                            .map(|part| part.as_str())
                            .collect::<Vec<_>>()
                            .join(" "),
                    ) =>
                {
                    next_nodes.push(node)
                }
                None => wall_flags.push(
                    "a button has an unresolved accessible name; manual review is needed".into(),
                ),
                _ => {}
            }
        }

        // Determine rendered visibility and a unique structural selector from
        // the live page. Bound both the count and the total inspection time.
        controls.retain(|control| {
            let ignored = control.attrs.get("type").is_some_and(|kind| {
                matches!(
                    kind.to_ascii_lowercase().as_str(),
                    "hidden" | "submit" | "button" | "reset" | "image"
                )
            });
            if ignored {
                add_hidden_selectors(&mut hidden_selectors, control);
            }
            !ignored
        });
        if controls
            .len()
            .saturating_add(custom_nodes.len())
            .saturating_add(next_nodes.len())
            .saturating_add(frame_nodes.len())
            .saturating_add(shadow_nodes.len())
            > 160
        {
            return Err(DriverError(
                "Form has too many controls for one inspection; manual review is needed".into(),
            ));
        }
        let mut css_paths = HashMap::new();
        let start = Instant::now();
        let previous_timeout = self.cdp.timeout;
        self.cdp.timeout = Duration::from_secs(1);
        let checked = (|| -> Result<Vec<Control>, DriverError> {
            let mut visible = Vec::new();
            for control in controls {
                if start.elapsed() > Duration::from_secs(8) {
                    return Err(DriverError("Form visibility inspection exceeded its time budget; manual review is needed".into()));
                }
                let (rendered, path, available, needs_review, dialog_obscured) =
                    match self.control_visibility_and_path(control.node_id) {
                        Ok(result) => result,
                        Err(error)
                            if error.0
                                == "Field visibility is unavailable; manual review is needed" =>
                        {
                            wall_flags.push("an unavailable field needs manual review".into());
                            continue;
                        }
                        Err(error) => return Err(error),
                    };
                if rendered && available {
                    css_paths.insert(control.node_id, path);
                    visible.push(control);
                } else {
                    let required = control.attrs.contains_key("required")
                        || control
                            .attrs
                            .get("aria-required")
                            .is_some_and(|value| value.eq_ignore_ascii_case("true"));
                    if !rendered && required {
                        wall_flags.push("a required hidden control needs manual review".into());
                    }
                    if needs_review {
                        wall_flags.push("a custom control needs manual review".into());
                        wall_flags.push("a custom control needs manual review: A field may be obscured, clipped or transparent; check overlays and the visible browser".into());
                        if dialog_obscured {
                            wall_flags.push("a custom control needs manual review: A dialog covers a form field; resolve it in the visible browser, then inspect again".into());
                        }
                    }
                    if rendered && !available {
                        wall_flags.push(if required {
                            "a required field is disabled or read-only".into()
                        } else {
                            "an unavailable field needs manual review".into()
                        });
                    }
                    add_hidden_selectors(&mut hidden_selectors, &control);
                }
            }
            for (node, label, references, required) in custom_nodes {
                if start.elapsed() > Duration::from_secs(8) {
                    return Err(DriverError("Form visibility inspection exceeded its time budget; manual review is needed".into()));
                }
                if self.control_visibility_and_path(node)?.0 {
                    let label = if let Some(ids) = references {
                        let parts: Vec<_> = ids.split_whitespace().collect();
                        if ids.len() <= 512 && !parts.is_empty() && parts.len() <= 16 {
                            parts
                                .iter()
                                .map(|id| next_labels.get(*id).and_then(Option::as_ref))
                                .collect::<Option<Vec<_>>>()
                                .map(|parts| {
                                    parts
                                        .into_iter()
                                        .map(String::as_str)
                                        .collect::<Vec<_>>()
                                        .join(" ")
                                })
                                .filter(|value| !value.is_empty() && value.len() <= 160)
                        } else {
                            None
                        }
                    } else {
                        label
                    };
                    let prefix = if required {
                        "a required custom control needs manual review"
                    } else {
                        "a custom control needs manual review"
                    };
                    wall_flags.push(match label {
                        Some(label) => format!("{prefix}: {label}"),
                        None => prefix.into(),
                    });
                }
            }
            for (node, closed_form) in shadow_nodes {
                if start.elapsed() > Duration::from_secs(8) {
                    return Err(DriverError("Form visibility inspection exceeded its time budget; manual review is needed".into()));
                }
                if self.control_visibility_and_path(node)?.0 {
                    wall_flags.push(SHADOW_FORM_REVIEW.into());
                    if closed_form {
                        wall_flags.push(CLOSED_SHADOW_FORM_REVIEW.into());
                    }
                }
            }
            for node in frame_nodes {
                if start.elapsed() > Duration::from_secs(8) {
                    return Err(DriverError("Form visibility inspection exceeded its time budget; manual review is needed".into()));
                }
                if self.control_visibility_and_path(node)?.0 {
                    let model = self.cdp.call("DOM.getBoxModel", json!({"nodeId":node}));
                    let empty = model.as_ref().ok().is_some_and(|value| {
                        value.pointer("/model/width").and_then(Value::as_f64) == Some(0.0)
                            || value.pointer("/model/height").and_then(Value::as_f64) == Some(0.0)
                    });
                    if !empty
                        && !wall_flags
                            .iter()
                            .any(|reason| reason == UNINSPECTED_FRAME_REVIEW)
                    {
                        wall_flags.push(UNINSPECTED_FRAME_REVIEW.into());
                    }
                }
            }
            for node in next_nodes {
                if possible_next_step {
                    break;
                }
                if start.elapsed() > Duration::from_secs(8) {
                    return Err(DriverError("Form visibility inspection exceeded its time budget; manual review is needed".into()));
                }
                let (visible, _, _, needs_review, _) = self.control_visibility_and_path(node)?;
                // Clipped or transparent indicators cannot be confidently
                // dismissed as absent; leave the workflow for manual review.
                possible_next_step = visible || needs_review;
            }
            Ok(visible)
        })();
        self.cdp.timeout = previous_timeout;
        let controls = checked?;

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
        for group_ref in radio_containers
            .values()
            .map(|(_, _, group_ref)| group_ref)
            .chain(
                field_containers
                    .values()
                    .flat_map(|groups| groups.iter().map(|(_, _, group_ref, _, _, _)| group_ref)),
            )
        {
            if let Some(ids) = group_ref.as_ref().filter(|s| s.len() <= 512) {
                for id in ids.split_whitespace().take(16) {
                    if wanted.len() < 160 {
                        wanted.insert(id.to_owned());
                    }
                }
            }
        }
        let mut referenced: HashMap<String, Option<String>> = HashMap::new();
        walk(root, false, &mut |node, _| {
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
        if group_scan_incomplete {
            wall_flags.push("Form section context exceeded the inspection budget; review the complete form manually".into());
        }
        let mut walls = wall_flags;
        // Error references are read only for visible fields, and are resolved
        // separately from accessible names so duplicate IDs cannot be guessed.
        let mut wanted_errors = std::collections::HashSet::new();
        for control in &controls {
            if control.attrs.get("aria-invalid").is_some_and(|value| {
                let value = value.trim();
                !value.is_empty() && !value.eq_ignore_ascii_case("false")
            }) {
                if let Some(ids) = control
                    .attrs
                    .get("aria-errormessage")
                    .filter(|ids| ids.len() <= 512 && ids.split_whitespace().count() <= 4)
                {
                    for id in ids.split_whitespace() {
                        if wanted_errors.len() < 32 {
                            wanted_errors.insert(id.to_owned());
                        }
                    }
                }
            }
        }
        let mut error_nodes: HashMap<String, Option<u64>> = HashMap::new();
        walk(root, false, &mut |node, _| {
            let attrs = attributes(node);
            if let Some(id) = attrs.get("id").filter(|id| wanted_errors.contains(*id)) {
                if error_nodes.contains_key(id) {
                    error_nodes.insert(id.clone(), None);
                } else {
                    error_nodes.insert(id.clone(), node.get("nodeId").and_then(Value::as_u64));
                }
            }
            true
        });
        let mut checked_errors: HashMap<u64, Option<String>> = HashMap::new();
        let error_check_started = Instant::now();
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
        let mut checked_section_headings: HashMap<u64, Option<String>> = HashMap::new();
        let mut checked_labels: HashMap<u64, Option<String>> = HashMap::new();
        let mut unannotated_choice_checks = 0usize;
        let mut unassociated_question_count = 0usize;
        let mut required_marker_checks = 0usize;
        let marker_started = Instant::now();
        let mut selector_counts: HashMap<String, usize> = HashMap::new();
        for control in &controls {
            *selector_counts.entry(selector_for(control)).or_default() += 1;
        }
        for control in controls {
            let selector = selector_for(&control);
            let selector = if hidden_selectors.contains(&selector)
                || supported_editing_host(&control.tag, &control.attrs)
                || (!control
                    .attrs
                    .get("type")
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("radio"))
                    && selector_counts.get(&selector).copied().unwrap_or(0) > 1)
            {
                css_paths
                    .get(&control.node_id)
                    .cloned()
                    .ok_or_else(|| DriverError("Visible field has no stable selector".into()))?
            } else {
                selector
            };
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
            if referenced_label
                .as_ref()
                .is_some_and(|value| value.as_ref().is_none_or(|text| text.trim().is_empty()))
            {
                walls.push("a field has an unresolved or ambiguous accessible label".into());
            }
            let has_usable_label = referenced_label
                .as_ref()
                .and_then(Option::as_deref)
                .is_some_and(|value| !value.trim().is_empty())
                || control
                    .attrs
                    .get("aria-label")
                    .is_some_and(|value| !value.trim().is_empty())
                || labels
                    .get(&id)
                    .is_some_and(|value| !value.trim().is_empty())
                || wrapping_labels
                    .get(&control.node_id)
                    .is_some_and(|value| !value.trim().is_empty())
                || control
                    .attrs
                    .get("placeholder")
                    .is_some_and(|value| !value.trim().is_empty());
            let mut label = referenced_label
                .flatten()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| {
                    control
                        .attrs
                        .get("aria-label")
                        .filter(|value| !value.trim().is_empty())
                        .cloned()
                })
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
            let mut label_unverified = false;
            let mut visible_label_required_marker = false;
            if !control.attrs.contains_key("aria-labelledby")
                && control
                    .attrs
                    .get("aria-label")
                    .is_none_or(|value| value.trim().is_empty())
            {
                let label_node = if labels.contains_key(&id) {
                    label_nodes.get(&id)
                } else {
                    wrapping_label_nodes.get(&control.node_id)
                };
                if let Some(label_node) = label_node {
                    let rendered = if let Some(rendered) = checked_labels.get(label_node) {
                        rendered.clone()
                    } else if checked_labels.len() < 64 {
                        let rendered = self
                            .with_review_timeout(|driver| driver.rendered_form_label(*label_node))
                            .ok()
                            .flatten();
                        checked_labels.insert(*label_node, rendered.clone());
                        rendered
                    } else {
                        None
                    };
                    if let Some(rendered) = rendered {
                        visible_label_required_marker = rendered.trim_end().ends_with('*');
                        label = rendered;
                    } else {
                        label_unverified = true;
                    }
                } else if labels.contains_key(&id) || wrapping_labels.contains_key(&control.node_id)
                {
                    label_unverified = true;
                }
                if duplicate_labels.contains(&id)
                    || label_nodes
                        .get(&id)
                        .zip(wrapping_label_nodes.get(&control.node_id))
                        .is_some_and(|(first, second)| first != second)
                {
                    label_unverified = true;
                }
            }
            if label_unverified {
                label = format!("Unverified label (review): {label}");
            }
            let unassociated_textarea = control.tag == "textarea" && !has_usable_label;
            if unassociated_textarea && unassociated_question_count < 16 {
                unassociated_question_count += 1;
                if let Ok(Some(context)) = self.with_review_timeout(|driver| {
                    driver.rendered_unassociated_question(control.node_id, false)
                }) {
                    label = format!("Possible question (review): {context}");
                }
            }
            if unassociated_textarea && label.trim().is_empty() {
                label = control
                    .attrs
                    .get("name")
                    .filter(|name| !name.trim().is_empty())
                    .cloned()
                    .unwrap_or_else(|| "unlabelled field".into());
            }
            let mut section_path = Vec::new();
            let mut section_context_complete = !group_scan_incomplete;
            if input_type != "radio" {
                if let Some(groups) = field_containers.get(&control.node_id) {
                    if groups.len() > 8 {
                        section_context_complete = false;
                        walls.push("field group nesting exceeds the inspection limit".into());
                    }
                    let mut context = Vec::new();
                    for (group_id, legend, group_ref, heading, ambiguous, inferred) in
                        groups.iter().take(8)
                    {
                        if *ambiguous {
                            section_context_complete = false;
                            walls.push(
                                "a section heading is ambiguous; review the form manually".into(),
                            );
                            continue;
                        }
                        let verified_heading = if let Some(node) = heading {
                            let text = if let Some(text) = checked_section_headings.get(node) {
                                text.clone()
                            } else if checked_section_headings.len() < 32 {
                                let text = self
                                    .with_review_timeout(|driver| {
                                        driver.rendered_section_heading(*node)
                                    })
                                    .ok()
                                    .flatten();
                                checked_section_headings.insert(*node, text.clone());
                                text
                            } else {
                                None
                            };
                            if text.is_none() {
                                section_context_complete = false;
                                walls.push("a section heading could not be verified; review the form manually".into());
                                continue;
                            }
                            text
                        } else {
                            None
                        };
                        let group = if let Some(ids) = group_ref {
                            let parts: Vec<_> = ids.split_whitespace().collect();
                            if ids.len() <= 512 && !parts.is_empty() && parts.len() <= 16 {
                                parts
                                    .iter()
                                    .map(|id| referenced.get(*id).and_then(Option::as_ref))
                                    .collect::<Option<Vec<_>>>()
                                    .map(|parts| {
                                        parts
                                            .into_iter()
                                            .map(String::as_str)
                                            .collect::<Vec<_>>()
                                            .join(" ")
                                    })
                            } else {
                                None
                            }
                        } else {
                            verified_heading.or_else(|| Some(legend.clone()))
                        };
                        if let Some(group) =
                            group.filter(|group| !group.trim().is_empty() && group.len() <= 500)
                        {
                            // Keep structural identity even when the display path deduplicates
                            // an equal legend. Repeated records may have identical labels.
                            section_path.push(waypoint_applications::form_mapping::FormSection {
                                id: format!("group:{group_id}"),
                                label: group.clone(),
                                inferred: *inferred,
                            });
                            if !label.eq_ignore_ascii_case(&group)
                                && !context.iter().any(|existing: &String| {
                                    existing
                                        .strip_prefix("Possible section: ")
                                        .unwrap_or(existing)
                                        .eq_ignore_ascii_case(&group)
                                })
                            {
                                context.push(if *inferred {
                                    format!("Possible section: {group}")
                                } else {
                                    group
                                });
                            }
                        } else {
                            section_context_complete = false;
                            walls.push(
                                "a field group has an unresolved or ambiguous accessible label"
                                    .into(),
                            );
                        }
                    }
                    if !context.is_empty() {
                        context.push(label);
                        label = context.join(" — ");
                    }
                }
            }

            // A verified parent cannot stand in for an unresolved child section.
            // Keep the inspection warning, but expose no selectable partial path.
            if !section_context_complete {
                section_path.clear();
            }

            if input_type == "password" {
                walls.push("login required (page asks for a password)".into());
            }

            if control.attrs.get("aria-invalid").is_some_and(|value| {
                let value = value.trim();
                !value.is_empty() && !value.eq_ignore_ascii_case("false")
            }) {
                let short_label: String = label.chars().take(160).collect();
                let details = control.attrs.get("aria-errormessage").map(|ids| {
                    let references: Vec<_> = ids.split_whitespace().collect();
                    if ids.len() > 512 || references.is_empty() || references.len() > 4 {
                        return None;
                    }
                    references
                        .into_iter()
                        .map(|id| {
                            let node = error_nodes.get(id).copied().flatten()?;
                            if let Some(text) = checked_errors.get(&node) {
                                return text.clone();
                            }
                            if error_check_started.elapsed() > Duration::from_secs(2) {
                                return None;
                            }
                            let text = self
                                .with_review_timeout(|driver| driver.rendered_section_heading(node))
                                .ok()
                                .flatten();
                            checked_errors.insert(node, text.clone());
                            text
                        })
                        .collect::<Option<Vec<_>>>()
                        .map(|parts| parts.join("; "))
                });
                let suffix = match details {
                    Some(Some(text)) => format!(" — {text}"),
                    Some(None) => " — site error detail unavailable".into(),
                    None => String::new(),
                };
                walls.push(format!(
                    "{INVALID_FIELD_REVIEW_PREFIX}{short_label}{suffix}"
                ));
            }

            let mut required = control.attrs.contains_key("required")
                || control
                    .attrs
                    .get("aria-required")
                    .is_some_and(|v| v.eq_ignore_ascii_case("true"))
                || radio_group_required
                    .get(&control.node_id)
                    .copied()
                    .unwrap_or(false);
            if !required && !label_unverified && visible_label_required_marker {
                required = true;
                walls.push("a custom control needs manual review: A visible required marker was inferred from the label; verify the field requirement".into());
            }
            if !required
                && !label_unverified
                && !labels.contains_key(&id)
                && !wrapping_labels.contains_key(&control.node_id)
                && !control.attrs.contains_key("aria-labelledby")
                && !control.attrs.contains_key("aria-label")
                && control
                    .attrs
                    .get("placeholder")
                    .is_some_and(|value| !value.trim().is_empty())
            {
                if required_marker_checks < 24 && marker_started.elapsed() < Duration::from_secs(4)
                {
                    required_marker_checks += 1;
                    match self.with_review_timeout(|driver| driver.nearby_required_marker(control.node_id)) {
                        Ok(Some(true)) => {
                            required = true;
                            walls.push("a custom control needs manual review: A nearby visible required marker was inferred; verify the field requirement".into());
                        }
                        Ok(Some(false)) => {},
                        _ => walls.push("a custom control needs manual review: A nearby required marker could not be checked".into()),
                    }
                } else {
                    walls.push("a custom control needs manual review: Required marker checks reached their limit".into());
                }
            }
            if control.attrs.contains_key("disabled")
                || control.attrs.contains_key("readonly")
                || control
                    .attrs
                    .get("aria-disabled")
                    .is_some_and(|value| value.eq_ignore_ascii_case("true"))
                || control
                    .attrs
                    .get("aria-readonly")
                    .is_some_and(|value| value.eq_ignore_ascii_case("true"))
            {
                if required {
                    walls.push("a required field is disabled or read-only".into());
                }
                continue;
            }
            let may_be_unannotated_choice = if possible_choice_nodes.contains(&control.node_id) {
                if unannotated_choice_checks < 64 {
                    unannotated_choice_checks += 1;
                    self.with_review_timeout(|driver| {
                        driver.unannotated_choice_hint(control.node_id)
                    })
                    .ok()
                    .flatten()
                    .unwrap_or(true)
                } else {
                    true
                }
            } else {
                false
            };
            if may_be_unannotated_choice {
                walls.push(format!("a custom control needs manual review: {label}"));
            }
            // These native widgets cannot be driven by the text-field path.
            // Keep them visible in the schema, but require human handling.
            let unsupported_native =
                control.tag == "input" && matches!(input_type.as_str(), "week" | "range" | "color");
            if unsupported_native {
                let prefix = if required {
                    "a required custom control needs manual review"
                } else {
                    "a custom control needs manual review"
                };
                walls.push(format!(
                    "{prefix}: {}",
                    label.chars().take(160).collect::<String>()
                ));
            }
            let checkbox_context_needed =
                input_type == "checkbox" && !field_containers.contains_key(&control.node_id);
            let checkbox_context_limit_hit =
                checkbox_context_needed && unassociated_question_count >= 16;
            let unassociated_checkbox_group =
                if checkbox_context_needed && !checkbox_context_limit_hit {
                    unassociated_question_count += 1;
                    self.with_review_timeout(|driver| {
                        driver.rendered_unassociated_choice_question(control.node_id, "checkbox")
                    })
                    .ok()
                    .flatten()
                } else {
                    None
                };
            if let Some(question) = &unassociated_checkbox_group {
                label = if question.is_empty() {
                    format!("Question context unavailable (review): {label}")
                } else {
                    format!("Possible question (review): {question} — {label}")
                };
            }
            if checkbox_context_limit_hit {
                label = format!("Question context not inspected (review): {label}");
                walls.push("a custom control needs manual review: Checkbox question context exceeded the inspection limit".into());
            }
            let checkbox_text = if input_type == "checkbox"
                && !has_usable_label
                && unassociated_checkbox_group.is_none()
                && !checkbox_context_limit_hit
                && unassociated_question_count < 16
            {
                unassociated_question_count += 1;
                self.with_review_timeout(|driver| {
                    driver.rendered_unassociated_question(control.node_id, true)
                })
                .ok()
                .flatten()
            } else {
                None
            };
            if let Some(text) = &checkbox_text {
                label = format!("Possible checkbox text (review): {text}");
                walls.push("a custom control needs manual review: Nearby checkbox text is not an accessible label".into());
            }
            let sensitive = checkbox_text.is_some()
                || label_unverified
                || may_be_unannotated_choice
                || unsupported_native
                || unassociated_textarea
                || unassociated_checkbox_group.is_some()
                || checkbox_context_limit_hit
                || is_sensitive_label(&label)
                || input_type == "password";
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
                            context: Default::default(),
                            selector: selector.clone(),
                            label: group_label.clone(),
                            field_type: FormFieldType::Radio {
                                options: vec![label],
                            },
                            required,
                            sensitive: sensitive || is_sensitive_label(&group_label),
                            editing_mode: None,
                            max_length_utf16: None,
                            min_length_utf16: None,
                            pattern: None,
                            autocomplete: None,
                        });
                    }
                    continue;
                }
                if let (Some(form_id), Some(name)) = (
                    radio_form_owners.get(&control.node_id),
                    control
                        .attrs
                        .get("name")
                        .filter(|name| !name.is_empty() && name.len() <= 512),
                ) {
                    let key = (*form_id, name.clone());
                    if let Some(index) = radio_groups.get(&key).copied() {
                        let field: &mut waypoint_applications::form_mapping::FormField =
                            &mut fields[index];
                        if let FormFieldType::Radio { options } = &mut field.field_type {
                            options.push(label);
                        }
                        field.required |= required;
                    } else {
                        radio_groups.insert(key, fields.len());
                        let context = if unassociated_question_count < 16 {
                            unassociated_question_count += 1;
                            self.with_review_timeout(|driver| {
                                driver
                                    .rendered_unassociated_choice_question(control.node_id, "radio")
                            })
                            .ok()
                            .flatten()
                        } else {
                            None
                        };
                        let context = context.filter(|text| !text.is_empty());
                        fields.push(FormField {
                            context: Default::default(),
                            selector,
                            label: context.map_or_else(
                                || format!("Unlabelled radio group: {name}"),
                                |text| format!("Possible question (review): {text}"),
                            ),
                            field_type: FormFieldType::Radio {
                                options: vec![label],
                            },
                            required,
                            sensitive: true,
                            editing_mode: None,
                            max_length_utf16: None,
                            min_length_utf16: None,
                            pattern: None,
                            autocomplete: None,
                        });
                    }
                    continue;
                }
            }
            let native_datalist = control.tag == "input"
                && native_datalist_options(&control.attrs, &datalists).is_some();
            if native_datalist && !walls.iter().any(|wall| wall == NATIVE_DATALIST_REVIEW) {
                walls.push(NATIVE_DATALIST_REVIEW.into());
            }
            if input_type == "number"
                && ["min", "max", "step"].iter().any(|key| {
                    control
                        .attrs
                        .get(*key)
                        .is_some_and(|value| value.len() > 128)
                })
            {
                walls.push(
                    "numeric constraints exceed the inspection limit; review manually".into(),
                );
            }
            let field_type = match (control.tag.as_str(), input_type.as_str()) {
                ("input", _) if native_datalist => FormFieldType::Select {
                    options: control.options.clone(),
                },
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
                (_, "number") => FormFieldType::Number {
                    min: control
                        .attrs
                        .get("min")
                        .filter(|value| value.len() <= 128)
                        .cloned(),
                    max: control
                        .attrs
                        .get("max")
                        .filter(|value| value.len() <= 128)
                        .cloned(),
                    step: control
                        .attrs
                        .get("step")
                        .filter(|value| value.len() <= 128)
                        .cloned(),
                },
                (_, "email") => FormFieldType::Email,
                (_, "date") => FormFieldType::Date,
                (_, "month") => FormFieldType::Month,
                (_, "time") => FormFieldType::Time,
                (_, "datetime-local") => FormFieldType::DateTimeLocal,
                ("textarea", _) => FormFieldType::TextArea,
                (_, _) if supported_editing_host(&control.tag, &control.attrs) => {
                    FormFieldType::TextArea
                }
                _ => FormFieldType::Text,
            };
            let pattern = if control.tag == "input"
                && matches!(
                    input_type.as_str(),
                    "text" | "email" | "search" | "tel" | "url"
                ) {
                match control.attrs.get("pattern") {
                    Some(value) if value.len() <= 512 => Some(value.clone()),
                    Some(_) => {
                        walls.push("A field format pattern exceeds the inspection limit; review it manually".into());
                        None
                    }
                    None => None,
                }
            } else {
                None
            };
            fields.push(FormField {
                context: waypoint_applications::form_mapping::FormFieldContext {
                    sections: section_path,
                },
                selector,
                label,
                field_type,
                required,
                sensitive,
                editing_mode: if supported_editing_host(&control.tag, &control.attrs) {
                    Some(
                        if control
                            .attrs
                            .get("contenteditable")
                            .is_some_and(|value| value.eq_ignore_ascii_case("plaintext-only"))
                        {
                            waypoint_applications::form_mapping::EditingMode::PlainText
                        } else {
                            waypoint_applications::form_mapping::EditingMode::RichText
                        },
                    )
                } else {
                    None
                },
                max_length_utf16: if control.tag == "textarea"
                    || (control.tag == "input"
                        && matches!(
                            input_type.as_str(),
                            "text" | "email" | "search" | "tel" | "url"
                        )) {
                    control
                        .attrs
                        .get("maxlength")
                        .and_then(|value| value.trim().parse::<u32>().ok())
                } else {
                    None
                },
                min_length_utf16: if control.tag == "textarea"
                    || (control.tag == "input"
                        && matches!(
                            input_type.as_str(),
                            "text" | "email" | "search" | "tel" | "url"
                        )) {
                    control
                        .attrs
                        .get("minlength")
                        .and_then(|value| value.trim().parse::<u32>().ok())
                } else {
                    None
                },
                pattern,
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

    /// Check a number against Chromium's current native constraints on a
    /// detached clone, so an invalid candidate does not alter the live input.
    fn check_number_candidate(&mut self, object: &str, candidate: &str) -> Result<(), DriverError> {
        let result = self
            .cdp
            .call(
                "Runtime.callFunctionOn",
                json!({"objectId":object,"functionDeclaration":r#"function(value) {
            if (!this.isConnected) return false;
            if (!(this instanceof HTMLInputElement) || this.type !== 'number') return true;
            const probe = this.cloneNode(false);
            probe.value = value;
            return probe.value === value && probe.validity.valid;
        }"#,"arguments":[{"value":candidate}],"returnByValue":true}),
            )
            .map_err(derr)?;
        if result.get("exceptionDetails").is_some()
            || result.pointer("/result/value").and_then(Value::as_bool) != Some(true)
        {
            return Err(DriverError(
                "Number does not satisfy the field's current constraints; review it manually"
                    .into(),
            ));
        }
        Ok(())
    }

    /// Native constraints can change after inspection or on focus. Evaluate a
    /// candidate without dispatching input events to the employer's page.
    fn check_text_candidate(&mut self, object: &str, candidate: &str) -> Result<(), DriverError> {
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function(value) {
            if (!this.isConnected) return 'unknown';
            if (!(this instanceof HTMLInputElement || this instanceof HTMLTextAreaElement)) return 'ok';
            if (this instanceof HTMLInputElement && this.type === 'number') return 'ok';
            const probe = this.cloneNode(false);
            probe.value = value;
            if (probe.value !== value) return 'input';
            if (probe.maxLength >= 0 && value.length > probe.maxLength) return 'long';
            if (value.length > 0 && probe.minLength >= 0 && value.length < probe.minLength) return 'short';
            if (probe.validity.valueMissing) return 'missing';
            if (probe.validity.typeMismatch) return 'type';
            if (probe.validity.patternMismatch) return 'pattern';
            return 'ok';
        }"#,"arguments":[{"value":candidate}],"returnByValue":true})).map_err(derr)?;
        let reason = match result.pointer("/result/value").and_then(Value::as_str) {
            Some("ok") if result.get("exceptionDetails").is_none() => return Ok(()),
            Some("missing") => "a value is required",
            Some("type") => "the value does not match the expected type",
            Some("pattern") => "the value does not match the required format",
            Some("short") => "the value is too short",
            Some("long") => "the value is too long",
            Some("input") => "the browser cannot preserve the value",
            _ => "the field constraints could not be checked",
        };
        Err(DriverError(format!("Text field needs review: {reason}")))
    }

    fn ensure_text_focus(&mut self, node: u64, object: &str) -> Result<(), DriverError> {
        if !self.control_visibility_and_path(node)?.0 {
            return Err(DriverError(
                "Text field is no longer visible; review it manually".into(),
            ));
        }
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function() { return this.isConnected && this.getRootNode().activeElement === this && !this.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]') && (((this instanceof HTMLTextAreaElement || (this instanceof HTMLInputElement && ['text','email','tel','url','search','number'].includes(this.type))) && !this.matches(':disabled') && !this.readOnly && !this.hasAttribute('list') && this.getAttribute('role')?.toLowerCase() !== 'combobox' && !['list','both','inline'].includes(this.getAttribute('aria-autocomplete')?.toLowerCase())) || ((this.getAttribute('contenteditable')?.toLowerCase() === 'plaintext-only' || (['','true'].includes(this.getAttribute('contenteditable')?.toLowerCase()) && this.getAttribute('role')?.toLowerCase() === 'textbox' && (this.getAttribute('aria-label')?.trim() || this.getAttribute('aria-labelledby')?.trim()))) && this.isContentEditable && !this.querySelector('input,textarea,select,button,[contenteditable]'))); }","returnByValue":true})).map_err(derr)?;
        if result.get("exceptionDetails").is_some()
            || result.pointer("/result/value").and_then(Value::as_bool) != Some(true)
        {
            return Err(DriverError(
                "Text field lost focus or changed type; review it manually".into(),
            ));
        }
        Ok(())
    }

    /// Report only fixed browser validity categories, never site-provided text
    /// or the candidate's value. Failure to inspect remains an unknown reason.
    fn native_text_validation_reason(
        &mut self,
        object: &str,
        expected: &str,
    ) -> Option<&'static str> {
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function(expected) {
            if (!this.isConnected || !(this instanceof HTMLInputElement || this instanceof HTMLTextAreaElement) || this.value !== expected || !this.willValidate || this.validity.valid) return null;
            const v = this.validity;
            if (v.valueMissing) return 'missing';
            if (v.typeMismatch) return 'type';
            if (v.patternMismatch) return 'pattern';
            if (v.tooShort) return 'short';
            if (v.tooLong) return 'long';
            if (v.rangeUnderflow) return 'under';
            if (v.rangeOverflow) return 'over';
            if (v.stepMismatch) return 'step';
            if (v.badInput) return 'input';
            if (v.customError) return 'custom';
            return 'other';
        }"#, "arguments":[{"value":expected}], "returnByValue":true})).ok()?;
        if result.get("exceptionDetails").is_some() {
            return None;
        }
        match result.pointer("/result/value").and_then(Value::as_str)? {
            "missing" => Some("a value is required"),
            "type" => Some("the value does not match the expected type"),
            "pattern" => Some("the value does not match the required format"),
            "short" => Some("the value is too short"),
            "long" => Some("the value is too long"),
            "under" => Some("the value is below the allowed minimum"),
            "over" => Some("the value is above the allowed maximum"),
            "step" => Some("the value does not match the allowed increment"),
            "input" => Some("the browser cannot interpret the value"),
            "custom" => Some("the site supplied a custom validation error"),
            "other" => Some("the browser reports invalid input"),
            _ => None,
        }
    }

    fn fill(&mut self, selector: &str, value: &PacketValue) -> Result<(), DriverError> {
        let Some(node) = self.node_id(selector)? else {
            return Err(DriverError(format!("field {selector} not found")));
        };
        if matches!(
            value,
            PacketValue::Text(_) | PacketValue::Bool(_) | PacketValue::Date(_)
        ) && !self.control_visibility_and_path(node)?.0
        {
            return Err(DriverError(
                "Field is no longer visibly rendered; review it manually".into(),
            ));
        }
        match value {
            PacketValue::FileRef { name } => {
                let path = self.resolve_upload(name)?;
                self.attach_local_file(selector, &path)?;
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
                let check = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function() { return (this.isConnected && !this.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]')) && ((this instanceof HTMLTextAreaElement || (this instanceof HTMLInputElement && ['text','email','tel','url','search','number'].includes(this.type))) && !this.matches(':disabled') && !this.readOnly && !this.hasAttribute('list') && this.getAttribute('role')?.toLowerCase() !== 'combobox' && !['list','both','inline'].includes(this.getAttribute('aria-autocomplete')?.toLowerCase()) || (this.getAttribute('contenteditable')?.toLowerCase() === 'plaintext-only' || (['','true'].includes(this.getAttribute('contenteditable')?.toLowerCase()) && this.getAttribute('role')?.toLowerCase() === 'textbox' && (this.getAttribute('aria-label')?.trim() || this.getAttribute('aria-labelledby')?.trim()))) && this.isContentEditable && !this.querySelector('input,textarea,select,button,[contenteditable]') && this.getAttribute('aria-disabled') !== 'true' && this.getAttribute('aria-readonly') !== 'true'); }","returnByValue":true})).map_err(derr)?;
                if check.get("exceptionDetails").is_some()
                    || check.pointer("/result/value").and_then(Value::as_bool) != Some(true)
                {
                    return Err(DriverError(
                        "Text field is unavailable or requires manual handling".into(),
                    ));
                }
                self.check_number_candidate(object, t)?;
                self.check_text_candidate(object, t)?;
                let rich = if t.contains('\n') {
                    let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function() { return ['','true'].includes(this.getAttribute('contenteditable')?.toLowerCase()) && this.getAttribute('role')?.toLowerCase() === 'textbox'; }","returnByValue":true})).map_err(derr)?;
                    if result.get("exceptionDetails").is_some() {
                        return Err(DriverError(
                            "Text editor could not be inspected; review it manually".into(),
                        ));
                    }
                    result
                        .pointer("/result/value")
                        .and_then(Value::as_bool)
                        .ok_or_else(|| {
                            DriverError(
                                "Text editor could not be inspected; review it manually".into(),
                            )
                        })?
                } else {
                    false
                };
                if rich && t.bytes().filter(|byte| *byte == b'\n').count() > 40 {
                    return Err(DriverError(
                        "Rich text has too many lines; review it manually".into(),
                    ));
                }
                // Observation is partial and cannot prevent a page from sending data.
                let network_before = self.observed_non_read_requests()?;
                // Focus the control and insert text through Chromium input.
                // Input.insertText does not reproduce physical keystroke events.
                // Replace existing content instead of appending to it.
                self.cdp
                    .call("DOM.focus", json!({ "nodeId": node }))
                    .map_err(derr)?;
                let focus = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function() { return this.getRootNode().activeElement === this && !this.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]') && !((this instanceof HTMLInputElement || this instanceof HTMLTextAreaElement) && (this.hasAttribute('list') || this.getAttribute('role')?.toLowerCase() === 'combobox' || ['list','both','inline'].includes(this.getAttribute('aria-autocomplete')?.toLowerCase()))); }","returnByValue":true})).map_err(derr)?;
                if focus.pointer("/result/value").and_then(Value::as_bool) != Some(true) {
                    return Err(DriverError(
                        "Text field did not retain safe focus or changed to a choice control; review it manually".into(),
                    ));
                }
                if !self.control_visibility_and_path(node)?.0 {
                    return Err(DriverError(
                        "Field became hidden on focus; review it manually".into(),
                    ));
                }
                // Focus handlers may change min, max or step.
                self.check_number_candidate(object, t)?;
                self.check_text_candidate(object, t)?;
                self.cdp.call("Input.dispatchKeyEvent", json!({"type":"rawKeyDown","key":"a","code":"KeyA","windowsVirtualKeyCode":65,"modifiers":2})).map_err(derr)?;
                self.cdp.call("Input.dispatchKeyEvent", json!({"type":"keyUp","key":"a","code":"KeyA","windowsVirtualKeyCode":65,"modifiers":2})).map_err(derr)?;
                self.ensure_text_focus(node, object)?;
                self.cdp.call("Input.dispatchKeyEvent", json!({"type":"rawKeyDown","key":"Backspace","code":"Backspace","windowsVirtualKeyCode":8})).map_err(derr)?;
                self.cdp.call("Input.dispatchKeyEvent", json!({"type":"keyUp","key":"Backspace","code":"Backspace","windowsVirtualKeyCode":8})).map_err(derr)?;
                self.ensure_text_focus(node, object)?;
                self.stop_if_non_read_request_observed(network_before)?;
                if rich {
                    for (index, line) in t.split('\n').enumerate() {
                        self.ensure_text_focus(node, object)?;
                        self.stop_if_non_read_request_observed(network_before)?;
                        if index > 0 {
                            self.cdp.call("Input.dispatchKeyEvent", json!({"type":"rawKeyDown","key":"Enter","code":"Enter","windowsVirtualKeyCode":13,"modifiers":8})).map_err(derr)?;
                            self.ensure_text_focus(node, object)?;
                            self.cdp.call("Input.dispatchKeyEvent", json!({"type":"char","key":"Enter","code":"Enter","text":"\r","unmodifiedText":"\r","windowsVirtualKeyCode":13,"modifiers":8})).map_err(derr)?;
                            self.cdp.call("Input.dispatchKeyEvent", json!({"type":"keyUp","key":"Enter","code":"Enter","windowsVirtualKeyCode":13,"modifiers":8})).map_err(derr)?;
                        }
                        if !line.is_empty() {
                            for segment in text_segments(line) {
                                self.ensure_text_focus(node, object)?;
                                self.stop_if_non_read_request_observed(network_before)?;
                                self.cdp
                                    .call("Input.insertText", json!({"text":segment}))
                                    .map_err(derr)?;
                            }
                        }
                    }
                } else if !t.is_empty()
                    && t.len() <= 64
                    && t.chars().all(|character| ascii_key(character).is_some())
                {
                    // Short, simple text can use individual key events. For
                    // other text retain Chromium's Unicode-aware insertion.
                    for character in t.chars() {
                        let (code, key_code, modifiers, unmodified) = ascii_key(character)
                            .ok_or_else(|| DriverError("Unsupported keyboard character".into()))?;
                        let key = character.to_string();
                        self.ensure_text_focus(node, object)?;
                        self.stop_if_non_read_request_observed(network_before)?;
                        self.cdp.call("Input.dispatchKeyEvent", json!({"type":"rawKeyDown","key":key,"code":code,"windowsVirtualKeyCode":key_code,"modifiers":modifiers})).map_err(derr)?;
                        self.ensure_text_focus(node, object)?;
                        self.cdp.call("Input.dispatchKeyEvent", json!({"type":"char","key":key,"code":code,"text":key,"unmodifiedText":unmodified.to_string(),"windowsVirtualKeyCode":key_code,"modifiers":modifiers})).map_err(derr)?;
                        self.ensure_text_focus(node, object)?;
                        self.cdp.call("Input.dispatchKeyEvent", json!({"type":"keyUp","key":key,"code":code,"windowsVirtualKeyCode":key_code,"modifiers":modifiers})).map_err(derr)?;
                    }
                } else {
                    for segment in text_segments(t) {
                        self.ensure_text_focus(node, object)?;
                        self.stop_if_non_read_request_observed(network_before)?;
                        self.cdp
                            .call("Input.insertText", json!({ "text": segment }))
                            .map_err(derr)?;
                    }
                }
                // The final input event can itself hide or replace the control,
                // move focus, or display an overlay. Do not report a completed
                // fill until the same field is still visible and focused.
                self.ensure_text_focus(node, object)?;
                self.stop_if_non_read_request_observed(network_before)?;
                let retained = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function(value) { return (this.isConnected && !this.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]')) && ((this.getAttribute('contenteditable')?.toLowerCase() === 'plaintext-only' || (['','true'].includes(this.getAttribute('contenteditable')?.toLowerCase()) && this.getAttribute('role')?.toLowerCase() === 'textbox' && (this.getAttribute('aria-label')?.trim() || this.getAttribute('aria-labelledby')?.trim()))) ? this.isContentEditable && !this.querySelector('input,textarea,select,button,[contenteditable]') && this.getAttribute('aria-disabled') !== 'true' && this.getAttribute('aria-readonly') !== 'true' && this.innerText === value : !this.matches(':disabled') && !this.readOnly && !this.hasAttribute('list') && this.getAttribute('role')?.toLowerCase() !== 'combobox' && !['list','both','inline'].includes(this.getAttribute('aria-autocomplete')?.toLowerCase()) && this.value === value && this.validity.valid); }","arguments":[{"value":t}],"returnByValue":true})).map_err(derr)?;
                if retained.get("exceptionDetails").is_some()
                    || retained.pointer("/result/value").and_then(Value::as_bool) != Some(true)
                {
                    if let Some(reason) = self.native_text_validation_reason(object, t) {
                        return Err(DriverError(format!("The field retained the text but failed a browser constraint: {reason}. Review it manually")));
                    }
                    return Err(DriverError(
                        "Text was not retained exactly or the field is invalid; review it manually"
                            .into(),
                    ));
                }
                // Sites can report an error through ARIA even when native
                // constraint validation succeeds and the value is retained.
                self.verify_aria_validity(node, false)?;
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
                let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function(value) { if ((!this.isConnected || this.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]')) || this.tagName !== 'INPUT' || this.type !== 'checkbox' || this.matches(':disabled')) return false; if (this.checked !== value) { const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'checked').set; set.call(this, value); this.dispatchEvent(new Event('input', {bubbles:true})); this.dispatchEvent(new Event('change', {bubbles:true})); } return (this.isConnected && !this.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]')) && !this.matches(':disabled') && this.checked === value && this.validity.valid; }","arguments":[{"value":b}],"returnByValue":true})).map_err(derr)?;
                if result.get("exceptionDetails").is_some()
                    || result.pointer("/result/value").and_then(Value::as_bool) != Some(true)
                {
                    return Err(DriverError(
                        "Checkbox was unavailable, invalid, or its value was not retained".into(),
                    ));
                }
                self.verify_aria_validity(node, false)
            }
            PacketValue::Date(d) => {
                let expected_type = if waypoint_applications::form_mapping::valid_iso_date(d) {
                    "date"
                } else if waypoint_applications::form_mapping::valid_iso_month(d) {
                    "month"
                } else if waypoint_applications::form_mapping::valid_iso_time(d) {
                    "time"
                } else if waypoint_applications::form_mapping::valid_iso_datetime_local(d) {
                    "datetime-local"
                } else {
                    return Err(DriverError("Invalid calendar date, month or time".into()));
                };
                let resolved = self
                    .cdp
                    .call("DOM.resolveNode", json!({"nodeId":node}))
                    .map_err(derr)?;
                let object = resolved
                    .pointer("/object/objectId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| DriverError("Cannot inspect date field".into()))?;
                let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function(value, expectedType) { if ((!this.isConnected || this.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]')) || this.tagName !== 'INPUT' || this.type !== expectedType || this.matches(':disabled') || this.readOnly) return false; const previous = this.value; const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set; set.call(this, value); if (this.value !== value || !this.validity.valid) { set.call(this, previous); return false; } this.dispatchEvent(new Event('input', {bubbles:true})); this.dispatchEvent(new Event('change', {bubbles:true})); return (this.isConnected && !this.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]')) && !this.matches(':disabled') && !this.readOnly && this.value === value && this.validity.valid; }","arguments":[{"value":d},{"value":expected_type}],"returnByValue":true})).map_err(derr)?;
                if result.get("exceptionDetails").is_some()
                    || result.pointer("/result/value").and_then(Value::as_bool) != Some(true)
                {
                    return Err(DriverError(
                        "Date is unavailable, outside the field's limits, or was not retained"
                            .into(),
                    ));
                }
                self.verify_aria_validity(node, false)
            }
            PacketValue::TextList(_) => Err(DriverError("Use a multiple-choice command".into())),
            PacketValue::None => Ok(()),
        }
    }

    // ARIA validation is independent of native HTML constraint validation.
    // Radio-group errors may be reported on the group or the checked radio.
    fn verify_aria_validity(&mut self, node: u64, radio: bool) -> Result<(), DriverError> {
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId": node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Cannot inspect field validation state".into()))?;
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function(radio) {
            const invalid = element => { const value = (element.getAttribute('aria-invalid') || '').trim().toLowerCase(); return value !== '' && value !== 'false'; };
            if (!this.isConnected || invalid(this)) return false;
            if (!radio) return true;
            const group = this.closest('fieldset,[role="radiogroup"]');
            if (!group || invalid(group)) return false;
            const radios = group.querySelectorAll('input[type=radio]'); if (radios.length > 5000) return false;
            const checked = Array.from(radios).filter(r => r.name === this.name && r.form === this.form && r.checked);
            return checked.length === 1 && !invalid(checked[0]);
        }"#,"arguments":[{"value":radio}],"returnByValue":true}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let result = result.map_err(derr)?;
        released.map_err(derr)?;
        if result.get("exceptionDetails").is_some()
            || result.pointer("/result/value").and_then(Value::as_bool) != Some(true)
        {
            return Err(DriverError(
                "The site reports this field as invalid; review it manually".into(),
            ));
        }
        Ok(())
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
        if apply && !self.control_visibility_and_path(node)?.0 {
            return Err(DriverError(
                "Field is no longer visibly rendered; review it manually".into(),
            ));
        }
        let result = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object_id = result
            .get("object")
            .and_then(|v| v.get("objectId"))
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Cannot inspect select field".into()))?;
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object_id,"functionDeclaration":"function(label, apply) { const visible = node => { if (!node.getClientRects().length) return false; for (let item = node, depth = 0; item; item = item.parentElement) { if (++depth > 64) return false; const style = getComputedStyle(item); if (style.visibility === 'hidden' || style.visibility === 'collapse' || Number(style.opacity) === 0) return false; } return true; }; if (!apply && !visible(this)) return false; if (this instanceof HTMLInputElement && this.hasAttribute('list')) { const input = this, root = input.getRootNode(), list = input.list, listId = input.getAttribute('list'); const safe = () => input.isConnected && !input.matches(':disabled') && !input.readOnly && !input.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]') && ['text','search','email','tel','url'].includes(input.type) && !input.hasAttribute('role') && !input.hasAttribute('aria-autocomplete') && !input.multiple && list instanceof HTMLDataListElement && list.isConnected && !list.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]') && input.list === list && root.querySelectorAll('#' + CSS.escape(listId)).length === 1; if (!safe()) return false; const options = Array.from(list.options); if (!options.length || options.length > 100 || options.some(o => o.disabled || o.closest('optgroup[disabled], [aria-disabled=\"true\" i], [aria-readonly=\"true\" i]') || (!o.hasAttribute('value') && (o.childNodes.length !== 1 || o.firstChild.nodeType !== Node.TEXT_NODE || o.firstChild.data !== o.value)) || !o.value || o.value.length > 200 || o.value.trim() !== o.value)) return false; const values = options.map(o => o.value); if (new Set(values.map(v => v.toLowerCase())).size !== values.length) return false; const matches = options.filter(o => o.value.toLowerCase() === label.toLowerCase()); if (matches.length !== 1) return false; const chosen = matches[0]; if (apply) { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, chosen.value); input.dispatchEvent(new Event('input', {bubbles:true})); input.dispatchEvent(new Event('change', {bubbles:true})); } return safe() && input.validity.valid && input.value === chosen.value && Array.from(list.options).length === options.length && Array.from(list.options).every((o,i) => o === options[i] && !o.disabled && !o.closest('optgroup[disabled], [aria-disabled=\"true\" i], [aria-readonly=\"true\" i]') && o.value === values[i]); } if (this.tagName !== 'SELECT' || this.multiple || this.matches(':disabled') || (!this.isConnected || this.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]'))) return false; const options = Array.from(this.options); const matches = options.filter(o => o.label.trim().replace(/\\s+/g, ' ').toLowerCase() === label.trim().replace(/\\s+/g, ' ').toLowerCase() && !o.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]') && !o.disabled && !(o.parentElement && o.parentElement.tagName === 'OPTGROUP' && o.parentElement.disabled)); if (matches.length !== 1 || options.filter(o => o.value === matches[0].value).length !== 1) return false; const values = options.map(o => o.value), optionLabels = options.map(o => o.label); if (apply) { this.value = matches[0].value; this.dispatchEvent(new Event('input', {bubbles:true})); this.dispatchEvent(new Event('change', {bubbles:true})); } return (this.isConnected && !this.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]')) && !this.matches(':disabled') && this.selectedOptions.length === 1 && this.selectedOptions[0] === matches[0] && this.options.length === options.length && Array.from(this.options).every((o,i) => o === options[i] && o.value === values[i] && o.label === optionLabels[i]) && !matches[0].closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]') && !matches[0].disabled && !(matches[0].parentElement && matches[0].parentElement.tagName === 'OPTGROUP' && matches[0].parentElement.disabled) && this.validity.valid; }","arguments":[{"value":label},{"value":apply}],"returnByValue":true}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object_id}));
        let result = result.map_err(derr)?;
        released.map_err(derr)?;
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
        if apply && !self.control_visibility_and_path(node)?.0 {
            return Err(DriverError(
                "Field is no longer visibly rendered; review it manually".into(),
            ));
        }
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Cannot inspect select field".into()))?;
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function(labels, apply) {
            const visible = node => { if (!node.getClientRects().length) return false; for (let item = node, depth = 0; item; item = item.parentElement) { if (++depth > 64) return false; const style = getComputedStyle(item); if (style.visibility === 'hidden' || style.visibility === 'collapse' || Number(style.opacity) === 0) return false; } return true; }; if (!apply && !visible(this)) return false;
            if (!(this instanceof HTMLSelectElement) || !this.multiple || this.matches(':disabled') || (!this.isConnected || this.closest('[aria-disabled="true" i], [aria-readonly="true" i]'))) return false;
            const norm = s => s.trim().replace(/\s+/g, ' ').toLowerCase();
            const options = Array.from(this.options);
            const values = options.map(o => o.value), optionLabels = options.map(o => o.label);
            const chosen = [];
            for (const label of labels) {
                const matches = options.filter(o => norm(o.label) === norm(label) && !o.closest('[aria-disabled="true" i], [aria-readonly="true" i]') && !o.disabled && !(o.parentElement && o.parentElement.tagName === 'OPTGROUP' && o.parentElement.disabled));
                if (matches.length !== 1 || chosen.includes(matches[0]) || options.filter(o => o.value === matches[0].value).length !== 1) return false;
                chosen.push(matches[0]);
            }
            if (apply) {
                for (const option of options) option.selected = chosen.includes(option);
                this.dispatchEvent(new Event('input', {bubbles:true}));
                this.dispatchEvent(new Event('change', {bubbles:true}));
            }
            return (this.isConnected && !this.closest('[aria-disabled="true" i], [aria-readonly="true" i]')) && !this.matches(':disabled') && this.validity.valid && Array.from(this.options).length === options.length && Array.from(this.options).every((o, i) => o === options[i] && o.value === values[i] && o.label === optionLabels[i] && o.selected === chosen.includes(o)) && chosen.every(o => !o.closest('[aria-disabled="true" i], [aria-readonly="true" i]') && !o.disabled && !(o.parentElement && o.parentElement.tagName === 'OPTGROUP' && o.parentElement.disabled));
        }"#,"arguments":[{"value":labels},{"value":apply}],"returnByValue":true}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let result = result.map_err(derr)?;
        released.map_err(derr)?;
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
        self.radio_selection(selector, label, apply, false)
            .map(drop)
    }

    fn radio_selection(
        &mut self,
        selector: &str,
        label: &str,
        apply: bool,
        capture_node: bool,
    ) -> Result<Option<u64>, DriverError> {
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
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function(label, apply, captureNode) {
          if (this.tagName !== 'INPUT' || this.type !== 'radio' || !this.name) return false;
          const selector = 'fieldset,[role="radiogroup"]';
          const root = this.getRootNode();
          const group = this.closest(selector); if (!group) return false;
          const norm = s => s.trim().replace(/\s+/g, ' ').toLowerCase();
          const optionLabel = r => {
            const refs = r.getAttribute('aria-labelledby');
            if (refs !== null) {
              const ids = refs.trim().split(/\s+/).filter(Boolean);
              if (!ids.length || ids.length > 16 || refs.length > 512) return null;
              const parts = [];
              for (const id of ids) {
                const nodes = root.querySelectorAll('#' + CSS.escape(id));
                if (nodes.length !== 1 || !nodes[0].textContent.trim()) return null;
                parts.push(nodes[0].textContent);
              }
              return parts.join(' ');
            }
            return r.getAttribute('aria-label') || (r.labels && r.labels[0] && r.labels[0].textContent) || r.getAttribute('name') || '';
          };
          const unavailable = r => r.matches(':disabled') || !!r.closest('[aria-disabled="true" i], [aria-readonly="true" i]');
          const radios = Array.from(group.querySelectorAll('input[type=radio]')).filter(r => r.closest(selector) === group && r.name === this.name && r.form === this.form && !unavailable(r));
          const allRadios = root.querySelectorAll('input[type=radio]'); if (allRadios.length > 5000) return false;
          const sameName = Array.from(allRadios).filter(r => r.name === this.name && r.form === this.form);
          if (sameName.some(r => r.closest(selector) !== group)) return false;
          const choices = radios.filter(r => { const text = optionLabel(r); return text !== null && norm(text) === norm(label); });
          if (choices.length !== 1) return false;
          const chosen = choices[0];
          const visible = node => { if (!node.getClientRects().length) return false; const rect = node.getBoundingClientRect(); let left = rect.left, right = rect.right, top = rect.top, bottom = rect.bottom; for (let item = node, depth = 0; item; item = item.parentElement) { if (++depth > 64) return false; const style = getComputedStyle(item); if (style.visibility === 'hidden' || style.visibility === 'collapse' || Number(style.opacity) === 0) return false; if (item !== node) { const box = item.getBoundingClientRect(); if (['hidden','clip'].includes(style.overflowX)) { left = Math.max(left, box.left + item.clientLeft); right = Math.min(right, box.left + item.clientLeft + item.clientWidth); } if (['hidden','clip'].includes(style.overflowY)) { top = Math.max(top, box.top + item.clientTop); bottom = Math.min(bottom, box.top + item.clientTop + item.clientHeight); } } } return right > left && bottom > top; }; if (!visible(chosen)) return false;
          if (apply && !chosen.checked) { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'checked').set.call(chosen, true); chosen.dispatchEvent(new Event('input', {bubbles:true})); chosen.dispatchEvent(new Event('change', {bubbles:true})); }
          if (!chosen.isConnected || !chosen.checked || unavailable(chosen) || !chosen.validity.valid) return false;
          return captureNode ? chosen : true;
        }"#,"arguments":[{"value":label},{"value":apply},{"value":capture_node}],"returnByValue":!capture_node}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let result = result.map_err(derr)?;
        released.map_err(derr)?;
        let unavailable =
            || DriverError("Radio option is unavailable, ambiguous, or was not retained".into());
        if result.get("exceptionDetails").is_some() {
            return Err(unavailable());
        }
        if !capture_node {
            return (result.pointer("/result/value").and_then(Value::as_bool) == Some(true))
                .then_some(None)
                .ok_or_else(unavailable);
        }
        let option = result
            .pointer("/result/objectId")
            .and_then(Value::as_str)
            .ok_or_else(unavailable)?;
        let requested = self.cdp.call("DOM.requestNode", json!({"objectId":option}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":option}));
        let requested = requested.map_err(derr)?;
        released.map_err(derr)?;
        requested["nodeId"]
            .as_u64()
            .filter(|id| *id != 0)
            .map(Some)
            .ok_or_else(unavailable)
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
        self.click_node_guarded(node, selector, ClickGuard::Ordinary)
    }

    fn click_node_guarded(
        &mut self,
        node: u64,
        selector: &str,
        guard: ClickGuard<'_>,
    ) -> Result<(), DriverError> {
        let model = self
            .cdp
            .call("DOM.getBoxModel", json!({ "nodeId": node }))
            .map_err(derr)?;
        let quad = model
            .pointer("/model/border")
            .and_then(|v| v.as_array())
            .ok_or_else(|| DriverError(format!("{selector} has no layout box")))?;
        let nums: Vec<f64> = quad.iter().filter_map(|v| v.as_f64()).collect();
        if nums.len() < 8 {
            return Err(DriverError(format!("{selector} has a degenerate box")));
        }
        let x = (nums[0] + nums[2] + nums[4] + nums[6]) / 4.0;
        let y = (nums[1] + nums[3] + nums[5] + nums[7]) / 4.0;
        let metrics = self
            .cdp
            .call("Page.getLayoutMetrics", json!({}))
            .map_err(derr)?;
        let viewport = metrics
            .get("cssVisualViewport")
            .or_else(|| metrics.get("visualViewport"))
            .ok_or_else(|| {
                DriverError("Click viewport is unavailable; review it manually".into())
            })?;
        let width = viewport
            .get("clientWidth")
            .and_then(Value::as_f64)
            .filter(|v| *v > 1.0 && v.is_finite())
            .ok_or_else(|| {
                DriverError("Click viewport is unavailable; review it manually".into())
            })?;
        let height = viewport
            .get("clientHeight")
            .and_then(Value::as_f64)
            .filter(|v| *v > 1.0 && v.is_finite())
            .ok_or_else(|| {
                DriverError("Click viewport is unavailable; review it manually".into())
            })?;
        if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 || x >= width || y >= height {
            return Err(DriverError(
                "Click target is outside the viewport; review it manually".into(),
            ));
        }
        let captured_point = self.captured_pointer_point(node, x, y)?;
        let (pointer_session, offset_x, offset_y) = captured_point
            .as_ref()
            .map(|point| (point.session.clone(), point.x - x, point.y - y))
            .unwrap_or_else(|| (self.cdp.session.clone(), 0.0, 0.0));
        // The first position is not observable through CDP. Subsequent moves
        // follow a bounded curve from the last point, with no artificial wait.
        let (start_x, start_y) = self.pointer_position.unwrap_or((x, y));
        let (start_x, start_y) = (
            start_x.clamp(0.0, width - 0.01),
            start_y.clamp(0.0, height - 0.01),
        );
        let dx = x - start_x;
        let dy = y - start_y;
        let distance = dx.hypot(dy);
        let steps = if distance < 1.0 { 1 } else { 5 };
        for step in 1..=steps {
            let t = f64::from(step) / f64::from(steps);
            let progress = t * t * (3.0 - 2.0 * t);
            let curve = if distance > 0.0 {
                (distance * 0.07).min(12.0) * (std::f64::consts::PI * t).sin() / distance
            } else {
                0.0
            };
            let px = (start_x + dx * progress - dy * curve).clamp(0.0, width - 0.01);
            let py = (start_y + dy * progress + dx * curve).clamp(0.0, height - 0.01);
            self.cdp
                .call_session(
                    &pointer_session,
                    "Input.dispatchMouseEvent",
                    json!({"type":"mouseMoved","x":px+offset_x,"y":py+offset_y}),
                )
                .map_err(derr)?;
            self.pointer_position = Some((px, py));
        }
        // Hover handlers can move or cover a control. Recheck the target after
        // moving the pointer so a changed page cannot redirect the click.
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Click target disappeared; review it manually".into()))?;
        let guard_value = match guard {
            ClickGuard::Ordinary => json!({"kind":"ordinary"}),
            ClickGuard::NonSubmit => json!({"kind":"non-submit"}),
            ClickGuard::ClosedCombobox => json!({"kind":"combobox"}),
            ClickGuard::RadioOption {
                label,
                group_selector,
            } => json!({"kind":"radio", "label":label, "group_selector":group_selector}),
            ClickGuard::ChoiceOption {
                label,
                list_id,
                owner_selector,
            } => {
                json!({"kind":"option","label":label,"list_id":list_id,"owner_selector":owner_selector})
            }
        };
        let mut guard_value = guard_value;
        guard_value["recorded_frame"] = json!(captured_point.is_some());
        let checked = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function(guard) {
            if (!this.isConnected || this.matches(':disabled') || this.closest('[aria-disabled="true" i]')) return false;
            const unsafeTarget = node => {
                if (node.closest('button:not([type="button"]), input[type="submit" i], input[type="image" i], a[href]')) return true;
                const control = node.closest('label')?.control;
                return !!control && control.matches('button:not([type="button"]), input[type="submit" i], input[type="image" i]');
            };
            if (guard.kind !== 'ordinary' && unsafeTarget(this)) return 'unsafe';
            if (guard.kind === 'combobox' && ((this.getAttribute('role') || '').toLowerCase() !== 'combobox' || (this.getAttribute('aria-expanded') || '').toLowerCase() !== 'false')) return false;
            if (guard.kind === 'radio') {
                const root=this.getRootNode(), groups=root.querySelectorAll(guard.group_selector), group=this.closest('[role="radiogroup"]');
                if (groups.length!==1 || groups[0]!==group || (this.getAttribute('role') || '').toLowerCase()!=='radio' || this.tagName==='INPUT' || this.closest('[aria-hidden="true" i], [aria-readonly="true" i]')) return false;
                const name = item => {
                    const refs=item.getAttribute('aria-labelledby');
                    if (refs!==null) {const ids=refs.trim().split(/\s+/).filter(Boolean); if(!ids.length || ids.length>16 || refs.length>512) return ''; const parts=[]; for(const id of ids){const found=root.querySelectorAll('#'+CSS.escape(id));if(found.length!==1)return '';parts.push(found[0].textContent || '');}return parts.join(' ').trim().replace(/\s+/g,' ');}
                    return (item.hasAttribute('aria-label') ? item.getAttribute('aria-label') : (item.innerText || '')).trim().replace(/\s+/g,' ');
                };
                const optionName=item=>{const full=name(item),groupRefs=(group.getAttribute('aria-labelledby')||'').trim().split(/\s+/).filter(Boolean),refs=(item.getAttribute('aria-labelledby')||'').trim().split(/\s+/).filter(Boolean);if(groupRefs.length && refs.length>groupRefs.length && groupRefs.every((id,index)=>refs[index]===id)){const prefix=name(group);if(prefix && full.startsWith(prefix+' '))return full.slice(prefix.length).trim();}return full;};
                const options=group.querySelectorAll('[role="radio"]'); if(!options.length || options.length>30) return false;
                const labels=[]; for(const option of options) {const label=optionName(option); if(option.closest('[role="radiogroup"]')!==group || option.tagName==='INPUT' || (option.getAttribute('aria-checked') || '').toLowerCase()!=='false' || !label || label.length>200) return false; labels.push(label.toLowerCase());}
                if(new Set(labels).size!==labels.length || optionName(this)!==guard.label) return false;
            }
            if (guard.kind === 'option') {
                /*CHOICE_LIST_RESOLVER*/
                if (this.closest('[aria-hidden="true" i]')) return false;
                const root = this.getRootNode(), list = this.closest('[role="listbox"]');
                const label = (this.getAttribute('aria-label') || this.innerText || '').trim().replace(/\s+/g,' ');
                const owners = root.querySelectorAll(guard.owner_selector), lists = root.querySelectorAll('#' + CSS.escape(guard.list_id));
                if ((this.getAttribute('role') || '').toLowerCase() !== 'option' || label !== guard.label || !list || list.id !== guard.list_id || lists.length !== 1 || owners.length !== 1 || (owners[0].getAttribute('role') || '').toLowerCase() !== 'combobox' || (owners[0].getAttribute('aria-expanded') || '').toLowerCase() !== 'true' || resolveChoiceList(owners[0]) !== list) return false;
                const options = list.querySelectorAll('[role="option"]');
                if (options.length > 500) return false;
                let matches = 0;
                for (const option of options) if ((option.getAttribute('aria-label') || option.innerText || '').trim().replace(/\s+/g,' ') === guard.label) matches++;
                if (matches !== 1) return false;
            }
            const rect = this.getBoundingClientRect();
            const x = (rect.left + rect.right) / 2, y = (rect.top + rect.bottom) / 2;
            if (rect.width <= 0 || rect.height <= 0 || x < 0 || y < 0 || x >= innerWidth || y >= innerHeight) return false;
            for (let node = this, depth = 0; node; node = node.parentElement) {
                if (++depth > 64) return false;
                const style = getComputedStyle(node);
                if (style.visibility === 'hidden' || style.visibility === 'collapse' || Number(style.opacity) === 0) return false;
            }
            let current = this, view = window, px = x, py = y;
            for (let depth = 0; depth < 64; depth++) {
                const root = current.getRootNode();
                const hit = typeof root.elementFromPoint === 'function' ? root.elementFromPoint(px, py) : null;
                if (!hit || (hit !== current && !current.contains(hit))) return false;
                if (guard.kind !== 'ordinary' && unsafeTarget(hit)) return 'unsafe';
                if (root.nodeType === 11 && root.host) { current = root.host; continue; }
                if (view === view.top || (guard.recorded_frame && view === window)) return true;
                const frame = view.frameElement;
                if (!frame || view.parent.getComputedStyle(frame).transform !== 'none') return false;
                const bounds = frame.getBoundingClientRect();
                px += bounds.left + frame.clientLeft;
                py += bounds.top + frame.clientTop;
                current = frame;
                view = view.parent;
            }
            return false;
        }"#.replace("/*CHOICE_LIST_RESOLVER*/", CHOICE_LIST_RESOLVER), "arguments":[{"value":guard_value}], "returnByValue":true}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let checked = checked.map_err(derr)?;
        released.map_err(derr)?;
        if checked.pointer("/result/value").and_then(Value::as_str) == Some("unsafe") {
            return Err(DriverError(
                "Click target could submit or navigate; review it manually".into(),
            ));
        }
        if checked.get("exceptionDetails").is_some()
            || checked.pointer("/result/value").and_then(Value::as_bool) != Some(true)
        {
            return Err(DriverError(
                "Click target is hidden, covered, or unavailable; review it manually".into(),
            ));
        }
        let current = self
            .cdp
            .call("DOM.getBoxModel", json!({"nodeId":node}))
            .map_err(derr)?;
        let quad = current
            .pointer("/model/border")
            .and_then(Value::as_array)
            .ok_or_else(|| DriverError("Click target moved; review it manually".into()))?;
        let coordinates: Vec<_> = quad.iter().filter_map(Value::as_f64).collect();
        if coordinates.len() < 8
            || (((coordinates[0] + coordinates[2] + coordinates[4] + coordinates[6]) / 4.0) - x)
                .abs()
                > 0.5
            || (((coordinates[1] + coordinates[3] + coordinates[5] + coordinates[7]) / 4.0) - y)
                .abs()
                > 0.5
        {
            return Err(DriverError("Click target moved; review it manually".into()));
        }
        let after = self.captured_pointer_point(node, x, y)?;
        if after != captured_point {
            return Err(DriverError(
                "The embedded click position changed; inspect it again".into(),
            ));
        }
        self.cdp
            .call_session(
                &pointer_session,"Input.dispatchMouseEvent",
                json!({ "type": "mousePressed", "x": x+offset_x, "y": y+offset_y, "button": "left", "clickCount": 1 }),
            )
            .map_err(derr)?;
        self.cdp
            .call_session(
                &pointer_session,"Input.dispatchMouseEvent",
                json!({ "type": "mouseReleased", "x": x+offset_x, "y": y+offset_y, "button": "left", "clickCount": 1 }),
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

fn check_navigation_result(navigation: &Value) -> Result<(), DriverError> {
    if let Some(error) = navigation
        .get("errorText")
        .and_then(Value::as_str)
        .filter(|error| !error.is_empty())
    {
        let safe_error: String = error
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-' | '.'))
            .take(100)
            .collect();
        return Err(DriverError(format!(
            "Browser navigation failed: {safe_error}"
        )));
    }
    if navigation.get("isDownload").and_then(Value::as_bool) == Some(true) {
        return Err(DriverError(
            "The destination started a download instead of opening a page".into(),
        ));
    }
    Ok(())
}

fn retain_observed_walls(previous: Vec<String>, next: &mut LiveForm) {
    for wall in previous {
        if !next.walls.contains(&wall) {
            next.walls.push(wall);
        }
    }
}

pub const DYNAMIC_FIELD_LOSS_REVIEW: &str = "a custom control needs manual review: Fields seen during page loading disappeared; inspect the entire application again";
pub const DYNAMIC_UNSETTLED_REVIEW: &str = "a custom control needs manual review: The form did not settle during the bounded loading check; inspect the entire application again";

#[derive(Default)]
struct LoadingStability {
    snapshots: usize,
    unchanged: usize,
}

impl LoadingStability {
    fn observe(&mut self, previous: &LiveForm, next: &LiveForm) {
        self.snapshots += 1;
        self.unchanged = if !next.schema.fields.is_empty() && previous.schema == next.schema {
            self.unchanged + 1
        } else {
            0
        };
    }

    fn settled(&self) -> bool {
        self.unchanged >= 2
    }
}

fn retain_loading_observations(previous: LiveForm, next: &mut LiveForm) {
    if previous.schema.fields.iter().any(|field| {
        !next
            .schema
            .fields
            .iter()
            .any(|current| current.selector == field.selector)
    }) && !next
        .walls
        .iter()
        .any(|wall| wall == DYNAMIC_FIELD_LOSS_REVIEW)
    {
        next.walls.push(DYNAMIC_FIELD_LOSS_REVIEW.into());
    }
    retain_observed_walls(previous.walls, next);
}

impl BrowserDriver for CdpDriver {
    fn check_temporal_candidate(&mut self, selector: &str, value: &str) -> Result<(), DriverError> {
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Date or time field is unavailable".into()))?;
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Date or time field cannot be inspected".into()))?;
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function(value) {
            if (!this.isConnected || !(this instanceof HTMLInputElement) || !['date','month','time','datetime-local'].includes(this.type)) return false;
            const probe = this.cloneNode(false);
            probe.value = value;
            return probe.value === value && probe.validity.valid;
        }"#,"arguments":[{"value":value}],"returnByValue":true})).map_err(derr)?;
        if result.get("exceptionDetails").is_some()
            || result.pointer("/result/value").and_then(Value::as_bool) != Some(true)
        {
            return Err(DriverError(
                "Date or time does not satisfy the field's current constraints; review it manually"
                    .into(),
            ));
        }
        Ok(())
    }
    fn check_text_candidate(&mut self, selector: &str, value: &str) -> Result<(), DriverError> {
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Text field is unavailable".into()))?;
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Text field cannot be inspected".into()))?;
        CdpDriver::check_text_candidate(self, object, value)
    }
    fn check_numeric_candidate(&mut self, selector: &str, value: &str) -> Result<(), DriverError> {
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Numeric field is unavailable".into()))?;
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Numeric field cannot be inspected".into()))?;
        self.check_number_candidate(object, value)
    }
    fn observed_non_read_requests(&mut self) -> Result<Option<u64>, DriverError> {
        CdpDriver::observed_non_read_requests(self).map(Some)
    }

    fn current_url(&mut self) -> Result<String, DriverError> {
        CdpDriver::current_url(self)
    }

    fn application_links(&mut self) -> Result<Vec<String>, DriverError> {
        CdpDriver::application_links(self)
    }

    fn page_links(&mut self) -> Result<Vec<waypoint_applications::driver::PageLink>, DriverError> {
        CdpDriver::page_links(self)
    }

    fn open(&mut self, url: &str) -> Result<LiveForm, DriverError> {
        if self.captured_scope.is_some() {
            return Err(DriverError(
                "Return to the parent page before opening another page".into(),
            ));
        }
        self.cdp.take_events();
        let navigation = self
            .cdp
            .call("Page.navigate", json!({ "url": url }))
            .map_err(derr)?;
        check_navigation_result(&navigation)?;
        let frame_id = navigation.get("frameId").and_then(Value::as_str);
        // Same-document hash navigation does not fire a new load event.
        // Wait for either navigation event, then inspect the resulting page.
        let deadline = Instant::now() + self.load_wait;
        loop {
            self.cdp.pump(Duration::from_millis(200));
            if self.cdp.saw_event("Page.loadEventFired")
                || frame_id.is_some_and(|frame| {
                    self.cdp.events.iter().any(|(name, event)| {
                        name == "Page.navigatedWithinDocument"
                            && event.pointer("/params/frameId").and_then(Value::as_str)
                                == Some(frame)
                    })
                })
            {
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
        }
        let mut result = self.inspect_with_retry()?;
        // Client-rendered forms may start with fields and add more immediately
        // after load. Recheck even a nonempty first snapshot, then allow a
        // changing schema to settle within the same bounded loading window.
        let dynamic_deadline = Instant::now() + Duration::from_secs(3);
        let initial_nonempty = !result.schema.fields.is_empty();
        let mut loading = LoadingStability::default();
        for observation in 0..3 {
            if Instant::now() >= dynamic_deadline {
                break;
            }
            let delay = if observation == 0 && initial_nonempty {
                350
            } else {
                750
            };
            self.cdp.pump(
                Duration::from_millis(delay)
                    .min(dynamic_deadline.saturating_duration_since(Instant::now())),
            );
            let mut next = self.inspect_with_retry()?;
            loading.observe(&result, &next);
            retain_loading_observations(result, &mut next);
            result = next;
            if loading.settled() {
                break;
            }
        }
        if !result.schema.fields.is_empty() && !loading.settled() {
            result.walls.push(DYNAMIC_UNSETTLED_REVIEW.into());
        }
        self.trace.push(format!(
            "loading check: {} snapshots, {} consecutive unchanged",
            loading.snapshots + 1,
            loading.unchanged
        ));
        // A same-document route has no new loader or HTTP response. For an
        // ordinary HTTP navigation, bind the status to the main frame and
        // loader, never to a subresource or an earlier page.
        if (url.starts_with("https://") || url.starts_with("http://"))
            && navigation.get("loaderId").and_then(Value::as_str).is_some()
        {
            let loader = navigation["loaderId"].as_str();
            let status = self
                .cdp
                .events
                .iter()
                .rev()
                .find_map(|(name, event)| {
                    (frame_id.is_some()
                        && name == "Network.responseReceived"
                        && event.pointer("/params/frameId").and_then(Value::as_str) == frame_id
                        && event.pointer("/params/loaderId").and_then(Value::as_str) == loader)
                        .then(|| event.pointer("/params/status").and_then(Value::as_u64))
                        .flatten()
                })
                .or_else(|| {
                    self.document_status
                        .as_ref()
                        .filter(|(saved_frame, saved_loader, _)| {
                            Some(saved_frame.as_str()) == frame_id
                                && Some(saved_loader.as_str()) == loader
                        })
                        .map(|(_, _, code)| *code)
                });
            match status {
                Some(code @ 400..=599) => return Err(DriverError(format!("The page returned HTTP {code}; manual review is required"))),
                Some(200..=299) => (),
                _ if !result.walls.iter().any(|wall| wall.contains("main page response status")) => result.walls.push("The main page response status could not be verified; manual review is required".into()),
                _ => (),
            }
        }
        self.trace.push(format!(
            "opened {url} ({} fields)",
            result.schema.fields.len()
        ));
        Ok(result)
    }

    fn inspect_current(&mut self) -> Result<LiveForm, DriverError> {
        self.refresh_captured_form()?;
        let form = self.inspect_with_retry()?;
        Ok(self.bind_captured_form(form))
    }

    fn text_value_snapshot(
        &mut self,
        selector: &str,
    ) -> Result<waypoint_applications::driver::TextSnapshot, DriverError> {
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Field is no longer present".into()))?;
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Existing text cannot be inspected".into()))?;
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function() {
            if (!this.isConnected || this.closest('[aria-disabled="true" i], [aria-readonly="true" i]')) return null;
            const editable = (this.getAttribute('contenteditable')?.toLowerCase() === 'plaintext-only' || (['','true'].includes(this.getAttribute('contenteditable')?.toLowerCase()) && this.getAttribute('role')?.toLowerCase() === 'textbox' && (this.getAttribute('aria-label')?.trim() || this.getAttribute('aria-labelledby')?.trim()))) && this.isContentEditable && !this.querySelector('input,textarea,select,button,[contenteditable]');
            let value;
            if (editable) value = this.innerText;
            else if ((this instanceof HTMLTextAreaElement || (this instanceof HTMLInputElement && ['text','email','tel','url','search','number','date','month','time','datetime-local'].includes(this.type))) && !this.matches(':disabled') && !this.readOnly && !this.hasAttribute('list') && this.getAttribute('role')?.toLowerCase() !== 'combobox' && !['list','both','inline'].includes(this.getAttribute('aria-autocomplete')?.toLowerCase())) value = this.value;
            else return null;
            return typeof value === 'string' && value.length <= 4000 ? {value} : null;
        }"#,"returnByValue":true})).map_err(derr)?;
        if result.get("exceptionDetails").is_some() {
            return Err(DriverError("Existing text cannot be inspected".into()));
        }
        let value = result
            .pointer("/result/value/value")
            .and_then(Value::as_str)
            .filter(|v| {
                v.len() <= 16_000
                    && !v
                        .chars()
                        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
            })
            .ok_or_else(|| DriverError("Existing text cannot be displayed".into()))?;
        Ok(waypoint_applications::driver::TextSnapshot {
            value: value.to_owned(),
        })
    }

    fn ensure_text_replaceable(
        &mut self,
        selector: &str,
        expected: &str,
    ) -> Result<(), DriverError> {
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Field is no longer present".into()))?;
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Existing field value cannot be inspected".into()))?;
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function(expected) { if (!this.isConnected || this.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]')) return false; const editable = (this.getAttribute('contenteditable')?.toLowerCase() === 'plaintext-only' || (['','true'].includes(this.getAttribute('contenteditable')?.toLowerCase()) && this.getAttribute('role')?.toLowerCase() === 'textbox' && (this.getAttribute('aria-label')?.trim() || this.getAttribute('aria-labelledby')?.trim()))) && this.isContentEditable && !this.querySelector('input,textarea,select,button,[contenteditable]'); if (editable) return this.innerText === '' || this.innerText === expected; if (!(this instanceof HTMLTextAreaElement || (this instanceof HTMLInputElement && ['text','email','tel','url','search','number','date','month','time','datetime-local'].includes(this.type))) || this.matches(':disabled') || this.readOnly || this.hasAttribute('list') || this.getAttribute('role')?.toLowerCase() === 'combobox' || ['list','both','inline'].includes(this.getAttribute('aria-autocomplete')?.toLowerCase())) return false; return this.value === '' || this.value === expected; }","arguments":[{"value":expected}],"returnByValue":true})).map_err(derr)?;
        if result.get("exceptionDetails").is_some()
            || result.pointer("/result/value").and_then(Value::as_bool) != Some(true)
        {
            return Err(DriverError("Field already contains a different value or cannot be inspected; review it in the browser".into()));
        }
        Ok(())
    }

    fn verify_text_value(&mut self, selector: &str, expected: &str) -> Result<(), DriverError> {
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Previously filled field is no longer present".into()))?;
        if !self.control_visibility_and_path(node)?.0 {
            return Err(DriverError(
                "Previously filled field is not visibly rendered; review it manually".into(),
            ));
        }
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Cannot inspect previously filled field".into()))?;
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function(value) { if (!this.getClientRects().length) return false; for (let node = this, depth = 0; node; node = node.parentElement) { if (++depth > 64) return false; const style = getComputedStyle(node); if (style.visibility === 'hidden' || style.visibility === 'collapse' || Number(style.opacity) === 0) return false; } return (this.isConnected && !this.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]')) && ((this.getAttribute('contenteditable')?.toLowerCase() === 'plaintext-only' || (['','true'].includes(this.getAttribute('contenteditable')?.toLowerCase()) && this.getAttribute('role')?.toLowerCase() === 'textbox' && (this.getAttribute('aria-label')?.trim() || this.getAttribute('aria-labelledby')?.trim()))) ? this.isContentEditable && !this.querySelector('input,textarea,select,button,[contenteditable]') && this.getAttribute('aria-disabled') !== 'true' && this.getAttribute('aria-readonly') !== 'true' && this.innerText === value : (this instanceof HTMLTextAreaElement || (this instanceof HTMLInputElement && ['text','email','tel','url','search','number','date','month','time','datetime-local'].includes(this.type))) && !this.matches(':disabled') && !this.readOnly && !this.hasAttribute('list') && this.getAttribute('role')?.toLowerCase() !== 'combobox' && !['list','both','inline'].includes(this.getAttribute('aria-autocomplete')?.toLowerCase()) && this.value === value && this.validity.valid); }","arguments":[{"value":expected}],"returnByValue":true})).map_err(derr)?;
        if result.get("exceptionDetails").is_some()
            || result.pointer("/result/value").and_then(Value::as_bool) != Some(true)
        {
            if let Some(reason) = self.native_text_validation_reason(object, expected) {
                return Err(DriverError(format!(
                    "A previously filled field failed a browser constraint: {reason}"
                )));
            }
            return Err(DriverError(
                "A previously filled field changed or became invalid".into(),
            ));
        }
        self.verify_aria_validity(node, false)
    }

    fn choice_snapshot(
        &mut self,
        selector: &str,
        kind: &str,
    ) -> Result<waypoint_applications::driver::ChoiceSnapshot, DriverError> {
        if !matches!(kind, "checkbox" | "radio" | "multi") {
            return Err(DriverError("Unsupported choice snapshot".into()));
        }
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Choice field is no longer present".into()))?;
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Existing choices cannot be inspected".into()))?;
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function(kind) {
            if (!this.isConnected || this.matches(':disabled') || this.closest('[aria-disabled="true" i], [aria-readonly="true" i]')) return null;
            if (kind === 'checkbox') return this instanceof HTMLInputElement && this.type === 'checkbox' ? {kind,checked:this.checked,selections:[]} : null;
            if (kind === 'multi') {
                if (!(this instanceof HTMLSelectElement) || !this.multiple || this.options.length > 1000 || this.selectedOptions.length > 100) return null;
                const selections = Array.from(this.options).map((o,index) => ({o,index})).filter(p => p.o.selected).map(p => ({index:p.index,label:p.o.label.trim().replace(/\s+/g,' '),value:p.o.value}));
                return selections.some(p => p.label.length > 500 || p.value.length > 500) ? null : {kind,checked:null,selections};
            }
            if (kind !== 'radio' || !(this instanceof HTMLInputElement) || this.type !== 'radio' || !this.name) return null;
            const groupSelector='fieldset,[role="radiogroup"]', group=this.closest(groupSelector), root=this.getRootNode(); if (!group) return null;
            const all=root.querySelectorAll('input[type=radio]'); if (all.length > 5000) return null;
            const same=Array.from(all).filter(r => r.name === this.name && r.form === this.form);
            if (!same.length || same.length > 1000 || same.some(r => r.closest(groupSelector) !== group)) return null;
            const label = r => {const refs=r.getAttribute('aria-labelledby'); if (refs !== null) { const ids=refs.trim().split(/\s+/).filter(Boolean); if (!ids.length || ids.length > 16 || refs.length > 512) return null; const nodes=ids.map(id => root.querySelectorAll('#'+CSS.escape(id))); if (nodes.some(n => n.length !== 1 || !n[0].textContent.trim())) return null; return nodes.map(n => n[0].textContent).join(' '); } return r.getAttribute('aria-label') || (r.labels && r.labels[0] && r.labels[0].textContent) || r.name || '';};
            const selected=same.map((r,index)=>({r,index})).filter(p=>p.r.checked); if (selected.length > 1) return null;
            const selections=[]; for (const p of selected) {if (p.r.matches(':disabled') || p.r.closest('[aria-disabled="true" i], [aria-readonly="true" i]')) return null; const text=label(p.r); if (text === null || text.length > 500 || p.r.value.length > 500) return null; selections.push({index:p.index,label:text.trim().replace(/\s+/g,' '),value:p.r.value});}
            return {kind,checked:null,selections};
        }"#,"arguments":[{"value":kind}],"returnByValue":true})).map_err(derr)?;
        if result.get("exceptionDetails").is_some() {
            return Err(DriverError("Existing choices cannot be inspected".into()));
        }
        let mut snapshot: waypoint_applications::driver::ChoiceSnapshot = serde_json::from_value(
            result
                .pointer("/result/value")
                .cloned()
                .unwrap_or(Value::Null),
        )
        .map_err(|_| DriverError("Existing choices cannot be inspected".into()))?;
        if snapshot.kind != kind
            || snapshot.selections.len() > 100
            || (kind == "checkbox") != snapshot.checked.is_some()
            || (kind == "checkbox" && !snapshot.selections.is_empty())
            || (kind == "radio" && snapshot.selections.len() > 1)
            || snapshot.selections.iter().any(|v| {
                v.index >= 1000
                    || v.label.len() > 2000
                    || v.value.len() > 2000
                    || v.label.chars().chain(v.value.chars()).any(char::is_control)
            })
        {
            return Err(DriverError("Existing choices cannot be displayed".into()));
        }
        snapshot.selections.sort_by_key(|v| v.index);
        Ok(snapshot)
    }

    fn selected_option_snapshot(
        &mut self,
        selector: &str,
    ) -> Result<waypoint_applications::driver::SelectSnapshot, DriverError> {
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Select field is no longer present".into()))?;
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Existing selection cannot be inspected".into()))?;
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function() {
            if (!(this instanceof HTMLSelectElement) || this.multiple || !this.isConnected || this.matches(':disabled') || this.closest('[aria-disabled="true" i], [aria-readonly="true" i]') || this.options.length > 1000 || this.selectedOptions.length !== 1 || this.selectedIndex < 0) return null;
            const o = this.selectedOptions[0];
            if (o.label.length > 500 || o.value.length > 500) return null;
            return {index:this.selectedIndex, label:o.label, value:o.value};
        }"#,"returnByValue":true})).map_err(derr)?;
        if result.get("exceptionDetails").is_some() {
            return Err(DriverError("Existing selection cannot be inspected".into()));
        }
        let value = result
            .pointer("/result/value")
            .ok_or_else(|| DriverError("Existing selection cannot be inspected".into()))?;
        let index = value["index"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| *n < 1000)
            .ok_or_else(|| DriverError("Existing selection cannot be inspected".into()))?;
        let read = |key: &str| {
            value[key]
                .as_str()
                .filter(|v| v.len() <= 2000 && !v.chars().any(char::is_control))
                .map(str::to_owned)
                .ok_or_else(|| DriverError("Existing selection cannot be displayed".into()))
        };
        Ok(waypoint_applications::driver::SelectSnapshot {
            index,
            label: read("label")?,
            value: read("value")?,
        })
    }

    fn ensure_choice_replaceable(&mut self, command: &BrowserCommand) -> Result<(), DriverError> {
        let (selector, kind) = match command {
            BrowserCommand::SelectOption { selector, .. } => (selector, "select"),
            BrowserCommand::SelectOptions { selector, .. } => (selector, "multi"),
            BrowserCommand::SelectRadio { selector, .. } => (selector, "radio"),
            BrowserCommand::Fill {
                selector,
                value: PacketValue::Bool(_),
            } => (selector, "checkbox"),
            _ => return Err(DriverError("Unsupported choice inspection".into())),
        };
        if self.verify_choice(command).is_ok() {
            return Ok(());
        }
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Choice field is no longer present".into()))?;
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("Existing choice cannot be inspected".into()))?;
        let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function(kind) {
            if (!this.isConnected || this.matches(':disabled') || this.closest('[aria-disabled="true" i], [aria-readonly="true" i]')) return false;
            if (kind === 'checkbox') return this instanceof HTMLInputElement && this.type === 'checkbox' && !this.checked;
            if (kind === 'select' && this instanceof HTMLInputElement && this.hasAttribute('list')) return !this.readOnly && this.value === '';
            if (kind === 'select') return this instanceof HTMLSelectElement && !this.multiple && this.options.length <= 5000 && Array.from(this.selectedOptions).every(o => o.value === '');
            if (kind === 'multi') return this instanceof HTMLSelectElement && this.multiple && this.options.length <= 5000 && this.selectedOptions.length === 0;
            if (kind !== 'radio' || !(this instanceof HTMLInputElement) || this.type !== 'radio' || !this.name) return false;
            const groupSelector = 'fieldset,[role="radiogroup"]', group = this.closest(groupSelector), root = this.getRootNode();
            if (!group) return false;
            const all = root.querySelectorAll('input[type=radio]'); if (all.length > 5000) return false;
            const same = Array.from(all).filter(r => r.name === this.name && r.form === this.form);
            return same.length > 0 && same.every(r => r.closest(groupSelector) === group && !r.checked);
        }"#,"arguments":[{"value":kind}],"returnByValue":true})).map_err(derr)?;
        if result.get("exceptionDetails").is_some()
            || result.pointer("/result/value").and_then(Value::as_bool) != Some(true)
        {
            return Err(DriverError("Field already has a different selection or cannot be inspected; review it in the browser".into()));
        }
        Ok(())
    }

    fn verify_choice(&mut self, command: &BrowserCommand) -> Result<(), DriverError> {
        let selector = match command {
            BrowserCommand::SelectOptions { selector, .. }
            | BrowserCommand::SelectOption { selector, .. }
            | BrowserCommand::SelectRadio { selector, .. }
            | BrowserCommand::Fill {
                selector,
                value: PacketValue::Bool(_),
            } => selector,
            _ => return Err(DriverError("Unsupported choice verification".into())),
        };
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Previously filled choice is missing".into()))?;
        if !self.control_visibility_and_path(node)?.0 {
            return Err(DriverError(
                "Previously filled choice is not visibly rendered; review it manually".into(),
            ));
        }
        let radio = matches!(command, BrowserCommand::SelectRadio { .. });
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
                let result = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function(value) { const visible = node => { if (!node.getClientRects().length) return false; for (let item = node, depth = 0; item; item = item.parentElement) { if (++depth > 64) return false; const style = getComputedStyle(item); if (style.visibility === 'hidden' || style.visibility === 'collapse' || Number(style.opacity) === 0) return false; } return true; }; if (!visible(this)) return false; return (this.isConnected && !this.closest('[aria-disabled=\"true\" i], [aria-readonly=\"true\" i]')) && this instanceof HTMLInputElement && this.type === 'checkbox' && !this.matches(':disabled') && this.checked === value && this.validity.valid; }","arguments":[{"value":expected}],"returnByValue":true})).map_err(derr)?;
                if result.get("exceptionDetails").is_some()
                    || result.pointer("/result/value").and_then(Value::as_bool) != Some(true)
                {
                    return Err(DriverError("A checkbox changed or became invalid".into()));
                }
                Ok(())
            }
            _ => Err(DriverError("Unsupported choice verification".into())),
        }?;
        // The choice inspection can refresh the DOM document and invalidate node IDs.
        let current = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("Previously filled choice is missing".into()))?;
        self.verify_aria_validity(current, radio)
    }

    fn execute(&mut self, command: &BrowserCommand) -> Result<(), DriverError> {
        match command {
            BrowserCommand::Navigate { url } => {
                self.open(url)?;
                Ok(())
            }
            BrowserCommand::Fill { selector, value } => self.fill(selector, value),
            BrowserCommand::SelectOption { selector, label } => {
                self.select_option(selector, label, true)?;
                self.verify_choice(command)
            }
            BrowserCommand::SelectOptions { selector, labels } => {
                self.select_options(selector, labels, true)?;
                self.verify_choice(command)
            }
            BrowserCommand::SelectRadio { selector, label } => {
                self.select_radio(selector, label, true)?;
                self.verify_choice(command)
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
                if self.captured_scope.is_some() {
                    self.scroll_field_into_view(selector)?;
                }
                let Some(node) = self.node_id(selector)? else {
                    return Err(DriverError(format!("{selector} not found")));
                };
                self.click_node_guarded(node, selector, ClickGuard::NonSubmit)
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
                self.browser.shutdown().map_err(derr)?;
                Ok(())
            }
        }
    }

    fn commit(&mut self) -> Result<SiteAck, DriverError> {
        #[cfg(windows)]
        if matches!(&self.browser, BrowserOwner::Rehearsal(_)) {
            return Err(DriverError(
                "Submission is unavailable in isolated rehearsal".into(),
            ));
        }
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
        if let Err(error) = self.browser.shutdown() {
            eprintln!("Browser cleanup was not confirmed: {error}");
        }
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

fn add_hidden_selectors(hidden: &mut std::collections::HashSet<String>, control: &Control) {
    hidden.insert(selector_for(control));
    hidden.insert(control.tag.clone());
    if let Some(name) = control.attrs.get("name").filter(|name| !name.is_empty()) {
        hidden.insert(format!("{}[name=\"{}\"]", control.tag, css_string(name)));
    }
    if let Some(kind) = control.attrs.get("type") {
        hidden.insert(format!("{}[type=\"{}\"]", control.tag, css_string(kind)));
    }
}

fn supported_editing_host(tag: &str, attrs: &HashMap<String, String>) -> bool {
    if matches!(tag, "input" | "textarea" | "select") {
        return false;
    }
    let Some(editable) = attrs.get("contenteditable") else {
        return false;
    };
    let role = attrs.get("role");
    if editable.eq_ignore_ascii_case("plaintext-only") {
        return role.is_none_or(|value| value.eq_ignore_ascii_case("textbox"));
    }
    (editable.is_empty() || editable.eq_ignore_ascii_case("true"))
        && role.is_some_and(|value| value.eq_ignore_ascii_case("textbox"))
        && ["aria-label", "aria-labelledby"].iter().any(|name| {
            attrs
                .get(*name)
                .is_some_and(|value| !value.trim().is_empty())
        })
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

fn anchor_references(root: &Value) -> HashMap<String, Option<String>> {
    control_references(root, false)
}
fn button_references(root: &Value) -> HashMap<String, Option<String>> {
    control_references(root, true)
}
fn control_references(root: &Value, buttons: bool) -> HashMap<String, Option<String>> {
    let mut wanted = std::collections::HashSet::new();
    walk(root, false, &mut |node, _| {
        let tag = node.get("nodeName").and_then(Value::as_str).unwrap_or("");
        let attrs = attributes(node);
        let wanted_control = if buttons {
            tag.eq_ignore_ascii_case("button")
                || attrs.get("role").is_some_and(|role| {
                    role.eq_ignore_ascii_case("button") || role.eq_ignore_ascii_case("combobox")
                })
                || tag.eq_ignore_ascii_case("input")
        } else {
            tag.eq_ignore_ascii_case("a")
        };
        if wanted_control {
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
    referenced
}

fn anchor_label(
    node: &Value,
    attrs: &HashMap<String, String>,
    referenced: &HashMap<String, Option<String>>,
) -> Option<String> {
    let label = if let Some(ids) = attrs.get("aria-labelledby") {
        let parts: Vec<_> = ids.split_whitespace().collect();
        if ids.len() > 512 || parts.is_empty() || parts.len() > 16 {
            return None;
        }
        parts
            .iter()
            .map(|id| {
                referenced
                    .get(*id)
                    .and_then(Option::as_ref)
                    .map(String::as_str)
            })
            .collect::<Option<Vec<_>>>()?
            .join(" ")
    } else if let Some(aria) = attrs
        .get("aria-label")
        .filter(|label| !label.trim().is_empty())
    {
        aria.clone()
    } else {
        let mut text = String::new();
        collect_text(node, false, &mut text);
        text
    };
    Some(label.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn application_links_from_document(root: &Value) -> Vec<String> {
    let referenced = anchor_references(root);
    let mut links = Vec::new();
    walk(root, false, &mut |node, _| {
        let tag = node.get("nodeName").and_then(Value::as_str).unwrap_or("");
        if tag.eq_ignore_ascii_case("a") {
            let attrs = attributes(node);
            let Some(label) = anchor_label(node, &attrs, &referenced) else {
                return true;
            };
            let label = label.to_lowercase();
            // Decorative arrows and punctuation are often part of a link's accessible name.
            // Keep the vocabulary conservative: this only proposes a destination, not a submission.
            let normalized = label.trim_matches(|c: char| !c.is_alphanumeric());
            let words: Vec<_> = normalized.split_whitespace().collect();
            let candidate = matches!(
                words.as_slice(),
                ["apply", "for", "job" | "position" | "role"]
                    | ["apply", "for", "this" | "the", "job" | "position" | "role"]
            ) || [
                "apply",
                "apply now",
                "start application",
                "start your application",
                "bewerben",
                "jetzt bewerben",
                "postuler",
                "postuler maintenant",
                "postularse",
                "solicitar empleo",
                "candidatar-se",
                "candidatar-me",
            ]
            .contains(&normalized);
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
            if attrs.contains_key("disabled")
                || attrs
                    .get("aria-disabled")
                    .is_some_and(|value| value.eq_ignore_ascii_case("true"))
                || attrs
                    .get("aria-readonly")
                    .is_some_and(|value| value.eq_ignore_ascii_case("true"))
            {
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

// Shared budget bounds repeated group-subtree traversal on nested forms.
// False means some nodes were not visited and callers must report incomplete context.
fn walk_group_bounded<F: FnMut(&Value) -> bool>(
    node: &Value,
    budget: &mut usize,
    cb: &mut F,
) -> bool {
    let mut pending = vec![node];
    while let Some(current) = pending.pop() {
        if *budget == 0 {
            return false;
        }
        *budget -= 1;
        if !cb(current) {
            continue;
        }
        if let Some(children) = current.get("children").and_then(Value::as_array) {
            pending.extend(children.iter().rev());
        }
    }
    true
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

fn tail_text(root: &Value, limit: usize) -> (String, bool) {
    let mut chars = std::collections::VecDeque::new();
    let mut bytes = 0usize;
    let mut truncated = false;
    walk(root, false, &mut |node, in_control| {
        let tag = node.get("nodeName").and_then(Value::as_str).unwrap_or("");
        if ["script", "style", "noscript", "template"]
            .iter()
            .any(|name| tag.eq_ignore_ascii_case(name))
        {
            return false;
        }
        if tag == "#text" && !in_control {
            if let Some(value) = node.get("nodeValue").and_then(Value::as_str) {
                for c in value.chars().chain(std::iter::once(' ')) {
                    while bytes + c.len_utf8() > limit {
                        let Some(old): Option<char> = chars.pop_front() else {
                            truncated = true;
                            break;
                        };
                        bytes -= old.len_utf8();
                        truncated = true;
                    }
                    if c.len_utf8() <= limit {
                        chars.push_back(c);
                        bytes += c.len_utf8();
                    }
                }
            }
        }
        true
    });
    let text: String = chars.into_iter().collect();
    (
        text.split_whitespace().collect::<Vec<_>>().join(" "),
        truncated,
    )
}

fn unique_section_text(root: &Value, id: &str, limit: usize) -> Option<(String, bool)> {
    let mut matches = 0usize;
    let mut result = None;
    walk(root, false, &mut |node, _| {
        if attributes(node).get("id").is_some_and(|value| value == id) {
            matches += 1;
            let tag = node.get("nodeName").and_then(Value::as_str).unwrap_or("");
            if matches == 1
                && (tag.eq_ignore_ascii_case("section") || tag.eq_ignore_ascii_case("article"))
            {
                let mut text = String::new();
                let mut truncated = false;
                walk(node, false, &mut |child, in_control| {
                    let tag = child.get("nodeName").and_then(Value::as_str).unwrap_or("");
                    if ["script", "style", "noscript", "template"]
                        .iter()
                        .any(|skip| tag.eq_ignore_ascii_case(skip))
                    {
                        return false;
                    }
                    if tag == "#text" && !in_control {
                        if let Some(value) = child.get("nodeValue").and_then(Value::as_str) {
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
                result = Some((text, truncated));
            }
        }
        true
    });
    (matches == 1).then_some(result).flatten()
}

/// Collect only unambiguous static datalists in this document or shadow root.
fn static_datalists(root: &Value) -> HashMap<String, Vec<String>> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut lists: HashMap<String, Option<Vec<String>>> = HashMap::new();
    walk(root, false, &mut |node, _| {
        let attrs = attributes(node);
        if let Some(id) = attrs
            .get("id")
            .filter(|id| !id.is_empty() && id.len() <= 512)
        {
            *counts.entry(id.clone()).or_default() += 1;
            if node
                .get("nodeName")
                .and_then(Value::as_str)
                .is_some_and(|name| name.eq_ignore_ascii_case("datalist"))
            {
                let mut values: Vec<String> = Vec::new();
                let mut valid = !attrs.contains_key("disabled")
                    && !attrs
                        .get("aria-disabled")
                        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
                    && !attrs
                        .get("aria-readonly")
                        .is_some_and(|value| value.eq_ignore_ascii_case("true"));
                walk(node, false, &mut |child, _| {
                    let tag = child.get("nodeName").and_then(Value::as_str).unwrap_or("");
                    let attrs = attributes(child);
                    if (tag.eq_ignore_ascii_case("option") || tag.eq_ignore_ascii_case("optgroup"))
                        && (attrs.contains_key("disabled")
                            || attrs
                                .get("aria-disabled")
                                .is_some_and(|value| value.eq_ignore_ascii_case("true"))
                            || attrs
                                .get("aria-readonly")
                                .is_some_and(|value| value.eq_ignore_ascii_case("true")))
                    {
                        valid = false;
                    }
                    if tag.eq_ignore_ascii_case("option") {
                        // HTML uses an option's text as its value when no value
                        // attribute is present. Accept only one unchanged text
                        // node; markup and whitespace normalization need review.
                        let fallback = || {
                            let children = child.get("children")?.as_array()?;
                            if children.len() != 1
                                || children[0].get("nodeName")?.as_str()? != "#text"
                            {
                                return None;
                            }
                            let text = children[0].get("nodeValue")?.as_str()?;
                            if text.chars().any(|c| c.is_whitespace() && c != ' ')
                                || text.contains("  ")
                            {
                                return None;
                            }
                            Some(text.to_owned())
                        };
                        match attrs.get("value").cloned().or_else(fallback) {
                            Some(value)
                                if !value.is_empty()
                                    && value.len() <= 200
                                    && value.trim() == value
                                    && !values.iter().any(|previous| {
                                        previous.to_lowercase() == value.to_lowercase()
                                    })
                                    && values.len() < 100 =>
                            {
                                values.push(value)
                            }
                            _ => valid = false,
                        }
                    }
                    true
                });
                lists.insert(id.clone(), (valid && !values.is_empty()).then_some(values));
            }
        }
        true
    });
    lists
        .into_iter()
        .filter_map(|(id, values)| {
            (counts.get(&id) == Some(&1))
                .then_some(values)
                .flatten()
                .map(|values| (id, values))
        })
        .collect()
}

fn native_datalist_options<'a>(
    attrs: &HashMap<String, String>,
    lists: &'a HashMap<String, Vec<String>>,
) -> Option<&'a Vec<String>> {
    let kind = attrs
        .get("type")
        .map(|kind| kind.to_ascii_lowercase())
        .unwrap_or_else(|| "text".into());
    if !matches!(kind.as_str(), "text" | "search" | "email" | "tel" | "url")
        || attrs.contains_key("role")
        || attrs.contains_key("aria-autocomplete")
        || attrs.contains_key("multiple")
    {
        return None;
    }
    attrs.get("list").and_then(|id| lists.get(id))
}

fn document_has_embedded_scope(document: &Value) -> bool {
    let mut found = false;
    walk_with_visibility(document, false, false, &mut |node, _, hidden| {
        if !hidden {
            let tag = node.get("nodeName").and_then(Value::as_str).unwrap_or("");
            if tag.eq_ignore_ascii_case("iframe")
                || tag.eq_ignore_ascii_case("frame")
                || node
                    .get("shadowRoots")
                    .and_then(Value::as_array)
                    .is_some_and(|roots| {
                        roots.iter().any(|root| {
                            matches!(
                                root.get("shadowRootType").and_then(Value::as_str),
                                Some("open" | "closed")
                            ) && document_may_contain_form_or_scope(root)
                        })
                    })
            {
                found = true;
            }
        }
        true
    });
    found
}

fn document_may_contain_form_or_scope(document: &Value) -> bool {
    let mut found = document_has_form_controls(document);
    if found {
        return true;
    }
    walk_with_visibility(document, false, false, &mut |node, _, hidden| {
        if !hidden {
            let tag = node.get("nodeName").and_then(Value::as_str).unwrap_or("");
            let attrs = attributes(node);
            if tag.eq_ignore_ascii_case("iframe")
                || tag.eq_ignore_ascii_case("frame")
                || node
                    .get("shadowRoots")
                    .and_then(Value::as_array)
                    .is_some_and(|roots| {
                        roots.iter().any(|root| {
                            matches!(
                                root.get("shadowRootType").and_then(Value::as_str),
                                Some("open" | "closed")
                            )
                        })
                    })
                || attrs.get("role").is_some_and(|role| {
                    matches!(
                        role.to_ascii_lowercase().as_str(),
                        "textbox" | "combobox" | "checkbox" | "radio" | "switch" | "slider"
                    )
                })
                || attrs.contains_key("contenteditable")
            {
                found = true;
            }
        }
        true
    });
    found
}

fn document_has_form_controls(document: &Value) -> bool {
    let mut found = false;
    walk_with_visibility(document, false, false, &mut |field, _, hidden| {
        let tag = field.get("nodeName").and_then(Value::as_str).unwrap_or("");
        if !hidden
            && (tag.eq_ignore_ascii_case("input")
                || tag.eq_ignore_ascii_case("textarea")
                || tag.eq_ignore_ascii_case("select"))
        {
            let attrs = attributes(field);
            let non_answer = attrs.get("type").is_some_and(|kind| {
                matches!(
                    kind.to_ascii_lowercase().as_str(),
                    "hidden" | "submit" | "button" | "reset" | "image"
                )
            });
            if !non_answer {
                found = true;
            }
        }
        true
    });
    found
}

fn walk_with_visibility<F: FnMut(&Value, bool, bool) -> bool>(
    node: &Value,
    in_control: bool,
    hidden: bool,
    cb: &mut F,
) {
    let mut pending = vec![(node, in_control, hidden)];
    while let Some((current, inside_control, parent_hidden)) = pending.pop() {
        let attrs = attributes(current);
        let hidden = parent_hidden
            || attrs.contains_key("hidden")
            || attrs
                .get("aria-hidden")
                .is_some_and(|value| value.eq_ignore_ascii_case("true"));
        if !cb(current, inside_control, hidden) {
            continue;
        }
        let tag = current
            .get("nodeName")
            .and_then(Value::as_str)
            .unwrap_or("");
        let child_in_control = inside_control
            || ["input", "textarea", "select"]
                .iter()
                .any(|kind| tag.eq_ignore_ascii_case(kind))
            || supported_editing_host(&tag.to_ascii_lowercase(), &attributes(current))
            || (!tag.eq_ignore_ascii_case("select")
                && attributes(current)
                    .get("role")
                    .is_some_and(|role| role.eq_ignore_ascii_case("combobox")));
        if let Some(children) = current.get("children").and_then(Value::as_array) {
            pending.extend(
                children
                    .iter()
                    .rev()
                    .map(|child| (child, child_in_control, hidden)),
            );
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
    #[test]
    #[ignore = "one owned headless browser and fictional file fixture; stable captures without field or DOM mutation"]
    fn native_caret_capture_is_stable_restored_and_does_not_hide_text_changes() {
        use std::io::Write;
        fn probe(driver: &mut CdpDriver) -> Value {
            let response=driver.cdp.call("Runtime.evaluate",json!({"expression":"({events:window.captureEvents,mutations:window.captureMutations,focused:document.activeElement.id,value:document.querySelector('input').value,caret:getComputedStyle(document.querySelector('input')).caretColor})","returnByValue":true})).unwrap();
            assert!(response.get("exceptionDetails").is_none());
            response["result"]["value"].clone()
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(format!("caret-capture-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let fixture = root.join("fictional.html");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&fixture)
            .unwrap();
        file.write_all(br#"<!doctype html><meta charset='utf-8'><style>input{font:20px Arial;caret-color:rgb(10,20,30)}</style><label>Fictional name<input id='fictional'></label><script>window.captureEvents=0;window.captureMutations=0;for(const type of ['input','change','focus','blur'])document.addEventListener(type,()=>window.captureEvents++,true);new MutationObserver(items=>window.captureMutations+=items.length).observe(document,{subtree:true,attributes:true,childList:true,characterData:true});</script>"#).unwrap();
        drop(file);
        let url = format!(
            "file:///{}",
            fixture.display().to_string().replace('\\', "/")
        );
        let mut driver = CdpDriver::launch(vec![]).unwrap();
        let schema = driver.open(&url).unwrap().schema;
        driver
            .execute(&BrowserCommand::Fill {
                selector: "#fictional".into(),
                value: PacketValue::Text("Fictional Candidate".into()),
            })
            .unwrap();
        driver
            .cdp
            .call(
                "Runtime.evaluate",
                json!({"expression":"window.captureEvents=0;window.captureMutations=0"}),
            )
            .unwrap();
        let original = probe(&mut driver);
        assert_eq!(original["focused"], "fictional");
        let first = driver.screenshot_base64().unwrap();
        driver.cdp.pump(Duration::from_millis(750));
        let second = driver.screenshot_base64().unwrap();
        assert_eq!(
            first, second,
            "caret blink must not produce a false page change"
        );
        let (width, height) = driver.page_dimensions().unwrap();
        let tile = driver.screenshot_tile(0.0, 0.0, width, height).unwrap();
        driver.cdp.pump(Duration::from_millis(650));
        assert_eq!(
            tile,
            driver.screenshot_tile(0.0, 0.0, width, height).unwrap()
        );
        let crop = driver.screenshot_field_context("#fictional").unwrap();
        driver.cdp.pump(Duration::from_millis(650));
        assert_eq!(crop, driver.screenshot_field_context("#fictional").unwrap());
        assert_eq!(
            probe(&mut driver),
            original,
            "capture must restore caret styling without DOM mutations or focus/input/change events"
        );
        assert!(driver
            .capture_png(
                json!({"format":"png","clip":{"x":0,"y":0,"width":-1,"height":10,"scale":1}})
            )
            .is_err());
        assert_eq!(
            probe(&mut driver),
            original,
            "failed capture must still restore styling"
        );
        assert_eq!(driver.inspect_current().unwrap().schema, schema);
        driver
            .execute(&BrowserCommand::Fill {
                selector: "#fictional".into(),
                value: PacketValue::Text("Fictional Other".into()),
            })
            .unwrap();
        assert_ne!(
            first,
            driver.screenshot_base64().unwrap(),
            "actual field changes must remain visible"
        );
        driver.open(&url).unwrap();
        driver
            .execute(&BrowserCommand::Fill {
                selector: "#fictional".into(),
                value: PacketValue::Text("Fictional Candidate".into()),
            })
            .unwrap();
        assert!(!driver.screenshot_base64().unwrap().is_empty());
        assert_eq!(probe(&mut driver)["caret"], original["caret"]);
        std::fs::write(root.join("qualification.private.json"),json!({"browser":driver.browser_version().unwrap(),"source_sha256":waypoint_domain::sha256_hex(include_str!("lib.rs").as_bytes()),"fixture_sha256":waypoint_domain::sha256_hex(&std::fs::read(&fixture).unwrap()),"viewport_stable":true,"tile_stable":true,"crop_stable":true,"restore_on_error":true,"dom_mutations":original["mutations"],"field_events":original["events"],"navigation_rebound":true,"text_change_visible":true}).to_string()).unwrap();
        println!("Caret capture evidence: {}", root.display());
    }

    use super::*;

    #[test]
    fn group_walk_shares_a_finite_budget_and_reports_incomplete() {
        let node = json!({"nodeName":"DIV","children":[{"nodeName":"H4"},{"nodeName":"INPUT"}]});
        let mut remaining = 4;
        let mut visited = 0;
        assert!(walk_group_bounded(&node, &mut remaining, &mut |_| {
            visited += 1;
            true
        }));
        assert_eq!(visited, 3);
        assert_eq!(remaining, 1);
        assert!(!walk_group_bounded(&node, &mut remaining, &mut |_| {
            visited += 1;
            true
        }));
        assert_eq!(remaining, 0);
        assert_eq!(visited, 4);
    }

    #[test]
    fn tail_text_is_bounded_and_skips_controls_and_scripts() {
        let root = json!({"nodeName":"BODY","children":[{"nodeName":"#text","nodeValue":"beginning"},{"nodeName":"INPUT","children":[{"nodeName":"#text","nodeValue":"private value"}]},{"nodeName":"SCRIPT","children":[{"nodeName":"#text","nodeValue":"script text"}]},{"nodeName":"#text","nodeValue":"end café"}]});
        let (text, truncated) = tail_text(&root, 10);
        assert_eq!(text, "end café");
        assert!(truncated);
        let (text, truncated) = tail_text(&root, 100);
        assert_eq!(text, "beginning end café");
        assert!(!truncated);
    }

    #[test]
    fn section_text_requires_one_semantic_id_and_skips_controls() {
        let section = serde_json::json!({"nodeName":"SECTION","attributes":["id","role-a"],"children":[{"nodeName":"#text","nodeValue":"Selected role"},{"nodeName":"SCRIPT","children":[{"nodeName":"#text","nodeValue":"ignore script"}]},{"nodeName":"INPUT","children":[{"nodeName":"#text","nodeValue":"ignore value"}]}]});
        let root = serde_json::json!({"nodeName":"BODY","children":[section.clone(),{"nodeName":"SECTION","attributes":["id","role-b"],"children":[{"nodeName":"#text","nodeValue":"Other role"}]}]});
        let (text, truncated) = unique_section_text(&root, "role-a", 100).unwrap();
        assert_eq!(text.trim(), "Selected role");
        assert!(!truncated);
        assert!(unique_section_text(&root, "missing", 100).is_none());
        let duplicate = serde_json::json!({"nodeName":"BODY","children":[section.clone(),section]});
        assert!(unique_section_text(&duplicate, "role-a", 100).is_none());
        let wrong_tag = serde_json::json!({"nodeName":"DIV","attributes":["id","role-a"]});
        assert!(unique_section_text(&wrong_tag, "role-a", 100).is_none());
        assert!(unique_section_text(&root, "role-a", 5).unwrap().1);
    }

    #[test]
    fn context_capture_keeps_target_and_uses_available_left_space() {
        assert_eq!(context_left_edge(400.0, 850.0, 0.0, 1400.0), 160.0);
        assert_eq!(context_left_edge(400.0, 1050.0, 0.0, 1400.0), 330.0);
        assert_eq!(context_left_edge(150.0, 400.0, 100.0, 1400.0), 100.0);
    }

    #[test]
    fn changed_inspection_retries_boundedly_and_keeps_manual_review_wall() {
        let form = || LiveForm {
            origin: "https://example.invalid".into(),
            schema: waypoint_applications::form_mapping::FormSchema {
                origin: "https://example.invalid".into(),
                fields: vec![],
                multi_step: false,
                steps: 1,
            },
            walls: vec!["captcha present".into()],
        };
        let stale =
            || DriverError("DOM.resolveNode: protocol error No node with given id found".into());
        let mut attempts = 0;
        let result = retry_changed_inspection(Err(stale()), |_| {
            attempts += 1;
            if attempts < 3 {
                Err(stale())
            } else {
                Ok(form())
            }
        })
        .unwrap();
        assert_eq!(attempts, 3);
        assert!(result
            .walls
            .iter()
            .any(|wall| wall == CHANGED_DURING_INSPECTION));
        assert!(result.walls.iter().any(|wall| wall == "captcha present"));
        let mut attempts = 0;
        let error = retry_changed_inspection(Err(stale()), |_| {
            attempts += 1;
            Err(stale())
        })
        .unwrap_err();
        assert_eq!(attempts, 3);
        assert!(error.0.contains("review it manually"));
        assert!(retry_changed_inspection(
            Err(DriverError("browser disconnected".into())),
            |_| panic!("unexpected retry")
        )
        .is_err());
    }

    #[test]
    fn only_the_known_disappeared_node_error_is_recoverable() {
        assert!(is_stale_inspection_node(&CdpError(
            "DOM.resolveNode: protocol error {\"code\":-32000,\"message\":\"No node with given id found\"}".into()
        )));
        assert!(!is_stale_inspection_node(&CdpError(
            "browser disconnected".into()
        )));
        assert!(!is_stale_inspection_node(&CdpError(
            "protocol timeout".into()
        )));
    }

    #[test]
    fn printable_ascii_has_key_metadata_but_unicode_and_controls_do_not() {
        for character in ' '..='~' {
            assert!(ascii_key(character).is_some(), "missing {character:?}");
        }
        for character in ['\n', '\t', '\u{7f}', 'é'] {
            assert!(ascii_key(character).is_none(), "unexpected {character:?}");
        }
        assert_eq!(ascii_key('@'), Some(("Digit2".into(), 50, 8, '2')));
        assert_eq!(ascii_key('A'), Some(("KeyA".into(), 65, 8, 'a')));
    }

    #[test]
    fn long_text_segments_preserve_content_and_bound_round_trips() {
        let text = "Résumé 🧭 and relevant experience. ".repeat(100);
        let segments = text_segments(&text);
        assert_eq!(segments.concat(), text);
        assert_eq!(segments.len(), 8);
        assert!(segments[..7].iter().all(|segment| segment.ends_with(' ')));
        let unbroken = "é".repeat(100);
        assert_eq!(text_segments(&unbroken), vec![unbroken.as_str()]);
        assert!(text_segments("").is_empty());
    }

    #[test]
    fn delayed_inspection_keeps_previously_observed_walls() {
        let mut next = LiveForm {
            origin: "https://example.invalid".into(),
            schema: waypoint_applications::form_mapping::FormSchema {
                origin: "https://example.invalid".into(),
                fields: vec![],
                multi_step: false,
                steps: 1,
            },
            walls: vec!["other wall".into()],
        };
        retain_observed_walls(
            vec!["captcha present".into(), "other wall".into()],
            &mut next,
        );
        assert_eq!(next.walls, vec!["other wall", "captcha present"]);
    }

    fn loading_test_page(selectors: &[&str]) -> LiveForm {
        use waypoint_applications::form_mapping::{FormField, FormFieldType, FormSchema};
        LiveForm {
            origin: "https://example.invalid".into(),
            schema: FormSchema {
                origin: "https://example.invalid".into(),
                fields: selectors
                    .iter()
                    .map(|selector| FormField {
                        context: Default::default(),
                        selector: (*selector).into(),
                        label: "Field".into(),
                        field_type: FormFieldType::Text,
                        required: true,
                        sensitive: false,
                        editing_mode: None,
                        max_length_utf16: None,
                        min_length_utf16: None,
                        pattern: None,
                        autocomplete: None,
                    })
                    .collect(),
                multi_step: false,
                steps: 1,
            },
            walls: vec![],
        }
    }

    #[test]
    fn loading_stability_requires_two_consecutive_nonempty_matches() {
        let a = loading_test_page(&["#a"]);
        let b = loading_test_page(&["#a", "#b"]);
        let empty = loading_test_page(&[]);
        let mut progress = LoadingStability::default();
        assert!(!progress.settled());
        progress.observe(&a, &a);
        assert!(!progress.settled());
        progress.observe(&a, &b);
        assert_eq!(progress.unchanged, 0);
        progress.observe(&b, &b);
        assert!(!progress.settled());
        progress.observe(&b, &b);
        assert!(progress.settled());
        progress.observe(&b, &empty);
        progress.observe(&empty, &empty);
        assert!(!progress.settled(), "empty forms must not count as settled");
        assert_eq!(progress.snapshots, 6);
    }

    #[test]
    fn loading_field_loss_remains_visible_after_recovery() {
        let page = loading_test_page;
        let mut grown = page(&["#a", "#b"]);
        retain_loading_observations(page(&["#a"]), &mut grown);
        assert!(grown.walls.is_empty());
        let mut disappeared = page(&["#b"]);
        retain_loading_observations(grown, &mut disappeared);
        assert_eq!(disappeared.walls, vec![DYNAMIC_FIELD_LOSS_REVIEW]);
        let mut recovered = page(&["#a", "#b"]);
        retain_loading_observations(disappeared, &mut recovered);
        assert_eq!(recovered.walls, vec![DYNAMIC_FIELD_LOSS_REVIEW]);
    }

    #[test]
    fn navigation_result_rejects_errors_and_downloads() {
        assert!(check_navigation_result(&json!({"frameId":"frame"})).is_ok());
        assert!(
            check_navigation_result(&json!({"errorText":"net::ERR_FILE_NOT_FOUND"}))
                .unwrap_err()
                .0
                .contains("ERR_FILE_NOT_FOUND")
        );
        assert!(check_navigation_result(&json!({"isDownload":true}))
            .unwrap_err()
            .0
            .contains("download"));
    }

    #[test]
    fn file_accept_checks_extensions_mime_types_and_unknown_formats() {
        for allowed in [
            "",
            ".PDF",
            "application/pdf",
            ".docx, application/pdf",
            "application/*",
            "*/*",
        ] {
            assert!(
                check_file_accept(allowed, "resume.pdf").is_ok(),
                "{allowed}"
            );
        }
        assert!(check_file_accept(".docx,application/msword", "resume.pdf").is_err());
        assert!(check_file_accept("image/*", "resume.pdf").is_err());
        assert!(check_file_accept("application/x-unknown", "file.unknown")
            .unwrap_err()
            .0
            .contains("Cannot verify"));
        assert!(check_file_accept(".unknown", "file.unknown").is_ok());
    }

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

#[cfg(test)]
mod embedded_url_tests {
    use super::*;
    fn tree() -> Value {
        json!({"frameTree":{"frame":{"id":"child","parentId":"parent","url":"https://frame.example.invalid/RoleAbC","securityOrigin":"https://frame.example.invalid","urlFragment":"#StepAbC"}}})
    }
    #[test]
    fn child_url_preserves_case_and_fragment_without_using_the_parent_history() {
        assert_eq!(
            embedded_frame_url(&tree()).unwrap(),
            "https://frame.example.invalid/RoleAbC#StepAbC"
        );
        let mut value = tree();
        value["frameTree"]["frame"]["url"] = json!("https://frame.example.invalid/RoleAbC#StepAbC");
        assert_eq!(
            embedded_frame_url(&value).unwrap(),
            "https://frame.example.invalid/RoleAbC#StepAbC"
        );
    }
    #[test]
    fn unknown_or_conflicting_child_metadata_never_becomes_a_verified_url() {
        for (field, bad) in [
            ("parentId", Value::Null),
            ("parentId", json!("child")),
            ("id", json!("")),
            ("securityOrigin", json!("https://other.example.invalid")),
            ("securityOrigin", Value::Null),
            (
                "url",
                json!("https://fictional:credential@frame.example.invalid/"),
            ),
            (
                "securityOrigin",
                json!("https://frame.example.invalid/not-an-origin"),
            ),
            ("urlFragment", json!(format!("#{}", "x".repeat(4096)))),
            ("url", json!("javascript:alert(1)")),
            ("url", json!("https://frame.example.invalid/a#Different")),
            ("url", json!(" https://frame.example.invalid/a")),
            ("urlFragment", json!("missing-marker")),
            ("urlFragment", json!(17)),
        ] {
            let mut value = tree();
            value["frameTree"]["frame"][field] = bad;
            assert!(embedded_frame_url(&value).is_err(), "{value}");
        }
    }
    #[test]
    fn only_the_explicit_top_level_protocol_error_can_select_the_child_fallback() {
        let native=CdpError("Page.getNavigationHistory: protocol error {\"code\":-32000,\"message\":\"Command can only be executed on top-level targets\"}".into());
        assert!(history_requires_top_level(&native));
        for message in ["Page.getNavigationHistory: timed out","Page.getNavigationHistory: protocol error {\"code\":-32000,\"message\":\"Allocation failed\"}","Page.getNavigationHistory: protocol error {\"code\":-32603,\"message\":\"Command can only be executed on top-level targets\"}"] { assert!(!history_requires_top_level(&CdpError(message.into()))); }
    }
}

#[cfg(all(test, windows))]
mod replay_lifecycle_qualification;
