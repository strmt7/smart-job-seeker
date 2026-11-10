//! Local privacy checks for anonymous capture references, before any transport.
//! This is not source authorization, DNS validation or proof that arbitrary
//! opaque application identifiers contain no personal information.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UrlPrivacyError {
    Invalid,
    PrivateReference,
    EncodingLimit,
}

/// Return a bounded, decoded view for additional local candidate-identifier
/// checks. Never replace the original request URL with this decoded view.
pub fn checked_reference(raw: &str) -> Result<String, UrlPrivacyError> {
    if raw.len() > 4096 || raw.trim() != raw || raw.chars().any(char::is_control) {
        return Err(UrlPrivacyError::Invalid);
    }
    let mut decoded = raw.to_owned();
    for round in 0..=3 {
        let url = url::Url::parse(&decoded).map_err(|_| UrlPrivacyError::Invalid)?;
        if !url.username().is_empty() || url.password().is_some() || email_like(&decoded) {
            return Err(UrlPrivacyError::PrivateReference);
        }
        let pairs: Vec<_> = url.query_pairs().collect();
        if pairs.len() > 64 {
            return Err(UrlPrivacyError::Invalid);
        }
        for (key, value) in pairs {
            if private_key(&key) || email_like(&value) {
                return Err(UrlPrivacyError::PrivateReference);
            }
            // Public redirects and nested application URLs are common. Check
            // their query keys too, with a shared, strictly decreasing budget.
            if let Some(nested) = nested_candidate(&value, &url)? {
                nested_reference(&nested, 3 - round)?;
            }
        }
        let next = percent_decoded(&decoded)?;
        if next == decoded {
            return Ok(decoded);
        }
        if round == 3 {
            return Err(UrlPrivacyError::EncodingLimit);
        }
        decoded = next;
    }
    Err(UrlPrivacyError::EncodingLimit)
}

fn nested_candidate(value: &str, base: &url::Url) -> Result<Option<url::Url>, UrlPrivacyError> {
    let value = value.trim_start();
    let is_reference = value.starts_with('/')
        || value.starts_with('?')
        || value.split_once(':').is_some_and(|(scheme, _)| {
            scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
        });
    if is_reference {
        base.join(value)
            .map(Some)
            .map_err(|_| UrlPrivacyError::Invalid)
    } else {
        Ok(None)
    }
}

fn nested_reference(url: &url::Url, remaining: usize) -> Result<(), UrlPrivacyError> {
    if remaining == 0 {
        return Err(UrlPrivacyError::EncodingLimit);
    }
    if !url.username().is_empty() || url.password().is_some() || email_like(url.as_str()) {
        return Err(UrlPrivacyError::PrivateReference);
    }
    let mut count = 0;
    for (key, value) in url.query_pairs() {
        count += 1;
        if count > 64 {
            return Err(UrlPrivacyError::Invalid);
        }
        if private_key(&key) || email_like(&value) {
            return Err(UrlPrivacyError::PrivateReference);
        }
        if let Some(nested) = nested_candidate(&value, url)? {
            nested_reference(&nested, remaining - 1)?;
        }
    }
    Ok(())
}

fn private_key(key: &str) -> bool {
    fn sensitive(key: &str) -> bool {
        matches!(
            key,
            "token"
                | "accesstoken"
                | "refreshtoken"
                | "idtoken"
                | "clientsecret"
                | "code"
                | "ticket"
                | "sessionid"
                | "session"
                | "password"
                | "auth"
                | "authorization"
                | "signature"
                | "apikey"
                | "email"
                | "emailaddress"
                | "phone"
                | "phonenumber"
        )
    }
    let normalized: String = key
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    sensitive(&normalized)
        || key
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|part| sensitive(&part.to_ascii_lowercase()))
}

fn email_like(text: &str) -> bool {
    text.match_indices('@').any(|(index, _)| {
        let before = &text[..index];
        if !before
            .chars()
            .next_back()
            .is_some_and(char::is_alphanumeric)
        {
            return false;
        }
        let domain: String = text[index + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
            .collect();
        let domain = domain.trim_end_matches('.');
        domain.rsplit_once('.').is_some_and(|(host, suffix)| {
            !host.is_empty() && suffix.len() >= 2 && suffix.bytes().all(|b| b.is_ascii_alphabetic())
        })
    })
}

fn percent_decoded(raw: &str) -> Result<String, UrlPrivacyError> {
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut offset = 0;
    while offset < bytes.len() {
        if bytes[offset] == b'%' {
            let digits = bytes
                .get(offset + 1..offset + 3)
                .ok_or(UrlPrivacyError::Invalid)?;
            let high = (digits[0] as char)
                .to_digit(16)
                .ok_or(UrlPrivacyError::Invalid)?;
            let low = (digits[1] as char)
                .to_digit(16)
                .ok_or(UrlPrivacyError::Invalid)?;
            decoded.push((high * 16 + low) as u8);
            offset += 3;
        } else {
            decoded.push(bytes[offset]);
            offset += 1;
        }
    }
    let decoded = String::from_utf8(decoded).map_err(|_| UrlPrivacyError::Invalid)?;
    if decoded.chars().any(char::is_control) {
        return Err(UrlPrivacyError::Invalid);
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_private_keys_nested_references_and_encoded_identifiers() {
        for raw in [
            "https://fixture.invalid/?access_token=secret",
            "https://fixture.invalid/?access-token=secret",
            "https://fixture.invalid/?user[email]=hidden",
            "https://fixture.invalid/?t%256fken=hidden",
            "https://fixture.invalid/avery%2540fixture.invalid",
            "https://fixture.invalid/?next=https%3A%2F%2Fother.invalid%2F%3Fsession_id%3Dhidden",
            "https://fixture.invalid/?next=HTTPS%3A%2F%2Fother.invalid%2F%3Fsession_id%3Dhidden",
            "https://fixture.invalid/?next=%2F%2Fother.invalid%2F%3Fsession_id%3Dhidden",
            "https://fixture.invalid/?next=%2Fform%3Fsession_id%3Dhidden",
            "https://fixture.invalid/?next=%3Fsession_id%3Dhidden",
            "https://user:secret@fixture.invalid/",
        ] {
            assert_eq!(
                checked_reference(raw),
                Err(UrlPrivacyError::PrivateReference),
                "{raw}"
            );
        }
    }

    #[test]
    fn keeps_public_job_and_scoped_asset_references_usable() {
        for raw in [
            "https://fixture.invalid/jobs?jobId=42&language=en",
            "https://fixture.invalid/assets/@scope/package@1.2.3/app.js",
            "https://fixture.invalid/?next=https%3A%2F%2Fother.invalid%2Fjobs%3FjobId%3D42",
            "https://fixture.invalid/?next=%2Fjobs%3FjobId%3D42",
            "https://fixture.invalid/jobs/Avery%2520Invented",
        ] {
            assert!(checked_reference(raw).is_ok(), "{raw}");
        }
        assert_eq!(
            checked_reference("https://fixture.invalid/Avery%2520Invented").unwrap(),
            "https://fixture.invalid/Avery Invented"
        );
    }

    #[test]
    fn malformed_or_unbounded_input_is_not_a_public_reference() {
        for raw in [
            "https://fixture.invalid/%",
            "https://fixture.invalid/%FF",
            "https://fixture.invalid/%00",
            " https://fixture.invalid/",
            "https://fixture.invalid/%2525252520",
        ] {
            assert!(checked_reference(raw).is_err(), "{raw}");
        }
        assert!(
            checked_reference(&format!("https://fixture.invalid/{}", "a".repeat(4096))).is_err()
        );
    }

    #[test]
    fn query_and_nested_reference_budgets_are_explicit() {
        let mut public = url::Url::parse("https://fixture.invalid/jobs").unwrap();
        for index in 0..64 {
            public
                .query_pairs_mut()
                .append_pair(&format!("filter{index}"), "public");
        }
        assert!(checked_reference(public.as_str()).is_ok());
        public.query_pairs_mut().append_pair("extra", "public");
        assert_eq!(
            checked_reference(public.as_str()),
            Err(UrlPrivacyError::Invalid)
        );
        let mut nested = url::Url::parse("https://fixture.invalid/jobs?jobId=42").unwrap();
        for _ in 0..4 {
            let mut outer = url::Url::parse("https://fixture.invalid/jobs").unwrap();
            outer.query_pairs_mut().append_pair("next", nested.as_str());
            nested = outer;
        }
        assert_eq!(
            checked_reference(nested.as_str()),
            Err(UrlPrivacyError::EncodingLimit)
        );
    }
}
