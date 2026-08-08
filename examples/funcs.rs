//! Debug helper: print a file's extracted functions — name, lines, containing
//! type, decorations — plus the string-literal arguments of every call, which
//! is what block-style test frameworks (`it("...")`, `TEST_CASE("...")`) carry
//! their case names in. Usage: cargo run --example funcs -- path/to/file.rb

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    assert!(!paths.is_empty(), "usage: funcs <file>...");
    for path in &paths {
        if paths.len() > 1 {
            println!("### {path}");
        }
        dump(path);
    }
}

fn dump(path: &str) {
    let source = std::fs::read_to_string(path).expect("read file");
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .expect("file needs an extension")
        .to_ascii_lowercase();
    let Some(spec) = gigagraph::lang::spec_for_ext(&ext) else {
        println!("(unsupported extension .{ext})");
        return;
    };
    let ex = gigagraph::extract::extract(spec, &source).expect("extract failed");
    println!("language={:?} package={:?}", ex.language, ex.package);
    for f in &ex.functions {
        let decos: Vec<&str> = f.decorations.iter().map(|d| d.name.as_str()).collect();
        println!(
            "fn {:30} lines={}-{} type={:?} decos={:?} sig={:?}",
            f.name, f.start_line, f.end_line, f.containing_type, decos, f.signature
        );
        for c in &f.calls {
            let strs: Vec<&str> = c.arg_lits.iter().map(|l| l.text.as_str()).collect();
            if !strs.is_empty() {
                println!("    call {:20} line={} args={:?}", c.name, c.line, strs);
            }
        }
    }
    for (ty, d) in &ex.type_decorations {
        println!("type-deco {ty} -> {}", d.name);
    }
    for (a, b) in &ex.hierarchy {
        println!("hierarchy {a} : {b}");
    }
}
