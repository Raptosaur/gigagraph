//! React Native bridge correlation over the bridge fixture.

use gigagraph::indexer::build_index;
use gigagraph::types::Confidence;
use std::path::Path;

#[test]
fn correlates_js_bridge_calls_with_native_methods() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bridge");
    let index = build_index(&root, true).expect("index build failed");
    let g = &index.graph;
    let b = &g.bridge;

    // Native side: both @ReactMethod functions per module, helper excluded.
    let native_methods: Vec<(&str, &str)> = b
        .natives
        .iter()
        .map(|n| (n.module.as_str(), n.method.as_str()))
        .collect();
    assert!(
        native_methods.contains(&("PaymentsModule", "charge")),
        "{native_methods:?}"
    );
    assert!(native_methods.contains(&("PaymentsModule", "refund")));
    assert!(native_methods.contains(&("AnalyticsModule", "track")));
    assert!(!native_methods.iter().any(|(_, m)| *m == "internalHelper"));

    // Explicit NativeModules.Payments.charge -> High match via alias.
    let charge_call = b
        .calls
        .iter()
        .find(|c| c.method == "charge")
        .expect("charge call detected");
    assert_eq!(charge_call.module.as_deref(), Some("Payments"));
    let m = b
        .matches
        .iter()
        .find(|(cid, _, _)| *cid == charge_call.id)
        .expect("charge matched");
    assert_eq!(b.natives[m.1 as usize].module, "PaymentsModule");
    assert_eq!(m.2, Confidence::High);

    // Destructured receiver (`Analytics.track`) matches heuristically.
    let track_call = b
        .calls
        .iter()
        .find(|c| c.method == "track")
        .expect("track call detected");
    assert_eq!(track_call.confidence, Confidence::Heuristic);
    assert!(
        b.matches
            .iter()
            .any(|(cid, nid, conf)| *cid == track_call.id
                && b.natives[*nid as usize].module == "AnalyticsModule"
                && *conf == Confidence::Heuristic)
    );

    // ObjC RCT_EXPORT_METHOD: module from file stem, selector from macro
    // args, matched to NativeModules.Location.locate via alias.
    let locate_native = b
        .natives
        .iter()
        .find(|n| n.method == "locate")
        .expect("objc native detected");
    assert_eq!(locate_native.module, "LocationModule");
    assert_eq!(locate_native.mechanism, "objc-export");
    let locate_call = b
        .calls
        .iter()
        .find(|c| c.method == "locate")
        .expect("locate call detected");
    assert!(
        b.matches.iter().any(|(cid, nid, _)| *cid == locate_call.id
            && b.natives[*nid as usize].id == locate_native.id)
    );

    // getName() alias beats the class-name heuristic: GeoModule declares
    // "Geo2", and NativeModules.Geo2.ping matches through it.
    let geo = b
        .natives
        .iter()
        .find(|n| n.method == "ping")
        .expect("geo native detected");
    assert_eq!(geo.module_alias, "Geo2");
    let ping = b
        .calls
        .iter()
        .find(|c| c.method == "ping")
        .expect("ping call detected");
    assert!(
        b.matches
            .iter()
            .any(|(cid, nid, _)| *cid == ping.id && *nid == geo.id)
    );
    // getName itself never becomes a bridge method.
    assert!(!b.natives.iter().any(|n| n.method == "getName"));

    // Swift @objc method in a React-importing file; plain helper excluded.
    let badge = b
        .natives
        .iter()
        .find(|n| n.method == "show")
        .expect("swift native detected");
    assert_eq!(badge.mechanism, "swift-objc");
    assert!(!b.natives.iter().any(|n| n.method == "helper"));
    let show = b
        .calls
        .iter()
        .find(|c| c.method == "show")
        .expect("show call detected");
    assert!(
        b.matches
            .iter()
            .any(|(cid, nid, _)| *cid == show.id && *nid == badge.id)
    );

    // ObjC RCT_EXTERN_METHOD (Swift-backed modules declare through it).
    let speed = b
        .natives
        .iter()
        .find(|n| n.method == "speed")
        .expect("extern native detected");
    assert_eq!(speed.mechanism, "objc-extern");

    // Unknown module stays unmatched.
    let gone = b
        .calls
        .iter()
        .find(|c| c.method == "vanish")
        .expect("vanish call detected");
    assert!(b.matches.iter().all(|(cid, _, _)| *cid != gone.id));

    // Bridge-called natives stay alive in the dead-code inventory: `charge`
    // appears at a JS call site, so called_names shields the Java method.
    assert!(g.called_names.contains("charge"));
    assert!(g.called_names.contains("track"));
}
