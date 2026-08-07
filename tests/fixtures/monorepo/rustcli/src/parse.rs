pub fn parse_flags(raw: &[String]) -> Vec<String> {
    raw.iter().filter(|a| a.starts_with("--")).cloned().collect()
}

pub fn strip_prefix(flag: &str) -> &str {
    flag.trim_start_matches('-')
}
