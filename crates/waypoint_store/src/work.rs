//! Persistence for the "act on a job" layer: candidate facts (claims),
//! prepared packets and one-use approval grants.
//!
//! Invariants this file exists to protect:
//! - A grant is persisted *with* its consumed flag, so a restart can never
//!   resurrect an approval that was already spent.
//! - Correcting a fact supersedes the old claim rather than editing it, so the
//!   history of what was asserted, and when, survives.
//! - A packet records the claims it cites, which is what makes correction
//!   propagation possible without re-parsing rendered text.

use rusqlite::params;
use waypoint_seal::SealError;

use waypoint_domain::{ClaimStatus, GrantId, JobIdentityId, SourceKind};

use crate::{Store, StoreError};

fn seal_err(e: SealError) -> StoreError {
    StoreError::ToSqlConversionFailure(Box::new(e))
}

/// A candidate fact, as persisted. Mirrors `waypoint_domain::Claim` plus the
/// supersession link used by correction propagation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredClaim {
    pub id: String,
    pub profile_revision: u64,
    pub text: String,
    pub status: ClaimStatus,
    pub source_kind: SourceKind,
    pub requires_review: bool,
    pub supersedes: Option<String>,
}

/// A prepared application packet for one job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPacket {
    pub job_id: String,
    pub packet_sha256: String,
    /// The full semantic document, so exports and revisions never depend on
    /// re-rendering the body text.
    pub doc_json: String,
    pub body: String,
    pub cited_claim_ids: Vec<String>,
    pub profile_revision: u64,
    pub created_at: String,
    /// Set when a cited fact was corrected: the packet must be re-prepared.
    pub invalidated_at: Option<String>,
}

/// A persisted approval grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredGrant {
    pub id: String,
    pub job_id: String,
    pub intent_id: String,
    pub bound_payload_sha256: String,
    pub packet_sha256: String,
    pub form_schema_sha256: String,
    pub destination_origin: String,
    pub profile_revision: u64,
    pub nonce: String,
    pub explicit_user_gesture: bool,
    pub consumed: bool,
    pub expired: bool,
}

fn encode<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".into())
}

fn decode_status(s: &str) -> ClaimStatus {
    serde_json::from_str(s).unwrap_or(ClaimStatus::Unknown)
}

fn decode_source(s: &str) -> SourceKind {
    serde_json::from_str(s).unwrap_or(SourceKind::Inference)
}

impl Store {
    /* ------------------------------- claims -------------------------------- */

    pub fn insert_claim(&mut self, claim: &StoredClaim) -> Result<(), StoreError> {
        // The candidate's own words are sealed when a backend is attached; the
        // plaintext column is written empty so there is exactly one source of
        // truth and nothing sensitive is duplicated.
        let sealed = self.seal(&claim.text).map_err(seal_err)?;
        let plaintext = if sealed.is_some() {
            ""
        } else {
            claim.text.as_str()
        };
        self.conn.execute(
            "INSERT INTO claims(id,profile_revision,text,text_sealed,status,source_kind,evidence_ids,requires_review,supersedes)
             VALUES(?1,?2,?3,?4,?5,?6,'[]',?7,?8)",
            params![
                claim.id,
                claim.profile_revision as i64,
                plaintext,
                sealed,
                encode(&claim.status),
                encode(&claim.source_kind),
                claim.requires_review as i64,
                claim.supersedes
            ],
        )?;
        Ok(())
    }

    /// All claims, newest revision first, superseded ones included (history is
    /// what lets a correction explain itself).
    pub fn all_claims(&self) -> Result<Vec<StoredClaim>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id,profile_revision,text,text_sealed,status,source_kind,requires_review,supersedes
             FROM claims ORDER BY profile_revision DESC, id ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            let plain: String = r.get(2)?;
            let sealed: Option<Vec<u8>> = r.get(3)?;
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)? as u64,
                plain,
                sealed,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, i64>(6)? != 0,
                r.get::<_, Option<String>>(7)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, revision, plain, sealed, status, source, review, supersedes) = row?;
            let text = match sealed {
                Some(bytes) => self.unseal_str(&bytes).map_err(seal_err)?,
                None => plain,
            };
            out.push(StoredClaim {
                id,
                profile_revision: revision,
                text,
                status: decode_status(&status),
                source_kind: decode_source(&source),
                requires_review: review,
                supersedes,
            });
        }
        Ok(out)
    }

    /// Claims that are still in force (not superseded) at the current revision.
    pub fn active_claims(&self) -> Result<Vec<StoredClaim>, StoreError> {
        let all = self.all_claims()?;
        // Owned copies: the supersession set must outlive the borrow of `all`.
        let superseded: Vec<String> = all.iter().filter_map(|c| c.supersedes.clone()).collect();
        Ok(all
            .into_iter()
            .filter(|c| !superseded.contains(&c.id))
            .collect())
    }

    pub fn claim(&self, id: &str) -> Result<Option<StoredClaim>, StoreError> {
        Ok(self.all_claims()?.into_iter().find(|c| c.id == id))
    }

    /* ---------------------------- profile revision -------------------------- */

    pub fn profile_revision(&self) -> Result<u64, StoreError> {
        let raw: String = self.conn.query_row(
            "SELECT value FROM meta WHERE key='profile_revision'",
            [],
            |r| r.get(0),
        )?;
        Ok(raw.parse().unwrap_or(1))
    }

    /// Monotonic: every correction advances the revision and never rewinds it.
    pub fn bump_profile_revision(&mut self) -> Result<u64, StoreError> {
        let next = self.profile_revision()?.saturating_add(1);
        self.conn.execute(
            "UPDATE meta SET value=?1 WHERE key='profile_revision'",
            params![next.to_string()],
        )?;
        Ok(next)
    }

    /* ------------------------------- packets ------------------------------- */

    pub fn upsert_packet(&mut self, packet: &StoredPacket) -> Result<(), StoreError> {
        // Prepared documents are the candidate's material: sealed when a
        // backend is attached, with the plaintext columns left empty.
        let sealed_body = self.seal(&packet.body).map_err(seal_err)?;
        let sealed_doc = self.seal(&packet.doc_json).map_err(seal_err)?;
        let body = if sealed_body.is_some() {
            ""
        } else {
            packet.body.as_str()
        };
        let doc_json = if sealed_doc.is_some() {
            ""
        } else {
            packet.doc_json.as_str()
        };
        self.conn.execute(
            "INSERT INTO packets(job_id,packet_sha256,doc_json,doc_json_sealed,body,body_sealed,cited_claim_ids,profile_revision,created_at,invalidated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(job_id) DO UPDATE SET
               packet_sha256=excluded.packet_sha256,
               doc_json=excluded.doc_json,
               doc_json_sealed=excluded.doc_json_sealed,
               body=excluded.body,
               body_sealed=excluded.body_sealed,
               cited_claim_ids=excluded.cited_claim_ids,
               profile_revision=excluded.profile_revision,
               created_at=excluded.created_at,
               invalidated_at=excluded.invalidated_at",
            params![
                packet.job_id,
                packet.packet_sha256,
                doc_json,
                sealed_doc,
                body,
                sealed_body,
                encode(&packet.cited_claim_ids),
                packet.profile_revision as i64,
                packet.created_at,
                packet.invalidated_at
            ],
        )?;
        Ok(())
    }

    pub fn packet_for_job(
        &self,
        job_id: &JobIdentityId,
    ) -> Result<Option<StoredPacket>, StoreError> {
        self.conn
            .query_row(
                "SELECT job_id,packet_sha256,doc_json,doc_json_sealed,body,body_sealed,cited_claim_ids,profile_revision,created_at,invalidated_at
                 FROM packets WHERE job_id=?1",
                params![job_id.as_str()],
                |r| self.row_to_packet_full(r),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })
    }

    /// Packets that cite a claim — the lookup that makes correction
    /// propagation exact instead of text-matching.
    pub fn packets_citing_claim(&self, claim_id: &str) -> Result<Vec<StoredPacket>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT job_id,packet_sha256,doc_json,doc_json_sealed,body,body_sealed,cited_claim_ids,profile_revision,created_at,invalidated_at
             FROM packets",
        )?;
        let rows = stmt.query_map([], |r| self.row_to_packet_full(r))?;
        let all: Vec<StoredPacket> = rows.collect::<Result<_, _>>()?;
        Ok(all
            .into_iter()
            .filter(|p| p.cited_claim_ids.iter().any(|c| c == claim_id))
            .collect())
    }

    pub fn all_packets(&self) -> Result<Vec<StoredPacket>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT job_id,packet_sha256,doc_json,doc_json_sealed,body,body_sealed,cited_claim_ids,profile_revision,created_at,invalidated_at
             FROM packets ORDER BY created_at DESC, job_id ASC",
        )?;
        let rows = stmt.query_map([], |r| self.row_to_packet_full(r))?;
        rows.collect()
    }

    pub fn invalidate_packet(&mut self, job_id: &str, at: &str) -> Result<bool, StoreError> {
        Ok(self.conn.execute(
            "UPDATE packets SET invalidated_at=?1 WHERE job_id=?2 AND invalidated_at IS NULL",
            params![at, job_id],
        )? > 0)
    }

    /* -------------------------------- grants -------------------------------- */

    pub fn insert_grant(&mut self, grant: &StoredGrant) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO grants(id,job_id,intent_id,bound_payload_sha256,packet_sha256,form_schema_sha256,
                                destination_origin,profile_revision,nonce,explicit_user_gesture,consumed,expired)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                grant.id,
                grant.job_id,
                grant.intent_id,
                grant.bound_payload_sha256,
                grant.packet_sha256,
                grant.form_schema_sha256,
                grant.destination_origin,
                grant.profile_revision as i64,
                grant.nonce,
                grant.explicit_user_gesture as i64,
                grant.consumed as i64,
                grant.expired as i64
            ],
        )?;
        Ok(())
    }

    pub fn grant_for_job(&self, job_id: &JobIdentityId) -> Result<Option<StoredGrant>, StoreError> {
        self.conn
            .query_row(
                "SELECT id,job_id,intent_id,bound_payload_sha256,packet_sha256,form_schema_sha256,
                        destination_origin,profile_revision,nonce,explicit_user_gesture,consumed,expired
                 FROM grants WHERE job_id=?1 ORDER BY rowid DESC LIMIT 1",
                params![job_id.as_str()],
                row_to_grant,
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })
    }

    /// Expire every live grant bound to one of these packet hashes.
    /// Returns the grant ids that actually changed state.
    pub fn expire_grants_for_packets(
        &mut self,
        packet_hashes: &[String],
    ) -> Result<Vec<String>, StoreError> {
        let mut changed = vec![];
        for hash in packet_hashes {
            let mut stmt = self
                .conn
                .prepare("SELECT id FROM grants WHERE packet_sha256=?1 AND expired=0")?;
            let ids: Vec<String> = stmt
                .query_map(params![hash], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            drop(stmt);
            for id in ids {
                self.conn
                    .execute("UPDATE grants SET expired=1 WHERE id=?1", params![id])?;
                changed.push(id);
            }
        }
        Ok(changed)
    }

    pub fn grant_count_for_job(&self, job_id: &JobIdentityId) -> Result<u64, StoreError> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM grants WHERE job_id=?1",
            params![job_id.as_str()],
            |r| r.get(0),
        )?;
        Ok(n as u64)
    }

    /// Mark a grant spent. This is the only way `consumed` becomes true, and it
    /// is what makes the approval single-use across restarts.
    pub fn mark_grant_consumed(&mut self, grant_id: &GrantId) -> Result<bool, StoreError> {
        Ok(self.conn.execute(
            "UPDATE grants SET consumed=1 WHERE id=?1 AND consumed=0",
            params![grant_id.as_str()],
        )? > 0)
    }
}

impl Store {
    /// Full layout, including the sealed columns.
    fn row_to_packet_full(&self, r: &rusqlite::Row<'_>) -> Result<StoredPacket, rusqlite::Error> {
        let plain_doc: String = r.get(2)?;
        let sealed_doc: Option<Vec<u8>> = r.get(3)?;
        let plain_body: String = r.get(4)?;
        let sealed_body: Option<Vec<u8>> = r.get(5)?;
        let cited: String = r.get(6)?;
        let doc_json = match sealed_doc {
            Some(bytes) => self.unseal_str(&bytes).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Blob,
                    Box::new(e),
                )
            })?,
            None => plain_doc,
        };
        let body = match sealed_body {
            Some(bytes) => self.unseal_str(&bytes).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Blob,
                    Box::new(e),
                )
            })?,
            None => plain_body,
        };
        Ok(StoredPacket {
            job_id: r.get(0)?,
            packet_sha256: r.get(1)?,
            doc_json,
            body,
            cited_claim_ids: serde_json::from_str(&cited).unwrap_or_default(),
            profile_revision: r.get::<_, i64>(7)? as u64,
            created_at: r.get(8)?,
            invalidated_at: r.get(9)?,
        })
    }
}

fn row_to_grant(r: &rusqlite::Row<'_>) -> Result<StoredGrant, rusqlite::Error> {
    Ok(StoredGrant {
        id: r.get(0)?,
        job_id: r.get(1)?,
        intent_id: r.get(2)?,
        bound_payload_sha256: r.get(3)?,
        packet_sha256: r.get(4)?,
        form_schema_sha256: r.get(5)?,
        destination_origin: r.get(6)?,
        profile_revision: r.get::<_, i64>(7)? as u64,
        nonce: r.get(8)?,
        explicit_user_gesture: r.get::<_, i64>(9)? != 0,
        consumed: r.get::<_, i64>(10)? != 0,
        expired: r.get::<_, i64>(11)? != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(id: &str, rev: u64, text: &str) -> StoredClaim {
        StoredClaim {
            id: id.into(),
            profile_revision: rev,
            text: text.into(),
            status: ClaimStatus::UserAttested,
            source_kind: SourceKind::CandidateAttestation,
            requires_review: false,
            supersedes: None,
        }
    }

    #[test]
    fn claims_roundtrip_and_supersession_hides_the_old_fact() {
        let mut s = Store::in_memory().unwrap();
        s.insert_claim(&claim("c1", 1, "Eight years of Rust"))
            .unwrap();
        let old = s.claim("c1").unwrap().unwrap();
        assert_eq!(old.text, "Eight years of Rust");
        assert_eq!(s.active_claims().unwrap().len(), 1);

        let mut replacement = claim("c2", 2, "Nine years of Rust");
        replacement.supersedes = Some("c1".into());
        s.insert_claim(&replacement).unwrap();

        assert_eq!(s.active_claims().unwrap().len(), 1);
        assert_eq!(s.active_claims().unwrap()[0].text, "Nine years of Rust");
        assert_eq!(s.all_claims().unwrap().len(), 2, "history is retained");
    }

    #[test]
    fn profile_revision_only_moves_forward() {
        let mut s = Store::in_memory().unwrap();
        let first = s.profile_revision().unwrap();
        assert_eq!(s.bump_profile_revision().unwrap(), first + 1);
        assert_eq!(s.bump_profile_revision().unwrap(), first + 2);
        assert_eq!(s.profile_revision().unwrap(), first + 2);
    }

    #[test]
    fn packets_are_found_by_the_claims_they_cite() {
        let mut s = Store::in_memory().unwrap();
        s.upsert_job(&crate::tests_support::job("j1")).unwrap();
        s.upsert_job(&crate::tests_support::job("j2")).unwrap();
        let packet = |job: &str, sha: &str, cited: Vec<String>| StoredPacket {
            job_id: job.into(),
            packet_sha256: sha.into(),
            doc_json: "{}".into(),
            body: "body".into(),
            cited_claim_ids: cited,
            profile_revision: 1,
            created_at: "100".into(),
            invalidated_at: None,
        };
        s.upsert_packet(&packet("j1", "aa", vec!["c1".into()]))
            .unwrap();
        s.upsert_packet(&packet("j2", "bb", vec!["c9".into()]))
            .unwrap();

        let hit = s.packets_citing_claim("c1").unwrap();
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].job_id, "j1");
        assert!(s.packets_citing_claim("nope").unwrap().is_empty());

        assert!(s.invalidate_packet("j1", "999").unwrap());
        assert!(
            !s.invalidate_packet("j1", "1000").unwrap(),
            "already invalidated"
        );
        assert_eq!(
            s.packet_for_job(&JobIdentityId::new("j1").unwrap())
                .unwrap()
                .unwrap()
                .invalidated_at
                .as_deref(),
            Some("999")
        );
    }

    #[test]
    fn grant_consumption_is_persisted_and_one_way() {
        let mut s = Store::in_memory().unwrap();
        let grant = StoredGrant {
            id: "g1".into(),
            job_id: "j1".into(),
            intent_id: "i1".into(),
            bound_payload_sha256: "a".repeat(64),
            packet_sha256: "b".repeat(64),
            form_schema_sha256: "c".repeat(64),
            destination_origin: "https://boards.greenhouse.io".into(),
            profile_revision: 1,
            nonce: "n1".into(),
            explicit_user_gesture: true,
            consumed: false,
            expired: false,
        };
        s.insert_grant(&grant).unwrap();
        let gid = GrantId::new("g1").unwrap();

        assert!(s.mark_grant_consumed(&gid).unwrap());
        assert!(!s.mark_grant_consumed(&gid).unwrap(), "cannot spend twice");
        let read = s
            .grant_for_job(&JobIdentityId::new("j1").unwrap())
            .unwrap()
            .unwrap();
        assert!(read.consumed);
        assert!(read.explicit_user_gesture);
    }

    #[test]
    fn correcting_a_fact_expires_only_the_grants_bound_to_affected_packets() {
        let mut s = Store::in_memory().unwrap();
        for (id, job, packet) in [("g1", "j1", "aa"), ("g2", "j2", "bb")] {
            s.insert_grant(&StoredGrant {
                id: id.into(),
                job_id: job.into(),
                intent_id: "i".into(),
                bound_payload_sha256: "a".repeat(64),
                packet_sha256: packet.repeat(32),
                form_schema_sha256: "c".repeat(64),
                destination_origin: "https://x.example".into(),
                profile_revision: 1,
                nonce: "n".into(),
                explicit_user_gesture: true,
                consumed: false,
                expired: false,
            })
            .unwrap();
        }
        let changed = s.expire_grants_for_packets(&["aa".repeat(32)]).unwrap();
        assert_eq!(changed, vec!["g1".to_string()], "only the affected grant");

        let untouched = s
            .grant_for_job(&JobIdentityId::new("j2").unwrap())
            .unwrap()
            .unwrap();
        assert!(!untouched.expired, "unrelated approval must survive");
    }
}
