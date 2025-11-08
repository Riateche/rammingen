use {
    anyhow::{Context, Result},
    derive_more::Display,
    futures::{future::select, FutureExt},
    std::{future::Future, pin::pin},
    tokio::signal::ctrl_c,
};

#[derive(Display)]
pub enum ShutdownSignal {
    Sigint,
    Sigterm,
}

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

fn sigterm() -> Result<impl Future<Output = ()>> {
    #[cfg(target_family = "unix")]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate())?;
        Ok(async move {
            sigterm.recv().await;
        })
    }

    #[cfg(not(target_family = "unix"))]
    Ok(std::future::pending())
}
