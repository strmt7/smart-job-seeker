//! Reconcile prepared answers with independent native and visual review evidence.
use serde::Deserialize;
use serde_json::{json, Value};
use waypoint_applications::driver::{DriverError, LiveForm};
use waypoint_browser::{CdpDriver, RecordedSummaryCapture, SummaryAnswer, SummaryKey};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VisionRead {
    phase: Phase,
    observations: Vec<VisualAnswer>,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Phase {
    FinalReview,
    OtherScreen,
    Unclear,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VisualAnswer {
    sections: Vec<String>,
    label: String,
    visible_value: String,
}

struct ExpectedAnswer<'a> {
    key: SummaryKey,
    value: &'a str,
    attachment: bool,
}

impl<'a> ExpectedAnswer<'a> {
    fn from_entry(entry: &'a Value) -> Result<Self, &'static str> {
        if entry["status"] != "visually_matched" || entry["native_verified"] != true {
            return Err("A prior entry was not verified");
        }
        Ok(Self {
            key: serde_json::from_value(entry["summary_key"].clone())
                .map_err(|_| "A prior entry has no scoped label")?,
            value: entry["intended_value"]
                .as_str()
                .ok_or("A prior entry has no prepared answer")?,
            attachment: entry["kind"] == "file",
        })
    }
}

fn identity(sections: &[String], label: &str) -> Vec<String> {
    sections
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(label))
        .map(|part| {
            part.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
        })
        .collect()
}

fn parse_visual(raw: &Value) -> Result<VisionRead, &'static str> {
    let visual: VisionRead =
        serde_json::from_value(raw.clone()).map_err(|_| "The image read has an invalid shape")?;
    if visual.observations.len() > 20
        || visual.observations.iter().any(|answer| {
            answer.visible_value.len() > 1000
                || answer
                    .sections
                    .iter()
                    .chain(std::iter::once(&answer.label))
                    .any(|label| label.trim().is_empty() || label.len() > 1000)
        })
    {
        return Err("The image read exceeds the summary evidence bounds");
    }
    Ok(visual)
}

pub(super) fn inspect(
    driver: &mut CdpDriver,
    form: &LiveForm,
    image: &str,
    native: Result<Box<RecordedSummaryCapture>, DriverError>,
    fields: &[Value],
    read: impl FnOnce(&str) -> Result<Value, String>,
) -> Value {
    let native = match native {
        Ok(native) => native,
        Err(error) => {
            return json!({"scope":"offline_saved_summary","status":"pending_manual_review",
            "reason":error.to_string()})
        }
    };
    let visual = if native.answers().next().is_some() {
        read(image)
    } else {
        Err("No structured answers were found in the saved summary".into())
    };
    let revalidated = driver
        .verify_recorded_summary(form, &native)
        .map_err(|error| error.to_string());
    reconcile(&native, fields, visual, revalidated)
}

fn reconcile(
    native: &RecordedSummaryCapture,
    fields: &[Value],
    visual: Result<Value, String>,
    revalidated: Result<(), String>,
) -> Value {
    let rows: Vec<_> = native.answers().collect();
    let mut result = json!({"status":"pending_manual_review","scope":"offline_saved_summary",
        "native_sha256":native.digest(),"native_incomplete":native.incomplete(),
        "native_rows":rows.iter().map(|(answer,in_image)|json!({"answer":answer,"in_image":in_image})).collect::<Vec<_>>(),
        "expected_count":fields.len(),"matched_count":0});
    let parsed = match visual {
        Ok(raw) => {
            let parsed = parse_visual(&raw).map_err(str::to_owned);
            result["visual"] = raw;
            parsed
        }
        Err(reason) => Err(reason),
    };
    if let Err(reason) = revalidated {
        result["reason"] = json!(reason);
        result["native_revalidated"] = json!(false);
        return result;
    }
    result["native_revalidated"] = json!(true);
    let expected = fields
        .iter()
        .map(ExpectedAnswer::from_entry)
        .collect::<Result<Vec<_>, _>>()
        .map_err(str::to_owned);
    let (expected, visual) = match (expected, parsed) {
        (Ok(expected), Ok(visual)) => (expected, visual),
        (Err(error), _) | (_, Err(error)) => {
            result["reason"] = json!(error);
            return result;
        }
    };
    let (comparisons, matched, extra_native, extra_visual) =
        compare_answers(&expected, &rows, &visual.observations);
    result["comparisons"] = json!(comparisons);
    result["matched_count"] = json!(matched);
    result["extra_native_count"] = json!(extra_native);
    result["extra_visual_count"] = json!(extra_visual);
    let reason = if visual.phase != Phase::FinalReview {
        "The image did not establish a final application review screen"
    } else if native.incomplete() {
        "The summary contains rows whose labels or values could not be bound"
    } else if expected.is_empty() || extra_native + extra_visual > 0 {
        "The summary and the prepared answers cover different fields"
    } else if matched != expected.len() {
        "Some prepared answers need review"
    } else {
        result["status"] = json!("answers_verified");
        return result;
    };
    result["reason"] = json!(reason);
    result
}

fn compare_answers(
    expected: &[ExpectedAnswer<'_>],
    native: &[(&SummaryAnswer, bool)],
    visual: &[VisualAnswer],
) -> (Vec<Value>, usize, usize, usize) {
    let expected_keys: Vec<_> = expected
        .iter()
        .map(|answer| identity(&answer.key.sections, &answer.key.label))
        .collect();
    let native_keys: Vec<_> = native
        .iter()
        .map(|(answer, _)| identity(&answer.key.sections, &answer.key.label))
        .collect();
    let visual_keys: Vec<_> = visual
        .iter()
        .map(|answer| identity(&answer.sections, &answer.label))
        .collect();
    let mut matched = 0;
    let comparisons = expected
        .iter()
        .zip(&expected_keys)
        .map(|(expected, key)| {
            let status = if expected_keys
                .iter()
                .filter(|candidate| *candidate == key)
                .count()
                != 1
            {
                "prepared_label_ambiguous"
            } else {
                compare_answer(expected, key, native, visual, &native_keys, &visual_keys)
            };
            matched += usize::from(status == "matched");
            json!({"key":expected.key,"status":status})
        })
        .collect();
    let extra_native = native_keys
        .iter()
        .filter(|key| !expected_keys.contains(key))
        .count();
    let extra_visual = visual_keys
        .iter()
        .filter(|key| !expected_keys.contains(key))
        .count();
    (comparisons, matched, extra_native, extra_visual)
}

fn compare_answer(
    expected: &ExpectedAnswer<'_>,
    key: &[String],
    native: &[(&SummaryAnswer, bool)],
    visual: &[VisualAnswer],
    native_keys: &[Vec<String>],
    visual_keys: &[Vec<String>],
) -> &'static str {
    let mut native = native
        .iter()
        .zip(native_keys)
        .filter(|(_, found)| *found == key);
    let Some(((answer, in_image), _)) = native.next() else {
        return "missing_from_summary";
    };
    if native.next().is_some() {
        return "native_label_ambiguous";
    }
    if answer.value != expected.value {
        return "native_value_differs";
    }
    if !in_image {
        return "outside_review_image";
    }
    let mut visual = visual
        .iter()
        .zip(visual_keys)
        .filter(|(_, found)| *found == key);
    let Some((answer, _)) = visual.next() else {
        return "missing_from_image_read";
    };
    if visual.next().is_some() {
        return "visual_label_ambiguous";
    }
    if answer.visible_value != expected.value {
        return "visual_value_differs";
    }
    if expected.attachment {
        return "attachment_content_unverified";
    }
    "matched"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_exact_values_within_distinct_repeated_sections() {
        let first = SummaryKey {
            sections: vec!["Current employer".into()],
            label: "Organization".into(),
        };
        let second = SummaryKey {
            sections: vec!["Previous employer".into()],
            label: "Organization".into(),
        };
        let expected = [
            ExpectedAnswer {
                key: first.clone(),
                value: "Anvil & Co.",
                attachment: false,
            },
            ExpectedAnswer {
                key: second.clone(),
                value: "Anchor GmbH",
                attachment: false,
            },
        ];
        let first = SummaryAnswer {
            key: first,
            value: "Anvil & Co.".into(),
        };
        let second = SummaryAnswer {
            key: second,
            value: "Anchor GmbH".into(),
        };
        let visual = parse_visual(&json!({"phase":"final_review","observations":[
            {"sections":["Previous employer"],"label":"Organization","visible_value":"Anchor GmbH"},
            {"sections":["Current employer"],"label":"Organization","visible_value":"Anvil & Co."}
        ]}))
        .unwrap();
        assert!(visual.phase == Phase::FinalReview);
        let (_, matched, extra_native, extra_visual) = compare_answers(
            &expected,
            &[(&first, true), (&second, true)],
            &visual.observations,
        );
        assert_eq!((matched, extra_native, extra_visual), (2, 0, 0));
        let wrong = SummaryAnswer {
            value: "Anvil & Co".into(),
            ..first.clone()
        };
        let (checks, matched, _, _) = compare_answers(
            &expected,
            &[(&wrong, true), (&second, false)],
            &visual.observations,
        );
        assert_eq!(matched, 0);
        assert_eq!(checks[0]["status"], "native_value_differs");
        assert_eq!(checks[1]["status"], "outside_review_image");

        let (checks, matched, _, _) = compare_answers(
            &expected,
            &[(&first, true), (&first, true)],
            &visual.observations,
        );
        assert_eq!(matched, 0);
        assert_eq!(checks[0]["status"], "native_label_ambiguous");
        assert_eq!(checks[1]["status"], "missing_from_summary");
    }

    #[test]
    fn visual_omissions_conflicts_and_filenames_cannot_clear_summary() {
        let key = SummaryKey {
            sections: vec![],
            label: "Résumé".into(),
        };
        let native = SummaryAnswer {
            key: key.clone(),
            value: "Resume.pdf".into(),
        };
        let expected = [ExpectedAnswer {
            key,
            value: "Resume.pdf",
            attachment: true,
        }];
        let row = json!({"sections":[],"label":"Résumé","visible_value":"Resume.pdf"});
        let wrong = json!({"sections":[],"label":"Résumé","visible_value":"resume.pdf"});
        let unknown = json!({"sections":[],"label":"Unexpected","visible_value":"Resume.pdf"});
        for (observations, wanted, extra) in [
            (vec![], "missing_from_image_read", 0),
            (vec![wrong.clone()], "visual_value_differs", 0),
            (vec![row.clone(), wrong], "visual_label_ambiguous", 0),
            (vec![row, unknown], "attachment_content_unverified", 1),
        ] {
            let observed =
                parse_visual(&json!({"phase":"final_review","observations":observations})).unwrap();
            let (checks, matched, _, extra_visual) =
                compare_answers(&expected, &[(&native, true)], &observed.observations);
            assert_eq!(matched, 0);
            assert_eq!(checks[0]["status"], wanted);
            assert_eq!(extra_visual, extra);
        }
        assert!(parse_visual(&json!({"phase":"final_review","observations":[
            {"sections":[],"label":"Résumé","visible_value":"Resume.pdf","extra":true}
        ]}))
        .is_err());
    }
}
