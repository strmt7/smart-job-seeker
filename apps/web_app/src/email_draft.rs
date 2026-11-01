//! Create an unsent MIME draft for a local email client. This module does not send mail.
use base64::{engine::general_purpose::STANDARD, Engine};

fn lines(bytes: &[u8]) -> String {
    let encoded = STANDARD.encode(bytes);
    encoded
        .as_bytes()
        .chunks(76)
        .map(|chunk| std::str::from_utf8(chunk).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\r\n")
}

// Adapted from rejection-rejector src/mail.rs at 48c672b29cf098ff5da10d324de3e87766a2f483.
// License and copyright: docs/REJECTION_REJECTOR_LICENSE.md.
fn subject_words(subject: &str) -> String {
    let mut out = Vec::new();
    let mut part = String::default();
    for c in subject.chars() {
        if part.len() + c.len_utf8() > 42 {
            out.push(format!("=?UTF-8?B?{}?=", STANDARD.encode(part.as_bytes())));
            part.clear();
        }
        part.push(c);
    }
    if !part.is_empty() {
        out.push(format!("=?UTF-8?B?{}?=", STANDARD.encode(part.as_bytes())));
    }
    out.join("\r\n ")
}

// RFC 2231 extended parameters preserve UTF-8 names alongside the ASCII fallback.
fn extended_filename(name: &str) -> String {
    let mut encoded = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(b));
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    encoded
}

pub fn build(
    recipient: &str,
    subject: &str,
    body: &str,
    attachments: &[(String, Vec<u8>)],
    boundary: &str,
) -> Result<Vec<u8>, String> {
    let email = recipient.trim();
    if !crate::smtp_handoff::address_valid(email) {
        return Err(
            "Enter one supported ASCII recipient address (for example, name@example.com)".into(),
        );
    }
    if subject.trim().is_empty()
        || subject.len() > 200
        || subject.chars().any(char::is_control)
        || body.trim().is_empty()
        || body.len() > 30000
    {
        return Err("Enter a subject and message within the size limits".into());
    }
    if attachments.len() > 5
        || attachments
            .iter()
            .map(|(_, bytes)| bytes.len())
            .sum::<usize>()
            > 10 * 1024 * 1024
    {
        return Err("Choose at most five PDF files totaling 10 MiB".into());
    }
    if boundary.is_empty() || !boundary.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err("Invalid MIME boundary".into());
    }
    let mut message = format!("To: {email}\r\nSubject: {}\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"{boundary}\"\r\n\r\n--{boundary}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n", subject_words(subject), lines(body.as_bytes()));
    let mut used_names = std::collections::HashSet::new();
    let mut used_extended_names = std::collections::HashSet::new();
    for (name, bytes) in attachments {
        if !is_pdf_attachment(name, bytes) {
            return Err("Attachments must be PDF files".into());
        }
        let portable = crate::attempt_record::attachment_filename(name);
        let stem = &portable[..portable.len() - 4];
        let safe_stem: String = stem
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                    c
                } else {
                    '_'
                }
            })
            .take(96)
            .collect();
        let safe = format!("{safe_stem}.pdf");
        if !used_names.insert(safe.to_ascii_lowercase())
            || !used_extended_names.insert(portable.to_lowercase())
        {
            return Err(
                "PDF attachment names conflict after sanitizing; rename one and try again".into(),
            );
        }
        let extended = extended_filename(&portable);
        message.push_str(&format!("--{boundary}\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"{safe}\";\r\n filename*=UTF-8''{extended}\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n", lines(bytes)));
    }
    message.push_str(&format!("--{boundary}--\r\n"));
    Ok(message.into_bytes())
}

/// Check the filename and signature; this does not validate PDF contents or layout.
pub fn is_pdf_attachment(name: &str, bytes: &[u8]) -> bool {
    name.to_ascii_lowercase().ends_with(".pdf") && bytes.starts_with(b"%PDF-")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attachment_requires_pdf_name_and_signature() {
        assert!(is_pdf_attachment("resume.PDF", b"%PDF-fictional"));
        assert!(!is_pdf_attachment("resume.docx", b"%PDF-fictional"));
        assert!(!is_pdf_attachment("resume.pdf", b"not a PDF"));
    }
    #[test]
    fn draft_recipient_matches_transport_address_rules() {
        for invalid in [
            "a..b@example.org",
            ".a@example.org",
            "a@example..org",
            "a@-example.org",
            "a@ex_ample.org",
            "é@example.org",
            "a@example.org,b@example.org",
        ] {
            assert!(
                build(invalid, "Subject", "Body", &[], "boundary").is_err(),
                "accepted {invalid}"
            );
        }
        for valid in ["first.last+jobs@example.org", "o'neil@example.org"] {
            assert!(crate::smtp_handoff::address_valid(valid));
            let draft =
                String::from_utf8(build(valid, "Subject", "Body", &[], "boundary").unwrap())
                    .unwrap();
            assert!(draft.starts_with(&format!("To: {valid}\r\n")));
        }
    }

    #[test]
    fn attachment_names_keep_pdf_extension_and_reject_collisions() {
        let long = format!("{}.PDF", "a".repeat(150));
        let draft = String::from_utf8(
            build(
                "hiring@example.org",
                "Subject",
                "Body",
                &[(long, b"%PDF-fictional".to_vec())],
                "boundary",
            )
            .unwrap(),
        )
        .unwrap();
        assert!(draft.contains(&format!("filename=\"{}.pdf\"", "a".repeat(96))));
        for names in [
            ("résumé.pdf", "r_sum_.pdf"),
            ("CV.PDF", "cv.pdf"),
            ("CON.pdf", "_CON.pdf"),
        ] {
            let attachments = [
                (names.0.to_owned(), b"%PDF-one".to_vec()),
                (names.1.to_owned(), b"%PDF-two".to_vec()),
            ];
            assert!(build(
                "hiring@example.org",
                "Subject",
                "Body",
                &attachments,
                "boundary"
            )
            .unwrap_err()
            .contains("names conflict"));
        }
    }

    #[test]
    fn international_attachment_name_uses_utf8_extended_parameter() {
        let draft = String::from_utf8(
            build(
                "hiring@example.org",
                "Subject",
                "Body",
                &[("résumé '2026'.pdf".into(), b"%PDF-fictional".to_vec())],
                "boundary",
            )
            .unwrap(),
        )
        .unwrap();
        assert!(draft.contains("filename=\"r_sum___2026_.pdf\""));
        assert!(draft.contains("filename*=UTF-8''r%C3%A9sum%C3%A9%20%272026%27.pdf"));
        let injected = String::from_utf8(
            build(
                "hiring@example.org",
                "Subject",
                "Body",
                &[(
                    "cv\r\nBcc: evil@example.org.pdf".into(),
                    b"%PDF-fictional".to_vec(),
                )],
                "boundary",
            )
            .unwrap(),
        )
        .unwrap();
        assert!(!injected.contains("\r\nBcc:"));
    }

    #[test]
    fn email_is_unsent_and_rejects_header_injection() {
        let draft = build(
            "hiring@example.org",
            "Application – Engineer",
            "Hello",
            &[("cv.pdf".into(), b"%PDF-fictional".to_vec())],
            "fictionalBoundary",
        )
        .unwrap();
        let text = String::from_utf8(draft).unwrap();
        assert!(text.contains("To: hiring@example.org\r\n"));
        assert!(text.contains("filename=\"cv.pdf\""));
        assert!(build(
            "a@example.org\r\nBcc: x@example.org",
            "Hello",
            "Hi",
            &[],
            "boundary"
        )
        .is_err());
        assert!(build(
            "a@example.org",
            "Hello",
            "Hi",
            &[("cv.docx".into(), vec![])],
            "boundary"
        )
        .is_err());
    }
}
