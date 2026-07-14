use std::io;

use anyhow::{Error, Result};
use rains::{
    cli::{Opts, Subcommand},
    commands::{InfoOptions, QuoteOptions, handle_info, handle_quote, handle_search},
    format::sanitize_text,
    provider::SinaProvider,
};
use tracing::error;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        if is_broken_pipe(&err) {
            return;
        }
        let message = format!("{err:#}");
        error!("{}", sanitize_text(&message));
        std::process::exit(1);
    }
}

fn is_broken_pipe(error: &Error) -> bool {
    error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<io::Error>())
        .any(|error| error.kind() == io::ErrorKind::BrokenPipe)
}

async fn run() -> Result<()> {
    let args = Opts::parse_args()?;

    // 初始化日志
    init_logging(args.debug);

    // 创建数据提供者
    let provider = SinaProvider::default();

    // 执行命令
    match args.cmd {
        Subcommand::Search { query, limit } => {
            handle_search(&provider, &query, limit as usize).await?;
        }
        Subcommand::Info { symbol, all, financials, structure, dividends, presses } => {
            let options = InfoOptions { all, financials, structure, dividends, presses };
            handle_info(&provider, &symbol, options).await?;
        }
        Subcommand::Quote { symbol, no_check, realtime, multiline } => {
            let options = QuoteOptions { no_check, realtime, multiline };
            handle_quote(&provider, &symbol, options).await?;
        }
    }

    Ok(())
}

fn init_logging(debug: bool) {
    if debug {
        let filter = "debug,html5ever=info,selectors=info".parse::<EnvFilter>().unwrap();
        tracing_subscriber::fmt().with_writer(std::io::stderr).with_env_filter(filter).init();
    } else {
        tracing_subscriber::fmt().with_writer(std::io::stderr).without_time().with_target(false).init();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_broken_pipe_through_anyhow_context() {
        let error = Error::new(io::Error::new(io::ErrorKind::BrokenPipe, "closed")).context("render failed");
        assert!(is_broken_pipe(&error));
        assert!(!is_broken_pipe(&anyhow::anyhow!("other")));
    }
}
