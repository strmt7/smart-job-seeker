//! Bind a changed browser document to its acknowledged offline replay response.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RecordedDocumentTransition {
    from_url_sha256: String,
    url_sha256: String,
    archive_body_sha256: String,
    frame: String,
    loader: String,
    network_request: String,
    http_status: u64,
}

fn request_digest(url: &str) -> Option<(url::Origin, String)> {
    let mut url = url::Url::parse(url).ok()?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    // The broker hashes the HTTP request identity, which excludes a fragment.
    url.set_fragment(None);
    if url.query() == Some("") {
        url.set_query(None);
    }
    Some((
        url.origin(),
        waypoint_domain::sha256_hex(url.as_str().as_bytes()),
    ))
}

fn document_responses(report: &Value) -> Option<&[Value]> {
    if report["mode"] != "offline_response_replay" || report["omitted_document_responses"] != 0 {
        return None;
    }
    report["document_responses"]
        .as_array()
        .filter(|rows| rows.len() <= 64)
        .map(Vec::as_slice)
}

fn transition(
    before: &RecordedDocument,
    current: &RecordedDocument,
    checkpoint: &Value,
    report: &Value,
    events: &[(String, Value)],
) -> Option<RecordedDocumentTransition> {
    if before.frame != current.frame
        || before.loader == current.loader
        || before.lease.is_some()
        || current.lease.is_some()
    {
        return None;
    }
    let (prior_origin, prior_sha256) = request_digest(&before.url)?;
    let (origin, url_sha256) = request_digest(&current.url)?;
    if prior_origin != origin || prior_sha256 == url_sha256 {
        return None;
    }
    let prior = document_responses(checkpoint)?;
    let success = |row: &Value| {
        row["bodyless_get"] == true
            && row["redirected"] == false
            && row["status"]
                .as_u64()
                .is_some_and(|code| (200..=299).contains(&code))
    };
    let in_scope = |row: &Value| row["frame"].as_str() == Some(current.frame.as_str());
    if !prior
        .iter()
        .any(|row| in_scope(row) && row["url_sha256"] == prior_sha256 && success(row))
        || prior
            .iter()
            .any(|row| in_scope(row) && row["url_sha256"] == url_sha256)
    {
        return None;
    }
    let new = document_responses(report)?.strip_prefix(prior)?;
    let mut observed = new.iter().filter(|row| in_scope(row));
    let response = observed.next()?;
    if observed.next().is_some() || !success(response) || response["url_sha256"] != url_sha256 {
        return None;
    }
    let status = response["status"].as_u64()?;
    let archive_body_sha256 = response["body_sha256"].as_str()?;
    waypoint_domain::Sha256Digest::new(archive_body_sha256).ok()?;
    let network_request = response["network_request"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 512 && !id.chars().any(char::is_control))?;
    // A Fetch acknowledgment only proves that the broker provided bytes. A
    // native response binds the same request to this frame's current loader.
    if !events.iter().any(|(method, event)| {
        let params = &event["params"];
        method == "Network.responseReceived"
            && params["frameId"] == current.frame
            && params["loaderId"] == current.loader
            && params["requestId"] == network_request
            && params["status"] == status
    }) {
        return None;
    }
    Some(RecordedDocumentTransition {
        from_url_sha256: prior_sha256,
        url_sha256,
        archive_body_sha256: archive_body_sha256.to_owned(),
        frame: current.frame.clone(),
        loader: current.loader.clone(),
        network_request: network_request.to_owned(),
        http_status: status,
    })
}

impl CdpDriver {
    pub(super) fn recorded_archive_transition(
        &mut self,
        before: &RecordedDocument,
        active: &RecordedDocument,
        checkpoint: &Value,
    ) -> Result<Option<RecordedDocumentTransition>, DriverError> {
        let report = self.rehearsal_replay_report()?;
        Ok(transition(
            before,
            active,
            checkpoint,
            &report,
            &self.cdp.events,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_archived_route_requires_a_fresh_matching_native_response() {
        let before = RecordedDocument {
            url: "https://work.invalid/start".into(),
            frame: "root".into(),
            loader: "first-loader".into(),
            lease: None,
        };
        let after = RecordedDocument {
            url: "https://work.invalid/next?job=17#questions".into(),
            loader: "next-loader".into(),
            ..before.clone()
        };
        let receipt = |url: &str, request: &str| {
            json!({
                "frame":"root", "network_request":request,"status":200,
                "url_sha256":request_digest(url).unwrap().1,"body_sha256":"a".repeat(64),
                "bodyless_get":true,"redirected":false
            })
        };
        let first = receipt(&before.url, "first-request");
        let next = receipt("https://work.invalid/next?job=17", "next-request");
        let report = |responses: Vec<Value>| {
            json!({
                "mode":"offline_response_replay","document_responses":responses,"omitted_document_responses":0
            })
        };
        let checkpoint = report(vec![first.clone()]);
        let complete = report(vec![first.clone(), next.clone()]);
        let event = (
            "Network.responseReceived".to_owned(),
            json!({"params":{
                "requestId":"next-request","loaderId":"next-loader","frameId":"root","status":200
            }}),
        );
        let events = [event];
        assert!(transition(&before, &after, &checkpoint, &complete, &events).is_some());
        assert!(transition(&before, &after, &checkpoint, &complete, &[]).is_none());
        let mut event_changed = events.clone();
        event_changed[0].1["params"]["requestId"] = json!("different-request");
        assert!(transition(&before, &after, &checkpoint, &complete, &event_changed).is_none());
        for (key, value) in [
            ("status", json!(302)),
            ("redirected", json!(true)),
            ("bodyless_get", json!(false)),
            ("frame", json!("other")),
            ("url_sha256", json!("c".repeat(64))),
            ("body_sha256", json!("missing")),
        ] {
            let mut mismatched = complete.clone();
            mismatched["document_responses"][1][key] = value;
            assert!(
                transition(&before, &after, &checkpoint, &mismatched, &events).is_none(),
                "{key}"
            );
        }
        let mut incomplete = complete.clone();
        incomplete["omitted_document_responses"] = json!(1);
        assert!(transition(&before, &after, &checkpoint, &incomplete, &events).is_none());
        let twice = report(vec![first, next.clone(), next]);
        assert!(transition(&before, &after, &checkpoint, &twice, &events).is_none());
        assert!(transition(&before, &after, &complete, &twice, &events).is_none());
        let new_scope = RecordedDocument {
            url: "https://other.invalid/next".into(),
            ..after.clone()
        };
        let same_loader = RecordedDocument {
            loader: before.loader.clone(),
            ..after.clone()
        };
        let reloaded = RecordedDocument {
            url: before.url.clone(),
            ..after.clone()
        };
        for unadmitted in [new_scope, same_loader, reloaded] {
            assert!(transition(&before, &unadmitted, &checkpoint, &complete, &events).is_none());
        }
    }
}
