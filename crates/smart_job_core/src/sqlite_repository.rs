use crate::domain::{JobOpportunity, UserProfile};
use crate::repository::{JobRepository, ProfileRepository, RepositoryError, RepositoryResult};
use rusqlite::{params, Connection, OptionalExtension};

pub struct SqliteRepository {
    conn: Connection,
}

impl SqliteRepository {
    pub fn new(path: &str) -> RepositoryResult<Self> {
        let conn = Connection::open(path).map_err(storage_error)?;
        let mut repository = Self { conn };
        repository.initialize_schema()?;
        Ok(repository)
    }

    pub fn in_memory() -> RepositoryResult<Self> {
        let conn = Connection::open_in_memory().map_err(storage_error)?;
        let mut repository = Self { conn };
        repository.initialize_schema()?;
        Ok(repository)
    }

    fn initialize_schema(&mut self) -> RepositoryResult<()> {
        self.conn
            .execute_batch(
                "
                CREATE TABLE IF NOT EXISTS jobs (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    company TEXT NOT NULL,
                    location TEXT NOT NULL,
                    remote INTEGER NOT NULL,
                    required_skills_json TEXT NOT NULL,
                    min_salary_usd INTEGER
                );

                CREATE TABLE IF NOT EXISTS profile (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    skills_json TEXT NOT NULL,
                    preferred_locations_json TEXT NOT NULL,
                    remote_only INTEGER NOT NULL,
                    min_salary_expectation_usd INTEGER
                );
                ",
            )
            .map_err(storage_error)
    }
}

impl JobRepository for SqliteRepository {
    fn replace_jobs(&mut self, jobs: Vec<JobOpportunity>) -> RepositoryResult<()> {
        let tx = self.conn.transaction().map_err(storage_error)?;
        tx.execute("DELETE FROM jobs", []).map_err(storage_error)?;

        {
            let mut stmt = tx
                .prepare(
                    "
                    INSERT INTO jobs (
                        id, title, company, location, remote, required_skills_json, min_salary_usd
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                    ",
                )
                .map_err(storage_error)?;

            for job in jobs {
                let skills_json =
                    serde_json::to_string(&job.required_skills).map_err(serialization_error)?;
                stmt.execute(params![
                    job.id,
                    job.title,
                    job.company,
                    job.location,
                    i64::from(job.remote),
                    skills_json,
                    job.min_salary_usd.map(i64::from),
                ])
                .map_err(storage_error)?;
            }
        }

        tx.commit().map_err(storage_error)
    }

    fn all_jobs(&self) -> RepositoryResult<Vec<JobOpportunity>> {
        let mut stmt = self
            .conn
            .prepare(
                "
                SELECT id, title, company, location, remote, required_skills_json, min_salary_usd
                FROM jobs
                ",
            )
            .map_err(storage_error)?;

        let rows = stmt
            .query_map([], |row| {
                let skills_json: String = row.get(5)?;
                let required_skills: Vec<String> =
                    serde_json::from_str(&skills_json).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            5,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                let min_salary_i64: Option<i64> = row.get(6)?;
                let min_salary_usd = min_salary_i64.and_then(|value| u32::try_from(value).ok());

                Ok(JobOpportunity {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    company: row.get(2)?,
                    location: row.get(3)?,
                    remote: row.get::<_, i64>(4)? == 1,
                    required_skills,
                    min_salary_usd,
                })
            })
            .map_err(storage_error)?;

        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row.map_err(storage_error)?);
        }

        Ok(jobs)
    }
}

impl ProfileRepository for SqliteRepository {
    fn set_profile(&mut self, profile: UserProfile) -> RepositoryResult<()> {
        let skills_json = serde_json::to_string(&profile.skills).map_err(serialization_error)?;
        let preferred_locations_json =
            serde_json::to_string(&profile.preferred_locations).map_err(serialization_error)?;

        self.conn
            .execute(
                "
                INSERT INTO profile (
                    id, skills_json, preferred_locations_json, remote_only, min_salary_expectation_usd
                ) VALUES (1, ?1, ?2, ?3, ?4)
                ON CONFLICT(id) DO UPDATE SET
                    skills_json = excluded.skills_json,
                    preferred_locations_json = excluded.preferred_locations_json,
                    remote_only = excluded.remote_only,
                    min_salary_expectation_usd = excluded.min_salary_expectation_usd
                ",
                params![
                    skills_json,
                    preferred_locations_json,
                    i64::from(profile.remote_only),
                    profile.min_salary_expectation_usd.map(i64::from)
                ],
            )
            .map_err(storage_error)?;

        Ok(())
    }

    fn profile(&self) -> RepositoryResult<Option<UserProfile>> {
        self.conn
            .query_row(
                "
                SELECT skills_json, preferred_locations_json, remote_only, min_salary_expectation_usd
                FROM profile
                WHERE id = 1
                ",
                [],
                |row| {
                    let skills_json: String = row.get(0)?;
                    let preferred_locations_json: String = row.get(1)?;
                    let skills = serde_json::from_str::<Vec<String>>(&skills_json).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
                    })?;
                    let preferred_locations =
                        serde_json::from_str::<Vec<String>>(&preferred_locations_json).map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
                        })?;

                    let min_salary_i64: Option<i64> = row.get(3)?;
                    let min_salary_expectation_usd =
                        min_salary_i64.and_then(|value| u32::try_from(value).ok());

                    Ok(UserProfile {
                        skills,
                        preferred_locations,
                        remote_only: row.get::<_, i64>(2)? == 1,
                        min_salary_expectation_usd,
                    })
                },
            )
            .optional()
            .map_err(storage_error)
    }
}

fn storage_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn serialization_error(error: serde_json::Error) -> RepositoryError {
    RepositoryError::Serialization(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_profile_and_jobs_in_memory_db() {
        let mut repo = SqliteRepository::in_memory().expect("repo");

        repo.set_profile(UserProfile {
            skills: vec!["rust".to_string(), "sqlite".to_string()],
            preferred_locations: vec!["seattle".to_string()],
            remote_only: false,
            min_salary_expectation_usd: Some(120_000),
        })
        .expect("profile");

        repo.replace_jobs(vec![JobOpportunity {
            id: "job-1".to_string(),
            title: "Rust Engineer".to_string(),
            company: "Acme".to_string(),
            location: "Seattle".to_string(),
            remote: true,
            required_skills: vec!["rust".to_string()],
            min_salary_usd: Some(130_000),
        }])
        .expect("jobs");

        let profile = repo.profile().expect("read profile").expect("some profile");
        let jobs = repo.all_jobs().expect("read jobs");

        assert_eq!(
            profile.skills,
            vec!["rust".to_string(), "sqlite".to_string()]
        );
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, "job-1");
    }
}
