//! Command-line interface for the Rilldown engine.
//!
//! The CLI is intentionally synchronous: `add` owns the transfer until it
//! completes, fails, or is cancelled with Ctrl+C. The desktop app is the
//! long-running manager.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use rilldown_engine::{
    Checksum, ChecksumAlgorithm, DownloadFilter, DownloadId, DownloadSpec, DownloadState, Engine,
    EngineConfig, EngineResult, Url,
};

#[derive(Parser, Debug)]
#[command(
    name = "rilldown",
    version,
    about = "Rilldown download engine CLI",
    long_about = "Download a URL with Rilldown's segmented engine, inspect the local database, or print one record as JSON."
)]
struct Cli {
    /// Override the data directory (database and logs).
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,

    /// Override the default download directory.
    #[arg(long, global = true)]
    download_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Download a URL and wait for it to finish.
    Add {
        url: String,
        /// Save this download somewhere other than the configured directory.
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Use this exact filename instead of a server-provided one.
        #[arg(long)]
        name: Option<String>,
        /// Connections to use (1-32).
        #[arg(short = 'c', long, default_value_t = 8)]
        connections: u8,
        /// Verify the finished file, for example `sha256:abc...`.
        #[arg(long)]
        checksum: Option<String>,
        /// Extra request header, repeatable, for example `--header "Accept: application/pdf"`.
        #[arg(long = "header")]
        headers: Vec<String>,
        #[arg(long)]
        referrer: Option<String>,
        #[arg(long)]
        cookies: Option<String>,
        #[arg(long)]
        user_agent: Option<String>,
    },
    /// List downloads in the local database.
    List,
    /// Print one download as JSON.
    Get { id: String },
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("RILLDOWN_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_target(false)
        .init();

    match run().await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> EngineResult<ExitCode> {
    let cli = Cli::parse();
    let mut config = EngineConfig::platform_default()?;
    if let Some(data_dir) = cli.data_dir {
        config.data_dir = data_dir;
        config.database_path = config.data_dir.join("rilldown.db");
    }
    if let Some(download_dir) = cli.download_dir {
        config.download_dir = download_dir;
    }
    let engine = Engine::new(config)?;

    match cli.command {
        Command::Add {
            url,
            dir,
            name,
            connections,
            checksum,
            headers,
            referrer,
            cookies,
            user_agent,
        } => {
            let parsed_url = Url::parse(&url)
                .map_err(|error| rilldown_engine::EngineError::InvalidUrl(error.to_string()))?;
            let mut spec = DownloadSpec::new(
                parsed_url,
                dir.unwrap_or_else(|| engine.config().download_dir.clone()),
            );
            spec.connections = connections;
            spec.filename = name;
            spec.checksum = checksum.as_deref().map(parse_checksum).transpose()?;
            spec.referrer = referrer;
            spec.cookies = cookies;
            spec.user_agent = user_agent;
            spec.headers = parse_headers(&headers)?;

            let record = engine.add(spec).await?;
            println!("added {} {}", record.id, record.filename);
            let outcome = tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    eprintln!("cancelling...");
                    engine.cancel(record.id).await?;
                    ExitCode::from(130)
                }
                code = wait_for(&engine, record.id) => code?,
            };
            Ok(outcome)
        }
        Command::List => {
            let records = engine.list(&DownloadFilter::default())?;
            if records.is_empty() {
                println!("No downloads.");
                return Ok(ExitCode::SUCCESS);
            }
            println!("{:<10} {:<12} {:>12}  NAME", "ID", "STATE", "SIZE");
            for record in records {
                println!(
                    "{:<10} {:<12} {:>12}  {}",
                    short_id(record.id),
                    record.state.as_str(),
                    record
                        .total_bytes
                        .map_or_else(|| "?".to_string(), format_bytes),
                    record.filename
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Get { id } => {
            let id: DownloadId = id.parse()?;
            let record = engine.get(id)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&record)
                    .map_err(rilldown_engine::EngineError::from)?
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}

async fn wait_for(engine: &Engine, id: DownloadId) -> EngineResult<ExitCode> {
    let mut last_report = Instant::now();
    let mut last_bytes = 0_u64;
    loop {
        let record = engine.get(id)?;
        match record.state {
            DownloadState::Completed => {
                println!(
                    "\ncompleted {} ({} bytes) -> {}",
                    record.id,
                    record.downloaded_bytes,
                    record.destination().display()
                );
                if let Some(actual) = &record.checksum_actual {
                    println!("checksum {actual}");
                }
                return Ok(ExitCode::SUCCESS);
            }
            DownloadState::Failed => {
                eprintln!(
                    "\nfailed {}: {}",
                    record.id,
                    record.error.as_deref().unwrap_or("unknown error")
                );
                return Ok(ExitCode::FAILURE);
            }
            DownloadState::Cancelled => {
                eprintln!("\ncancelled {}", record.id);
                return Ok(ExitCode::from(130));
            }
            state => {
                if last_report.elapsed() >= Duration::from_millis(500) {
                    let speed = (record.downloaded_bytes.saturating_sub(last_bytes) as f64
                        / last_report.elapsed().as_secs_f64().max(0.001))
                        as u64;
                    let total = record
                        .total_bytes
                        .map_or_else(|| "?".to_string(), format_bytes);
                    println!(
                        "{}  {} / {}  {}/s  {}",
                        state.as_str(),
                        format_bytes(record.downloaded_bytes),
                        total,
                        format_bytes(speed),
                        record.filename
                    );
                    last_report = Instant::now();
                    last_bytes = record.downloaded_bytes;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

fn parse_checksum(value: &str) -> EngineResult<Checksum> {
    let (algorithm, digest) = value
        .split_once(':')
        .ok_or_else(|| rilldown_engine::EngineError::Other("checksum must be ALG:HEX".into()))?;
    let algorithm: ChecksumAlgorithm = algorithm.parse()?;
    let checksum = Checksum {
        algorithm,
        expected_hex: digest.trim().to_ascii_lowercase(),
    };
    checksum.validate()?;
    Ok(checksum)
}

fn parse_headers(values: &[String]) -> EngineResult<BTreeMap<String, String>> {
    let mut headers = BTreeMap::new();
    for value in values {
        let (name, header_value) = value.split_once(':').ok_or_else(|| {
            rilldown_engine::EngineError::Other(format!("invalid header: {value}"))
        })?;
        let name = name.trim();
        if name.is_empty() {
            return Err(rilldown_engine::EngineError::Other(format!(
                "invalid header: {value}"
            )));
        }
        headers.insert(name.to_string(), header_value.trim().to_string());
    }
    Ok(headers)
}

fn short_id(id: DownloadId) -> String {
    id.to_string().chars().take(8).collect()
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    #[allow(clippy::cast_precision_loss)]
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 || value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_checksums() {
        let checksum = parse_checksum(&format!("sha256:{}", "a".repeat(64))).unwrap();
        assert_eq!(checksum.algorithm, ChecksumAlgorithm::Sha256);
        assert!(parse_checksum("sha256:nope").is_err());
        assert!(parse_checksum("nope").is_err());
    }

    #[test]
    fn parses_headers() {
        let headers = parse_headers(&["Accept: application/pdf".to_string()]).unwrap();
        assert_eq!(headers["Accept"], "application/pdf");
        assert!(parse_headers(&["missing-colon".to_string()]).is_err());
    }
}
