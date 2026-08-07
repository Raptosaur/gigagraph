//! Debug: list LSP-candidate call sites and raw tsserver answers for a root.
//! cargo run --example lspdebug -- <root>

use gigagraph::lsp::{DefQuery, LspProvider, TsServer};
use gigagraph::types::{Confidence, Resolution};
use std::time::{Duration, Instant};

fn main() {
    let root = std::path::PathBuf::from(std::env::args().nth(1).expect("root arg"))
        .canonicalize()
        .unwrap();
    let index = gigagraph::indexer::build_index(&root, false).unwrap();
    let g = &index.graph;
    let mut queries = Vec::new();
    for call in g.calls.iter() {
        let Resolution::Internal {
            callee,
            confidence,
            ambiguous_with,
        } = &call.resolution
        else {
            continue;
        };
        if *confidence == Confidence::High && ambiguous_with.is_empty() {
            continue;
        }
        let file = g.file_of(call.caller);
        if !matches!(
            file.language,
            gigagraph::types::Lang::JavaScript
                | gigagraph::types::Lang::TypeScript
                | gigagraph::types::Lang::Tsx
        ) {
            continue;
        }
        println!(
            "SITE {}:{}:{} name={} recv={:?} static={} amb={:?}",
            file.path,
            call.name_line,
            call.name_col,
            call.name,
            call.receiver,
            g.functions[*callee as usize].qualified_name,
            ambiguous_with
                .iter()
                .map(|&i| g.functions[i as usize].qualified_name.clone())
                .collect::<Vec<_>>()
        );
        queries.push(DefQuery {
            file: file.path.clone(),
            line: call.name_line,
            col: call.name_col,
        });
    }
    let mut ts = TsServer::detect(&root).expect("tsserver detected");
    let answers = ts
        .resolve(&root, &queries, Instant::now() + Duration::from_secs(30))
        .unwrap();
    for (q, a) in queries.iter().zip(&answers) {
        println!("ANSWER {}:{}:{} -> {:?}", q.file, q.line, q.col, a);
    }
}
