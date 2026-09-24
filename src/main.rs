use anyhow::Result;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Both `ring` (via lettre) and `aws-lc-rs` (tokio-rustls's default) end up
    // compiled in, so rustls cannot pick a provider on its own and panics at
    // the first TLS handshake — inside a spawned sync task, where nobody sees it.
    let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();

    if std::env::args().any(|arg| arg == "--doctor") {
        let runtime = tokio::runtime::Runtime::new()?;
        return runtime.block_on(airmail::doctor::run());
    }

    airmail::ui::run()
}
