//! Debug helper: print a file's tree-sitter AST as an s-expression.
//! Usage: cargo run --example dump -- path/to/file.kt

use tree_sitter::{Node, Parser};

fn main() {
    let path = std::env::args().nth(1).expect("usage: dump <file>");
    let source = std::fs::read_to_string(&path).expect("read file");
    let ext = std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .expect("file needs an extension")
        .to_ascii_lowercase();
    let spec = gigagraph::lang::spec_for_ext(&ext).expect("unsupported extension");
    let mut parser = Parser::new();
    parser.set_language(&spec.language).unwrap();
    let tree = parser.parse(&source, None).expect("parse failed");
    print_node(tree.root_node(), &source, 0);
}

fn print_node(node: Node, source: &str, depth: usize) {
    if !node.is_named() {
        return;
    }
    let text = &source[node.start_byte()..node.end_byte()];
    let preview: String = text
        .chars()
        .take(40)
        .collect::<String>()
        .replace('\n', "\\n");
    let field = node
        .parent()
        .and_then(|p| {
            let mut c = p.walk();
            let mut found = None;
            for (i, child) in p.children(&mut c).enumerate() {
                if child.id() == node.id() {
                    found = p.field_name_for_child(i as u32);
                    break;
                }
            }
            found
        })
        .map(|f| format!("{f}: "))
        .unwrap_or_default();
    println!(
        "{}{}({})  `{}`",
        "  ".repeat(depth),
        field,
        node.kind(),
        preview
    );
    let mut c = node.walk();
    for child in node.children(&mut c) {
        print_node(child, source, depth + 1);
    }
}
