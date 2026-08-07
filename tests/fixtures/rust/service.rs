use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use crate::validators::{normalize, read_lines, validate_name};
use crate::models::*;

pub struct UserService {
    users: HashMap<String, u32>,
    next_id: u32,
}

pub trait Describe {
    fn describe(&self) -> String;

    fn summary(&self) -> String {
        let detail = self.describe();
        format!("[{}]", detail)
    }
}

impl UserService {
    pub fn new() -> Self {
        UserService {
            users: HashMap::new(),
            next_id: 1,
        }
    }

    pub fn register(&mut self, raw_name: &str) -> Option<u32> {
        let name = normalize(raw_name);
        if !validate_name(&name) {
            eprintln!("rejected {}", raw_name);
            return None;
        }
        let id = self.next_id;
        self.users.insert(name, id);
        self.next_id += 1;
        Some(id)
    }

    fn count(&self) -> usize {
        self.users.len()
    }
}

impl Describe for UserService {
    fn describe(&self) -> String {
        format!("{} users", self.count())
    }
}

pub fn create_service() -> UserService {
    UserService::new()
}

pub async fn import_names(path: &str) -> Vec<String> {
    let mut service = create_service();
    let mut labels = Vec::new();
    for line in read_lines(path) {
        if let Some(id) = service.register(&line) {
            labels.push(format!("{}:{}", id, line));
        }
    }
    labels
}

pub fn max_score<T: PartialOrd>(a: T, b: T) -> T {
    if a > b { a } else { b }
}

pub fn tally(scores: &[u32]) -> u32 {
    fn clamp(x: u32) -> u32 {
        if x > 100 { 100 } else { x }
    }
    let mut total = 0;
    for s in scores {
        total += clamp(*s);
    }
    match total {
        0 => println!("no scores"),
        t => println!("total {}", t),
    }
    total
}
