use std::path::Path;

pub fn validate_name(name: &str) -> bool {
    !name.is_empty() && name.len() < 64
}

pub fn normalize(raw: &str) -> String {
    raw.trim().to_lowercase()
}

pub fn read_lines(path: &str) -> Vec<String> {
    let p = Path::new(path);
    std::fs::read_to_string(p)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_lowercases() {
        let n = normalize(" MiXeD ");
        assert_eq!(n, "mixed");
    }
}
