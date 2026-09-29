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

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--doctor") {
        let runtime = tokio::runtime::Runtime::new()?;
        return runtime.block_on(airmail::doctor::run());
    }
    if args.iter().any(|arg| arg == "--set-default") {
        airmail::ui::set_default_mail_client()?;
        println!("AirMail is now the default for mailto: links");
        return Ok(());
    }

    // What the desktop entry's `%u` hands over when a mailto: link is opened.
    // Anything else on the command line is not AirMail's to interpret.
    let links: Vec<String> = args
        .into_iter()
        .filter(|arg| airmail::mailto::Mailto::parse(arg).is_some())
        .collect();
    airmail::ui::run(&links)
}
