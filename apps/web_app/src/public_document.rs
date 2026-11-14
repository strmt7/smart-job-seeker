//! Static public-document extraction, never browser execution or form entry.
use scraper::{node::Node, ElementRef, Html, Selector};
use serde::Serialize;
use waypoint_research::tools::PublicLink;
const TEXT_BYTES: usize = 6000;
const LINK_LIMIT: usize = 8;

#[derive(Debug, Default, Serialize)]
pub(crate) struct Extracted {
    pub text: String,
    pub text_offset_bytes: usize,
    pub focus: Option<String>,
    pub focus_matches: usize,
    pub reference_base_usable: bool,
    pub scope: String,
    pub requested_fragment: Option<String>,
    pub text_truncated: bool,
    pub section_text_bytes: usize,
    pub parser_errors: usize,
    pub references: Vec<PublicLink>,
    pub link_elements_examined: usize,
    pub references_omitted: usize,
}
fn compact(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn excerpt(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}
fn heading(element: ElementRef<'_>) -> Option<u8> {
    match element.value().name() {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}
fn context(element: ElementRef<'_>) -> Result<(bool, Option<ElementRef<'_>>), String> {
    let mut block = None;
    for (depth, element) in std::iter::once(element)
        .chain(element.ancestors().filter_map(ElementRef::wrap))
        .enumerate()
    {
        if depth >= 64 {
            return Err("Public document exceeds the structural depth budget".into());
        }
        let value = element.value();
        let name = value.name();
        if matches!(
            name,
            "script"
                | "style"
                | "nav"
                | "aside"
                | "header"
                | "footer"
                | "template"
                | "noscript"
                | "iframe"
        ) || value.attr("hidden").is_some()
            || value
                .attr("aria-hidden")
                .is_some_and(|v| v.eq_ignore_ascii_case("true"))
        {
            return Ok((true, None));
        }
        if block.is_none()
            && matches!(
                name,
                "address"
                    | "article"
                    | "blockquote"
                    | "dd"
                    | "div"
                    | "dl"
                    | "dt"
                    | "fieldset"
                    | "figcaption"
                    | "figure"
                    | "h1"
                    | "h2"
                    | "h3"
                    | "h4"
                    | "h5"
                    | "h6"
                    | "li"
                    | "main"
                    | "ol"
                    | "p"
                    | "pre"
                    | "section"
                    | "table"
                    | "td"
                    | "th"
                    | "tr"
                    | "ul"
            )
        {
            block = Some(element);
        }
    }
    Ok((false, block))
}
fn finish(text: String, mut output: Extracted, focus: Option<&str>) -> Result<Extracted, String> {
    let text = compact(&text);
    if text.is_empty() {
        return Err("Public document has no usable text in the requested scope".into());
    }
    let (offset, count) = if let Some(focus) = focus {
        if focus.trim().is_empty() || focus.len() > 120 || focus.chars().any(char::is_control) {
            return Err("Invalid document focus".into());
        }
        let folded = text.to_ascii_lowercase();
        let needle = compact(focus).to_ascii_lowercase();
        let mut matches = folded.match_indices(&needle);
        let first = matches
            .next()
            .ok_or("Requested text was not found in the public document scope")?
            .0;
        let count = 1 + matches.count();
        let mut start = first.saturating_sub(240);
        while !text.is_char_boundary(start) {
            start -= 1;
        }
        (start, count)
    } else {
        (0, 0)
    };
    output.text = excerpt(&text[offset..], TEXT_BYTES);
    output.section_text_bytes = text.len();
    output.text_truncated = offset > 0 || text.len().saturating_sub(offset) > TEXT_BYTES;
    output.text_offset_bytes = offset;
    output.focus = focus.map(str::to_owned);
    output.focus_matches = count;
    Ok(output)
}
pub(crate) fn extract(
    body: &str,
    mime: Option<&str>,
    requested: &str,
    final_url: &str,
) -> Result<Extracted, String> {
    extract_focused(body, mime, requested, final_url, None)
}
pub(crate) fn extract_focused(
    body: &str,
    mime: Option<&str>,
    requested: &str,
    final_url: &str,
    focus: Option<&str>,
) -> Result<Extracted, String> {
    if body.len() > 1024 * 1024 {
        return Err("Public document exceeds the input byte budget".into());
    }
    let request = url::Url::parse(requested).map_err(|_| "Invalid public-document reference")?;
    let base = url::Url::parse(final_url).map_err(|_| "Invalid final public-document reference")?;
    let fragment = request
        .fragment()
        .filter(|v| !v.is_empty())
        .map(|v| {
            percent_encoding::percent_decode_str(v)
                .decode_utf8()
                .map(|v| v.into_owned())
                .map_err(|_| "Public fragment is not UTF-8")
        })
        .transpose()?;
    if !matches!(mime, Some("text/html" | "application/xhtml+xml")) {
        if fragment.is_some() {
            return Err("Fragment-directed reading is unavailable for this document type".into());
        }
        return finish(
            body.into(),
            Extracted {
                scope: "plain_text".into(),
                reference_base_usable: true,
                ..Default::default()
            },
            focus,
        );
    }
    let document = Html::parse_document(body);
    let base_selector = Selector::parse("base[href]").expect("fixed base selector");
    let link_base = if let Some(href) = document
        .select(&base_selector)
        .next()
        .and_then(|e| e.value().attr("href"))
    {
        base.join(href)
            .ok()
            .filter(|u| u.scheme() == "https" && u.username().is_empty() && u.password().is_none())
    } else {
        Some(base.clone())
    };
    if document.root_element().descendants().take(100_001).count() > 100_000 {
        return Err("Public document exceeds the node budget".into());
    }
    let mut root = document.root_element();
    let mut scope = "document".to_owned();
    for tag in ["main", "article", "body"] {
        let selector = Selector::parse(tag).expect("fixed content selector");
        if let Some(element) = document
            .select(&selector)
            .find(|element| context(*element).is_ok_and(|v| !v.0))
        {
            root = element;
            scope = tag.into();
            break;
        }
    }
    let mut start = None;
    let mut end_level = None;
    if let Some(fragment) = &fragment {
        let mut matches = Vec::new();
        for node in root.descendants() {
            if let Some(element) = ElementRef::wrap(node) {
                if (element.value().id() == Some(fragment.as_str())
                    || (element.value().name() == "a"
                        && element.value().attr("name") == Some(fragment.as_str())))
                    && !context(element)?.0
                {
                    matches.push(element);
                    if matches.len() > 1 {
                        break;
                    }
                }
            }
        }
        if matches.len() != 1 {
            return Err(if matches.is_empty() {
                "Requested public-document fragment was not found"
            } else {
                "Requested public-document fragment is ambiguous"
            }
            .into());
        }
        let target = matches[0];
        let section = std::iter::once(target)
            .chain(target.ancestors().filter_map(ElementRef::wrap))
            .find(|element| heading(*element).is_some());
        if let Some(section) = section {
            start = Some(section.id());
            end_level = heading(section);
            scope = format!("heading:{}", section.value().name());
        } else {
            root = target;
            scope = format!("element:{}", target.value().name());
        }
    }
    let mut active = start.is_none();
    let mut text = String::new();
    let mut last_block = None;
    let mut references = Vec::new();
    let mut preferred: Vec<bool> = Vec::new();
    let mut examined: usize = 0;
    for node in root.descendants() {
        let element = ElementRef::wrap(node).or_else(|| node.parent().and_then(ElementRef::wrap));
        let Some(element) = element else {
            continue;
        };
        let (excluded, block) = context(element)?;
        if excluded {
            continue;
        }
        if !active {
            if Some(node.id()) != start {
                continue;
            }
            active = true;
        }
        if Some(node.id()) != start
            && ElementRef::wrap(node).is_some_and(|e| {
                heading(e)
                    .zip(end_level)
                    .is_some_and(|(level, end)| level <= end)
            })
        {
            break;
        }
        if let Some(element) = ElementRef::wrap(node) {
            if element.value().name() == "br" {
                text.push(' ');
            }
            if element.value().name() == "a" {
                if let Some(href) = element.value().attr("href") {
                    examined += 1;
                    let resolved = link_base
                        .as_ref()
                        .and_then(|base| base.join(href).ok())
                        .filter(|u| {
                            u.scheme() == "https"
                                && u.username().is_empty()
                                && u.password().is_none()
                                && u.as_str().len() <= 2048
                        });
                    if let Some(url) = resolved {
                        let title = compact(&element.text().collect::<String>());
                        if !title.is_empty() {
                            let selected = focus.is_some_and(|f| {
                                title.to_ascii_lowercase().contains(&f.to_ascii_lowercase())
                                    || url
                                        .as_str()
                                        .to_ascii_lowercase()
                                        .contains(&f.to_ascii_lowercase())
                            });
                            let duplicate = references
                                .iter()
                                .any(|link: &PublicLink| link.url == url.as_str());
                            let slot = if references.len() < LINK_LIMIT {
                                Some(references.len())
                            } else if selected {
                                preferred.iter().position(|v| !*v)
                            } else {
                                None
                            };
                            if !duplicate {
                                if let Some(slot) = slot {
                                    let link = PublicLink {
                                        url: url.into(),
                                        title: excerpt(&title, 160),
                                    };
                                    if slot == references.len() {
                                        references.push(link);
                                        preferred.push(selected);
                                    } else {
                                        references[slot] = link;
                                        preferred[slot] = selected;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Node::Text(value) = node.value() {
            let current = block.map(|e| e.id());
            if !text.is_empty() && current != last_block {
                text.push(' ');
            }
            text.push_str(value.text.as_ref());
            last_block = current;
        }
    }
    let omitted = examined.saturating_sub(references.len());
    finish(
        text,
        Extracted {
            scope,
            requested_fragment: fragment,
            parser_errors: document.errors.len(),
            references,
            link_elements_examined: examined,
            references_omitted: omitted,
            reference_base_usable: link_base.is_some(),
            ..Default::default()
        },
        focus,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn read(html: &str, fragment: &str) -> Result<Extracted, String> {
        extract(
            html,
            Some("text/html"),
            &format!("https://fixture.invalid/docs{fragment}"),
            "https://fixture.invalid/reference/page",
        )
    }
    #[test]
    fn focused_read_exposes_late_text_and_actual_matching_links_with_a_bounded_window() {
        let links = (0..12)
            .map(|i| format!("<a href='unrelated/{i}'>Other {i}</a>"))
            .collect::<String>();
        let body=format!("<head><base href='https://example.org/reference/'></head><main>{links}<p>{}</p><p>ObservableState is returned by the query.</p><a href='observable-state'>ObservableState reference</a></main>","Introductory text. ".repeat(1200));
        let d = extract_focused(
            &body,
            Some("text/html"),
            "https://fixture.invalid/docs",
            "https://fixture.invalid/docs",
            Some("observablestate"),
        )
        .unwrap();
        assert!(d.text.contains("ObservableState is returned"));
        assert!(d.text_offset_bytes > 6000);
        assert_eq!(d.focus_matches, 2);
        assert!(d.text_truncated);
        assert!(d
            .references
            .iter()
            .any(|r| r.url == "https://example.org/reference/observable-state"));
        assert!(d.references.len() <= 8);
        assert_eq!(d.link_elements_examined, 13);
        assert!(extract_focused(
            &body,
            Some("text/html"),
            "https://fixture.invalid/docs",
            "https://fixture.invalid/docs",
            Some("absent-term")
        )
        .is_err());
    }
    #[test]
    fn focus_stays_inside_the_requested_section_and_unsupported_base_is_disclosed() {
        let d=extract_focused("<base href='javascript:bad()'><main><h2 id=one>First</h2><p>ππ QueryToken text.</p><a href='other'>Other</a><h2 id=two>Second</h2><p>OutsideNeedle</p></main>",Some("text/html"),"https://fixture.invalid/docs#one","https://fixture.invalid/docs",Some("QueryToken")).unwrap();
        assert_eq!(d.scope, "heading:h2");
        assert!(!d.text.contains("OutsideNeedle"));
        assert!(!d.reference_base_usable);
        assert!(d.references.is_empty());
        assert_eq!(d.references_omitted, 1);
        assert!(extract_focused(
            "<main><h2 id=one>First</h2><p>Text.</p><h2 id=two>OutsideNeedle</h2></main>",
            Some("text/html"),
            "https://fixture.invalid/docs#one",
            "https://fixture.invalid/docs",
            Some("OutsideNeedle")
        )
        .is_err());
        let d = extract_focused(
            &format!("{}État{}", "π".repeat(3000), "π".repeat(3000)),
            Some("text/plain"),
            "https://fixture.invalid/raw",
            "https://fixture.invalid/raw",
            Some("État"),
        )
        .unwrap();
        assert!(d.text.contains("État"));
        assert!(d.text.len() <= 6000);
    }
    #[test]
    fn parser_preserves_code_entities_and_quoted_tag_boundaries() {
        let d=read(r#"<nav>Navigation noise</nav><main><p data-note="x>y">An &lt;input&gt; &amp; a &lt;select&gt;.</p><p>check<em>Validity</em>()</p><script>privateNoise()</script><p hidden>Hidden noise</p><p aria-hidden=true>Hidden too</p></main>"#,"").unwrap();
        assert_eq!(d.text, "An <input> & a <select>. checkValidity()");
        assert_eq!(d.scope, "main");
    }
    #[test]
    fn fragment_reads_the_actual_heading_section_and_its_links_after_large_intro() {
        let body=format!("<main><p>{}</p><h2 id=validation>Validation</h2><p>Call <code>checkValidity()</code>.</p><h3>Feedback</h3><p>Listen for input.</p><a href='#next'>Next section</a><a href='javascript:bad()'>Bad link</a><h2 id=next>Next section</h2><p>Unrelated content</p></main>","Introductory text. ".repeat(14_000));
        let d = read(&body, "#%76alidation").unwrap();
        assert_eq!(d.scope, "heading:h2");
        assert!(d
            .text
            .starts_with("Validation Call checkValidity(). Feedback"));
        assert!(!d.text.contains("Introductory"));
        assert!(!d.text.contains("Unrelated"));
        assert_eq!(d.references.len(), 1);
        assert_eq!(
            d.references[0].url,
            "https://fixture.invalid/reference/page#next"
        );
        assert_eq!(d.references_omitted, 1);
    }
    #[test]
    fn named_and_unicode_fragments_preserve_literal_plus_and_reject_missing_or_ambiguous_scope() {
        let d=read("<main><section id='détails+suite'><p>Référence.</p></section><h2><a name=legacy>Older anchor</a></h2><p>Older text.</p></main>","#d%C3%A9tails+suite").unwrap();
        assert_eq!(d.text, "Référence.");
        assert!(read(
            "<main><h2 id=duplicate>A</h2><h2 id=duplicate>B</h2></main>",
            "#duplicate"
        )
        .unwrap_err()
        .contains("ambiguous"));
        assert!(read("<main><p>Content.</p></main>", "#missing")
            .unwrap_err()
            .contains("not found"));
        assert_eq!(read("<main><h2><a name=legacy>Older anchor</a></h2><p>Older text.</p><h2>Following section</h2></main>","#legacy").unwrap().text,"Older anchor Older text.");
    }
    #[test]
    fn hidden_duplicate_does_not_replace_a_visible_anchor_and_structural_limits_refuse() {
        assert_eq!(read("<main><section hidden id=target>Hidden</section><section id=target>Visible</section></main>","#target").unwrap().text,"Visible");
        assert!(read(
            &format!(
                "{}<p>Deep text</p>{}",
                "<div>".repeat(70),
                "</div>".repeat(70)
            ),
            ""
        )
        .unwrap_err()
        .contains("depth"));
        assert!(read(&"x".repeat(1024 * 1024 + 1), "").is_err());
    }
    #[test]
    fn plain_text_is_preserved_and_unicode_excerpt_truncation_is_explicit() {
        let d = extract(
            "Use <input> literally.",
            Some("text/plain"),
            "https://fixture.invalid/raw",
            "https://fixture.invalid/raw",
        )
        .unwrap();
        assert_eq!(d.text, "Use <input> literally.");
        assert!(extract(
            "Text",
            Some("text/plain"),
            "https://fixture.invalid/raw#missing",
            "https://fixture.invalid/raw"
        )
        .is_err());
        let d = read(&format!("<main>{}</main>", "π".repeat(4000)), "").unwrap();
        assert_eq!(d.text.len(), 6000);
        assert_eq!(d.section_text_bytes, 8000);
        assert!(d.text_truncated);
    }
}
