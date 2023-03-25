use anyhow::{bail, Result};
use chrono::{TimeZone, Utc};
use futures_util::TryStreamExt;
use rammingen_protocol::{
    Entry, EntryVersion, EntryVersionData, FileContent, GetEntries, GetVersions, Login, Request,
    RequestVariant, SourceId,
};
use serde::Serialize;
use sqlx::{query, types::time::OffsetDateTime, PgPool};
use tracing::{info, warn};

pub struct Handler {
    pool: PgPool,
    source_id: Option<SourceId>,
}

pub type Response<Request> = Result<<Request as RequestVariant>::Response>;

fn serialize_response<T: Serialize>(value: &Result<T>) -> Vec<u8> {
    bincode::serialize(&value.as_ref().map_err(|e| e.to_string()))
        .expect("bincode serialization failed")
}

macro_rules! convert_entry {
    ($row:expr) => {{
        let row = $row;
        Entry {
            id: row.id.into(),
            update_number: row.update_number.into(),
            parent_dir: row.parent_dir.map(Into::into),
            data: convert_version_data!(row),
        }
    }};
}

macro_rules! convert_entry_version {
    ($row:expr) => {{
        let row = $row;
        EntryVersion {
            id: row.id.into(),
            entry_id: row.entry_id.into(),
            snapshot_id: row.snapshot_id.map(Into::into),
            data: convert_version_data!(row),
        }
    }};
}

macro_rules! convert_version_data {
    ($row:expr) => {{
        let row = $row;
        EntryVersionData {
            path: row.path,
            recorded_at: Utc.timestamp_nanos(row.recorded_at.unix_timestamp_nanos() as i64),
            source_id: row.source_id.into(),
            record_trigger: row.record_trigger.try_into()?,
            kind: row.kind.try_into()?,
            exists: row.exists,
            content: if let (Some(modified_at), Some(size), Some(content_hash)) =
                (row.modified_at, row.size, row.content_hash)
            {
                Some(FileContent {
                    modified_at: Utc.timestamp_nanos(modified_at.unix_timestamp_nanos() as i64),
                    size: size.try_into()?,
                    content_hash: content_hash.into(),
                    unix_mode: row.unix_mode.map(TryInto::try_into).transpose()?,
                })
            } else {
                None
            },
        }
    }};
}

impl Handler {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            source_id: None,
        }
    }

    pub async fn handle(&mut self, request: Request) -> (Vec<u8>, bool) {
        match request {
            Request::Login(request) => {
                let result = self.login(request).await;
                (serialize_response(&result), result.is_ok())
            }
            request => {
                if self.source_id.is_none() {
                    warn!("received another message before login");
                    (Vec::new(), false)
                } else {
                    macro_rules! handle {
                        ($($variant:ident => $handler:ident,)*) => {
                            match request {
                                $(
                                    Request::$variant(request) => {
                                        serialize_response(&self.$handler(request).await)
                                    }
                                )*
                                _ => todo!(),
                            }

                        }
                    }

                    let response = handle! {
                        Login => login,
                        GetEntries => get_entries,
                        GetVersions => get_versions,
                    };
                    (response, true)
                }
            }
        }
    }

    async fn login(&mut self, request: Login) -> Response<Login> {
        let row = query!(
            "SELECT name FROM sources WHERE id = $1 AND secret = $2",
            request.source_id.0,
            request.secret
        )
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = row {
            info!("new login: {:?}", row.name);
            self.source_id = Some(request.source_id);
        } else {
            warn!("invalid login");
            bail!("invalid login");
        }
        Ok(())
    }

    async fn get_versions(&mut self, request: GetVersions) -> Response<GetVersions> {
        let mut output = Vec::new();
        let mut rows =
            query!(
                "SELECT * FROM entry_versions WHERE path = $1 AND recorded_at <= $2 ORDER BY recorded_at DESC LIMIT 1",
                request.path,
                OffsetDateTime::from_unix_timestamp_nanos(request.recorded_at.timestamp_nanos().into())?,
            ).fetch(&self.pool);

        while let Some(row) = rows.try_next().await? {
            output.push(convert_entry_version!(row));
        }

        let mut rows2 = query!(
            "SELECT * FROM (
                SELECT *,
                row_number() OVER(PARTITION BY entry_id ORDER BY recorded_at DESC) AS row_number
                FROM entry_versions
                WHERE path LIKE $1 AND recorded_at <= $2
            ) t WHERE row_number = 1",
            format!("{}/", request.path),
            OffsetDateTime::from_unix_timestamp_nanos(
                request.recorded_at.timestamp_nanos().into()
            )?,
        )
        .fetch(&self.pool);

        while let Some(row) = rows2.try_next().await? {
            output.push(convert_entry_version!(row));
        }
        Ok(Some(output))
    }

    async fn get_entries(&mut self, request: GetEntries) -> Response<GetEntries> {
        let mut output = Vec::new();
        let mut rows = query!(
            "SELECT * FROM entries WHERE update_number > $1",
            request.last_update_number.map(|x| x.0).unwrap_or(0)
        )
        .fetch(&self.pool);
        while let Some(row) = rows.try_next().await? {
            output.push(convert_entry!(row));
        }

        Ok(Some(output))
    }
}
