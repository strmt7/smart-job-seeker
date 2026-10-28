//! Bounded SMTP DATA handoff. Callers must authorize and journal external writes.
use std::io::{Read, Write};

#[derive(Debug, PartialEq, Eq)]
pub enum HandoffError {
    InvalidMessage,
    BeforeData,
    Uncertain,
}

fn address_valid(address: &str) -> bool {
    let Some((local, domain)) = address.split_once('@') else {
        return false;
    };
    address.len() <= 254
        && !local.is_empty()
        && local.len() <= 64
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".!#$%&'*+-/=?^_`{|}~".contains(&b))
        && domain.split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 63
                && !part.starts_with('-')
                && !part.ends_with('-')
                && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

fn valid_message(message: &[u8]) -> bool {
    if message.is_empty() || message.len() > 15 * 1024 * 1024 || !message.ends_with(b"\r\n") {
        return false;
    }
    message.split_inclusive(|b| *b == b'\n').all(|line| {
        line.ends_with(b"\r\n")
            && line.len() <= 1000
            && line[..line.len() - 2]
                .iter()
                .all(|b| (32..=126).contains(b) || *b == b'\t')
    })
}

/// Application callers must durably record the authorized intent before invoking this.
/// Acceptance is an SMTP handoff, not proof of inbox delivery. Never retry an
/// uncertain handoff automatically.
pub fn handoff(
    stream: &mut (impl Read + Write),
    sender: &str,
    recipient: &str,
    message: &[u8],
) -> Result<(), HandoffError> {
    if !address_valid(sender) || !address_valid(recipient) || !valid_message(message) {
        return Err(HandoffError::InvalidMessage);
    }
    for command in [
        format!("MAIL FROM:<{sender}>\r\n"),
        format!("RCPT TO:<{recipient}>\r\n"),
    ] {
        stream
            .write_all(command.as_bytes())
            .map_err(|_| HandoffError::BeforeData)?;
        crate::gmail_password::reply(stream, 250).map_err(|_| HandoffError::BeforeData)?;
    }
    stream
        .write_all(b"DATA\r\n")
        .map_err(|_| HandoffError::BeforeData)?;
    crate::gmail_password::reply(stream, 354).map_err(|_| HandoffError::BeforeData)?;
    for line in message.split_inclusive(|b| *b == b'\n') {
        if line.starts_with(b".") {
            stream
                .write_all(b".")
                .map_err(|_| HandoffError::Uncertain)?;
        }
        stream
            .write_all(line)
            .map_err(|_| HandoffError::Uncertain)?;
    }
    stream
        .write_all(b".\r\n")
        .map_err(|_| HandoffError::Uncertain)?;
    crate::gmail_password::reply(stream, 250).map_err(|_| HandoffError::Uncertain)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Wire {
        read: std::io::Cursor<Vec<u8>>,
        written: Vec<u8>,
    }
    impl Read for Wire {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.read.read(buf)
        }
    }
    impl Write for Wire {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.written.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    fn wire(response: &[u8]) -> Wire {
        Wire {
            read: std::io::Cursor::new(response.to_vec()),
            written: vec![],
        }
    }
    #[test]
    fn frames_data_and_distinguishes_uncertain_delivery() {
        let mut ok = wire(b"250 ok\r\n250 ok\r\n354 go\r\n250 accepted\r\n");
        handoff(
            &mut ok,
            "sender@example.com",
            "recipient@example.com",
            b"Subject: hi\r\n\r\n.first\r\n.\r\n",
        )
        .unwrap();
        assert!(ok.written.ends_with(b"..first\r\n..\r\n.\r\n"));
        let mut lost = wire(b"250 ok\r\n250 ok\r\n354 go\r\n");
        assert_eq!(
            handoff(
                &mut lost,
                "sender@example.com",
                "recipient@example.com",
                b"Hi\r\n"
            ),
            Err(HandoffError::Uncertain)
        );
        let mut invalid = wire(b"");
        assert_eq!(
            handoff(
                &mut invalid,
                "sender@example.com",
                "x@example.com\r\nDATA",
                b"Hi\r\n"
            ),
            Err(HandoffError::InvalidMessage)
        );
        assert_eq!(
            handoff(&mut invalid, "sender@example.com", "x@example.com", b"Hi\n"),
            Err(HandoffError::InvalidMessage)
        );
        assert!(invalid.written.is_empty());
        let draft = crate::email_draft::build(
            "recipient@example.com",
            "Application",
            "Hello",
            &[("resume.pdf".into(), b"%PDF-fictional".to_vec())],
            "fixtureBoundary",
        )
        .unwrap();
        let mut mime = wire(b"250 ok\r\n250 ok\r\n354 go\r\n250 accepted\r\n");
        handoff(
            &mut mime,
            "sender@example.com",
            "recipient@example.com",
            &draft,
        )
        .unwrap();
        assert!(mime
            .written
            .windows(b"Content-Type: application/pdf".len())
            .any(|part| part == b"Content-Type: application/pdf"));
    }
}
