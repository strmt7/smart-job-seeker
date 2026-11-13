//! Bounded display equality for explicit international telephone numbers.
//! This compares identity, not number assignment, country, or dialability.

/// Preserve every digit and the explicit international plus. Only ASCII display
/// separators are removable; extensions, URI syntax and national prefixes are
/// never inferred. Native field validity remains a separate requirement.
pub fn canonical_international(value: &str) -> Option<String> {
    if !(2..=128).contains(&value.len()) || !value.starts_with('+') {
        return None;
    }
    let mut digits = String::with_capacity(15);
    let mut group = None;
    for byte in value.bytes().skip(1) {
        match byte {
            b'0'..=b'9' => {
                if digits.is_empty() && byte == b'0' {
                    return None;
                }
                if digits.len() == 15 {
                    return None;
                }
                digits.push(char::from(byte));
            }
            b' ' | b'-' | b'.' if !digits.is_empty() => {}
            b'(' if !digits.is_empty() && group.is_none() => group = Some(digits.len()),
            b')' if group.is_some_and(|start| digits.len() > start) => group = None,
            _ => return None,
        }
    }
    if digits.is_empty()
        || group.is_some()
        || !matches!(value.as_bytes().last(), Some(b'0'..=b'9' | b')'))
    {
        return None;
    }
    Some(format!("+{digits}"))
}

pub fn same_international(expected: &str, observed: &str) -> bool {
    canonical_international(expected)
        .is_some_and(|key| canonical_international(observed).as_ref() == Some(&key))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_international_identity_across_display_grouping() {
        for (a, b) in [
            ("+12025550123", "+1 202-555-0123"),
            ("+442012345678", "+44 (20) 1234.5678"),
            ("+390212345678", "+39 02 1234 5678"),
            ("+861012345678", "+86 10 1234 5678"),
            ("+123456789012345", "+1 234 567 890 123 45"),
        ] {
            assert!(same_international(a, b));
            assert!(same_international(b, a));
        }
    }
    #[test]
    fn never_guesses_context_extensions_missing_digits_or_non_ascii_symbols() {
        for value in [
            "12025550123",
            "0012025550123",
            "+02025550123",
            "+1 202-555-0124",
            "+2025550123",
            "+1 2025550123 x4",
            "+1 2025550123;ext=4",
            "tel:+12025550123",
            "+1,2025550123",
            "+1#2025550123",
            "+1/2025550123",
            "+1\t2025550123",
            "+1\n2025550123",
            "+1 ２０２５５５０１２３",
            "+1−2025550123",
            "+1\u{200b}2025550123",
            "+1 (2025550123",
            "+1 ()2025550123",
            "+1 ((202))5550123",
            "+1 2025550123 ",
            "+1 2025550123-",
            "+1+2025550123",
            "+1202555012300000",
        ] {
            assert!(!same_international("+12025550123", value), "{value:?}");
        }
        assert!(canonical_international(&format!("+1{}", " ".repeat(128))).is_none());
    }
}
