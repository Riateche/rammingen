use anyhow::{bail, Result};
use rammingen_protocol::{GetVersions, Login, Request, RequestVariant, SourceId};
use serde::Serialize;
use sqlx::{query, PgPool};
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

    async fn get_versions(&mut self, _request: GetVersions) -> Response<GetVersions> {
        Ok(Vec::new())
    }
}
