use anyhow::Result;
use clap::{Parser, Subcommand};
use gigagraph::{api, indexer, lsp, mcp, viz};
use std::path::PathBuf;

/// gigagraph — semantic code-graph index + MCP server.
///
/// Indexes every function (definitions, call sites, imports) across C,
/// JavaScript, TypeScript, Java, Kotlin, and Swift, resolves the call graph,
/// and vectorizes functions structurally for similarity search.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run as an MCP server on stdio.
    Serve {
        /// Project root to serve (defaults to the current directory).
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Build (or refresh) the index and print stats.
    Index {
        /// Project root to index.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Re-parse everything, ignoring the extraction cache.
        #[arg(long)]
        force: bool,
    },
    /// Generate an interactive 3D map of the codebase (one self-contained
    /// HTML file): functions clustered by structural similarity, call-graph
    /// edges, endpoints highlighted.
    Visualize {
        /// Project root to visualize (index is loaded, or built if missing).
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Output HTML path (defaults to <root>/.gigagraph/map.html).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Open the map in the default browser after writing it.
        #[arg(long)]
        open: bool,
    },
    /// Record a touch: append recently edited files + a one-line rationale
    /// to the persistent touch ring (.gigagraph/touches.jsonl). This is the
    /// command editor hooks call after every Edit/Write.
    Touch {
        /// Project root.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// One-line rationale for the edit.
        #[arg(long)]
        why: String,
        /// Who is recording (agent name, "hook", ...).
        #[arg(long, default_value = "unknown")]
        agent: String,
        /// Files that were edited (repo-relative or absolute).
        #[arg(required = true)]
        files: Vec<String>,
    },
    /// Show recent touches (newest first), optionally for one file.
    /// Agent-/hook-reported — `git log` stays the authoritative history.
    Touches {
        /// Project root.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Only entries mentioning this file.
        #[arg(long)]
        file: Option<String>,
        /// Max entries to show (max 50).
        #[arg(long, default_value_t = 10)]
        limit: u64,
    },
    /// Call one tool directly (debugging). Prints the JSON result.
    Query {
        /// Tool name (e.g. search_functions, get_callers, find_similar).
        tool: String,
        /// Tool arguments as JSON, e.g. '{"query": "parse"}'.
        #[arg(default_value = "{}")]
        args: String,
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve { root } => {
            let state = api::AppState::new(root);
            mcp::serve(state)
        }
        Command::Index { path, force } => {
            let mut index = indexer::build_index(&path, force)?;
            // Optional LSP enrichment (auto-detected, silently skipped when
            // absent). The CLI is a batch tool, so it runs synchronously here;
            // the MCP server runs the same pass on a background thread.
            let mut providers = lsp::detect_providers(&path);
            if !providers.is_empty() && !index.stats.lsp_enriched {
                let root = path.canonicalize().unwrap_or(path);
                lsp::enrich_index(&mut index, &root, &mut providers);
            }
            println!("{}", serde_json::to_string_pretty(&index.stats)?);
            Ok(())
        }
        Command::Visualize { root, out, open } => {
            let path = viz::write_map(&root, out.as_deref())?;
            println!("{}", path.display());
            if open {
                viz::open_in_browser(&path)?;
            }
            Ok(())
        }
        Command::Touch {
            root,
            why,
            agent,
            files,
        } => {
            // Same append path as the record_touch MCP tool.
            let mut state = api::AppState::new(root);
            let args = serde_json::json!({ "files": files, "why": why, "agent": agent });
            let v = state.dispatch("record_touch", &args)?;
            println!("{}", serde_json::to_string_pretty(&v)?);
            Ok(())
        }
        Command::Touches { root, file, limit } => {
            let mut state = api::AppState::new(root);
            let mut args = serde_json::json!({ "limit": limit });
            if let Some(f) = file {
                args["file"] = serde_json::json!(f);
            }
            let v = state.dispatch("recent_touches", &args)?;
            println!("{}", serde_json::to_string_pretty(&v)?);
            Ok(())
        }
        Command::Query { tool, args, root } => {
            let mut state = api::AppState::new(root);
            let args: serde_json::Value = serde_json::from_str(&args)?;
            match state.dispatch(&tool, &args) {
                Ok(v) => {
                    println!("{}", serde_json::to_string_pretty(&v)?);
                    Ok(())
                }
                Err(e) => {
                    eprintln!("error: {e:#}");
                    std::process::exit(1);
                }
            }
        }
    }
}
