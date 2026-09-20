use anyhow::Result;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    if std::env::args().any(|arg| arg == "--doctor") {
        let runtime = tokio::runtime::Runtime::new()?;
        return runtime.block_on(airmail::doctor::run());
    }

    airmail::ui::run()
}
