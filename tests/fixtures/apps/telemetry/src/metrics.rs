use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Sample {
    Counter(u64),
    Gauge(f64),
}

impl fmt::Display for Sample {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Sample::Counter(v) => write!(f, "{v}"),
            Sample::Gauge(v) => write!(f, "{v:.3}"),
        }
    }
}

#[derive(Default)]
pub struct Registry {
    samples: HashMap<String, Sample>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn incr(&mut self, name: &str, by: u64) {
        let entry = self.samples.entry(name.to_string()).or_insert(Sample::Counter(0));
        if let Sample::Counter(v) = entry {
            *v += by;
        }
    }

    pub fn set_gauge(&mut self, name: &str, value: f64) {
        self.samples.insert(name.to_string(), Sample::Gauge(value));
    }

    pub fn get(&self, name: &str) -> Option<&Sample> {
        self.samples.get(name)
    }

    pub fn render(&self) -> String {
        let mut names: Vec<&String> = self.samples.keys().collect();
        names.sort();
        names
            .iter()
            .map(|n| format!("{n} {}", self.samples[*n]))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub trait Sink {
    fn emit(&self, line: &str);

    fn emit_all(&self, lines: &[String]) {
        for line in lines {
            self.emit(line);
        }
    }
}

pub struct StdoutSink;

impl Sink for StdoutSink {
    fn emit(&self, line: &str) {
        println!("{line}");
    }
}

pub fn parse_line(line: &str) -> Option<(String, Sample)> {
    let (name, value) = line.split_once(' ')?;
    let sample = value
        .parse::<u64>()
        .map(Sample::Counter)
        .or_else(|_| value.parse::<f64>().map(Sample::Gauge))
        .ok()?;
    Some((name.to_string(), sample))
}
