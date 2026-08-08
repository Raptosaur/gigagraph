use crate::metrics::{Registry, Sample, Sink, parse_line};

pub struct Pipeline<S: Sink> {
    registry: Registry,
    sink: S,
    dropped: usize,
}

impl<S: Sink> Pipeline<S> {
    pub fn new(sink: S) -> Self {
        Pipeline {
            registry: Registry::new(),
            sink,
            dropped: 0,
        }
    }

    pub fn ingest(&mut self, line: &str) -> bool {
        match parse_line(line) {
            Some((name, Sample::Counter(v))) => {
                self.registry.incr(&name, v);
                true
            }
            Some((name, Sample::Gauge(v))) => {
                self.registry.set_gauge(&name, v);
                true
            }
            None => {
                self.dropped += 1;
                false
            }
        }
    }

    pub fn flush(&self) {
        self.sink.emit(&self.registry.render());
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }
}

pub fn run<S: Sink>(sink: S, lines: &[&str]) -> usize {
    let mut pipeline = Pipeline::new(sink);
    let accepted = lines.iter().filter(|l| pipeline.ingest(l)).count();
    pipeline.flush();
    accepted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::StdoutSink;

    #[test]
    fn ingests_counters() {
        let mut p = Pipeline::new(StdoutSink);
        assert!(p.ingest("requests 4"));
    }

    #[test]
    fn counts_dropped_lines() {
        let mut p = Pipeline::new(StdoutSink);
        p.ingest("garbage");
        assert_eq!(p.dropped(), 1);
    }

    fn fixture() -> Pipeline<StdoutSink> {
        Pipeline::new(StdoutSink)
    }
}
