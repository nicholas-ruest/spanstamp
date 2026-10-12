use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use spanstamp::{
    capture_anchor, index_markdown, resolve_anchor, Anchor, Resolution, ResolutionStatus,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(
    name = "spanstamp",
    version,
    about = "Durable, content-addressed review anchors for Markdown"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Index top-level Markdown blocks with line ranges and digests.
    Index {
        /// Markdown document to index.
        document: PathBuf,
        /// Emit compact JSON instead of pretty JSON.
        #[arg(long)]
        compact: bool,
    },
    /// Capture the reviewable block containing a 1-based source line.
    Capture {
        /// Markdown document containing the review target.
        document: PathBuf,
        /// One-based source line inside the target block.
        #[arg(long)]
        line: usize,
        /// Write the anchor JSON to this file instead of stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Resolve a saved anchor against the current document revision.
    Resolve {
        /// Current Markdown document revision.
        document: PathBuf,
        /// Anchor JSON previously produced by `capture`.
        anchor: PathBuf,
        /// Human-readable text or machine-readable JSON.
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Index { document, compact } => {
            let source = read_utf8(&document)?;
            let index = index_markdown(&source);
            let output = if compact {
                serde_json::to_string(&index)
            } else {
                serde_json::to_string_pretty(&index)
            }
            .context("failed to serialize document index")?;
            println!("{output}");
            Ok(ExitCode::SUCCESS)
        }
        Command::Capture {
            document,
            line,
            output,
        } => {
            let source = read_utf8(&document)?;
            let index = index_markdown(&source);
            let anchor = capture_anchor(&index, line).context("failed to capture anchor")?;
            let json = serde_json::to_string_pretty(&anchor)
                .context("failed to serialize captured anchor")?;
            if let Some(path) = output {
                fs::write(&path, format!("{json}\n"))
                    .with_context(|| format!("failed to write {}", path.display()))?;
            } else {
                println!("{json}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Resolve {
            document,
            anchor,
            format,
        } => {
            let source = read_utf8(&document)?;
            let anchor_source = read_utf8(&anchor)?;
            let anchor: Anchor = serde_json::from_str(&anchor_source)
                .with_context(|| format!("failed to parse {} as an anchor", anchor.display()))?;
            let resolution = resolve_anchor(&index_markdown(&source), &anchor);
            print_resolution(&resolution, format)?;
            Ok(if resolution.status == ResolutionStatus::Resolved {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(2)
            })
        }
    }
}

fn read_utf8(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("failed to read {} as UTF-8", path.display()))
}

fn print_resolution(resolution: &Resolution, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(resolution)
                    .context("failed to serialize resolution")?
            );
        }
        OutputFormat::Text => {
            let status = match resolution.status {
                ResolutionStatus::Resolved => "RESOLVED",
                ResolutionStatus::Ambiguous => "AMBIGUOUS",
                ResolutionStatus::Missing => "MISSING",
            };
            println!("{status}: {}", resolution.reason);
            for candidate in &resolution.candidates {
                println!(
                    "  lines {}-{} {} context={} digest={} preview={:?}",
                    candidate.start_line,
                    candidate.end_line,
                    candidate.kind,
                    candidate.context_matches,
                    &candidate.source_sha256[..12],
                    candidate.preview
                );
            }
        }
    }
    Ok(())
}
