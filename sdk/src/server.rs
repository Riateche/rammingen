use {
    futures::FutureExt,
    hyper::{
        Request, Response,
        body::{Body, Incoming},
        server::conn::http1,
        service::service_fn,
    },
    hyper_util::{
        rt::TokioIo,
        server::graceful::{GracefulConnection, GracefulShutdown, Watcher},
    },
    std::{convert::Infallible, error::Error, future::Future, io, time::Duration},
    tokio::{
        io::{AsyncRead, AsyncWrite},
        time::timeout,
    },
    tracing::{debug, info, warn},
};

#[inline(never)]
pub fn serve_connection<C, H, Fut, B>(
    io: C,
    shutdown_watcher: Watcher,
    handler: H,
) -> impl Future<Output = ()>
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    H: Fn(Request<Incoming>) -> Fut,
    Fut: Future<Output = Response<B>>,
    B: Body + 'static,
    <B as Body>::Error: Error + Send + Sync,
{
    let serving = http1::Builder::new().keep_alive(true).serve_connection(
        TokioIo::new(io),
        service_fn(move |request| handler(request).map(Ok::<_, Infallible>)),
    );
    async move {
        let serving = shutdown_watcher.watch(serving);
        if let Err(err) = serving.await {
            if let Some(err) = err.source().and_then(|err| err.downcast_ref::<io::Error>()) {
                #[expect(clippy::wildcard_enum_match_arm, reason = "safe default")]
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
}

#[derive(Default)]
pub struct ShutdownWatcher {
    inner: GracefulShutdown,
}

impl ShutdownWatcher {
    #[inline]
    pub fn watch<C: GracefulConnection>(&self, io: C) -> impl Future<Output = C::Output> {
        self.inner.watch(io)
    }

    #[must_use]
    #[inline]
    pub fn watcher(&self) -> Watcher {
        self.inner.watcher()
    }

    #[inline]
    pub async fn shutdown(self) {
        const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
        let Ok(()) = timeout(SHUTDOWN_TIMEOUT, self.inner.shutdown()).await else {
            warn!("Timed out wait for all connections to close");
            return;
        };
        info!("All connections gracefully closed");
    }
}
