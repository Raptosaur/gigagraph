//! Debug helper: print a file's extracted RawCalls (name, receiver, bytes,
//! arg_lits) per function. Usage: cargo run --example calls -- path/to/file.ts

fn main() {
    let path = std::env::args().nth(1).expect("usage: calls <file>");
    let source = std::fs::read_to_string(&path).expect("read file");
    let ext = std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .expect("file needs an extension")
        .to_ascii_lowercase();
    let spec = gigagraph::lang::spec_for_ext(&ext).expect("unsupported extension");
    let ex = gigagraph::extract::extract(spec, &source).expect("extract failed");
    for f in &ex.functions {
        println!("fn {} (toplevel={})", f.name, f.is_toplevel);
        for c in &f.calls {
            println!(
                "  call {:24} recv={:?} assigned_to={:?} bytes={}..{} argc={}",
                c.name, c.receiver, c.assigned_to, c.start_byte, c.end_byte, c.arg_count
            );
            for l in &c.arg_lits {
                println!("      lit idx={} key={:?} {:?} {:?}", l.index, l.key, l.kind, l.text);
            }
        }
    }
}
