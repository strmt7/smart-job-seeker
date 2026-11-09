//! Compare native Gregorian month displays using ICU/CLDR, preserving precision.
//! Only the visual-review caller for completed native month controls uses this.
use icu_calendar::{Date, Gregorian};
use icu_datetime::{
    fieldsets::YM, options::YearStyle, pattern::FixedCalendarDateTimeNames,
    FixedCalendarDateTimeFormatter,
};
use icu_locale_core::Locale;
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
};

// Locale data supplies month names, order, punctuation and digit shaping. These
// are language preferences, never employer/site-specific field rules.
const LOCALES: &[&str] = &[
    "en-US", "en-GB", "de", "fr", "es", "pt-BR", "pt-PT", "it", "nl", "el", "pl", "ru", "uk", "ja",
    "zh-Hans", "zh-Hant", "ko", "ar", "he", "fa", "hi", "th", "vi", "id", "ms", "tr", "sv", "da",
    "nb", "fi", "cs", "ro", "hu", "bg", "ca", "eu", "gl", "bn", "ta", "ur", "sw", "af",
];
type Displays = BTreeMap<String, u16>;
thread_local! {
    // Bound memory even when a page supplies many years. No host locale changes.
    static CACHE: RefCell<VecDeque<(i32, Displays)>> = const { RefCell::new(VecDeque::new()) };
}
fn normalized(value: &str) -> String {
    value
        .chars()
        .filter(|c| {
            !c.is_whitespace()
                && !matches!(
                    c,
                    '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2066}'..='\u{2069}'
                )
        })
        .flat_map(char::to_lowercase)
        .collect()
}
fn displays(year: i32) -> Displays {
    let mut result = BTreeMap::new();
    for language in LOCALES {
        let Ok(locale) = language.parse::<Locale>() else {
            continue;
        };
        for length in [YM::long(), YM::medium(), YM::short()] {
            let length = length.with_year_style(YearStyle::Full);
            let preferences = locale.clone().into();
            let Ok(formatter) =
                FixedCalendarDateTimeFormatter::<Gregorian, _>::try_new(preferences, length)
            else {
                continue;
            };
            // Load numeric data explicitly as well as the selected pattern's
            // names. Some locale pattern alternatives need it only at format time.
            let mut names = FixedCalendarDateTimeNames::from_formatter(preferences, formatter);
            if names.include_decimal_formatter().is_err() {
                continue;
            }
            let Ok(formatter) = names.try_into_formatter(length) else {
                continue;
            };
            for month in 1..=12_u8 {
                // The day is an internal calendar anchor. YM never emits it;
                // neither candidate values nor their precision are changed.
                let Ok(date) = Date::try_new_gregorian(year, month, 1) else {
                    continue;
                };
                let text = normalized(&formatter.format(&date).to_string());
                *result.entry(text).or_insert(0) |= 1 << (month - 1);
            }
        }
    }
    result
}
pub(crate) fn equivalent(visible: &str, expected: &str) -> bool {
    if expected.len() != 7
        || !expected.is_ascii()
        || !waypoint_applications::form_mapping::valid_iso_month(expected)
        || visible.len() > 1000
    {
        return false;
    }
    let Ok(year) = expected[..4].parse::<i32>() else {
        return false;
    };
    let Ok(month) = expected[5..].parse::<u8>() else {
        return false;
    };
    if !(1..=12).contains(&month) {
        return false;
    }
    let visible = normalized(visible);
    // Never guess the century from a locale's reduced-year short form.
    let mut digits = 0;
    if !visible.chars().any(|c| {
        digits = if c.is_numeric() { digits + 1 } else { 0 };
        digits >= 4
    }) {
        return false;
    }
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !cache.iter().any(|(cached, _)| *cached == year) {
            if cache.len() == 2 {
                cache.pop_front();
            }
            cache.push_back((year, displays(year)));
        }
        cache
            .iter()
            .find(|(cached, _)| *cached == year)
            .and_then(|(_, values)| values.get(&visible))
            .is_some_and(|mask| *mask == 1 << (month - 1))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn localized_months_match_without_changing_year_or_precision() {
        for text in [
            "June 2020",
            "Jun 2020",
            "Juni 2020",
            "juin 2020",
            "junio de 2020",
            "2020年6月",
        ] {
            assert!(equivalent(text, "2020-06"), "{text}");
            assert!(!equivalent(text, "2020-05"), "{text}");
            assert!(!equivalent(text, "2021-06"), "{text}");
        }
        for text in [
            "June",
            "2020",
            "June 1, 2020",
            "June 2020 extra",
            "J 2020",
            "06/07/2020",
            "June 20",
        ] {
            assert!(!equivalent(text, "2020-06"), "{text}");
        }
        for value in [
            "2020",
            "2020-00",
            "2020-13",
            "2020-06-01",
            "20é0-06",
            "202é-6",
        ] {
            assert!(!equivalent("June 2020", value));
        }
    }
    #[test]
    fn reference_data_rejects_ambiguous_renderings_and_cache_is_bounded() {
        assert!(!equivalent("J 2020", "2020-06"));
        for year in 2019..2024 {
            assert!(equivalent(&format!("June {year}"), &format!("{year}-06")));
        }
        CACHE.with(|cache| assert_eq!(cache.borrow().len(), 2));
    }
}
