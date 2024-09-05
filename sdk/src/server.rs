use std::{convert::Infallible, error::Error, future::Future, io};

use hyper::{
    body::{Body, Incoming},
    server::conn::http1,
    service::service_fn,
    Request, Response,
};
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::{debug, warn};

pub async fn serve_connection<C, H, Fut, B>(io: C, handler: H)
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    H: Fn(Request<Incoming>) -> Fut,
    Fut: Future<Output = Result<Response<B>, Infallible>>,
    B: Body + 'static,
    <B as Body>::Error: Error + Send + Sync,
{
    let serving = http1::Builder::new()
        .keep_alive(true)
        .serve_connection(TokioIo::new(io), service_fn(handler))
        .await;
    if let Err(err) = serving {
        if let Some(err) = err.source().and_then(|err| err.downcast_ref::<io::Error>()) {
            match err.kind() {
                io::ErrorKind::NotConnected | io::ErrorKind::ConnectionReset => {
                    debug!(error = ?err, "canceled request");
                }
                _ => warn!(error = ?err, "error while serving"),
            }
        } else if err.is_incomplete_message() {
            debug!(error = ?err, "interrupted request");
        } else {
            warn!(error = ?err, "failed to serve HTTP");
        }
    }
}
