use crate::parse::parse_flags;
use serde_json::json;

mod parse;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let flags = parse_flags(&args);
    let payload = json!({ "flags": flags });
    println!("{payload}");
}
