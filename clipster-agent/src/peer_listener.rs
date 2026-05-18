//! Peer-facing TLS listener — exposes the clipster-core peer router so other
//! peers on the LAN can pull from this headless agent.

use clipster_core::ClipsterCore;
use clipster_core::peer::server::router as peer_router;
use std::sync::Arc;

pub async fn run(core: Arc<ClipsterCore>, port: u16) -> anyhow::Result<()> {
    let bind = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    let acceptor = core.identity.tls_acceptor()?;
    let app = peer_router(core.clone());

    tracing::info!(%bind, device = %core.identity.device_id, "agent peer listener up");

    loop {
        let (stream, _) = listener.accept().await?;
        let acc = acceptor.clone();
        let svc = app.clone();
        tokio::spawn(async move {
            match acc.accept(stream).await {
                Ok(tls) => {
                    let io = hyper_util::rt::TokioIo::new(tls);
                    let service = hyper_util::service::TowerToHyperService::new(svc);
                    let _ = hyper_util::server::conn::auto::Builder::new(
                        hyper_util::rt::TokioExecutor::new(),
                    )
                    .serve_connection(io, service)
                    .await;
                }
                Err(e) => tracing::debug!(error = %e, "agent TLS handshake failed"),
            }
        });
    }
}
