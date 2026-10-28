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

pub fn build(
    recipient: &str,
    subject: &str,
    body: &str,
    attachments: &[(String, Vec<u8>)],
    boundary: &str,
) -> Result<Vec<u8>, String> {
    let email = recipient.trim();
    if email.len() > 254
        || email.chars().any(|c| c.is_whitespace() || c.is_control())
        || email.matches('@').count() != 1
        || email.starts_with('@')
        || email.ends_with('@')
        || email.contains(['<', '>', ',', ';', '"'])
    {
        return Err("Enter one valid recipient email address".into());
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
    for (name, bytes) in attachments {
        if !name.to_ascii_lowercase().ends_with(".pdf") || !bytes.starts_with(b"%PDF-") {
            return Err("Attachments must be PDF files".into());
        }
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                    c
                } else {
                    '_'
                }
            })
            .take(100)
            .collect();
        message.push_str(&format!("--{boundary}\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"{safe}\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n", lines(bytes)));
    }
    message.push_str(&format!("--{boundary}--\r\n"));
    Ok(message.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
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
