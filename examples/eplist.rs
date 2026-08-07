//! Probe helper: index a directory and print every detected endpoint.
//! Usage: cargo run --release --example eplist -- path/to/project
//!
//! Rows: METHOD path_norm (framework, confidence) [file:line] {handler}

use gigagraph::indexer::build_index;
use std::path::Path;

fn main() {
    let root = std::env::args().nth(1).expect("usage: eplist <dir>");
    let index = build_index(Path::new(&root), true).expect("index build failed");
    let g = &index.graph;
    let mut rows: Vec<String> = g
        .endpoints
        .endpoints
        .iter()
        .map(|e| {
            let file = &g.files[e.file_id as usize].path;
            let handler = e
                .handler
                .map(|h| format!(" {{{}}}", g.functions[h as usize].name))
                .unwrap_or_default();
            format!(
                "{:7} {:50} ({}, {:?}) [{}:{}]{}",
                e.method.as_str(),
                e.path_norm,
                e.framework,
                e.confidence,
                file,
                e.line,
                handler
            )
        })
        .collect();
    rows.sort();
    for r in &rows {
        println!("{r}");
    }
    eprintln!("-- {} endpoints", rows.len());
}
