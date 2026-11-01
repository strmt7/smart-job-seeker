//! Candidate discovery only: never merge two jobs without user review.
use crate::{Store, StoredJob};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize)]
pub struct PossibleDuplicate {
    pub job_a: String,
    pub job_b: String,
    pub score: u32,
    pub revision: i64,
    pub reason: String,
    pub status: String,
    pub preferred_job: Option<String>,
    pub previous_decision: Option<PreviousDuplicateDecision>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviousDuplicateDecision {
    pub status: String,
    pub preferred_job: Option<String>,
    pub changed_job: String,
    pub change_kind: String,
    pub recorded_at: String,
}

fn decision_needs_recovery(status: &str, preferred: Option<&str>, a: &str, b: &str) -> bool {
    match status {
        "pending" | "different" => false,
        "same" => !matches!(preferred, Some(id) if id == a || id == b),
        _ => true,
    }
}

fn expose_recoverable_decision(pair: &mut PossibleDuplicate) {
    if decision_needs_recovery(
        &pair.status,
        pair.preferred_job.as_deref(),
        &pair.job_a,
        &pair.job_b,
    ) {
        pair.status = "pending".into();
        pair.preferred_job = None;
        pair.reason = format!(
            "The previous duplicate decision is incomplete or unrecognized; review it again. {}",
            pair.reason
        );
    }
}

fn words(s: &str) -> BTreeSet<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|v| !v.is_empty())
        .map(|v| match v.to_lowercase().as_str() {
            "sr" => "senior".into(),
            "jr" => "junior".into(),
            "dev" | "developer" => "engineer".into(),
            other => other.to_owned(),
        })
        .collect()
}

fn overlap(a: &BTreeSet<String>, b: &BTreeSet<String>) -> u32 {
    if a.is_empty() || b.is_empty() {
        return 0;
    }
    (200 * a.intersection(b).count() / (a.len() + b.len())) as u32
}

fn title_score(a: &str, b: &str) -> u32 {
    overlap(&title_words(a), &title_words(b))
}

fn title_terms(title: &str) -> BTreeSet<String> {
    title
        .split(|c: char| !c.is_alphanumeric() && c != '+' && c != '#')
        .map(|word| word.trim_start_matches(['+', '#']))
        .filter(|word| word.chars().any(char::is_alphanumeric))
        .map(|word| match word {
            // Uppercase Roman levels are common in job titles. Only use this
            // for candidate comparison; a shared level does not prove identity.
            "I" => "1".into(),
            "II" => "2".into(),
            "III" => "3".into(),
            "IV" => "4".into(),
            "V" => "5".into(),
            _ => match word.to_lowercase().as_str() {
                "sr" => "senior".into(),
                "jr" => "junior".into(),
                "dev" | "developer" | "engineering" => "engineer".into(),
                other => other.to_owned(),
            },
        })
        .collect()
}

/// Remove only a standalone, bracketed or trailing gender notation such as
/// (m/w/d). The remaining words, including single-letter skills, are retained.
fn title_words(title: &str) -> BTreeSet<String> {
    fn gender_marker(value: &str) -> bool {
        let mut count = 0;
        let mut separator = false;
        for ch in value.chars() {
            if matches!(ch.to_ascii_lowercase(), 'm' | 'f' | 'w' | 'd' | 'x') {
                count += 1;
            } else if matches!(ch, '/' | '-' | ',' | ' ' | '\t') {
                separator = true;
            } else {
                return false;
            }
        }
        separator && (2..=5).contains(&count)
    }
    let mut cleaned = String::with_capacity(title.len());
    let mut pending = title;
    while let Some(start) = pending.find(['(', '[']) {
        cleaned.push_str(&pending[..start]);
        let opening = pending[start..].chars().next().unwrap();
        let closing = if opening == '(' { ')' } else { ']' };
        let rest = &pending[start + 1..];
        let Some(end) = rest.find(closing) else {
            cleaned.push_str(&pending[start..]);
            pending = "";
            break;
        };
        if gender_marker(&rest[..end]) {
            cleaned.push(' ');
        } else {
            cleaned.push(opening);
            cleaned.push_str(&rest[..end]);
            cleaned.push(closing);
        }
        pending = &rest[end + 1..];
    }
    cleaned.push_str(pending);
    let trimmed = cleaned.trim_end();
    if let Some((prefix, last)) = trimmed.rsplit_once(char::is_whitespace) {
        if gender_marker(last) {
            return title_terms(prefix);
        }
    }
    title_terms(trimmed)
}

fn employer_words(s: &str) -> BTreeSet<String> {
    // Preserve the sequence of dotted initials before word-set sorting.
    let normalized = s
        .split_whitespace()
        .map(|token| {
            let parts: Vec<_> = token.trim_end_matches('.').split('.').collect();
            if (2..=10).contains(&parts.len())
                && parts.iter().all(|part| {
                    part.chars().count() == 1 && part.chars().all(char::is_alphanumeric)
                })
            {
                parts.concat()
            } else {
                token.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    words(&normalized)
        .into_iter()
        .filter(|w| {
            ![
                "inc",
                "incorporated",
                "ltd",
                "limited",
                "llc",
                "gmbh",
                "corp",
                "corporation",
                "company",
                "plc",
                "ag",
                "bv",
                "nv",
                "sa",
                "sas",
                "sarl",
                "srl",
                "spa",
                "pty",
                "oy",
                "ab",
                "kg",
            ]
            .contains(&w.as_str())
        })
        .collect()
}

/// A candidate key only: unrelated employers can share initials. Keep the
/// original word order, and reject long or non-ASCII abbreviations instead of
/// inventing transliterations.
fn employer_acronym(s: &str) -> Option<String> {
    let raw: Vec<String> = s
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .filter(|word| {
            !matches!(
                word.as_str(),
                "inc"
                    | "incorporated"
                    | "ltd"
                    | "limited"
                    | "llc"
                    | "gmbh"
                    | "corp"
                    | "corporation"
                    | "company"
                    | "plc"
                    | "ag"
                    | "bv"
                    | "nv"
                    | "sa"
                    | "sas"
                    | "sarl"
                    | "srl"
                    | "spa"
                    | "pty"
                    | "oy"
                    | "ab"
                    | "kg"
            )
        })
        .collect();
    let acronym = if raw.len() == 1 {
        raw[0].clone()
    } else if (2..=6).contains(&raw.len()) {
        raw.iter().filter_map(|word| word.chars().next()).collect()
    } else {
        return None;
    };
    (acronym.len() >= 2
        && acronym.len() <= 6
        && acronym.bytes().all(|byte| byte.is_ascii_lowercase()))
    .then_some(acronym)
}

impl Store {
    /// Add a previously missing identifier and refresh its review candidates atomically.
    pub fn add_requisition_and_refresh(&self, job: &StoredJob) -> rusqlite::Result<bool> {
        let transaction = self.conn.unchecked_transaction()?;
        let changed = transaction.execute(
            "UPDATE jobs SET requisition_id=?2 WHERE id=?1 AND (requisition_id IS NULL OR trim(requisition_id)='')",
            params![job.id.as_str(), job.requisition_id],
        )?;
        if changed != 0 {
            self.find_possible_duplicates(job)?;
        }
        transaction.commit()?;
        Ok(changed != 0)
    }

    pub fn find_possible_duplicates(
        &self,
        job: &StoredJob,
    ) -> rusqlite::Result<Vec<PossibleDuplicate>> {
        let employer_tokens = employer_words(&job.employer);
        let mut tokens: Vec<String> = employer_tokens
            .iter()
            .filter(|word| word.chars().count() >= 2 && word.len() <= 64)
            .cloned()
            .collect();
        tokens.sort_by(|a, b| {
            b.chars()
                .count()
                .cmp(&a.chars().count())
                .then_with(|| a.cmp(b))
        });
        let acronym = employer_acronym(&job.employer);
        let token_limit = if acronym.is_some() { 7 } else { 8 };
        let mut bounded = tokens.len() > token_limit;
        tokens.truncate(token_limit);
        if let Some(key) = &acronym {
            tokens.push(format!("@acr:{key}"));
        }
        // Mark complete only after comparisons finish, so interrupted indexing is retried.
        self.conn.execute(
            "DELETE FROM job_employer_indexed WHERE job_id=?1",
            [job.id.as_str()],
        )?;
        self.conn.execute(
            "DELETE FROM job_duplicate_coverage WHERE job_id=?1",
            [job.id.as_str()],
        )?;
        self.conn.execute(
            "DELETE FROM job_employer_tokens WHERE job_id=?1",
            [job.id.as_str()],
        )?;
        for token in &tokens {
            self.conn.execute(
                "INSERT INTO job_employer_tokens(job_id,token) VALUES(?1,?2)",
                params![job.id.as_str(), token],
            )?;
        }
        let missing_employer = tokens.is_empty();
        let usable_requisition = job
            .requisition_id
            .as_deref()
            .map(str::trim)
            .filter(|id| id.len() >= 3);
        let canonical_url = job
            .canonical_url
            .as_deref()
            .filter(|url| !url.trim().is_empty());
        if missing_employer && usable_requisition.is_none() && canonical_url.is_none() {
            self.conn.execute("INSERT INTO job_duplicate_coverage(job_id,status,candidates) VALUES(?1,'missing_employer',0)", [job.id.as_str()])?;
            self.conn.execute(
                "INSERT INTO job_employer_indexed(job_id) VALUES(?1)",
                [job.id.as_str()],
            )?;
            return Ok(Vec::new());
        }
        // Load and tokenize descriptions only when the title alone does not
        // already make the pair a review candidate. This keeps routine index
        // work bounded without losing the different-title comparison path.
        let mut description_words: Option<BTreeSet<String>> = None;
        let mut candidates = Vec::new();
        let mut seen = BTreeSet::new();
        // The exact saved URL is a candidate signal, not proof of identity:
        // some sites reuse a generic application URL for multiple positions.
        if let Some(url) = canonical_url {
            let mut stmt = self.conn.prepare("SELECT j.id,j.title,j.employer,j.location,j.state,j.canonical_url,j.content_hash,j.first_seen,j.last_seen,j.remote,j.source,j.requisition_id,j.posted_at FROM jobs j INDEXED BY jobs_canonical_url_lookup WHERE j.canonical_url=?1 AND j.id<>?2 ORDER BY COALESCE(j.last_seen,'') DESC,j.id ASC LIMIT 200")?;
            for row in stmt.query_map(params![url, job.id.as_str()], Self::row_to_job)? {
                let other = row?;
                if seen.insert(other.id.as_str().to_owned()) {
                    candidates.push(other);
                }
                if candidates.len() >= 200 {
                    bounded = true;
                    break;
                }
            }
        }
        // An exact requisition is a stronger review signal than arbitrary ID
        // order. Reserve candidate slots for it across all employer tokens
        // before spending the shared 200-candidate budget on general matches.
        if let Some(requisition) = job
            .requisition_id
            .as_deref()
            .map(str::trim)
            .filter(|id| id.len() >= 3)
            .filter(|_| candidates.len() < 200)
        {
            let normalized = requisition.to_ascii_lowercase();
            let mut exact = self.conn.prepare("SELECT j.id,j.title,j.employer,j.location,j.state,j.canonical_url,j.content_hash,j.first_seen,j.last_seen,j.remote,j.source,j.requisition_id,j.posted_at FROM jobs j INDEXED BY jobs_requisition_lookup JOIN job_employer_tokens t ON t.job_id=j.id WHERE lower(trim(j.requisition_id))=?1 AND t.token=?2 AND j.id<>?3 ORDER BY j.id LIMIT 200")?;
            for token in &tokens {
                for row in exact.query_map(
                    params![normalized, token, job.id.as_str()],
                    Self::row_to_job,
                )? {
                    let other = row?;
                    if seen.insert(other.id.as_str().to_owned()) {
                        candidates.push(other);
                    }
                    if candidates.len() >= 200 {
                        bounded = true;
                        break;
                    }
                }
                if candidates.len() >= 200 {
                    break;
                }
            }
        }
        // Also consider the same requisition under different employer names.
        // The shared candidate cap applies; dissimilar employers need strong
        // description corroboration below and are never merged automatically.
        if let Some(requisition) = usable_requisition.filter(|_| candidates.len() < 200) {
            let mut stmt = self.conn.prepare("SELECT j.id,j.title,j.employer,j.location,j.state,j.canonical_url,j.content_hash,j.first_seen,j.last_seen,j.remote,j.source,j.requisition_id,j.posted_at FROM jobs j INDEXED BY jobs_requisition_lookup WHERE lower(trim(j.requisition_id))=?1 AND j.id<>?2 ORDER BY j.id LIMIT 200")?;
            let mut count = 0usize;
            let mut additional = 0usize;
            for row in stmt.query_map(
                params![requisition.to_ascii_lowercase(), job.id.as_str()],
                Self::row_to_job,
            )? {
                count += 1;
                let other = row?;
                if !seen.contains(other.id.as_str()) {
                    // Do not let a widely reused identifier consume the
                    // entire budget before ordinary employer comparisons.
                    if !missing_employer && additional >= 50 {
                        bounded = true;
                        break;
                    }
                    seen.insert(other.id.as_str().to_owned());
                    candidates.push(other);
                    additional += 1;
                }
                if candidates.len() >= 200 {
                    bounded = true;
                    break;
                }
            }
            if count >= 200 {
                bounded = true;
            }
        }
        let mut stmt = self.conn.prepare("SELECT j.id,j.title,j.employer,j.location,j.state,j.canonical_url,j.content_hash,j.first_seen,j.last_seen,j.remote,j.source,j.requisition_id,j.posted_at FROM job_employer_tokens t JOIN jobs j ON j.id=t.job_id WHERE t.token=?1 AND j.id<>?2 ORDER BY j.id LIMIT 200")?;
        for token in &tokens {
            if candidates.len() >= 200 {
                break;
            }
            let rows = stmt.query_map(params![token, job.id.as_str()], Self::row_to_job)?;
            let mut token_count = 0usize;
            for row in rows {
                token_count += 1;
                let other = row?;
                if seen.insert(other.id.as_str().to_owned()) {
                    candidates.push(other);
                }
                if candidates.len() >= 200 {
                    bounded = true;
                    break;
                }
            }
            if token_count >= 200 {
                bounded = true;
            }
            if candidates.len() >= 200 {
                break;
            }
        }
        // Older dotted-initial-only employers were marked missing because each
        // indexed token was one character. Scan that bounded bucket when a
        // single compact acronym could correspond to one of those records.
        if tokens
            .iter()
            .filter(|token| !token.starts_with("@acr:"))
            .count()
            == 1
            && tokens[0].chars().count() <= 10
            && candidates.len() < 200
        {
            let mut stmt = self.conn.prepare("SELECT j.id,j.title,j.employer,j.location,j.state,j.canonical_url,j.content_hash,j.first_seen,j.last_seen,j.remote,j.source,j.requisition_id,j.posted_at FROM job_duplicate_coverage c JOIN jobs j ON j.id=c.job_id WHERE c.status='missing_employer' AND j.id<>?1 ORDER BY j.id LIMIT 200")?;
            let mut count = 0usize;
            for row in stmt.query_map([job.id.as_str()], Self::row_to_job)? {
                count += 1;
                let other = row?;
                if seen.insert(other.id.as_str().to_owned()) {
                    candidates.push(other);
                }
                if candidates.len() >= 200 {
                    bounded = true;
                    break;
                }
            }
            if count >= 200 {
                bounded = true;
            }
        }
        let candidate_count = candidates.len();
        let source_title_words = title_words(&job.title);
        let mut result = Vec::new();
        for other in candidates {
            let other_employer = employer_words(&other.employer);
            let employer_score = overlap(&employer_tokens, &other_employer);
            let same_acronym = acronym
                .as_ref()
                .is_some_and(|key| employer_acronym(&other.employer).as_ref() == Some(key));
            let same_requisition = job
                .requisition_id
                .as_deref()
                .zip(other.requisition_id.as_deref())
                .is_some_and(|(a, b)| {
                    a.trim().len() >= 3 && a.trim().eq_ignore_ascii_case(b.trim())
                });
            let same_url =
                canonical_url.is_some_and(|url| other.canonical_url.as_deref() == Some(url));
            // An exact identifier with a related employer is a review signal,
            // not proof: different employers may reuse identifiers.
            let missing_employer_pair = employer_tokens.is_empty() || other_employer.is_empty();
            let dissimilar_employers =
                !missing_employer_pair && employer_score < 40 && !same_acronym;
            if employer_score < 65
                && !(same_requisition && employer_score >= 40)
                && !same_acronym
                && !same_requisition
                && !same_url
            {
                continue;
            }
            let score = overlap(&source_title_words, &title_words(&other.title));
            let description_score = if score < 65 || dissimilar_employers {
                if description_words.is_none() {
                    let description = self.job_description(&job.id)?.unwrap_or_default();
                    description_words =
                        Some(words(&description.chars().take(30000).collect::<String>()));
                }
                let source_words = description_words.as_ref().expect("description was loaded");
                if source_words.len() >= 20 {
                    let other_description = self.job_description(&other.id)?.unwrap_or_default();
                    let other_words =
                        words(&other_description.chars().take(30000).collect::<String>());
                    (other_words.len() >= 20).then(|| overlap(source_words, &other_words))
                } else {
                    None
                }
            } else {
                None
            };
            let description_overlap = description_score.unwrap_or(0);
            if dissimilar_employers && description_overlap < 85 && !same_url {
                continue;
            }
            // Requisition identifiers can be reused across unrelated employers;
            // missing employer data requires independent textual corroboration.
            if missing_employer_pair
                && same_requisition
                && score < 65
                && description_overlap < 85
                && !same_url
            {
                continue;
            }
            if score < 65
                && !(score >= 30 && description_overlap >= 75)
                && description_overlap < 85
                && !same_requisition
                && !same_url
            {
                continue;
            }
            let (job_a, job_b) = if job.id.as_str() < other.id.as_str() {
                (job.id.as_str(), other.id.as_str())
            } else {
                (other.id.as_str(), job.id.as_str())
            };
            let description_evidence = description_score
                .map(|value| format!("description overlap {value}%"))
                .unwrap_or_else(|| {
                    if score >= 65 {
                        "description not compared".into()
                    } else {
                        "description unavailable or too short".into()
                    }
                });
            let reason = format!("Employer token overlap {employer_score}%; title overlap {score}%, {description_evidence}. {} {} {} {} {} Compare location, duties and requisition numbers.", if same_url { "Exact application URL matches; sites may reuse URLs for different positions." } else { "" }, if same_requisition { "Matching requisition identifiers." } else { "" }, if same_acronym && employer_score < 65 { "Employer initials match; initials can belong to different employers." } else { "" }, if missing_employer_pair { "Employer missing; identifiers can be reused by unrelated employers." } else { "" }, if dissimilar_employers { "Employer names differ; compare the listing details carefully." } else { "" });
            self.conn.execute("INSERT INTO possible_duplicates(job_a,job_b,score,reason,status) VALUES(?1,?2,?3,?4,'pending') ON CONFLICT(job_a,job_b) DO UPDATE SET score=excluded.score,reason=CASE WHEN possible_duplicates.status='pending' AND possible_duplicates.reason LIKE 'Listing changed after duplicate review; review again.%' THEN 'Listing changed after duplicate review; review again. ' || excluded.reason ELSE excluded.reason END", params![job_a,job_b,score,reason])?;
            let (status, preferred_job, stored_reason, revision) = self.conn.query_row(
                "SELECT status,preferred_job,reason,revision FROM possible_duplicates WHERE job_a=?1 AND job_b=?2",
                params![job_a, job_b],
                |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, String>(2)?, r.get(3)?)),
            )?;
            let previous_decision = if status == "pending"
                && stored_reason.starts_with("Listing changed after duplicate review;")
            {
                self.latest_duplicate_reopen(job_a, job_b)?
            } else {
                None
            };
            let mut pair = PossibleDuplicate {
                job_a: job_a.into(),
                job_b: job_b.into(),
                score,
                revision,
                reason: stored_reason,
                status,
                preferred_job,
                previous_decision,
            };
            expose_recoverable_decision(&mut pair);
            result.push(pair);
        }
        self.conn.execute(
            "INSERT INTO job_duplicate_coverage(job_id,status,candidates) VALUES(?1,?2,?3)",
            params![
                job.id.as_str(),
                if missing_employer {
                    "missing_employer"
                } else if bounded {
                    "bounded"
                } else {
                    "complete"
                },
                candidate_count as i64
            ],
        )?;
        self.conn.execute(
            "INSERT INTO job_employer_indexed(job_id) VALUES(?1)",
            [job.id.as_str()],
        )?;
        Ok(result)
    }

    /// Make a dedicated background index connection yield immediately to another writer.
    pub fn configure_nonblocking_duplicate_indexer(&self) -> rusqlite::Result<()> {
        self.conn.busy_timeout(std::time::Duration::ZERO)
    }

    pub fn duplicate_index_is_busy(error: &rusqlite::Error) -> bool {
        matches!(error, rusqlite::Error::SqliteFailure(code, _) if
            matches!(code.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked))
    }

    /// Index one job or advance one bounded legacy-recheck page.
    pub fn index_next_duplicate_job(&self, after: &str) -> rusqlite::Result<Option<String>> {
        // One idle indexing step is atomic: partial pair updates and index
        // markers roll back together if any comparison or write fails.
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let cursor: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key='duplicate_cross_employer_cursor'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let mut next = None;
        let mut progressed = None;
        if let Some(cursor) = cursor {
            let mut page = self
                .conn
                .prepare("SELECT id FROM jobs WHERE id>?1 ORDER BY id LIMIT 64")?;
            let ids = page
                .query_map([&cursor], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if let Some(last) = ids.last() {
                next = self.conn.query_row("SELECT j.id,j.title,j.employer,j.location,j.state,j.canonical_url,j.content_hash,j.first_seen,j.last_seen,j.remote,j.source,j.requisition_id,j.posted_at FROM jobs j LEFT JOIN job_descriptions d ON d.job_id=j.id WHERE j.id>?1 AND j.id<=?2 AND (NOT EXISTS (SELECT 1 FROM job_employer_indexed i WHERE i.job_id=j.id) OR (length(CAST(trim(j.requisition_id) AS BLOB))>=3 AND trim(j.employer)<>'' AND d.content IS NOT NULL AND d.content<>'')) ORDER BY j.id LIMIT 1", params![cursor, last], Self::row_to_job).optional()?;
                let advanced = next.as_ref().map(|job| job.id.as_str()).unwrap_or(last);
                self.conn.execute(
                    "UPDATE meta SET value=?1 WHERE key='duplicate_cross_employer_cursor'",
                    [advanced],
                )?;
                if next.is_none() {
                    progressed = Some(last.clone());
                }
            } else {
                self.conn.execute(
                    "DELETE FROM meta WHERE key='duplicate_cross_employer_cursor'",
                    [],
                )?;
            }
        } else if let Some(job) = self.conn.query_row("SELECT j.id,j.title,j.employer,j.location,j.state,j.canonical_url,j.content_hash,j.first_seen,j.last_seen,j.remote,j.source,j.requisition_id,j.posted_at FROM jobs j WHERE j.id>?1 AND NOT EXISTS (SELECT 1 FROM job_employer_indexed i WHERE i.job_id=j.id) ORDER BY j.id LIMIT 1", [after], Self::row_to_job).optional()? {
            next = Some(job);
        } else if let Some(cursor) = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key='duplicate_title_level_cursor'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            let mut page = self
                .conn
                .prepare("SELECT id,title FROM jobs WHERE id>?1 ORDER BY id LIMIT 64")?;
            let jobs = page
                .query_map([&cursor], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if let Some((last, _)) = jobs.last() {
                let candidate = jobs.iter().find(|(_, title)| {
                    title_words(title)
                        .iter()
                        .any(|term| matches!(term.as_str(), "1" | "2" | "3" | "4" | "5"))
                });
                let advanced = candidate.map(|(id, _)| id).unwrap_or(last);
                self.conn.execute(
                    "UPDATE meta SET value=?1 WHERE key='duplicate_title_level_cursor'",
                    [advanced],
                )?;
                if candidate.is_some() {
                    next = self.conn.query_row("SELECT id,title,employer,location,state,canonical_url,content_hash,first_seen,last_seen,remote,source,requisition_id,posted_at FROM jobs WHERE id=?1", [advanced], Self::row_to_job).optional()?;
                } else {
                    progressed = Some(last.clone());
                }
            } else {
                self.conn.execute(
                    "DELETE FROM meta WHERE key='duplicate_title_level_cursor'",
                    [],
                )?;
            }
        }
        let indexed = if let Some(job) = next {
            let id = job.id.as_str().to_owned();
            self.find_possible_duplicates(&job)?;
            Some(id)
        } else {
            progressed
        };
        transaction.commit()?;
        Ok(indexed)
    }

    pub fn duplicate_index_pending(&self) -> rusqlite::Result<bool> {
        self.conn.query_row("SELECT 1 WHERE EXISTS (SELECT 1 FROM meta WHERE key IN ('duplicate_cross_employer_cursor','duplicate_title_level_cursor')) OR EXISTS (SELECT 1 FROM jobs j WHERE NOT EXISTS (SELECT 1 FROM job_employer_indexed i WHERE i.job_id=j.id))", [], |_| Ok(())).optional().map(|v| v.is_some())
    }

    /// Counts are about bounded comparison work, not proof that every duplicate
    /// was identified. Missing rows represent pending indexing.
    pub fn duplicate_coverage_counts(&self) -> rusqlite::Result<(u64, u64, u64, u64)> {
        self.conn.query_row("SELECT COUNT(*), SUM(CASE WHEN c.job_id IS NULL OR i.job_id IS NULL THEN 1 ELSE 0 END), SUM(CASE WHEN c.status='bounded' AND i.job_id IS NOT NULL THEN 1 ELSE 0 END), SUM(CASE WHEN c.status='missing_employer' AND i.job_id IS NOT NULL THEN 1 ELSE 0 END) FROM jobs j LEFT JOIN job_duplicate_coverage c ON c.job_id=j.id LEFT JOIN job_employer_indexed i ON i.job_id=j.id", [], |row| Ok((row.get(0)?, row.get::<_, Option<u64>>(1)?.unwrap_or(0), row.get::<_, Option<u64>>(2)?.unwrap_or(0), row.get::<_, Option<u64>>(3)?.unwrap_or(0))))
    }
    pub fn duplicate_coverage_revision(&self) -> rusqlite::Result<i64> {
        self.conn.query_row(
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key='duplicate_coverage_revision'",
            [],
            |row| row.get(0),
        )
    }

    /// Gather bounded, undecided employer or requisition neighbors for review.
    /// A shared requisition alone is only a candidate, never a duplicate decision.
    pub fn duplicate_neighbors(&self, id: &str) -> rusqlite::Result<(Vec<StoredJob>, bool)> {
        let source = match waypoint_domain::JobIdentityId::new(id) {
            Ok(id) => self.get_job(&id)?,
            Err(_) => None,
        };
        let source_requisition = source
            .as_ref()
            .and_then(|job| job.requisition_id.as_deref())
            .map(str::trim)
            .filter(|value| value.len() >= 3);
        let mut tokens = self.conn.prepare("SELECT token FROM job_employer_tokens WHERE job_id=?1 ORDER BY length(token) DESC,token LIMIT 8")?;
        let tokens = tokens
            .query_map([id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut counts = BTreeMap::<String, usize>::new();
        let mut truncated = false;
        for token in tokens {
            let mut stmt = self.conn.prepare("SELECT t.job_id FROM job_employer_tokens t WHERE t.token=?1 AND t.job_id<>?2 AND NOT EXISTS (SELECT 1 FROM possible_duplicates p WHERE (p.job_a=?2 AND p.job_b=t.job_id) OR (p.job_b=?2 AND p.job_a=t.job_id)) ORDER BY t.job_id LIMIT 101")?;
            let rows = stmt.query_map(params![token, id], |r| r.get::<_, String>(0))?;
            for (index, row) in rows.enumerate() {
                let other = row?;
                if index == 100 {
                    truncated = true;
                    break;
                }
                *counts.entry(other).or_default() += 1;
            }
        }
        if let Some(requisition) = source_requisition {
            let mut stmt = self.conn.prepare("SELECT j.id FROM jobs j INDEXED BY jobs_requisition_lookup WHERE lower(trim(j.requisition_id))=?1 AND j.id<>?2 AND NOT EXISTS (SELECT 1 FROM possible_duplicates p WHERE (p.job_a=?2 AND p.job_b=j.id) OR (p.job_b=?2 AND p.job_a=j.id)) ORDER BY j.id LIMIT 101")?;
            for (index, row) in stmt
                .query_map(params![requisition.to_ascii_lowercase(), id], |r| {
                    r.get::<_, String>(0)
                })?
                .enumerate()
            {
                if index == 100 {
                    truncated = true;
                    break;
                }
                counts.entry(row?).or_default();
            }
        }
        let mut ranked: Vec<_> = counts.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        truncated |= ranked.len() > 12;
        let source_title = source.as_ref().map(|job| job.title.as_str()).unwrap_or("");
        let prioritize = ranked.len() > 12;
        let mut result = Vec::new();
        let mut candidates = Vec::new();
        for (candidate, count) in ranked {
            if let Ok(id) = waypoint_domain::JobIdentityId::new(&candidate) {
                if let Some(job) = self.get_job(&id)? {
                    if !prioritize || result.len() < 4 {
                        result.push(job);
                        continue;
                    }
                    let title = title_score(source_title, &job.title);
                    let requisition = source_requisition.is_some_and(|source| {
                        job.requisition_id
                            .as_deref()
                            .is_some_and(|other| source.eq_ignore_ascii_case(other.trim()))
                    });
                    candidates.push((job, count, title, requisition));
                    candidates.sort_by(
                        |(a, a_count, a_title, a_req), (b, b_count, b_title, b_req)| {
                            b_req
                                .cmp(a_req)
                                .then_with(|| b_title.cmp(a_title))
                                .then_with(|| b_count.cmp(a_count))
                                .then_with(|| a.id.as_str().cmp(b.id.as_str()))
                        },
                    );
                    candidates.truncate(8);
                }
            }
        }
        // Preserve a small set selected only by employer evidence, so title
        // changes cannot exclude all semantically equivalent roles.
        result.extend(candidates.into_iter().map(|(job, _, _, _)| job));
        Ok((result, truncated))
    }

    /// Keyset page over employer and requisition neighbors, bounded per index.
    pub fn duplicate_neighbors_page(
        &self,
        id: &str,
        after: &str,
    ) -> rusqlite::Result<(Vec<StoredJob>, Option<String>)> {
        let mut stmt = self.conn.prepare("SELECT token FROM job_employer_tokens WHERE job_id=?1 ORDER BY length(token) DESC,token LIMIT 8")?;
        let tokens = stmt
            .query_map([id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut ids = BTreeSet::new();
        let mut candidates = self.conn.prepare("SELECT t.job_id FROM job_employer_tokens t WHERE t.token=?1 AND t.job_id<>?2 AND t.job_id>?3 ORDER BY t.job_id LIMIT 13")?;
        for token in tokens {
            for candidate in
                candidates.query_map(params![token, id, after], |row| row.get::<_, String>(0))?
            {
                ids.insert(candidate?);
            }
        }
        let requisition: Option<String> = self
            .conn
            .query_row("SELECT requisition_id FROM jobs WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .optional()?
            .flatten();
        if let Some(requisition) = requisition
            .as_deref()
            .map(str::trim)
            .filter(|value| value.len() >= 3)
        {
            let mut exact = self.conn.prepare("SELECT j.id FROM jobs j INDEXED BY jobs_requisition_lookup WHERE lower(trim(j.requisition_id))=?1 AND j.id<>?2 AND j.id>?3 ORDER BY j.id LIMIT 13")?;
            for candidate in exact.query_map(
                params![requisition.to_ascii_lowercase(), id, after],
                |row| row.get::<_, String>(0),
            )? {
                ids.insert(candidate?);
            }
        }
        let more = ids.len() > 12;
        let mut jobs = Vec::new();
        let mut last = None;
        for candidate in ids.into_iter().take(12) {
            last = Some(candidate.clone());
            let decided: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM possible_duplicates WHERE (job_a=?1 AND job_b=?2) OR (job_a=?2 AND job_b=?1))", params![id, candidate], |row| row.get(0))?;
            if !decided {
                if let Ok(candidate_id) = waypoint_domain::JobIdentityId::new(&candidate) {
                    if let Some(job) = self.get_job(&candidate_id)? {
                        jobs.push(job);
                    }
                }
            }
        }
        let next = if more { last } else { None };
        Ok((jobs, next))
    }

    pub fn add_ai_duplicates(
        &self,
        first: &str,
        matches: &[(String, String)],
    ) -> rusqlite::Result<usize> {
        let transaction = self.conn.unchecked_transaction()?;
        let mut added = 0;
        for (second, reason) in matches {
            let (a, b) = if first < second.as_str() {
                (first, second.as_str())
            } else {
                (second.as_str(), first)
            };
            if a != b {
                added += transaction.execute("INSERT INTO possible_duplicates(job_a,job_b,score,reason,status) VALUES(?1,?2,0,?3,'pending') ON CONFLICT(job_a,job_b) DO NOTHING", params![a,b,reason])?;
            }
        }
        transaction.commit()?;
        Ok(added)
    }

    fn latest_duplicate_reopen(
        &self,
        a: &str,
        b: &str,
    ) -> rusqlite::Result<Option<PreviousDuplicateDecision>> {
        self.conn.query_row("SELECT previous_status,previous_preferred,changed_job,change_kind,recorded_at FROM duplicate_reopen_log WHERE job_a=?1 AND job_b=?2 ORDER BY id DESC LIMIT 1", params![a,b], |row| Ok(PreviousDuplicateDecision {status:row.get(0)?, preferred_job:row.get(1)?, changed_job:row.get(2)?, change_kind:row.get(3)?, recorded_at:row.get(4)?})).optional()
    }

    pub fn possible_duplicates(&self) -> rusqlite::Result<Vec<PossibleDuplicate>> {
        let mut stmt = self.conn.prepare("SELECT job_a,job_b,score,reason,status,preferred_job,revision FROM possible_duplicates WHERE status <> 'different' ORDER BY job_a,job_b")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (job_a, job_b, score, reason, status, preferred_job, revision) = row?;
            let previous_decision = if status == "pending"
                && reason.starts_with("Listing changed after duplicate review;")
            {
                self.latest_duplicate_reopen(&job_a, &job_b)?
            } else {
                None
            };
            let mut pair = PossibleDuplicate {
                job_a,
                job_b,
                score,
                revision,
                reason,
                status,
                preferred_job,
                previous_decision,
            };
            expose_recoverable_decision(&mut pair);
            result.push(pair);
        }
        Ok(result)
    }

    pub fn has_duplicate_hold(&self, id: &str) -> rusqlite::Result<bool> {
        self.conn.query_row("SELECT 1 FROM possible_duplicates WHERE (job_a=?1 OR job_b=?1) AND status<>'different' AND (status<>'same' OR preferred_job IS NULL OR preferred_job<>?1) LIMIT 1", [id], |_| Ok(())).optional().map(|v|v.is_some())
    }

    pub fn pending_duplicate_pair(&self, first: &str, second: &str) -> rusqlite::Result<bool> {
        let (a, b) = if first < second {
            (first, second)
        } else {
            (second, first)
        };
        self.conn.query_row("SELECT 1 FROM possible_duplicates WHERE job_a=?1 AND job_b=?2 AND (status='pending' OR (status='same' AND (preferred_job IS NULL OR preferred_job NOT IN (job_a,job_b))) OR status NOT IN ('pending','same','different'))", params![a,b], |_| Ok(())).optional().map(|v|v.is_some())
    }

    pub fn decide_duplicate_at_revision(
        &self,
        a: &str,
        b: &str,
        same: bool,
        revision: i64,
    ) -> rusqlite::Result<bool> {
        let preferred = a;
        let (a, b) = if a < b { (a, b) } else { (b, a) };
        Ok(self.conn.execute("UPDATE possible_duplicates SET status=?3,preferred_job=?4 WHERE job_a=?1 AND job_b=?2 AND revision=?5 AND (status='pending' OR (status='same' AND (preferred_job IS NULL OR preferred_job NOT IN (job_a,job_b))) OR status NOT IN ('pending','same','different'))", params![a,b,if same {"same"} else {"different"},if same {Some(preferred)} else {None},revision])? == 1)
    }

    pub fn decide_duplicate(&self, a: &str, b: &str, same: bool) -> rusqlite::Result<bool> {
        let preferred = a;
        let (a, b) = if a < b { (a, b) } else { (b, a) };
        Ok(self.conn.execute(
            "UPDATE possible_duplicates SET status=?3,preferred_job=?4 WHERE job_a=?1 AND job_b=?2",
            params![
                a,
                b,
                if same { "same" } else { "different" },
                if same { Some(preferred) } else { None }
            ],
        )? == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waypoint_domain::{ApplicationState, JobIdentityId};

    fn job(id: &str, title: &str, employer: &str, requisition: Option<&str>) -> StoredJob {
        StoredJob {
            id: JobIdentityId::new(id).unwrap(),
            title: title.into(),
            employer: employer.into(),
            location: "Berlin".into(),
            state: ApplicationState::Discovered,
            canonical_url: None,
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: false,
            source: "fixture".into(),
            requisition_id: requisition.map(str::to_owned),
            posted_at: None,
        }
    }

    #[test]
    fn exact_application_url_prompts_review_even_with_different_or_missing_employers() {
        let mut store = Store::in_memory().unwrap();
        let mut first = job("a", "Software Engineer", "Example", None);
        let mut second = job("b", "Platform Lead", "", None);
        first.canonical_url = Some("https://example.org/jobs/42".into());
        second.canonical_url = first.canonical_url.clone();
        for item in [&first, &second] {
            store.upsert_job(item).unwrap();
            store.find_possible_duplicates(item).unwrap();
        }
        let pairs = store.possible_duplicates().unwrap();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].status, "pending");
        assert!(pairs[0].reason.contains("Exact application URL matches"));
        assert!(store.has_duplicate_hold("a").unwrap());
        assert!(store.has_duplicate_hold("b").unwrap());
        store.decide_duplicate("a", "b", false).unwrap();
        store.find_possible_duplicates(&second).unwrap();
        assert!(!store.has_duplicate_hold("a").unwrap());
        second.canonical_url = Some("https://example.org/jobs/43".into());
        store.upsert_job(&second).unwrap();
        assert!(store.duplicate_index_pending().unwrap());
    }

    #[test]
    fn migration_rechecks_existing_shared_urls() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        #[cfg(feature = "sqlcipher")]
        conn.execute_batch("PRAGMA key = 'fictional-test-key';")
            .unwrap();
        for migration in &crate::MIGRATIONS[..39] {
            conn.execute_batch(migration).unwrap();
        }
        for id in ["a", "b"] {
            conn.execute("INSERT INTO jobs(id,title,employer,location,state,canonical_url) VALUES(?1,'Role','','','\"discovered\"','https://example.org/jobs/42')", [id]).unwrap();
            conn.execute("INSERT INTO job_employer_indexed(job_id) VALUES(?1)", [id])
                .unwrap();
        }
        let store = Store::migrate(conn).unwrap();
        assert!(store.duplicate_index_pending().unwrap());
        store.index_next_duplicate_job("").unwrap();
        assert_eq!(store.possible_duplicates().unwrap().len(), 1);
    }

    #[test]
    fn engineering_title_variants_are_rechecked_without_merging() {
        assert_eq!(
            title_score("Software Engineering", "Software Engineer"),
            100
        );
        assert!(title_score("Software Engineering", "Marketing Director") < 65);
        assert!(title_score("C++ Engineering", "C# Engineer") < 65);
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        #[cfg(feature = "sqlcipher")]
        conn.execute_batch("PRAGMA key = 'fictional-test-key';")
            .unwrap();
        for migration in &crate::MIGRATIONS[..40] {
            conn.execute_batch(migration).unwrap();
        }
        for (id, title) in [("a", "Software Engineering"), ("b", "Software Engineer")] {
            conn.execute("INSERT INTO jobs(id,title,employer,location,state) VALUES(?1,?2,'Example Labs','','\"discovered\"')", params![id,title]).unwrap();
            conn.execute(
                "INSERT INTO job_employer_tokens(job_id,token) VALUES(?1,'example'),(?1,'labs')",
                [id],
            )
            .unwrap();
            conn.execute("INSERT INTO job_employer_indexed(job_id) VALUES(?1)", [id])
                .unwrap();
        }
        let store = Store::migrate(conn).unwrap();
        assert!(store.duplicate_index_pending().unwrap());
        assert_eq!(
            store.index_next_duplicate_job("").unwrap().as_deref(),
            Some("a")
        );
        let pairs = store.possible_duplicates().unwrap();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].status, "pending");
        assert!(store.has_duplicate_hold("a").unwrap());
        assert!(store.has_duplicate_hold("b").unwrap());
    }

    #[test]
    fn crowded_shared_url_is_bounded_and_reports_incomplete_coverage() {
        let mut store = Store::in_memory().unwrap();
        for n in 0..201 {
            let mut item = job(&format!("job-{n:03}"), "Role", "Example", None);
            item.canonical_url = Some("https://example.org/apply".into());
            store.upsert_job(&item).unwrap();
        }
        let mut target = job("target", "Role", "Example", None);
        target.canonical_url = Some("https://example.org/apply".into());
        store.upsert_job(&target).unwrap();
        assert_eq!(store.find_possible_duplicates(&target).unwrap().len(), 200);
        let status: String = store
            .conn
            .query_row(
                "SELECT status FROM job_duplicate_coverage WHERE job_id='target'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "bounded");
    }

    #[test]
    fn incomplete_duplicate_decisions_hold_both_listings() {
        let mut store = Store::in_memory().unwrap();
        let first = job("a", "Engineer", "Example", None);
        let second = job("b", "Engineer", "Example", None);
        for item in [&first, &second] {
            store.upsert_job(item).unwrap();
            store.find_possible_duplicates(item).unwrap();
        }
        for (status, preferred) in [
            ("same", None),
            ("same", Some("other")),
            ("unrecognized", None),
        ] {
            store.conn.execute("UPDATE possible_duplicates SET status=?1, preferred_job=?2 WHERE job_a='a' AND job_b='b'", params![status, preferred]).unwrap();
            assert!(store.has_duplicate_hold("a").unwrap());
            assert!(store.has_duplicate_hold("b").unwrap());
            let pair = store.possible_duplicates().unwrap().remove(0);
            assert_eq!(pair.status, "pending");
            assert!(pair.preferred_job.is_none());
            assert!(pair.reason.contains("review it again"));
            assert!(store.pending_duplicate_pair("a", "b").unwrap());
            assert!(!store
                .decide_duplicate_at_revision("a", "b", true, pair.revision + 1)
                .unwrap());
            assert!(store
                .decide_duplicate_at_revision("a", "b", true, pair.revision)
                .unwrap());
            assert!(!store
                .decide_duplicate_at_revision("a", "b", true, pair.revision)
                .unwrap());
            assert!(!store.has_duplicate_hold("a").unwrap());
            assert!(store.has_duplicate_hold("b").unwrap());
        }
        store.conn.execute("UPDATE possible_duplicates SET status='same', preferred_job='a' WHERE job_a='a' AND job_b='b'", []).unwrap();
        assert!(!store.has_duplicate_hold("a").unwrap());
        assert!(store.has_duplicate_hold("b").unwrap());
        store.conn.execute("UPDATE possible_duplicates SET status='different', preferred_job=NULL WHERE job_a='a' AND job_b='b'", []).unwrap();
        assert!(!store.has_duplicate_hold("a").unwrap());
        assert!(!store.has_duplicate_hold("b").unwrap());
    }

    #[test]
    fn duplicate_decision_rejects_stale_pending_revision_and_replay() {
        let mut store = Store::in_memory().unwrap();
        let mut first = job("a", "Engineer", "Example", None);
        let second = job("b", "Engineer", "Example", None);
        for item in [&first, &second] {
            store.upsert_job(item).unwrap();
            store.find_possible_duplicates(item).unwrap();
        }
        let initial = store.possible_duplicates().unwrap()[0].revision;
        first.title = "Senior Engineer".into();
        store.upsert_job(&first).unwrap();
        let fresh = store.possible_duplicates().unwrap()[0].revision;
        assert!(fresh > initial);
        assert!(!store
            .decide_duplicate_at_revision("a", "b", false, initial)
            .unwrap());
        assert!(store.pending_duplicate_pair("a", "b").unwrap());
        store
            .set_job_description(&first.id, Some("New fictional duties"))
            .unwrap();
        assert!(!store
            .decide_duplicate_at_revision("a", "b", false, fresh)
            .unwrap());
        let current = store.possible_duplicates().unwrap()[0].revision;
        assert!(store
            .decide_duplicate_at_revision("a", "b", false, current)
            .unwrap());
        assert!(!store
            .decide_duplicate_at_revision("a", "b", true, current)
            .unwrap());
    }

    #[test]
    fn changed_listing_reopens_different_decision_atomically() {
        let mut store = Store::in_memory().unwrap();
        let mut first = job("a", "Engineer", "Example", None);
        let second = job("b", "Engineer", "Example", None);
        for item in [&first, &second] {
            store.upsert_job(item).unwrap();
            store.find_possible_duplicates(item).unwrap();
        }
        assert!(store.decide_duplicate("a", "b", false).unwrap());
        assert!(!store.has_duplicate_hold("a").unwrap());
        first.last_seen = Some("2".into());
        store.upsert_job(&first).unwrap();
        assert!(!store.has_duplicate_hold("a").unwrap());
        first.title = "Senior Engineer".into();
        store.upsert_job(&first).unwrap();
        assert!(store.has_duplicate_hold("a").unwrap());
        assert!(store.pending_duplicate_pair("a", "b").unwrap());
        let previous = store
            .possible_duplicates()
            .unwrap()
            .remove(0)
            .previous_decision
            .unwrap();
        assert_eq!(previous.status, "different");
        assert_eq!(previous.changed_job, "a");
        assert!(store.possible_duplicates().unwrap()[0]
            .reason
            .contains("after duplicate review"));
        store.find_possible_duplicates(&first).unwrap();
        assert!(store.possible_duplicates().unwrap()[0]
            .reason
            .contains("after duplicate review"));
        let count: i64 = store
            .conn
            .query_row("SELECT count(*) FROM duplicate_reopen_log", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
        assert!(store.decide_duplicate("a", "b", false).unwrap());
        store
            .set_job_description(&first.id, Some("New duties"))
            .unwrap();
        assert!(store.pending_duplicate_pair("a", "b").unwrap());
        assert!(store.decide_duplicate("a", "b", false).unwrap());
        store
            .set_job_description(&first.id, Some("New duties"))
            .unwrap();
        assert!(!store.has_duplicate_hold("a").unwrap());
        store.conn.execute_batch("CREATE TRIGGER fail_reopen BEFORE INSERT ON duplicate_reopen_log BEGIN SELECT RAISE(ABORT, 'fixture'); END;").unwrap();
        first.title = "Changed again".into();
        assert!(store.upsert_job(&first).is_err());
        assert_eq!(
            store.get_job(&first.id).unwrap().unwrap().title,
            "Senior Engineer"
        );
        assert!(!store.has_duplicate_hold("a").unwrap());
        store
            .conn
            .execute_batch("DROP TRIGGER fail_reopen;")
            .unwrap();
        assert!(store.decide_duplicate("a", "b", true).unwrap());
        assert!(!store.has_duplicate_hold("a").unwrap());
        assert!(store.has_duplicate_hold("b").unwrap());
        first.title = "Senior Engineer".into();
        first.remote = true;
        store.upsert_job(&first).unwrap();
        assert!(store.has_duplicate_hold("a").unwrap());
        assert!(store.has_duplicate_hold("b").unwrap());
        let prior: (String, Option<String>) = store.conn.query_row("SELECT previous_status,previous_preferred FROM duplicate_reopen_log ORDER BY id DESC LIMIT 1", [], |row| Ok((row.get(0)?,row.get(1)?))).unwrap();
        assert_eq!(prior, ("same".into(), Some("a".into())));
        let previous = store
            .possible_duplicates()
            .unwrap()
            .remove(0)
            .previous_decision
            .unwrap();
        assert_eq!(previous.status, "same");
        assert_eq!(previous.preferred_job.as_deref(), Some("a"));
    }

    fn check_background_indexer_yields(writer: &mut Store, indexer: &Store) {
        let candidate = job("pending", "Engineer", "Example", None);
        writer.upsert_job(&candidate).unwrap();
        indexer.configure_nonblocking_duplicate_indexer().unwrap();
        let timeout: i64 = indexer
            .conn
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .unwrap();
        assert_eq!(timeout, 0);
        writer.conn.execute_batch("BEGIN IMMEDIATE").unwrap();
        let error = indexer.index_next_duplicate_job("").unwrap_err();
        assert!(Store::duplicate_index_is_busy(&error));
        writer.conn.execute_batch("ROLLBACK").unwrap();
        assert!(indexer.duplicate_index_pending().unwrap());
        assert_eq!(
            indexer.index_next_duplicate_job("").unwrap().as_deref(),
            Some("pending")
        );
        assert!(!writer.duplicate_index_pending().unwrap());
    }

    #[test]
    fn background_indexer_yields_to_writer_and_retries() {
        let path = std::env::temp_dir().join(format!(
            "waypoint-index-lock-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let mut writer = Store::open(&path).unwrap();
            let indexer = Store::open(&path).unwrap();
            check_background_indexer_yields(&mut writer, &indexer);
        }
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "sqlcipher")]
    #[test]
    fn encrypted_background_indexer_yields_to_writer_and_retries() {
        let path = std::env::temp_dir().join(format!(
            "waypoint-index-cipher-lock-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let mut writer = Store::open_encrypted(&path, &[19; 32]).unwrap();
            let indexer = Store::open_encrypted(&path, &[19; 32]).unwrap();
            check_background_indexer_yields(&mut writer, &indexer);
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn missing_employer_with_matching_requisition_and_role_is_reviewed_in_either_order() {
        for reverse in [false, true] {
            let mut store = Store::in_memory().unwrap();
            let known = job(
                "known",
                "Senior Software Engineer",
                "Example Labs",
                Some("REQ-481"),
            );
            let missing = job("missing", "Software Engineer", "", Some("req-481"));
            for item in if reverse {
                [&missing, &known]
            } else {
                [&known, &missing]
            } {
                store.upsert_job(item).unwrap();
                store.find_possible_duplicates(item).unwrap();
            }
            let matches = store.possible_duplicates().unwrap();
            assert_eq!(matches.len(), 1);
            assert_eq!(matches[0].status, "pending");
            assert!(matches[0].reason.contains("Employer missing"));
            assert!(store.has_duplicate_hold("known").unwrap());
            assert!(store.has_duplicate_hold("missing").unwrap());
            assert_eq!(store.all_jobs().unwrap().len(), 2);
            assert_eq!(store.duplicate_coverage_counts().unwrap().3, 1);
        }
    }

    #[test]
    fn rechecking_missing_employer_does_not_reverse_a_user_decision() {
        let mut store = Store::in_memory().unwrap();
        let known = job(
            "known",
            "Senior Software Engineer",
            "Example Labs",
            Some("REQ-481"),
        );
        let missing = job("missing", "Software Engineer", "", Some("REQ-481"));
        for item in [&known, &missing] {
            store.upsert_job(item).unwrap();
            store.find_possible_duplicates(item).unwrap();
        }
        assert!(store.decide_duplicate("known", "missing", false).unwrap());
        store.find_possible_duplicates(&missing).unwrap();
        assert!(store.possible_duplicates().unwrap().is_empty());
        assert!(!store.has_duplicate_hold("known").unwrap());
        let status: String = store
            .conn
            .query_row(
                "SELECT status FROM possible_duplicates WHERE job_a='known' AND job_b='missing'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "different");
    }

    #[test]
    fn missing_employer_allows_distinct_titles_with_strong_description_evidence() {
        let mut store = Store::in_memory().unwrap();
        let known = job(
            "known",
            "Computational biologist",
            "Example Institute",
            Some("REQ-481"),
        );
        let missing = job("missing", "Genomics specialist", "", Some("REQ-481"));
        let description = "Analyze genomic sequencing data using Python and statistical methods collaborate with research teams prepare reproducible results manage experiments communicate findings across interdisciplinary scientific projects and maintain accurate documentation daily";
        for item in [&known, &missing] {
            store.upsert_job(item).unwrap();
            store
                .set_job_description(&item.id, Some(description))
                .unwrap();
            store.find_possible_duplicates(item).unwrap();
        }
        let matches = store.possible_duplicates().unwrap();
        assert_eq!(matches.len(), 1);
        assert!(matches[0].reason.contains("description overlap 100%"));
        assert_eq!(matches[0].status, "pending");
    }

    #[test]
    #[ignore = "bounded fictional workspace timing fixture"]
    fn measure_title_level_recheck_over_indexed_jobs() {
        let store = Store::in_memory().unwrap();
        store.conn.execute_batch("BEGIN").unwrap();
        for index in 0..10_000 {
            let id = format!("job-{index:05}");
            let title = if index % 1000 == 0 {
                "Engineer II"
            } else {
                "Software Engineer"
            };
            let employer = format!("company{index}");
            store.conn.execute("INSERT INTO jobs(id,title,employer,location,state) VALUES(?1,?2,?3,'','\"discovered\"')", params![id,title,employer]).unwrap();
            store
                .conn
                .execute("INSERT INTO job_employer_indexed(job_id) VALUES(?1)", [&id])
                .unwrap();
            store
                .conn
                .execute(
                    "INSERT INTO job_employer_tokens(job_id,token) VALUES(?1,?2)",
                    params![id, employer],
                )
                .unwrap();
        }
        store
            .conn
            .execute(
                "INSERT INTO meta(key,value) VALUES('duplicate_title_level_cursor','')",
                [],
            )
            .unwrap();
        store.conn.execute_batch("COMMIT").unwrap();
        let start = std::time::Instant::now();
        let mut max_step = std::time::Duration::ZERO;
        let mut steps = 0;
        let mut after = String::new();
        while store.duplicate_index_pending().unwrap() {
            let step = std::time::Instant::now();
            after = store
                .index_next_duplicate_job(&after)
                .unwrap()
                .unwrap_or_default();
            max_step = max_step.max(step.elapsed());
            steps += 1;
            assert!(steps <= 170);
        }
        println!("10,000 fictional indexed jobs, 10 affected titles: {steps} bounded steps, total {:?}, slowest {:?}; distinct employers, no descriptions or production delays", start.elapsed(), max_step);
    }

    #[test]
    #[ignore = "bounded in-memory duplicate-index timing fixture"]
    fn measure_duplicate_index_on_distinct_titles_and_long_descriptions() {
        let mut store = Store::in_memory().unwrap();
        let description = (0..2500)
            .map(|i| format!("skill{i:04} "))
            .collect::<String>();
        for index in 0..200 {
            let item = job(
                &format!("candidate-{index:03}"),
                "Software Engineer",
                "Example Labs",
                None,
            );
            store.upsert_job(&item).unwrap();
            store
                .set_job_description(&item.id, Some(&description))
                .unwrap();
            store.find_possible_duplicates(&item).unwrap();
        }
        let target = job("target", "Computational Scientist", "Example Labs", None);
        store.upsert_job(&target).unwrap();
        store
            .set_job_description(&target.id, Some(&description))
            .unwrap();
        let start = std::time::Instant::now();
        let matches = store.find_possible_duplicates(&target).unwrap();
        println!(
            "Fictional duplicate index: {} candidates, description bytes {}, elapsed {:?}",
            matches.len(),
            description.len(),
            start.elapsed()
        );
        assert_eq!(matches.len(), 200);
    }

    #[cfg(feature = "sqlcipher")]
    #[test]
    #[ignore = "bounded encrypted disk duplicate-index timing fixture"]
    fn measure_encrypted_duplicate_index_on_distinct_titles_and_long_descriptions() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .canonicalize()
            .unwrap();
        let path = root.join(format!(
            "waypoint-index-timing-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(!path.exists() && path.starts_with(&root));
        {
            let mut store = Store::open_encrypted(&path, &[17; 32]).unwrap();
            let description = (0..2500)
                .map(|i| format!("skill{i:04} "))
                .collect::<String>();
            store.conn.execute_batch("BEGIN").unwrap();
            for index in 0..200 {
                let item = job(
                    &format!("candidate-{index:03}"),
                    "Software Engineer",
                    "Example Labs",
                    None,
                );
                store.upsert_job(&item).unwrap();
                store
                    .set_job_description(&item.id, Some(&description))
                    .unwrap();
                store
                    .conn
                    .execute(
                        "INSERT INTO job_employer_tokens(job_id,token) VALUES(?1,'example')",
                        [item.id.as_str()],
                    )
                    .unwrap();
                store
                    .conn
                    .execute(
                        "INSERT INTO job_employer_indexed(job_id) VALUES(?1)",
                        [item.id.as_str()],
                    )
                    .unwrap();
                store.conn.execute("INSERT INTO job_duplicate_coverage(job_id,status,candidates) VALUES(?1,'complete',0)", [item.id.as_str()]).unwrap();
            }
            store.conn.execute_batch("COMMIT").unwrap();
            let target = job("target", "Computational Scientist", "Example Labs", None);
            store.upsert_job(&target).unwrap();
            store
                .set_job_description(&target.id, Some(&description))
                .unwrap();
            let start = std::time::Instant::now();
            assert_eq!(
                store.index_next_duplicate_job("").unwrap().as_deref(),
                Some("target")
            );
            let matches = store.possible_duplicates().unwrap();
            println!("Fictional SQLCipher disk duplicate index: {} candidates, description bytes {}, elapsed {:?}", matches.len(), description.len(), start.elapsed());
            assert_eq!(matches.len(), 200);
        }
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn strong_title_candidate_does_not_load_descriptions_but_different_titles_do() {
        let mut store = Store::in_memory().unwrap();
        let first = job("first", "Software Engineer", "Example Labs", None);
        let mut second = job("second", "Software Engineer", "Example Labs", None);
        store.upsert_job(&first).unwrap();
        store.find_possible_duplicates(&first).unwrap();
        store.upsert_job(&second).unwrap();
        store
            .set_job_description(&second.id, Some("temporary"))
            .unwrap();
        store
            .conn
            .execute(
                "UPDATE job_descriptions SET content=X'ff' WHERE job_id='second'",
                [],
            )
            .unwrap();
        let candidates = store.find_possible_duplicates(&second).unwrap();
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].reason.contains("description not compared"));
        second.title = "Different position".into();
        store.upsert_job(&second).unwrap();
        assert!(store.find_possible_duplicates(&second).is_err());
    }

    #[test]
    fn missing_employer_requisition_alone_is_not_sufficient() {
        let mut store = Store::in_memory().unwrap();
        let known = job(
            "known",
            "Research Scientist",
            "Example Labs",
            Some("REQ-481"),
        );
        let unrelated = job("other", "Restaurant Manager", "", Some("REQ-481"));
        let other_id = job("other-id", "Research Scientist", "", Some("REQ-999"));
        let no_id = job("no-id", "Research Scientist", "", None);
        for item in [&known, &unrelated, &other_id, &no_id] {
            store.upsert_job(item).unwrap();
            store.find_possible_duplicates(item).unwrap();
        }
        assert!(store.possible_duplicates().unwrap().is_empty());
    }

    #[test]
    fn employer_initials_find_expanded_names_without_merging() {
        assert_eq!(
            employer_acronym("International Business Machines Corporation").as_deref(),
            Some("ibm")
        );
        assert_eq!(employer_acronym("I.B.M. Ltd").as_deref(), Some("ibm"));
        for reverse in [false, true] {
            let mut store = Store::in_memory().unwrap();
            let a = job("a", "Senior Software Engineer", "IBM Ltd", None);
            let b = job(
                "b",
                "Senior Software Developer",
                "International Business Machines Corporation",
                None,
            );
            let order = if reverse { [&b, &a] } else { [&a, &b] };
            for item in order {
                store.upsert_job(item).unwrap();
                store.find_possible_duplicates(item).unwrap();
            }
            let matches = store.possible_duplicates().unwrap();
            assert_eq!(matches.len(), 1);
            assert_eq!(matches[0].status, "pending");
            assert!(matches[0]
                .reason
                .contains("initials can belong to different employers"));
            assert_eq!(store.all_jobs().unwrap().len(), 2);
            assert!(store.has_duplicate_hold("a").unwrap());
            assert!(store.has_duplicate_hold("b").unwrap());
            let other = job("other", "Restaurant Manager", "IBM", None);
            store.upsert_job(&other).unwrap();
            assert!(store.find_possible_duplicates(&other).unwrap().is_empty());
        }
    }

    #[test]
    fn idle_index_rolls_back_partial_pairs_after_a_write_failure() {
        let mut store = Store::in_memory().unwrap();
        let a = job("a", "Engineer", "Example Labs", None);
        let b = job("b", "Engineer", "Example Labs", None);
        let target = job("target", "Engineer", "Example Labs", None);
        for item in [&a, &b] {
            store.upsert_job(item).unwrap();
            store.find_possible_duplicates(item).unwrap();
        }
        store.upsert_job(&target).unwrap();
        store.conn.execute_batch("CREATE TRIGGER fixture_fail_second_pair BEFORE INSERT ON possible_duplicates WHEN NEW.job_a='b' AND NEW.job_b='target' BEGIN SELECT RAISE(ABORT, 'fixture failure'); END;").unwrap();
        assert!(store.index_next_duplicate_job("").is_err());
        assert!(store.duplicate_index_pending().unwrap());
        let count: u64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM possible_duplicates WHERE job_b='target'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        store
            .conn
            .execute_batch("DROP TRIGGER fixture_fail_second_pair")
            .unwrap();
        assert_eq!(
            store.index_next_duplicate_job("").unwrap().as_deref(),
            Some("target")
        );
        assert_eq!(
            store
                .possible_duplicates()
                .unwrap()
                .iter()
                .filter(|pair| pair.job_b == "target")
                .count(),
            2
        );
    }

    #[test]
    fn requisition_update_rolls_back_when_duplicate_indexing_fails() {
        let mut store = Store::in_memory().unwrap();
        let original = job("rollback", "Engineer", "Example", None);
        store.upsert_job(&original).unwrap();
        store.find_possible_duplicates(&original).unwrap();
        store.conn.execute_batch("CREATE TRIGGER fail_duplicate_refresh BEFORE INSERT ON job_employer_tokens BEGIN SELECT RAISE(ABORT, 'fixture failure'); END;").unwrap();
        let mut changed = original.clone();
        changed.requisition_id = Some("REQ-123".into());
        assert!(store.add_requisition_and_refresh(&changed).is_err());
        let stored: Option<String> = store
            .conn
            .query_row(
                "SELECT requisition_id FROM jobs WHERE id='rollback'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, None);
        assert_eq!(store.duplicate_coverage_counts().unwrap().1, 0);
    }

    #[test]
    fn manual_job_insert_rolls_back_when_duplicate_indexing_fails() {
        let mut store = Store::in_memory().unwrap();
        store.conn.execute_batch("CREATE TRIGGER fail_new_job_index BEFORE INSERT ON job_employer_tokens BEGIN SELECT RAISE(ABORT, 'fixture failure'); END;").unwrap();
        let candidate = job("new-job", "Engineer", "Example", Some("REQ-123"));
        assert!(store
            .insert_manual_job(&candidate, Some("Fictional description"))
            .is_err());
        let count: i64 = store
            .conn
            .query_row("SELECT count(*) FROM jobs WHERE id='new-job'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
        let descriptions: i64 = store
            .conn
            .query_row(
                "SELECT count(*) FROM job_descriptions WHERE job_id='new-job'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(descriptions, 0);
    }

    #[test]
    fn reports_comparison_caps_and_missing_employers_as_incomplete() {
        let mut store = Store::in_memory().unwrap();
        let target = job("target", "Engineer", "Example", None);
        store.upsert_job(&target).unwrap();
        for index in 0..200 {
            let candidate = job(
                &format!("candidate-{index:03}"),
                "Engineer",
                "Example",
                None,
            );
            store.upsert_job(&candidate).unwrap();
            store
                .conn
                .execute(
                    "INSERT INTO job_employer_tokens(job_id,token) VALUES(?1,'example')",
                    [candidate.id.as_str()],
                )
                .unwrap();
        }
        store.find_possible_duplicates(&target).unwrap();
        let missing = job("missing", "Role", "X", None);
        store.upsert_job(&missing).unwrap();
        store.find_possible_duplicates(&missing).unwrap();
        assert_eq!(store.duplicate_coverage_counts().unwrap(), (202, 200, 1, 1));
        let mut changed = target.clone();
        changed.title = "Changed role".into();
        store.upsert_job(&changed).unwrap();
        assert_eq!(store.duplicate_coverage_counts().unwrap(), (202, 201, 0, 1));
    }

    #[test]
    fn matching_requisition_is_considered_before_a_crowded_employer_cap() {
        let mut store = Store::in_memory().unwrap();
        for index in 0..205 {
            let candidate = job(&format!("a-{index:03}"), "Engineer", "Example", None);
            store.upsert_job(&candidate).unwrap();
            store
                .conn
                .execute(
                    "INSERT INTO job_employer_tokens(job_id,token) VALUES(?1,'example')",
                    [candidate.id.as_str()],
                )
                .unwrap();
        }
        let matching = job(
            "z-matching",
            "Different role wording",
            "Example",
            Some(" req-42 "),
        );
        store.upsert_job(&matching).unwrap();
        store
            .conn
            .execute(
                "INSERT INTO job_employer_tokens(job_id,token) VALUES(?1,'example')",
                [matching.id.as_str()],
            )
            .unwrap();
        let unrelated = job(
            "unrelated",
            "Research specialist",
            "Other Company",
            Some("REQ-42"),
        );
        store.upsert_job(&unrelated).unwrap();
        store
            .conn
            .execute(
                "INSERT INTO job_employer_tokens(job_id,token) VALUES(?1,'other')",
                [unrelated.id.as_str()],
            )
            .unwrap();
        let target = job("target", "Research specialist", "Example", Some("REQ-42"));
        store.upsert_job(&target).unwrap();
        let pairs = store.find_possible_duplicates(&target).unwrap();
        assert!(pairs.iter().any(|pair| pair.job_a == "target"
            && pair.job_b == "z-matching"
            && pair.status == "pending"));
        assert!(!pairs
            .iter()
            .any(|pair| pair.job_a == "unrelated" || pair.job_b == "unrelated"));
        assert!(store.has_duplicate_hold("target").unwrap());
        let coverage: String = store
            .conn
            .query_row(
                "SELECT status FROM job_duplicate_coverage WHERE job_id='target'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(coverage, "bounded");
    }

    #[test]
    fn reindexes_older_jobs_and_finds_reordered_unicode_employers_with_requisition() {
        let mut store = Store::in_memory().unwrap();
        let a = job(
            "a",
            "Research scientist",
            "Müller Global GmbH",
            Some("REQ-42"),
        );
        let b = job(
            "b",
            "Laboratory specialist",
            "Global Müller Ltd",
            Some("req-42"),
        );
        store.upsert_job(&a).unwrap();
        store.upsert_job(&b).unwrap();
        assert!(store.duplicate_index_pending().unwrap());
        assert_eq!(
            store.index_next_duplicate_job("").unwrap().as_deref(),
            Some("a")
        );
        assert_eq!(
            store.index_next_duplicate_job("a").unwrap().as_deref(),
            Some("b")
        );
        assert!(!store.duplicate_index_pending().unwrap());
        let pairs = store.possible_duplicates().unwrap();
        assert_eq!(pairs.len(), 1);
        assert!(pairs[0].reason.contains("Matching requisition"));
        assert!(store.has_duplicate_hold("a").unwrap());
    }

    #[test]
    fn equivalent_roman_and_numeric_levels_are_reviewed_without_merging() {
        assert_eq!(title_score("Nurse II", "Nurse 2"), 100);
        assert_eq!(title_score("Nurse III", "Nurse 3"), 100);
        assert!(title_score("Nurse II", "Nurse 3") < 65);
        assert!(title_words("C++ Developer").contains("c++"));
        let mut store = Store::in_memory().unwrap();
        let first = job("roman", "Nurse II", "Example Hospital", None);
        let second = job("digit", "Nurse 2", "Example Hospital", None);
        for item in [&first, &second] {
            store.upsert_job(item).unwrap();
            store.find_possible_duplicates(item).unwrap();
        }
        let pairs = store.possible_duplicates().unwrap();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].status, "pending");
        assert!(store.has_duplicate_hold("roman").unwrap());
        assert!(store.has_duplicate_hold("digit").unwrap());
    }

    #[test]
    fn gender_notation_does_not_hide_title_candidates_or_merge_jobs() {
        assert!(
            title_score(
                "Senior ML Software Engineer (m/f/d)",
                "ML Software Developer"
            ) >= 65
        );
        assert!(
            title_score(
                "Senior ML Software Engineer [w/m/x]",
                "ML Software Developer"
            ) >= 65
        );
        assert!(title_score("Senior ML Software Engineer m/f/d", "ML Software Developer") >= 65);
        assert!(title_words("C Developer (C++)").contains("c"));
        assert!(title_words("Developer (Remote)").contains("remote"));
        assert!(title_score("Senior ML Software Engineer (m/f/d)", "Marketing Director") < 65);
        let mut store = Store::in_memory().unwrap();
        let a = job(
            "a",
            "Senior ML Software Engineer (m/f/d)",
            "Example Research",
            None,
        );
        let b = job("b", "ML Software Developer", "Example Research", None);
        for item in [&a, &b] {
            store.upsert_job(item).unwrap();
            store.find_possible_duplicates(item).unwrap();
        }
        let pairs = store.possible_duplicates().unwrap();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].status, "pending");
        assert!(store.has_duplicate_hold("a").unwrap());
    }

    #[test]
    fn near_identical_descriptions_can_link_different_titles_without_requisition() {
        let mut store = Store::in_memory().unwrap();
        let a = job("a", "Computational biologist", "Example Institute", None);
        let b = job("b", "Genomics specialist", "Institute Example", None);
        let description = "Analyze genomic sequencing data using Python and statistical methods collaborate with research teams prepare reproducible results manage experiments communicate findings across interdisciplinary scientific projects and maintain accurate documentation daily";
        for item in [&a, &b] {
            store.upsert_job(item).unwrap();
            store
                .set_job_description(&item.id, Some(description))
                .unwrap();
            store.find_possible_duplicates(item).unwrap();
        }
        assert_eq!(store.possible_duplicates().unwrap().len(), 1);
    }

    #[test]
    fn matching_requisition_and_descriptions_flag_cross_employer_reposts() {
        let description = "Analyze genomic sequencing data using Python and statistical methods collaborate with research teams prepare reproducible results manage experiments communicate findings across interdisciplinary scientific projects and maintain accurate documentation daily";
        for reverse in [false, true] {
            let mut store = Store::in_memory().unwrap();
            let first = job(
                "first",
                "Computational biologist",
                "Alpha Institute",
                Some("REQ-481"),
            );
            let second = job(
                "second",
                "Genomics specialist",
                "Beta Staffing",
                Some("req-481"),
            );
            for item in if reverse {
                [&second, &first]
            } else {
                [&first, &second]
            } {
                store.upsert_job(item).unwrap();
                store
                    .set_job_description(&item.id, Some(description))
                    .unwrap();
                store.find_possible_duplicates(item).unwrap();
            }
            let pairs = store.possible_duplicates().unwrap();
            assert_eq!(pairs.len(), 1);
            assert_eq!(pairs[0].status, "pending");
            assert!(pairs[0].reason.contains("Employer names differ"));
            assert!(pairs[0].reason.contains("description overlap 100%"));
            assert_eq!(store.all_jobs().unwrap().len(), 2);
            assert!(store.decide_duplicate("first", "second", false).unwrap());
            store.find_possible_duplicates(&second).unwrap();
            assert!(store.possible_duplicates().unwrap().is_empty());
        }
    }

    #[test]
    fn cross_employer_requisition_and_title_without_matching_description_is_not_a_hold() {
        let mut store = Store::in_memory().unwrap();
        let first = job(
            "first",
            "Software engineer",
            "Alpha Systems",
            Some("REQ-481"),
        );
        let second = job(
            "second",
            "Software engineer",
            "Beta Staffing",
            Some("REQ-481"),
        );
        for item in [&first, &second] {
            store.upsert_job(item).unwrap();
            store.find_possible_duplicates(item).unwrap();
        }
        assert!(store.possible_duplicates().unwrap().is_empty());
        assert!(!store.has_duplicate_hold("first").unwrap());
    }

    #[test]
    fn crowded_requisition_keeps_capacity_for_same_employer_candidates() {
        let mut store = Store::in_memory().unwrap();
        for index in 0..210 {
            let candidate = job(
                &format!("other-{index:03}"),
                "Unrelated role",
                "Beta Staffing",
                Some("REQ-481"),
            );
            store.upsert_job(&candidate).unwrap();
        }
        let neighbor = job(
            "same-employer",
            "Software engineer",
            "Alpha Systems",
            Some("OTHER-REQ"),
        );
        store.upsert_job(&neighbor).unwrap();
        store.find_possible_duplicates(&neighbor).unwrap();
        let source = job(
            "source",
            "Software engineer",
            "Alpha Systems",
            Some("REQ-481"),
        );
        store.upsert_job(&source).unwrap();
        let pairs = store.find_possible_duplicates(&source).unwrap();
        assert!(pairs
            .iter()
            .any(|pair| pair.job_a == "same-employer" || pair.job_b == "same-employer"));
        assert!(store.has_duplicate_hold("source").unwrap());
        assert!(store.duplicate_coverage_counts().unwrap().2 > 0);
    }

    #[test]
    fn matching_requisition_with_related_employer_is_reviewed_not_merged() {
        let mut store = Store::in_memory().unwrap();
        let first = job(
            "first",
            "Research Scientist",
            "Example Research",
            Some("REQ-42"),
        );
        let second = job(
            "second",
            "Lab Specialist",
            "Example Holdings",
            Some("req-42"),
        );
        for item in [&first, &second] {
            store.upsert_job(item).unwrap();
            store.find_possible_duplicates(item).unwrap();
        }
        let pairs = store.possible_duplicates().unwrap();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].status, "pending");
        assert!(pairs[0].reason.contains("Employer token overlap 50%"));
        assert!(pairs[0].reason.contains("Matching requisition"));
        assert_eq!(store.all_jobs().unwrap().len(), 2);
        assert!(store.has_duplicate_hold("first").unwrap());
        assert!(store.has_duplicate_hold("second").unwrap());
        let third = job(
            "third",
            "Lab Specialist",
            "Example Ventures",
            Some("OTHER-9"),
        );
        store.upsert_job(&third).unwrap();
        assert!(store.find_possible_duplicates(&third).unwrap().is_empty());
    }
    #[test]
    fn dotted_employer_acronyms_match_without_merging_and_include_older_missing_index() {
        assert_eq!(
            employer_words("I.B.M. Corporation"),
            employer_words("IBM Ltd")
        );
        assert_eq!(
            employer_words("A.B. Research"),
            employer_words("AB Research")
        );
        assert_ne!(
            employer_words("A B Research"),
            employer_words("AB Research")
        );
        let mut store = Store::in_memory().unwrap();
        let older = job("old", "Senior Software Engineer", "I.B.M. Inc", None);
        store.upsert_job(&older).unwrap();
        // Simulate an older release which could not index single-letter tokens.
        store.conn.execute("INSERT INTO job_duplicate_coverage(job_id,status,candidates) VALUES(?1,'missing_employer',0)",[older.id.as_str()]).unwrap();
        store
            .conn
            .execute(
                "INSERT INTO job_employer_indexed(job_id) VALUES(?1)",
                [older.id.as_str()],
            )
            .unwrap();
        let newer = job("new", "Sr Software Developer", "IBM Ltd", None);
        store.upsert_job(&newer).unwrap();
        let matches = store.find_possible_duplicates(&newer).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].status, "pending");
        assert!(store.has_duplicate_hold("old").unwrap());
        assert!(store.has_duplicate_hold("new").unwrap());
        assert_eq!(store.all_jobs().unwrap().len(), 2);
    }
    #[test]
    fn international_legal_suffixes_do_not_hide_a_possible_duplicate() {
        let mut store = Store::in_memory().unwrap();
        let first = job("a", "Senior Software Engineer", "Example BV", None);
        let second = job("b", "Sr Software Developer", "Example PLC", None);
        for item in [&first, &second] {
            store.upsert_job(item).unwrap();
            store.find_possible_duplicates(item).unwrap();
        }
        assert_eq!(store.possible_duplicates().unwrap().len(), 1);
        assert!(store.has_duplicate_hold("a").unwrap());
        assert!(store.has_duplicate_hold("b").unwrap());
    }

    #[test]
    fn related_employer_neighbors_can_be_flagged_without_overriding_decisions() {
        let mut store = Store::in_memory().unwrap();
        let first = job("a", "Research scientist", "Acme Research", None);
        let second = job("b", "Laboratory lead", "Research Acme", None);
        for item in [&first, &second] {
            store.upsert_job(item).unwrap();
            store.find_possible_duplicates(item).unwrap();
        }
        assert!(store.possible_duplicates().unwrap().is_empty());
        let (neighbors, truncated) = store.duplicate_neighbors("a").unwrap();
        assert_eq!(neighbors.len(), 1);
        assert!(!truncated);
        assert_eq!(neighbors[0].id.as_str(), "b");
        assert_eq!(
            store
                .add_ai_duplicates("a", &[("b".into(), "AI suggestion".into())])
                .unwrap(),
            1
        );
        assert!(store.has_duplicate_hold("a").unwrap());
        assert!(store.decide_duplicate("a", "b", false).unwrap());
        assert!(store.duplicate_neighbors("a").unwrap().0.is_empty());
        assert_eq!(
            store
                .add_ai_duplicates("a", &[("b".into(), "Should not overwrite".into())])
                .unwrap(),
            0
        );
        assert!(!store.has_duplicate_hold("a").unwrap());
    }
    #[test]
    fn requisition_neighbors_include_other_employer_names_without_automatic_decisions() {
        let mut store = Store::in_memory().unwrap();
        let source = job(
            "source",
            "Software engineer",
            "Alpha Systems",
            Some("REQ-42"),
        );
        store.upsert_job(&source).unwrap();
        for index in 0..16 {
            let candidate = job(
                &format!("candidate-{index:02}"),
                "Infrastructure specialist",
                "Beta Staffing",
                Some(" req-42 "),
            );
            store.upsert_job(&candidate).unwrap();
        }
        let unrelated = job("other", "Different role", "Gamma", Some("REQ-99"));
        store.upsert_job(&unrelated).unwrap();
        let (first, truncated) = store.duplicate_neighbors("source").unwrap();
        assert_eq!(first.len(), 12);
        assert!(truncated);
        assert!(store.possible_duplicates().unwrap().is_empty());
        let (page, next) = store.duplicate_neighbors_page("source", "").unwrap();
        assert_eq!(page.len(), 12);
        let (next_page, end) = store
            .duplicate_neighbors_page("source", &next.unwrap())
            .unwrap();
        assert_eq!(next_page.len(), 4);
        assert!(end.is_none());
        assert!(page.iter().all(|a| next_page.iter().all(|b| a.id != b.id)));
        assert!(page
            .iter()
            .chain(&next_page)
            .all(|job| job.id.as_str() != "other"));
        store
            .add_ai_duplicates("source", &[("candidate-00".into(), "Review".into())])
            .unwrap();
        let (after_decision, _) = store.duplicate_neighbors("source").unwrap();
        assert!(after_decision
            .iter()
            .all(|job| job.id.as_str() != "candidate-00"));
        let short = job("short", "Engineer", "Delta", Some("42"));
        let short_other = job("short-other", "Engineer", "Omega", Some("42"));
        store.upsert_job(&short).unwrap();
        store.upsert_job(&short_other).unwrap();
        assert!(store.duplicate_neighbors("short").unwrap().0.is_empty());
        assert!(store
            .duplicate_neighbors_page("short", "")
            .unwrap()
            .0
            .is_empty());
    }
    #[test]
    fn bounded_neighbors_prioritize_similar_titles_and_keep_employer_only_slots() {
        let mut store = Store::in_memory().unwrap();
        let source = job("source", "Senior Platform Engineer", "Acme Research", None);
        store.upsert_job(&source).unwrap();
        store
            .conn
            .execute(
                "INSERT INTO job_employer_tokens(job_id,token) VALUES(?1,'acme')",
                [source.id.as_str()],
            )
            .unwrap();
        for index in 0..20 {
            let id = format!("candidate-{index:02}");
            let title = if index == 19 {
                "Sr Platform Developer"
            } else {
                "Unrelated vacancy"
            };
            let candidate = job(&id, title, "Acme Research", None);
            store.upsert_job(&candidate).unwrap();
            store
                .conn
                .execute(
                    "INSERT INTO job_employer_tokens(job_id,token) VALUES(?1,'acme')",
                    [candidate.id.as_str()],
                )
                .unwrap();
        }
        let (neighbors, truncated) = store.duplicate_neighbors("source").unwrap();
        assert!(truncated);
        assert_eq!(neighbors.len(), 12);
        assert!(neighbors
            .iter()
            .any(|job| job.id.as_str() == "candidate-19"));
        for index in 0..4 {
            assert!(neighbors
                .iter()
                .any(|job| job.id.as_str() == format!("candidate-{index:02}")));
        }
        assert!(store.possible_duplicates().unwrap().is_empty());
    }
    #[test]
    fn crowded_employer_keeps_matching_requisition_despite_changed_title() {
        let mut store = Store::in_memory().unwrap();
        let source = job(
            "source",
            "Senior Platform Engineer",
            "Acme Research",
            Some("REQ-42"),
        );
        store.upsert_job(&source).unwrap();
        store
            .conn
            .execute(
                "INSERT INTO job_employer_tokens(job_id,token) VALUES(?1,'acme')",
                [source.id.as_str()],
            )
            .unwrap();
        for index in 0..20 {
            let id = format!("candidate-{index:02}");
            let candidate = job(
                &id,
                if index == 19 {
                    "Infrastructure Specialist"
                } else {
                    "Senior Platform Engineer"
                },
                "Acme Research",
                if index == 19 {
                    Some(" req-42 ")
                } else {
                    Some("OTHER")
                },
            );
            store.upsert_job(&candidate).unwrap();
            store
                .conn
                .execute(
                    "INSERT INTO job_employer_tokens(job_id,token) VALUES(?1,'acme')",
                    [candidate.id.as_str()],
                )
                .unwrap();
        }
        let (neighbors, truncated) = store.duplicate_neighbors("source").unwrap();
        assert!(truncated);
        assert_eq!(neighbors.len(), 12);
        assert!(neighbors
            .iter()
            .any(|job| job.id.as_str() == "candidate-19"));
        assert!(store.possible_duplicates().unwrap().is_empty());
    }
    #[test]
    fn neighbor_pages_cover_bounded_candidates_without_repeating_ids() {
        let mut store = Store::in_memory().unwrap();
        let source = job("source", "Engineer", "Acme", None);
        store.upsert_job(&source).unwrap();
        store
            .conn
            .execute(
                "INSERT INTO job_employer_tokens(job_id,token) VALUES(?1,'acme')",
                [source.id.as_str()],
            )
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO job_employer_tokens(job_id,token) VALUES(?1,'research')",
                [source.id.as_str()],
            )
            .unwrap();
        for index in 0..29 {
            let candidate = job(&format!("candidate-{index:02}"), "Role", "Acme", None);
            store.upsert_job(&candidate).unwrap();
            store
                .conn
                .execute(
                    "INSERT INTO job_employer_tokens(job_id,token) VALUES(?1,'acme')",
                    [candidate.id.as_str()],
                )
                .unwrap();
            if index % 2 == 0 {
                store
                    .conn
                    .execute(
                        "INSERT INTO job_employer_tokens(job_id,token) VALUES(?1,'research')",
                        [candidate.id.as_str()],
                    )
                    .unwrap();
            }
        }
        let mut after = String::new();
        let mut seen = BTreeSet::new();
        loop {
            let (page, next) = store.duplicate_neighbors_page("source", &after).unwrap();
            assert!(page.len() <= 12);
            for job in &page {
                assert!(seen.insert(job.id.as_str().to_owned()));
            }
            match next {
                Some(cursor) => {
                    assert!(cursor > after);
                    after = cursor;
                }
                None => break,
            }
        }
        assert_eq!(seen.len(), 29);
        let decided: Vec<_> = (0..12)
            .map(|index| (format!("candidate-{index:02}"), "Reviewed".into()))
            .collect();
        assert_eq!(store.add_ai_duplicates("source", &decided).unwrap(), 12);
        let (empty, cursor) = store.duplicate_neighbors_page("source", "").unwrap();
        assert!(empty.is_empty());
        let cursor = cursor.expect("a page of already reviewed jobs must still advance");
        assert_eq!(cursor, "candidate-11");
        let (next, _) = store.duplicate_neighbors_page("source", &cursor).unwrap();
        assert_eq!(next.len(), 12);
    }
    #[test]
    fn fuzzy_title_overlap_is_bounded() {
        assert_eq!(
            title_score("Senior Rust Engineer", "Rust Engineer Senior"),
            100
        );
        assert!(title_score("Senior Rust Engineer", "Rust Engineer") >= 70);
        assert!(title_score("Senior Rust Engineer", "Marketing Director") < 70);
    }

    #[test]
    fn technology_punctuation_distinguishes_titles_without_overriding_requisition() {
        assert_eq!(title_score("C++ Developer", "C# Engineer"), 50);
        assert_eq!(title_score("C++ Developer", "C++ Engineer"), 100);
        assert_eq!(title_score("Engineer #42", "Developer 42"), 100);
        let mut store = Store::in_memory().unwrap();
        let cpp = job("cpp", "C++ Developer", "Acme", None);
        let csharp = job("csharp", "C# Developer", "Acme", None);
        for item in [&cpp, &csharp] {
            store.upsert_job(item).unwrap();
            store.find_possible_duplicates(item).unwrap();
        }
        assert!(store.possible_duplicates().unwrap().is_empty());
        let same_requisition = job("same", "C# Developer", "Acme", Some("REQ-42"));
        let mut identified_cpp = cpp.clone();
        identified_cpp.requisition_id = Some("REQ-42".into());
        store.upsert_job(&identified_cpp).unwrap();
        store.upsert_job(&same_requisition).unwrap();
        store.find_possible_duplicates(&same_requisition).unwrap();
        assert!(store
            .possible_duplicates()
            .unwrap()
            .iter()
            .any(|pair| pair.job_a == "cpp" && pair.job_b == "same"));
    }
}
