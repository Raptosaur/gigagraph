use telemetry::metrics::{Registry, Sample, StdoutSink, parse_line};
use telemetry::pipeline::{Pipeline, run};

#[test]
fn parses_counter_lines() {
    assert_eq!(parse_line("hits 3"), Some(("hits".into(), Sample::Counter(3))));
}

#[test]
fn parses_gauge_lines() {
    assert!(matches!(parse_line("load 0.5"), Some((_, Sample::Gauge(_)))));
}

#[test]
fn rejects_malformed_lines() {
    assert!(parse_line("nope").is_none());
}

#[tokio::test]
async fn runs_end_to_end() {
    assert_eq!(run(StdoutSink, &["a 1", "b 2"]), 2);
}

#[test]
#[should_panic(expected = "missing")]
fn panics_on_missing_metric() {
    let r = Registry::new();
    r.get("missing").expect("missing");
}

fn make_pipeline() -> Pipeline<StdoutSink> {
    Pipeline::new(StdoutSink)
}
