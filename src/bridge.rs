//! React Native bridge correlation: JS `NativeModules.X.y()` /
//! `TurboModuleRegistry` call sites matched to native implementations
//! (`@ReactMethod` on Java/Kotlin, `RCT_EXPORT_METHOD` on Objective-C).
//!
//! Cross-language dispatch is invisible to call resolution — this module
//! recovers it by name, with the same honest confidence labeling as endpoint
//! correlation. Kotlin/Swift coverage depends on their queries capturing
//! annotations/attributes as decorations; matching here is language-agnostic.

use crate::extract::{LitKind, RawCall, RawDecoration};
use crate::types::{Confidence, FileInfo, FunctionInfo, Lang};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// A natively-implemented bridge method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeNative {
    pub id: u32,
    /// Module name as the class declares it (e.g. `PaymentsModule`).
    pub module: String,
    /// JS-visible alias (class name with a trailing `Module` stripped).
    pub module_alias: String,
    pub method: String,
    pub function: u32,
    pub file_id: u32,
    pub line: u32,
    /// `react-method` (JVM annotation) or `objc-export` (RCT_EXPORT_METHOD).
    pub mechanism: String,
}

/// A JS-side call into the bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeCall {
    pub id: u32,
    /// Module named at the call site (`NativeModules.Payments.charge()` ->
    /// `Payments`); None when only the method name is known.
    pub module: Option<String>,
    pub method: String,
    pub caller: u32,
    pub file_id: u32,
    pub line: u32,
    /// High for explicit `NativeModules.`-rooted receivers; Heuristic for
    /// bare receivers that merely share a native module's name.
    pub confidence: Confidence,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BridgeIndex {
    pub natives: Vec<BridgeNative>,
    pub calls: Vec<BridgeCall>,
    /// (call id, native id, confidence)
    pub matches: Vec<(u32, u32, Confidence)>,
}

pub fn detect(
    files: &[FileInfo],
    functions: &[FunctionInfo],
    raw_calls: &[Vec<RawCall>],
    decorations: &[Vec<RawDecoration>],
    ret_strs: &[Vec<String>],
) -> BridgeIndex {
    let mut idx = BridgeIndex::default();

    // getName() overrides: (containing class) -> declared JS module name.
    // Also RCT_EXPORT_MODULE(JsName): (file id) -> declared name.
    let mut getname_by_class: rustc_hash::FxHashMap<&str, &str> = Default::default();
    let mut export_module_by_file: rustc_hash::FxHashMap<u32, String> = Default::default();
    for func in functions {
        if func.name == "getName" {
            if let (Some(t), Some(alias)) = (
                func.containing_type.as_deref(),
                ret_strs[func.id as usize].first(),
            ) {
                getname_by_class.insert(t, alias);
            }
        }
        for d in &decorations[func.id as usize] {
            if d.name == "RCT_EXPORT_MODULE" {
                // Bare `RCT_EXPORT_MODULE();` ERROR-recovery can drag stray
                // lowercase idents in as args — only an uppercase-initial
                // name is a plausible JS module alias.
                if let Some(alias) = d
                    .arg_lits
                    .iter()
                    .find(|l| l.text.chars().next().is_some_and(|c| c.is_uppercase()))
                {
                    export_module_by_file
                        .entry(func.file_id)
                        .or_insert_with(|| alias.text.clone());
                }
            }
        }
    }

    // ---- Native side ----
    for func in functions {
        let file = &files[func.file_id as usize];
        // Swift RN modules: @objc methods in a React-importing file.
        let swift_rn = file.language == Lang::Swift
            && file.imports.iter().any(|i| i.path == "React")
            && func.containing_type.is_some();
        for d in &decorations[func.id as usize] {
            let mechanism = match d.name.as_str() {
                "ReactMethod" if matches!(file.language, Lang::Java | Lang::Kotlin) => {
                    "react-method"
                }
                // Distinctive enough to need no language gate; flows in
                // automatically once the ObjC grammar lands.
                "RCT_EXPORT_METHOD" => "objc-export",
                "RCT_EXTERN_METHOD" => "objc-extern",
                "objc" if swift_rn => "swift-objc",
                _ => continue,
            };
            let mut module = func.containing_type.clone().unwrap_or_default();
            if module.is_empty() {
                // ObjC grammar exposes no @implementation name; RN convention
                // names the file after the module (PaymentsModule.m).
                module = Path::new(&file.path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
            }
            if module.is_empty() {
                continue;
            }
            // RCT_EXPORT_METHOD carries the selector in its arguments when
            // the grammar could not surface a named function.
            let method = if mechanism.starts_with("objc-") {
                d.arg_lits
                    .iter()
                    .find(|l| l.kind == LitKind::Ident)
                    .map(|l| l.text.split(':').next().unwrap_or(&l.text).to_string())
                    .unwrap_or_else(|| func.name.clone())
            } else {
                func.name.clone()
            };
            if method == "getName" {
                continue; // infrastructure, not a bridge method
            }
            // JS-visible alias precedence: getName() string, then
            // RCT_EXPORT_MODULE(JsName), then class name minus `Module`.
            let module_alias = getname_by_class
                .get(module.as_str())
                .map(|s| s.to_string())
                .or_else(|| export_module_by_file.get(&func.file_id).cloned())
                .unwrap_or_else(|| module.trim_end_matches("Module").to_string());
            idx.natives.push(BridgeNative {
                id: idx.natives.len() as u32,
                module_alias,
                module,
                method,
                function: func.id,
                file_id: func.file_id,
                line: func.start_line,
                mechanism: mechanism.to_string(),
            });
        }
    }

    let native_aliases: rustc_hash::FxHashSet<&str> = idx
        .natives
        .iter()
        .flat_map(|n| [n.module.as_str(), n.module_alias.as_str()])
        .collect();

    // ---- JS side ----
    for func in functions {
        let file = &files[func.file_id as usize];
        if !matches!(
            file.language,
            Lang::JavaScript | Lang::TypeScript | Lang::Tsx
        ) {
            continue;
        }
        for call in &raw_calls[func.id as usize] {
            let Some(recv) = call.receiver.as_deref() else {
                continue;
            };
            let (module, confidence) = if let Some(rest) = recv.strip_prefix("NativeModules.") {
                let module = rest.split('.').next().unwrap_or(rest).to_string();
                (Some(module), Confidence::High)
            } else if recv == "NativeModules" {
                // `NativeModules.Payments` as receiver of a property call
                // shape; call name is the module — skip, not a method call.
                continue;
            } else if native_aliases.contains(recv) {
                // Destructured module: `const { Payments } = NativeModules`.
                (Some(recv.to_string()), Confidence::Heuristic)
            } else {
                continue;
            };
            idx.calls.push(BridgeCall {
                id: idx.calls.len() as u32,
                module,
                method: call.name.clone(),
                caller: func.id,
                file_id: func.file_id,
                line: call.line,
                confidence,
            });
        }
    }

    // ---- Correlation ----
    for c in &idx.calls {
        let hits: Vec<&BridgeNative> = idx
            .natives
            .iter()
            .filter(|n| {
                n.method == c.method
                    && c.module
                        .as_deref()
                        .is_none_or(|m| m == n.module || m == n.module_alias)
            })
            .collect();
        let unique = hits.len() == 1;
        for n in hits {
            let conf = if unique && c.confidence == Confidence::High {
                Confidence::High
            } else {
                Confidence::Heuristic
            };
            idx.matches.push((c.id, n.id, conf));
        }
    }

    idx
}
