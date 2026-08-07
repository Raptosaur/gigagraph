//! Dead-code detection over the deadcode fixture: strong candidates,
//! demoted public functions, and every suppression class.

use gigagraph::api::AppState;
use serde_json::{Value, json};
use std::path::Path;

fn run_tool() -> Value {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/deadcode");
    let mut state = AppState::new(root);
    state
        .dispatch("unreferenced_functions", &json!({}))
        .expect("tool failed")
}

fn names(v: &Value, key: &str) -> Vec<String> {
    v[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn finds_dead_and_suppresses_live_and_framework_code() {
    let out = run_tool();
    let strong = names(&out, "strong_candidates");
    let demoted = names(&out, "public_unreferenced");

    // Truly unreferenced private functions surface.
    assert!(strong.contains(&"privateDead".to_string()), "{strong:?}");
    assert!(strong.contains(&"dead_helper".to_string()), "{strong:?}");

    // Referenced-by-value (callback) and dynamically-dispatched names live.
    for alive in ["usedAsCallback", "calledDynamically", "retry"] {
        assert!(!strong.contains(&alive.to_string()), "{alive} wrongly dead");
        assert!(
            !demoted.contains(&alive.to_string()),
            "{alive} wrongly demoted"
        );
    }

    // Exported-but-unreferenced demote instead of asserting dead.
    assert!(
        demoted.contains(&"exportedUnused".to_string()),
        "{demoted:?}"
    );
    assert!(!strong.contains(&"exportedUnused".to_string()));

    // Suppression classes: decorated (@functools.cache), JNI naming,
    // callback convention (onMessage).
    let excluded = &out["excluded_counts"];
    assert!(
        excluded["decorated"].as_u64().unwrap_or(0) >= 1,
        "{excluded}"
    );
    assert!(
        excluded["jni_export"].as_u64().unwrap_or(0) >= 1,
        "{excluded}"
    );
    assert!(
        excluded["callback_convention"].as_u64().unwrap_or(0) >= 1,
        "{excluded}"
    );

    // Nothing suppressed leaks into either list.
    for hidden in ["cached_lookup", "Java_com_acme_bridge_process", "onMessage"] {
        assert!(!strong.contains(&hidden.to_string()), "{hidden} leaked");
        assert!(!demoted.contains(&hidden.to_string()), "{hidden} leaked");
    }
}
