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
    pub reason: String,
    pub status: String,
    pub preferred_job: Option<String>,
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
    overlap(&words(a), &words(b))
}

fn employer_words(s: &str) -> BTreeSet<String> {
    words(s)
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
            ]
            .contains(&w.as_str())
        })
        .collect()
}

impl Store {
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
        let mut bounded = tokens.len() > 8;
        tokens.truncate(8);
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
        if tokens.is_empty() {
            self.conn.execute("INSERT INTO job_duplicate_coverage(job_id,status,candidates) VALUES(?1,'missing_employer',0)", [job.id.as_str()])?;
            self.conn.execute(
                "INSERT INTO job_employer_indexed(job_id) VALUES(?1)",
                [job.id.as_str()],
            )?;
            return Ok(Vec::new());
        }
        let description = self.job_description(&job.id)?.unwrap_or_default();
        let description_words = words(&description.chars().take(30000).collect::<String>());
        let mut candidates = Vec::new();
        let mut seen = BTreeSet::new();
        let mut stmt = self.conn.prepare("SELECT j.id,j.title,j.employer,j.location,j.state,j.canonical_url,j.content_hash,j.first_seen,j.last_seen,j.remote,j.source,j.requisition_id,j.posted_at FROM job_employer_tokens t JOIN jobs j ON j.id=t.job_id WHERE t.token=?1 AND j.id<>?2 ORDER BY j.id LIMIT 200")?;
        for token in &tokens {
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
        let candidate_count = candidates.len();
        let mut result = Vec::new();
        for other in candidates {
            if overlap(&employer_tokens, &employer_words(&other.employer)) < 65 {
                continue;
            }
            let score = title_score(&job.title, &other.title);
            let other_description = self.job_description(&other.id)?.unwrap_or_default();
            let other_words = words(&other_description.chars().take(30000).collect::<String>());
            let description_score = if description_words.len() >= 20 && other_words.len() >= 20 {
                overlap(&description_words, &other_words)
            } else {
                0
            };
            let same_requisition = job
                .requisition_id
                .as_deref()
                .zip(other.requisition_id.as_deref())
                .is_some_and(|(a, b)| {
                    a.trim().len() >= 3 && a.trim().eq_ignore_ascii_case(b.trim())
                });
            if score < 65
                && !(score >= 30 && description_score >= 75)
                && description_score < 85
                && !same_requisition
            {
                continue;
            }
            let (job_a, job_b) = if job.id.as_str() < other.id.as_str() {
                (job.id.as_str(), other.id.as_str())
            } else {
                (other.id.as_str(), job.id.as_str())
            };
            let reason = format!("Similar employer; title overlap {score}%, description overlap {description_score}%. {} Compare location, duties and requisition numbers.", if same_requisition { "Matching requisition identifiers." } else { "" });
            self.conn.execute("INSERT INTO possible_duplicates(job_a,job_b,score,reason,status) VALUES(?1,?2,?3,?4,'pending') ON CONFLICT(job_a,job_b) DO UPDATE SET score=excluded.score,reason=excluded.reason", params![job_a,job_b,score,reason])?;
            let (status, preferred_job) = self.conn.query_row(
                "SELECT status,preferred_job FROM possible_duplicates WHERE job_a=?1 AND job_b=?2",
                params![job_a, job_b],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            result.push(PossibleDuplicate {
                job_a: job_a.into(),
                job_b: job_b.into(),
                score,
                reason,
                status,
                preferred_job,
            });
        }
        self.conn.execute(
            "INSERT INTO job_duplicate_coverage(job_id,status,candidates) VALUES(?1,?2,?3)",
            params![
                job.id.as_str(),
                if bounded { "bounded" } else { "complete" },
                candidate_count as i64
            ],
        )?;
        self.conn.execute(
            "INSERT INTO job_employer_indexed(job_id) VALUES(?1)",
            [job.id.as_str()],
        )?;
        Ok(result)
    }

    /// Index and compare one previously unindexed job; caller schedules bounded work.
    pub fn index_next_duplicate_job(&self, after: &str) -> rusqlite::Result<Option<String>> {
        let next = self.conn.query_row("SELECT j.id,j.title,j.employer,j.location,j.state,j.canonical_url,j.content_hash,j.first_seen,j.last_seen,j.remote,j.source,j.requisition_id,j.posted_at FROM jobs j WHERE j.id>?1 AND NOT EXISTS (SELECT 1 FROM job_employer_indexed i WHERE i.job_id=j.id) ORDER BY j.id LIMIT 1", [after], Self::row_to_job).optional()?;
        if let Some(job) = next {
            let id = job.id.as_str().to_owned();
            self.find_possible_duplicates(&job)?;
            Ok(Some(id))
        } else {
            Ok(None)
        }
    }

    pub fn duplicate_index_pending(&self) -> rusqlite::Result<bool> {
        self.conn.query_row("SELECT 1 FROM jobs j WHERE NOT EXISTS (SELECT 1 FROM job_employer_indexed i WHERE i.job_id=j.id) LIMIT 1", [], |_| Ok(())).optional().map(|v| v.is_some())
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

    /// Gather bounded, undecided neighbors sharing employer tokens; no title threshold.
    pub fn duplicate_neighbors(&self, id: &str) -> rusqlite::Result<(Vec<StoredJob>, bool)> {
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
        let mut ranked: Vec<_> = counts.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        truncated |= ranked.len() > 12;
        let mut result = Vec::new();
        for (candidate, _) in ranked.into_iter().take(12) {
            if let Ok(id) = waypoint_domain::JobIdentityId::new(&candidate) {
                if let Some(job) = self.get_job(&id)? {
                    result.push(job);
                }
            }
        }
        Ok((result, truncated))
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

    pub fn possible_duplicates(&self) -> rusqlite::Result<Vec<PossibleDuplicate>> {
        let mut stmt=self.conn.prepare("SELECT job_a,job_b,score,reason,status,preferred_job FROM possible_duplicates WHERE status <> 'different' ORDER BY job_a,job_b")?;
        let rows = stmt.query_map([], |r| {
            Ok(PossibleDuplicate {
                job_a: r.get(0)?,
                job_b: r.get(1)?,
                score: r.get(2)?,
                reason: r.get(3)?,
                status: r.get(4)?,
                preferred_job: r.get(5)?,
            })
        })?;
        rows.collect()
    }

    pub fn has_duplicate_hold(&self, id: &str) -> rusqlite::Result<bool> {
        self.conn.query_row("SELECT 1 FROM possible_duplicates WHERE (job_a=?1 OR job_b=?1) AND (status='pending' OR (status='same' AND preferred_job<>?1)) LIMIT 1", [id], |_| Ok(())).optional().map(|v|v.is_some())
    }

    pub fn pending_duplicate_pair(&self, first: &str, second: &str) -> rusqlite::Result<bool> {
        let (a, b) = if first < second {
            (first, second)
        } else {
            (second, first)
        };
        self.conn.query_row("SELECT 1 FROM possible_duplicates WHERE job_a=?1 AND job_b=?2 AND status='pending'", params![a,b], |_| Ok(())).optional().map(|v|v.is_some())
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
    fn fuzzy_title_overlap_is_bounded() {
        assert_eq!(
            title_score("Senior Rust Engineer", "Rust Engineer Senior"),
            100
        );
        assert!(title_score("Senior Rust Engineer", "Rust Engineer") >= 70);
        assert!(title_score("Senior Rust Engineer", "Marketing Director") < 70);
    }
}
