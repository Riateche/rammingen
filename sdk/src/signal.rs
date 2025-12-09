use {
    anyhow::{Context, Result},
    derive_more::Display,
    futures::{FutureExt, future::select},
    std::{future::Future, pin::pin},
    tokio::signal::ctrl_c,
};

#[derive(Display)]
pub enum ShutdownSignal {
    Sigint,
    Sigterm,
}

#[inline(never)]
pub async fn shutdown_signal() -> Result<ShutdownSignal> {
    let sigint = ctrl_c().map(|signal| {
        signal
            .map(|()| ShutdownSignal::Sigint)
            .context("failed to install sigint signal handler")
    });
    let sigint = pin!(sigint);
    let sigterm = sigterm()
        .context("failed to install sigterm signal handler")?
        .map(|()| Ok(ShutdownSignal::Sigterm));
    let sigterm = pin!(sigterm);
    let (signal, _unfired_signal) = select(sigint, sigterm).await.factor_first();
    signal
}

#[allow(
    clippy::allow_attributes,
    clippy::unnecessary_wraps,
    reason = "must have same signature on all platforms"
)]
fn sigterm() -> Result<impl Future<Output = ()>> {
    #[cfg(target_family = "unix")]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigterm = signal(SignalKind::terminate())?;
        Ok(async move {
            sigterm.recv().await;
        })
    }

    #[cfg(not(target_family = "unix"))]
    #[expect(clippy::absolute_paths, reason = "single use")]
    Ok(std::future::pending())
}
