//! Bounded, user-sealed web-search receipts and candidate selection.
use crate::{Store, StoreError};
use rusqlite::{params, OptionalExtension};
use waypoint_seal::{Sealer, SealerKind};

const MAX_BYTES: usize = 12 * 1024 * 1024;

pub struct SavedWebSearchRun {
    pub task_id: String,
    pub planned: usize,
    pub plan: Vec<u8>,
    pub prior_pages: Vec<Vec<u8>>,
    pub pages: Vec<Vec<u8>>,
    pub terminal: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProviderPause {
    pub http_status: u16,
    /// UTC Unix seconds. The next request can start at or after this time.
    pub retry_at: i64,
    pub from_retry_after: bool,
}

fn failure(error: impl std::error::Error + Send + Sync + 'static) -> StoreError {
    StoreError::ToSqlConversionFailure(Box::new(error))
}

fn invalid(message: &'static str) -> StoreError {
    failure(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message,
    ))
}

fn valid_task_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn validate_provider_key(key: &str) -> Result<(), StoreError> {
    if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("Invalid search provider identity"));
    }
    Ok(())
}

impl Store {
    pub fn web_provider_pause(
        &self,
        key: &str,
        now: i64,
    ) -> Result<Option<ProviderPause>, StoreError> {
        validate_provider_key(key)?;
        self.conn
            .query_row(
                "SELECT http_status,retry_at,from_retry_after
             FROM web_provider_pauses WHERE origin_key=?1 AND retry_at>?2",
                params![key, now],
                |row| {
                    Ok(ProviderPause {
                        http_status: row.get(0)?,
                        retry_at: row.get(1)?,
                        from_retry_after: row.get(2)?,
                    })
                },
            )
            .optional()
    }

    pub fn save_web_provider_pause(
        &self,
        key: &str,
        pause: ProviderPause,
        now: i64,
    ) -> Result<(), StoreError> {
        validate_provider_key(key)?;
        if !matches!(pause.http_status, 429 | 503) || now < 0 || pause.retry_at < 0 {
            return Err(invalid("Invalid search provider pause"));
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM web_provider_pauses WHERE retry_at<=?1", [now])?;
        if pause.retry_at > now {
            tx.execute(
                "INSERT INTO web_provider_pauses(origin_key,retry_at,http_status,from_retry_after)
                 VALUES(?1,?2,?3,?4)
                 ON CONFLICT(origin_key) DO UPDATE SET retry_at=excluded.retry_at,
                    http_status=excluded.http_status,from_retry_after=excluded.from_retry_after
                 WHERE excluded.retry_at>web_provider_pauses.retry_at",
                params![
                    key,
                    pause.retry_at,
                    pause.http_status,
                    pause.from_retry_after
                ],
            )?;
        }
        tx.commit()
    }

    fn web_search_sealer(&self) -> Result<&dyn Sealer, StoreError> {
        self.sealer
            .as_deref()
            .filter(|s| s.kind() == SealerKind::Dpapi)
            .ok_or_else(|| failure(waypoint_seal::SealError::Unavailable))
    }

    pub fn save_selected_web_candidates(
        &self,
        expected: u64,
        content: &[u8],
    ) -> Result<u64, StoreError> {
        if content.is_empty() || content.len() > 300 * 1024 {
            return Err(failure(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid or oversized candidate selection",
            )));
        }
        let sealer = self
            .sealer
            .as_ref()
            .filter(|s| s.kind() == SealerKind::Dpapi)
            .ok_or_else(|| failure(waypoint_seal::SealError::Unavailable))?;
        let sealed = sealer.seal(content).map_err(failure)?;
        let changed = if expected == 0 {
            self.conn.execute("INSERT OR IGNORE INTO selected_web_candidates(id,revision,sealed_content) VALUES(1,1,?1)", [sealed])?
        } else {
            self.conn.execute("UPDATE selected_web_candidates SET revision=revision+1,sealed_content=?1 WHERE id=1 AND revision=?2 AND revision<9223372036854775807", params![sealed,expected])?
        };
        if changed != 1 {
            return Err(failure(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "Selection changed in another window; reload before saving",
            )));
        }
        Ok(expected + 1)
    }
    pub fn selected_web_candidates(&self) -> Result<Option<(u64, Vec<u8>)>, StoreError> {
        let data: Option<(u64, Vec<u8>)> = self
            .conn
            .query_row(
                "SELECT revision,sealed_content FROM selected_web_candidates WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        data.map(|(revision, sealed)| {
            let sealer = self
                .sealer
                .as_ref()
                .filter(|s| s.kind() == SealerKind::Dpapi)
                .ok_or_else(|| failure(waypoint_seal::SealError::Unavailable))?;
            Ok((revision, sealer.unseal(&sealed).map_err(failure)?))
        })
        .transpose()
    }
    pub fn save_latest_web_search(&self, task_id: &str, content: &[u8]) -> Result<(), StoreError> {
        if !valid_task_id(task_id) || content.is_empty() || content.len() > MAX_BYTES {
            return Err(failure(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid or oversized web search history",
            )));
        }
        let sealer = self
            .sealer
            .as_ref()
            .filter(|s| s.kind() == SealerKind::Dpapi)
            .ok_or_else(|| failure(waypoint_seal::SealError::Unavailable))?;
        let sealed = sealer.seal(content).map_err(failure)?;
        self.conn.execute("INSERT INTO latest_web_search(id,task_id,sealed_content) VALUES(1,?1,?2) ON CONFLICT(id) DO UPDATE SET task_id=excluded.task_id,sealed_content=excluded.sealed_content", params![task_id,sealed])?;
        Ok(())
    }
    pub fn latest_web_search(&self) -> Result<Option<(String, Vec<u8>)>, StoreError> {
        let row: Option<(String, Vec<u8>)> = self
            .conn
            .query_row(
                "SELECT task_id,sealed_content FROM latest_web_search WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        row.map(|(id, sealed)| {
            let sealer = self
                .sealer
                .as_ref()
                .filter(|s| s.kind() == SealerKind::Dpapi)
                .ok_or_else(|| failure(waypoint_seal::SealError::Unavailable))?;
            Ok((id, sealer.unseal(&sealed).map_err(failure)?))
        })
        .transpose()
    }

    /// Publish the plan and any reviewed prior receipts atomically.
    pub fn begin_web_search_run(
        &self,
        task_id: &str,
        planned: usize,
        plan: &[u8],
        prior_pages: &[Vec<u8>],
    ) -> Result<(), StoreError> {
        let bytes = prior_pages
            .iter()
            .fold(plan.len(), |total, page| total.saturating_add(page.len()));
        if !valid_task_id(task_id)
            || planned == 0
            || bytes > MAX_BYTES
            || plan.is_empty()
            || prior_pages.iter().any(Vec::is_empty)
        {
            return Err(invalid("Invalid or oversized web search plan"));
        }
        let sealer = self.web_search_sealer()?;
        let plan = sealer.seal(plan).map_err(failure)?;
        let prior = prior_pages
            .iter()
            .map(|page| sealer.seal(page).map_err(failure))
            .collect::<Result<Vec<_>, _>>()?;
        let tx = self.conn.unchecked_transaction()?;
        let same_id: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM web_search_run WHERE task_id=?1)",
            [task_id],
            |row| row.get(0),
        )?;
        if same_id {
            return Err(invalid("Web search identity is already in use"));
        }
        tx.execute("DELETE FROM web_search_run WHERE id=1", [])?;
        tx.execute(
            "INSERT INTO web_search_run(id,task_id,planned,payload_bytes,sealed_plan)
             VALUES (1,?1,?2,?3,?4)",
            params![task_id, planned, bytes, plan],
        )?;
        {
            let mut insert = tx.prepare(
                "INSERT INTO web_search_pages(run_id,sequence,sealed_page) VALUES(1,?1,?2)",
            )?;
            for (index, page) in prior.iter().enumerate() {
                insert.execute(params![-((prior.len() - index) as i64), page])?;
            }
        }
        tx.commit()
    }

    /// Commit exactly the next page and advance the cursor in one transaction.
    pub fn append_web_search_page(
        &self,
        task_id: &str,
        previous: usize,
        page: &[u8],
    ) -> Result<(), StoreError> {
        if page.is_empty() || page.len() > MAX_BYTES {
            return Err(invalid("Invalid or oversized web search page"));
        }
        let sealed = self.web_search_sealer()?.seal(page).map_err(failure)?;
        let tx = self.conn.unchecked_transaction()?;
        let changed = tx.execute(
            "UPDATE web_search_run SET committed=committed+1,payload_bytes=payload_bytes+?3
             WHERE id=1 AND task_id=?1 AND committed=?2 AND committed<planned
               AND sealed_terminal IS NULL AND payload_bytes<=?4",
            params![task_id, previous, page.len(), MAX_BYTES - page.len()],
        )?;
        if changed != 1 {
            return Err(invalid(
                "Web search cursor changed or storage budget was reached",
            ));
        }
        tx.execute(
            "INSERT INTO web_search_pages(run_id,sequence,sealed_page) VALUES(1,?1+1,?2)",
            params![previous, sealed],
        )?;
        tx.commit()
    }

    pub fn finish_web_search_run(
        &self,
        task_id: &str,
        processed: usize,
        terminal: &[u8],
    ) -> Result<(), StoreError> {
        if terminal.is_empty() || terminal.len() > MAX_BYTES {
            return Err(invalid("Invalid or oversized web search outcome"));
        }
        let sealed = self.web_search_sealer()?.seal(terminal).map_err(failure)?;
        let changed = self.conn.execute(
            "UPDATE web_search_run SET sealed_terminal=?3,payload_bytes=payload_bytes+?4
             WHERE id=1 AND task_id=?1 AND committed=?2 AND sealed_terminal IS NULL
               AND payload_bytes<=?5",
            params![
                task_id,
                processed,
                sealed,
                terminal.len(),
                MAX_BYTES - terminal.len()
            ],
        )?;
        if changed != 1 {
            return Err(invalid(
                "Web search cursor changed or storage budget was reached",
            ));
        }
        Ok(())
    }

    /// Read a single consistent snapshot, including receipts from an interrupted run.
    pub fn latest_web_search_run(&self) -> Result<Option<SavedWebSearchRun>, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let row = tx
            .query_row(
                "SELECT task_id,planned,committed,payload_bytes,sealed_plan,sealed_terminal
                 FROM web_search_run WHERE id=1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, usize>(1)?,
                        row.get::<_, usize>(2)?,
                        row.get::<_, usize>(3)?,
                        row.get::<_, Vec<u8>>(4)?,
                        row.get::<_, Option<Vec<u8>>>(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((task_id, planned, committed, bytes, plan, terminal)) = row else {
            return Ok(None);
        };
        let sealer = self.web_search_sealer()?;
        let plan = sealer.unseal(&plan).map_err(failure)?;
        let terminal = terminal
            .map(|content| sealer.unseal(&content).map_err(failure))
            .transpose()?;
        let mut decoded_bytes = plan.len() + terminal.as_ref().map_or(0, Vec::len);
        let mut pages = Vec::new();
        let mut prior_pages = Vec::new();
        {
            let mut stmt = tx.prepare(
                "SELECT sequence,sealed_page FROM web_search_pages WHERE run_id=1 ORDER BY sequence",
            )?;
            let mut rows = stmt.query([])?;
            let mut next_prior = None;
            while let Some(row) = rows.next()? {
                let seq: i64 = row.get(0)?;
                let sealed: Vec<u8> = row.get(1)?;
                let page = sealer.unseal(&sealed).map_err(failure)?;
                decoded_bytes = decoded_bytes.saturating_add(page.len());
                if page.is_empty() || decoded_bytes > bytes {
                    return Err(invalid("Invalid web search receipt size"));
                }
                if seq < 0 && next_prior.is_none_or(|next| seq == next) {
                    prior_pages.push(page);
                    next_prior = Some(seq + 1);
                } else if seq == pages.len() as i64 + 1 && pages.len() < committed {
                    pages.push(page);
                } else {
                    return Err(invalid("Web search receipt is out of sequence"));
                }
            }
            if next_prior.is_some_and(|next| next != 0) {
                return Err(invalid("Web search prior receipts are incomplete"));
            }
        }
        if pages.len() != committed || bytes != decoded_bytes {
            return Err(invalid(
                "Web search receipts do not match the committed cursor",
            ));
        }
        tx.commit()?;
        Ok(Some(SavedWebSearchRun {
            task_id,
            planned,
            plan,
            prior_pages,
            pages,
            terminal,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_pauses_expire_independently_and_cannot_be_shortened_by_stale_writes() {
        let store = Store::in_memory().unwrap();
        let first = "a".repeat(64);
        let second = "b".repeat(64);
        let rate_limit = ProviderPause {
            http_status: 429,
            retry_at: 160,
            from_retry_after: true,
        };
        let overload = ProviderPause {
            http_status: 503,
            retry_at: 250,
            from_retry_after: false,
        };
        store
            .save_web_provider_pause(&first, rate_limit, 100)
            .unwrap();
        store
            .save_web_provider_pause(&second, overload, 100)
            .unwrap();
        store
            .save_web_provider_pause(
                &first,
                ProviderPause {
                    retry_at: 105,
                    ..rate_limit
                },
                101,
            )
            .unwrap();
        assert_eq!(
            store.web_provider_pause(&first, 159).unwrap(),
            Some(rate_limit)
        );
        assert_eq!(store.web_provider_pause(&first, 160).unwrap(), None);
        assert_eq!(
            store.web_provider_pause(&second, 160).unwrap(),
            Some(overload)
        );
        assert!(store
            .save_web_provider_pause(
                &first,
                ProviderPause {
                    http_status: 403,
                    ..rate_limit
                },
                100
            )
            .is_err());
        assert!(store
            .web_provider_pause("https://example.org", 100)
            .is_err());
    }

    #[test]
    fn web_search_page_and_cursor_commit_together_and_reject_stale_writers() {
        let plain = Store::in_memory().unwrap();
        assert!(plain.begin_web_search_run("one", 2, b"plan", &[]).is_err());
        assert!(plain.latest_web_search_run().unwrap().is_none());
        let Some(sealer) =
            waypoint_seal::platform_sealer().filter(|s| s.kind() == SealerKind::Dpapi)
        else {
            return;
        };
        let store = Store::in_memory().unwrap().with_sealer(sealer);
        store
            .begin_web_search_run("one", 2, b"Fictional plan", &[b"Fictional prior".to_vec()])
            .unwrap();
        assert!(store.append_web_search_page("one", 1, b"Skipped").is_err());
        assert!(store.finish_web_search_run("one", 1, b"Premature").is_err());
        assert!(store
            .begin_web_search_run("one", 2, b"Same id", &[])
            .is_err());
        store
            .conn
            .execute_batch(
                "CREATE TRIGGER test_fail_append BEFORE INSERT ON web_search_pages
             WHEN NEW.sequence=1 BEGIN SELECT RAISE(ABORT,'fixture failure'); END;",
            )
            .unwrap();
        assert!(store
            .append_web_search_page("one", 0, b"Rolled back")
            .is_err());
        assert!(store
            .latest_web_search_run()
            .unwrap()
            .unwrap()
            .pages
            .is_empty());
        store
            .conn
            .execute_batch("DROP TRIGGER test_fail_append;")
            .unwrap();
        store
            .append_web_search_page("one", 0, b"Fictional page one")
            .unwrap();
        assert!(store
            .append_web_search_page("one", 0, b"Duplicate")
            .is_err());
        let first_sealed = || {
            store
                .conn
                .query_row(
                    "SELECT sealed_page FROM web_search_pages WHERE run_id=1 AND sequence=1",
                    [],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .unwrap()
        };
        let original = first_sealed();
        store
            .append_web_search_page("one", 1, b"Fictional page two")
            .unwrap();
        assert_eq!(first_sealed(), original);
        assert!(!original
            .windows(b"Fictional page one".len())
            .any(|value| value == b"Fictional page one"));
        store.finish_web_search_run("one", 2, b"Finished").unwrap();
        assert!(store.finish_web_search_run("one", 2, b"Replaced").is_err());
        let saved = store.latest_web_search_run().unwrap().unwrap();
        assert_eq!(saved.plan, b"Fictional plan");
        assert_eq!(saved.prior_pages, [b"Fictional prior"]);
        assert_eq!(saved.pages, [b"Fictional page one", b"Fictional page two"]);
        assert_eq!(saved.terminal.as_deref(), Some(&b"Finished"[..]));

        store.begin_web_search_run("two", 1, b"Next", &[]).unwrap();
        assert!(store.append_web_search_page("one", 0, b"Stale").is_err());
        let saved = store.latest_web_search_run().unwrap().unwrap();
        assert_eq!(saved.task_id, "two");
        assert!(saved.prior_pages.is_empty() && saved.pages.is_empty() && saved.terminal.is_none());
    }

    #[test]
    fn history_requires_user_sealing_and_replaces_only_the_latest_result() {
        let plain = Store::in_memory().unwrap();
        assert!(plain
            .save_latest_web_search("one", b"private query")
            .is_err());
        let Some(sealer) =
            waypoint_seal::platform_sealer().filter(|s| s.kind() == SealerKind::Dpapi)
        else {
            return;
        };
        let store = Store::in_memory().unwrap().with_sealer(sealer);
        store
            .save_latest_web_search("one", b"private query")
            .unwrap();
        let raw: Vec<u8> = store
            .conn
            .query_row(
                "SELECT sealed_content FROM latest_web_search WHERE id=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!raw
            .windows(b"private query".len())
            .any(|value| value == b"private query"));
        assert!(store
            .save_latest_web_search("bad id", b"replacement")
            .is_err());
        assert_eq!(
            store.latest_web_search().unwrap().unwrap().1,
            b"private query"
        );
        store.save_latest_web_search("two", b"next query").unwrap();
        assert_eq!(
            store.latest_web_search().unwrap(),
            Some(("two".into(), b"next query".to_vec()))
        );
        assert!(plain.save_selected_web_candidates(0, b"[]").is_err());
        assert_eq!(
            store
                .save_selected_web_candidates(0, b"private selected role")
                .unwrap(),
            1
        );
        let selected: Vec<u8> = store
            .conn
            .query_row(
                "SELECT sealed_content FROM selected_web_candidates WHERE id=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!selected
            .windows(b"private selected role".len())
            .any(|value| value == b"private selected role"));
        assert_eq!(
            store.selected_web_candidates().unwrap().unwrap(),
            (1, b"private selected role".to_vec())
        );
        assert!(store.save_selected_web_candidates(0, b"stale").is_err());
        assert_eq!(store.selected_web_candidates().unwrap().unwrap().0, 1);
    }
}
