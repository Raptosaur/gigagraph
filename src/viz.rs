//! Interactive code map: functions are projected from their 256-dim
//! structural vectors to 3D via PCA (power iteration, deterministic), so
//! structurally similar code clusters together. API endpoints ride along as
//! first-class nodes (diamond glyphs anchored near their handlers), and
//! correlated client-call -> endpoint matches are drawn as arcs — highlighted
//! when the two ends live in different service groups of a monorepo. A second
//! coordinated view, the "API surface" panel, lists every endpoint (method,
//! path, framework, confidence, handler, top callees) grouped by service,
//! with a service-to-service flow matrix in multi-service repos. The output
//! is ONE self-contained HTML file (raw WebGL, no CDN, works offline).

use crate::indexer::{self, Index};
use crate::types::{Confidence, Resolution};
use crate::vector::DIMS;
use anyhow::{Context, Result};
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Payload cap: beyond this, only the most connected functions are kept.
const MAX_NODES: usize = 5000;
/// Per-node cap on direct caller / callee id lists.
const MAX_NEIGHBORS: usize = 20;
/// Similar functions listed per node.
const SIMILAR_K: usize = 5;
/// Payload cap on embedded endpoints: beyond this the most-called are kept.
const MAX_ENDPOINTS: usize = 1500;
/// Payload cap on embedded client-call -> endpoint match rows.
const MAX_MATCH_ROWS: usize = 8000;
/// URLs longer than this are truncated in the payload.
const MAX_URL_CHARS: usize = 120;

/// Dependency manifests that mark a directory as an independent
/// service/package for monorepo detection.
const MANIFESTS: &[&str] = &[
    "package.json",
    "pyproject.toml",
    "requirements.txt",
    "setup.py",
    "go.mod",
    "Cargo.toml",
    "composer.json",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "Gemfile",
    "mix.exs",
];

/// Top-level directories that are grouping conventions, not services: the
/// service is one level below (`apps/web`, `packages/ui`).
const GROUP_DIRS: &[&str] = &["apps", "packages", "services", "libs", "modules"];

/// Top-level directories that are ordinary single-project structure, not
/// service boundaries. They still show up as groups in the emitted data, but
/// they never count toward "this repo is a monorepo".
const NON_SERVICE_DIRS: &[&str] = &[
    "(root)", "src", "lib", "app", "config", "test", "tests", "spec", "specs",
    "docs", "doc", "scripts", "tools", "examples", "example", "vendor",
    "public", "static", "assets", "build", "dist", "include", "migrations",
];

/// Loads (or builds) the index for `root`, renders the map, writes it to
/// `out` (default `<root>/.gigagraph/map.html`) and returns the absolute
/// path of the written file.
pub fn write_map(root: &Path, out: Option<&Path>) -> Result<PathBuf> {
    let index = match indexer::load_index(root) {
        Some(i) => i,
        None => indexer::build_index(root, false)?,
    };
    let out_path = match out {
        Some(p) => p.to_path_buf(),
        None => root.join(".gigagraph").join("map.html"),
    };
    write_html_for_index(&index, &out_path)?;
    out_path
        .canonicalize()
        .with_context(|| format!("cannot resolve output path {}", out_path.display()))
}

/// Renders the map for an already-loaded index and writes it to `out`.
pub fn write_html_for_index(index: &Index, out: &Path) -> Result<()> {
    if let Some(dir) = out.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("cannot create {}", dir.display()))?;
        }
    }
    std::fs::write(out, generate_html(index))
        .with_context(|| format!("cannot write {}", out.display()))?;
    Ok(())
}

/// Opens a file in the platform's default browser.
pub fn open_in_browser(path: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(path);
        c
    };
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]).arg(path);
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(path);
        c
    };
    cmd.spawn()
        .with_context(|| format!("cannot open {}", path.display()))?;
    Ok(())
}

/// Renders the complete self-contained HTML document. Deterministic for a
/// given index (no timestamps, seeded projection, sorted collections).
pub fn generate_html(index: &Index) -> String {
    let payload = build_payload(index);
    // `</` never appears in JSON structure, only (possibly) inside strings;
    // escaping it keeps a literal `</script>` in a name from closing the
    // data block. `<\/` is still valid JSON for the same string.
    let data = serde_json::to_string(&payload)
        .unwrap_or_else(|_| "{\"meta\":{},\"nodes\":[]}".to_string())
        .replace("</", "<\\/");
    HTML_TEMPLATE.replace("__GRAPH_DATA__", &data)
}

/// Service group of a repo-relative `/`-separated path: the top-level
/// directory, or the second level under grouping conventions (`apps/web`).
/// Files sitting at the repo root belong to `(root)`.
pub fn service_of(path: &str) -> String {
    let mut it = path.split('/');
    let first = match it.next() {
        Some(f) if !f.is_empty() => f,
        _ => return "(root)".to_string(),
    };
    let second = it.next();
    let third = it.next();
    match (second, third) {
        // apps/web/src/x.ts -> apps/web (needs a third segment so that a
        // FILE directly under apps/ still groups as `apps`).
        (Some(s), Some(_)) if GROUP_DIRS.contains(&first) => format!("{first}/{s}"),
        (Some(_), _) => first.to_string(),
        (None, _) => "(root)".to_string(),
    }
}

fn conf_str(c: Confidence) -> &'static str {
    match c {
        Confidence::High => "high",
        Confidence::Heuristic => "heuristic",
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}\u{2026}")
    }
}

/// Builds the JSON payload embedded in the page: metadata (incl. service
/// grouping), one node per (non-synthetic) function with 3D coordinates and
/// capped neighbor lists, endpoint nodes, and client-call -> endpoint match
/// rows.
fn build_payload(index: &Index) -> Value {
    let g = &index.graph;

    // ---- Service grouping (deterministic: BTreeMap keyed by name) ----
    #[derive(Default)]
    struct SvcStat {
        files: u32,
        functions: u32,
        endpoints: u32,
    }
    let mut svc_stats: BTreeMap<String, SvcStat> = BTreeMap::new();
    for f in &g.files {
        svc_stats.entry(service_of(&f.path)).or_default().files += 1;
    }
    for f in g.functions.iter().filter(|f| !f.is_toplevel) {
        let path = &g.files[f.file_id as usize].path;
        svc_stats.entry(service_of(path)).or_default().functions += 1;
    }
    for e in &g.endpoints.endpoints {
        let path = &g.files[e.file_id as usize].path;
        svc_stats.entry(service_of(path)).or_default().endpoints += 1;
    }
    let svc_names: Vec<&String> = svc_stats.keys().collect();
    let svc_index: FxHashMap<&str, usize> = svc_names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();
    let svc_of_file = |file_id: u32| -> usize {
        let path = &g.files[file_id as usize].path;
        svc_index[service_of(path).as_str()]
    };

    // Monorepo detection: >=2 candidate groups publish endpoints, or >=2
    // carry their own dependency manifest. Ordinary project-structure dirs
    // (src/, config/, tests/, the root itself) never count as services —
    // when grouping is ambiguous we fall back to the single-service view
    // rather than inventing services.
    let is_service_dir = |name: &str| !NON_SERVICE_DIRS.contains(&name);
    let ep_groups = svc_stats
        .iter()
        .filter(|(name, s)| s.endpoints > 0 && is_service_dir(name))
        .count();
    let root_path = Path::new(&g.root);
    let manifest_groups = svc_stats
        .keys()
        .filter(|name| {
            is_service_dir(name)
                && MANIFESTS
                    .iter()
                    .any(|m| root_path.join(name.as_str()).join(m).is_file())
        })
        .count();
    let multi = svc_stats.len() >= 2 && (ep_groups >= 2 || manifest_groups >= 2);

    // ---- Function nodes (real functions only; `(toplevel)` excluded) ----
    let mut kept: Vec<u32> = g
        .functions
        .iter()
        .filter(|f| !f.is_toplevel)
        .map(|f| f.id)
        .collect();
    let total = kept.len();
    let capped = total > MAX_NODES;
    if capped {
        // Keep the most connected functions (callers + callees), ties by id.
        let degree = |id: u32| -> usize {
            let out = g
                .calls_by_caller
                .get(id as usize)
                .map_or(0, |sites| sites.len());
            let inn = g.callers_of.get(&id).map_or(0, |sites| sites.len());
            out + inn
        };
        kept.sort_by_key(|&id| (std::cmp::Reverse(degree(id)), id));
        kept.truncate(MAX_NODES);
        kept.sort_unstable();
    }
    let kept_set: FxHashSet<u32> = kept.iter().copied().collect();

    // Endpoint handler functions get the visual ring.
    let endpoint_handlers: FxHashSet<u32> = g
        .endpoints
        .endpoints
        .iter()
        .filter_map(|e| e.handler)
        .collect();

    // ---- 3D projection ----
    const ZERO_ROW: [f32; DIMS] = [0.0; DIMS];
    let rows: Vec<&[f32]> = kept
        .iter()
        .map(|&id| index.vectors.vector_of(id).unwrap_or(&ZERO_ROW))
        .collect();
    let mut coords = pca_3d(&rows);

    // Normalize to a stable world scale and add a tiny deterministic jitter
    // so functions with identical vectors do not stack on one pixel. When the
    // projection is fully degenerate (< 4 functions or zero variance) the
    // jitter alone spreads the points.
    let max_abs = coords
        .iter()
        .flat_map(|c| c.iter())
        .fold(0.0f64, |m, v| m.max(v.abs()));
    let (scale, jitter) = if max_abs > 1e-9 {
        (100.0 / max_abs, 0.7)
    } else {
        (0.0, 25.0)
    };
    let mut rng = Lcg(0xC0DE_6A17_5EED_0001);
    for c in coords.iter_mut() {
        for v in c.iter_mut() {
            let jittered = *v * scale + rng.next_f64() * 2.0 * jitter;
            *v = if jittered.is_finite() {
                (jittered * 1000.0).round() / 1000.0
            } else {
                0.0
            };
        }
    }

    // ---- Per-node payload ----
    let mut languages: BTreeMap<&'static str, u32> = BTreeMap::new();
    let mut nodes: Vec<Value> = Vec::with_capacity(kept.len());
    for (i, &id) in kept.iter().enumerate() {
        let f = &g.functions[id as usize];
        *languages.entry(f.language.name()).or_insert(0) += 1;

        // Direct callees (deduped, capped, within the kept set).
        let mut out_ids: Vec<u32> = Vec::new();
        if let Some(sites) = g.calls_by_caller.get(id as usize) {
            for &ci in sites {
                if let Resolution::Internal { callee, .. } = &g.calls[ci as usize].resolution {
                    let c = *callee;
                    if c != id && kept_set.contains(&c) && !out_ids.contains(&c) {
                        out_ids.push(c);
                        if out_ids.len() >= MAX_NEIGHBORS {
                            break;
                        }
                    }
                }
            }
        }
        // Direct callers (deduped, capped, within the kept set).
        let mut in_ids: Vec<u32> = Vec::new();
        if let Some(sites) = g.callers_of.get(&id) {
            for &ci in sites {
                let c = g.calls[ci as usize].caller;
                if c != id && kept_set.contains(&c) && !in_ids.contains(&c) {
                    in_ids.push(c);
                    if in_ids.len() >= MAX_NEIGHBORS {
                        break;
                    }
                }
            }
        }
        // Top similar functions via the existing vector index (blended
        // structural + semantic, same scoring as find_similar).
        let sim: Vec<u32> = index
            .vectors
            .vector_of(id)
            .map(|v| {
                let sem = index.vectors.sem_vector_of(id).unwrap_or(&[]);
                index.vectors.top_k(v, sem, SIMILAR_K + 15, Some(id))
            })
            .unwrap_or_default()
            .into_iter()
            .filter(|&(sid, _)| kept_set.contains(&sid))
            .take(SIMILAR_K)
            .map(|(sid, _)| sid)
            .collect();

        let caller_sites = g.callers_of.get(&id).map_or(0, |v| v.len());
        let [x, y, z] = coords[i];
        nodes.push(json!({
            "id": id,
            "name": f.name,
            "qn": f.qualified_name,
            "file": g.file_of(id).path,
            "line": f.start_line,
            "lang": f.language.name(),
            "sig": f.signature,
            "x": x, "y": y, "z": z,
            "sz": caller_sites,
            "sv": svc_of_file(f.file_id),
            "ep": endpoint_handlers.contains(&id),
            "sim": sim,
            "out": out_ids,
            "in": in_ids,
        }));
    }

    // ---- Endpoint nodes ----
    // Matched-client-call count per endpoint (drives size + cap ranking).
    let mut match_count: FxHashMap<u32, u32> = FxHashMap::default();
    for &(_, eid, _) in &g.endpoints.matches {
        *match_count.entry(eid).or_insert(0) += 1;
    }
    let eps_total = g.endpoints.endpoints.len();
    let mut ep_kept: Vec<&crate::endpoints::Endpoint> = g.endpoints.endpoints.iter().collect();
    ep_kept.sort_by_key(|e| e.id);
    let eps_capped = eps_total > MAX_ENDPOINTS;
    if eps_capped {
        ep_kept.sort_by_key(|e| {
            (
                std::cmp::Reverse(match_count.get(&e.id).copied().unwrap_or(0)),
                e.id,
            )
        });
        ep_kept.truncate(MAX_ENDPOINTS);
        ep_kept.sort_by_key(|e| e.id);
    }
    let ep_kept_ids: FxHashSet<u32> = ep_kept.iter().map(|e| e.id).collect();

    // Positions: anchored beside the handler when it is in the map, else at
    // the service's centroid, else at the world origin — always with a
    // deterministic per-endpoint offset so endpoints never stack.
    let pos_of_fn = |id: u32| -> Option<[f64; 3]> {
        kept.binary_search(&id).ok().map(|i| coords[i])
    };
    let mut svc_centroid: Vec<([f64; 3], u32)> = vec![([0.0; 3], 0); svc_stats.len()];
    for (i, &id) in kept.iter().enumerate() {
        let sv = svc_of_file(g.functions[id as usize].file_id);
        let (c, n) = &mut svc_centroid[sv];
        for (a, b) in c.iter_mut().zip(coords[i]) {
            *a += b;
        }
        *n += 1;
    }
    let round3 = |v: f64| {
        if v.is_finite() {
            (v * 1000.0).round() / 1000.0
        } else {
            0.0
        }
    };
    let mut ep_rows: Vec<Value> = Vec::with_capacity(ep_kept.len());
    for e in &ep_kept {
        let sv = svc_of_file(e.file_id);
        let mut off = Lcg(0xE9D0_0AB5_u64.wrapping_add((e.id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)));
        let handler_kept = e.handler.filter(|h| kept_set.contains(h));
        let [px, py, pz] = match handler_kept.and_then(pos_of_fn) {
            Some([hx, hy, hz]) => {
                let r = 3.0;
                [hx + off.next_f64() * 2.0 * r, hy + off.next_f64() * 2.0 * r + 1.5, hz + off.next_f64() * 2.0 * r]
            }
            None => {
                let (c, n) = svc_centroid[sv];
                let r = 14.0;
                if n > 0 {
                    let n = n as f64;
                    [c[0] / n + off.next_f64() * 2.0 * r, c[1] / n + off.next_f64() * 2.0 * r, c[2] / n + off.next_f64() * 2.0 * r]
                } else {
                    [off.next_f64() * 2.0 * r, off.next_f64() * 2.0 * r, off.next_f64() * 2.0 * r]
                }
            }
        };
        let mut row = json!({
            "id": e.id,
            "kind": e.kind.as_str(),
            "method": e.method.as_str(),
            "path": e.path_raw,
            "framework": e.framework,
            "conf": conf_str(e.confidence),
            "file": g.files[e.file_id as usize].path,
            "line": e.line,
            "sv": sv,
            "m": match_count.get(&e.id).copied().unwrap_or(0),
            "x": round3(px), "y": round3(py), "z": round3(pz),
        });
        match (e.handler, handler_kept) {
            (_, Some(h)) => {
                row["h"] = json!(h);
            }
            (Some(h), None) => {
                // Handler exists but fell to the node cap (or is synthetic):
                // embed just enough for the API panel to stay informative.
                let hf = &g.functions[h as usize];
                row["hn"] = json!(hf.name);
                row["hf"] = json!(g.files[hf.file_id as usize].path);
                row["hl"] = json!(hf.start_line);
            }
            (None, None) => {}
        }
        ep_rows.push(row);
    }

    // ---- Client-call -> endpoint match rows ----
    let call_by_id: FxHashMap<u32, &crate::endpoints::ClientCall> =
        g.endpoints.client_calls.iter().map(|c| (c.id, c)).collect();
    let mut matches_sorted: Vec<&(u32, u32, Confidence)> = g.endpoints.matches.iter().collect();
    matches_sorted.sort_by_key(|(cid, eid, _)| (*cid, *eid));
    let mut cc_rows: Vec<Value> = Vec::new();
    for &&(cid, eid, conf) in matches_sorted.iter() {
        if cc_rows.len() >= MAX_MATCH_ROWS {
            break;
        }
        if !ep_kept_ids.contains(&eid) {
            continue;
        }
        let Some(c) = call_by_id.get(&cid) else {
            continue;
        };
        let caller = &g.functions[c.caller as usize];
        let mut row = json!({
            "to": eid,
            "fsv": svc_of_file(c.file_id),
            "conf": conf_str(conf),
            "kind": c.kind.as_str(),
            "method": c.method.as_str(),
            "url": truncate_chars(&c.url_raw, MAX_URL_CHARS),
            "lib": c.library,
            "file": g.files[c.file_id as usize].path,
            "line": c.line,
        });
        if !caller.is_toplevel && kept_set.contains(&c.caller) {
            row["from"] = json!(c.caller);
        } else if !caller.is_toplevel {
            row["fname"] = json!(caller.name);
        }
        cc_rows.push(row);
    }

    let services: Vec<Value> = svc_stats
        .iter()
        .map(|(name, s)| {
            json!({
                "name": name,
                "files": s.files,
                "functions": s.functions,
                "endpoints": s.endpoints,
            })
        })
        .collect();

    // Default view: repos with no endpoints open straight on the map;
    // single-service repos open on the API surface list; multi-service repos
    // open on the flow (map) view with arcs.
    let mode = if ep_rows.is_empty() {
        "map"
    } else if multi {
        "flow"
    } else {
        "api"
    };

    let root_name = Path::new(&g.root)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| g.root.clone());
    json!({
        "meta": {
            "root": root_name,
            "total": total,
            "shown": kept.len(),
            "capped": capped,
            "endpoints": endpoint_handlers.iter().filter(|id| kept_set.contains(id)).count(),
            "languages": languages,
            "services": services,
            "multi": multi,
            "mode": mode,
            "eps_total": eps_total,
            "eps_shown": ep_rows.len(),
            "eps_capped": eps_capped,
            "cc_total": g.endpoints.client_calls.len(),
            "matches": cc_rows.len(),
        },
        "nodes": nodes,
        "eps": ep_rows,
        "cc": cc_rows,
    })
}

/// Deterministic LCG (no external rand dependency); yields values in
/// [-0.5, 0.5).
struct Lcg(u64);

impl Lcg {
    fn next_f64(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64 / (1u64 << 53) as f64) - 0.5
    }
}

/// Projects rows onto their top-3 principal components: center, accumulate
/// the covariance matrix, then power iteration with deflation. Deterministic
/// (seeded init); degenerate inputs (zero variance, tiny n) yield zeros
/// rather than NaN.
fn pca_3d(rows: &[&[f32]]) -> Vec<[f64; 3]> {
    let n = rows.len();
    if n == 0 {
        return Vec::new();
    }
    let d = rows[0].len();

    let mut mean = vec![0.0f64; d];
    for row in rows {
        for (m, &v) in mean.iter_mut().zip(row.iter()) {
            *m += v as f64;
        }
    }
    for m in &mut mean {
        *m /= n as f64;
    }

    // Covariance (unnormalized — scaling does not change the eigenvectors).
    let mut cov = vec![0.0f64; d * d];
    let mut centered = vec![0.0f64; d];
    for row in rows {
        for (c, (&v, m)) in centered.iter_mut().zip(row.iter().zip(&mean)) {
            *c = v as f64 - m;
        }
        for i in 0..d {
            let xi = centered[i];
            if xi == 0.0 {
                continue;
            }
            let out = &mut cov[i * d..(i + 1) * d];
            for (o, &xj) in out.iter_mut().zip(&centered) {
                *o += xi * xj;
            }
        }
    }

    let l2 = |v: &[f64]| v.iter().map(|x| x * x).sum::<f64>().sqrt();
    let mut rng = Lcg(0x9E37_79B9_7F4A_7C15);
    let mut components: Vec<Vec<f64>> = Vec::with_capacity(3);
    for _ in 0..3 {
        let mut v: Vec<f64> = (0..d).map(|_| rng.next_f64()).collect();
        let norm = l2(&v);
        if norm > 0.0 {
            v.iter_mut().for_each(|x| *x /= norm);
        } else {
            v[0] = 1.0;
        }
        let mut lambda = 0.0f64;
        for _ in 0..128 {
            let mut w = vec![0.0f64; d];
            for (i, wi) in w.iter_mut().enumerate() {
                let row = &cov[i * d..(i + 1) * d];
                *wi = row.iter().zip(&v).map(|(a, b)| a * b).sum();
            }
            let norm = l2(&w);
            if norm < 1e-12 {
                // Zero-variance residual: no more structure to extract.
                lambda = 0.0;
                v.iter_mut().for_each(|x| *x = 0.0);
                break;
            }
            lambda = norm;
            w.iter_mut().for_each(|x| *x /= norm);
            let dot: f64 = w.iter().zip(&v).map(|(a, b)| a * b).sum();
            v = w;
            if (1.0 - dot.abs()) < 1e-10 {
                break;
            }
        }
        // Deflate: cov -= lambda * v * v^T.
        if lambda > 0.0 {
            for i in 0..d {
                let vi = lambda * v[i];
                if vi == 0.0 {
                    continue;
                }
                let out = &mut cov[i * d..(i + 1) * d];
                for (o, &vj) in out.iter_mut().zip(&v) {
                    *o -= vi * vj;
                }
            }
        }
        components.push(v);
    }

    rows.iter()
        .map(|row| {
            let mut p = [0.0f64; 3];
            for (k, comp) in components.iter().enumerate() {
                p[k] = comp
                    .iter()
                    .zip(row.iter().zip(&mean))
                    .map(|(c, (&v, m))| c * (v as f64 - m))
                    .sum();
                if !p[k].is_finite() {
                    p[k] = 0.0;
                }
            }
            p
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The page. One file, no network: raw WebGL point cloud + orbit camera, plus
// an HTML "API surface" panel over it. Language colors: the 8 most common
// languages use a CVD-validated dark categorical order; endpoint kinds get
// their own fixed hues and a diamond glyph; every color is also named in the
// legend / tooltip / detail panel, so identity never rides on hue alone.
// ---------------------------------------------------------------------------

const HTML_TEMPLATE: &str = r####"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>code map</title>
<style>
:root{
  --bg:#0f1115; --panel:rgba(18,20,26,.92); --line:rgba(255,255,255,.09);
  --ink:#ffffff; --ink2:#c3c2b7; --muted:#898781;
  --accent:#3987e5; --in:#f78fb8; --out:#7fd4f0; --xs:#f7c948;
}
*{box-sizing:border-box;margin:0;padding:0}
html,body{height:100%;overflow:hidden;background:var(--bg);color:var(--ink);
  font:13px/1.45 system-ui,-apple-system,"Segoe UI",sans-serif}
#gl{position:fixed;top:0;left:0;width:100vw;height:100vh;display:block;cursor:grab;touch-action:none}
.card{background:var(--panel);border:1px solid var(--line);border-radius:10px;
  padding:10px 12px;backdrop-filter:blur(8px)}
#side{position:fixed;z-index:3;left:14px;top:14px;width:276px;
  max-height:calc(100vh - 28px);display:flex;flex-direction:column;gap:10px}
h1{font-size:14px;font-weight:650;letter-spacing:.2px}
#stats{color:var(--ink2);font-size:11.5px;margin-top:2px}
#capnote{display:none;color:var(--ink2);font-size:11px;margin-top:6px;
  padding:5px 8px;border:1px solid var(--line);border-radius:6px;background:rgba(201,133,0,.10)}
#search{width:100%;background:rgba(255,255,255,.05);border:1px solid var(--line);
  border-radius:7px;color:var(--ink);padding:6px 9px;font-size:12.5px;outline:none}
#search:focus{border-color:rgba(57,135,229,.65)}
#results{margin-top:6px;max-height:180px;overflow:auto;display:none}
.row{display:flex;justify-content:space-between;gap:8px;align-items:baseline;
  padding:3px 5px;border-radius:5px;cursor:pointer;white-space:nowrap;overflow:hidden}
.row:hover{background:rgba(255,255,255,.06)}
.row .f{color:var(--muted);font-size:10.5px;overflow:hidden;text-overflow:ellipsis}
.lbl{font-size:10px;text-transform:uppercase;letter-spacing:.7px;color:var(--muted);margin-bottom:5px}
.seg{display:flex;gap:3px;flex-wrap:wrap}
.seg button{background:transparent;border:1px solid var(--line);color:var(--ink2);
  padding:3px 9px;border-radius:6px;font-size:11px;cursor:pointer;font-family:inherit}
.seg button:hover{color:var(--ink)}
.seg button.on{background:rgba(57,135,229,.20);color:#fff;border-color:rgba(57,135,229,.6)}
.ctl{display:flex;align-items:center;gap:8px;margin-top:7px}
.ctl:first-child{margin-top:0}
.ctl .k{width:44px;font-size:10px;text-transform:uppercase;letter-spacing:.7px;color:var(--muted)}
#legend{overflow:auto;min-height:40px}
#legendrows{max-height:200px;overflow:auto}
.lg{display:flex;align-items:center;gap:7px;padding:2.5px 5px;border-radius:5px;cursor:pointer}
.lg:hover{background:rgba(255,255,255,.06)}
.lg.off{opacity:.32}
.dot{width:9px;height:9px;border-radius:50%;flex:none}
.dia{width:9px;height:9px;flex:none;transform:rotate(45deg);border-radius:2px}
.lg .n{color:var(--ink2);flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.lg .c{color:var(--muted);font-size:11px;font-variant-numeric:tabular-nums}
.legfoot{margin-top:7px;padding-top:7px;border-top:1px solid var(--line);
  color:var(--muted);font-size:11px;display:flex;align-items:center;gap:7px}
.ring{width:11px;height:11px;border-radius:50%;border:2px solid #fff;
  background:rgba(255,255,255,.12);flex:none}
#detail{position:fixed;z-index:3;right:14px;top:14px;width:330px;
  max-height:calc(100vh - 28px);overflow:auto;display:none}
#detail h2{font-size:14px;font-weight:650;word-break:break-all;padding-right:20px}
#detail .qn{color:var(--muted);font-size:11px;word-break:break-all;margin-top:2px}
#detail .meta{display:flex;align-items:center;gap:7px;margin-top:7px;color:var(--ink2);
  font-size:11.5px;flex-wrap:wrap}
#detail code{display:block;margin-top:8px;padding:7px 9px;background:rgba(255,255,255,.05);
  border:1px solid var(--line);border-radius:6px;color:var(--ink2);
  font:11px/1.5 ui-monospace,SFMono-Regular,Menlo,monospace;white-space:pre-wrap;word-break:break-all}
#detail .sect{margin-top:10px}
#close{position:absolute;top:8px;right:10px;background:none;border:none;color:var(--muted);
  font-size:16px;cursor:pointer;line-height:1}
#close:hover{color:var(--ink)}
.item{display:flex;gap:7px;align-items:baseline;padding:2.5px 4px;border-radius:5px;
  cursor:pointer;white-space:nowrap;overflow:hidden}
.item:hover{background:rgba(255,255,255,.06)}
.item .f{color:var(--muted);font-size:10.5px;overflow:hidden;text-overflow:ellipsis}
.plain{cursor:default}
.plain:hover{background:none}
#tip{position:fixed;z-index:4;pointer-events:none;display:none;max-width:380px;
  background:var(--panel);border:1px solid var(--line);border-radius:8px;
  padding:8px 10px;backdrop-filter:blur(8px)}
#tip .t{font-weight:650;font-size:12.5px;word-break:break-all}
#tip .s{margin-top:4px;color:var(--ink2);font:10.5px/1.4 ui-monospace,SFMono-Regular,Menlo,monospace;
  word-break:break-all}
#tip .m{margin-top:4px;color:var(--muted);font-size:11px;word-break:break-all}
#tip .sim{margin-top:4px;color:var(--ink2);font-size:11px;word-break:break-all}
#help{position:fixed;z-index:2;bottom:12px;left:50%;transform:translateX(-50%);
  color:var(--muted);font-size:11px;text-align:center;pointer-events:none}
#fallback,#empty{position:fixed;z-index:5;inset:0;display:none;place-items:center;
  background:var(--bg);color:var(--ink2);text-align:center;padding:30px}
.ns{position:fixed;z-index:6;inset:0;display:grid;place-items:center;
  background:#0f1115;color:#c3c2b7;text-align:center;padding:30px;font-size:14px}

/* ---- API surface panel ---- */
#api{position:fixed;z-index:5;inset:0;display:none;overflow:auto;
  background:rgba(13,15,19,.97);padding:18px clamp(14px,4vw,48px) 40px}
.apihead{display:flex;align-items:baseline;gap:14px;flex-wrap:wrap;margin-bottom:14px}
.apihead h1{font-size:16px}
.apihead .sub{color:var(--muted);font-size:12px}
.apihead button{background:transparent;border:1px solid var(--line);color:var(--ink2);
  padding:5px 12px;border-radius:7px;font-size:12px;cursor:pointer;font-family:inherit}
.apihead button:hover{color:var(--ink);border-color:rgba(57,135,229,.6)}
.apibar{display:flex;gap:8px;flex-wrap:wrap;margin-bottom:14px}
.apibar input,.apibar select{background:rgba(255,255,255,.05);border:1px solid var(--line);
  border-radius:7px;color:var(--ink);padding:6px 9px;font-size:12.5px;outline:none;font-family:inherit}
.apibar input{flex:1;min-width:180px}
.apibar input:focus{border-color:rgba(57,135,229,.65)}
.apibar select option{background:#171a21}
#matrixwrap{margin-bottom:16px;overflow-x:auto}
#matrixwrap .lbl{margin-bottom:8px}
table.mx{border-collapse:collapse;font-size:12px}
table.mx th,table.mx td{border:1px solid var(--line);padding:5px 10px;text-align:right;
  font-variant-numeric:tabular-nums}
table.mx th{color:var(--muted);font-weight:500;text-align:left;white-space:nowrap}
table.mx td.n{cursor:pointer;color:var(--ink2)}
table.mx td.n:hover{outline:1px solid rgba(57,135,229,.7)}
table.mx td.sel{outline:2px solid var(--accent);color:#fff}
table.mx td.zero{color:rgba(255,255,255,.16);cursor:default}
.mxnote{color:var(--muted);font-size:11px;margin-top:6px}
.svcgrp{margin-bottom:18px}
.svch{display:flex;align-items:baseline;gap:10px;margin-bottom:6px}
.svch .nm{font-size:13px;font-weight:650}
.svch .ct{color:var(--muted);font-size:11.5px}
.eprow{display:grid;grid-template-columns:64px minmax(140px,1.2fr) minmax(150px,1fr) minmax(160px,1.4fr) 70px;
  gap:10px;align-items:baseline;padding:6px 9px;border:1px solid transparent;
  border-radius:7px;cursor:pointer}
.eprow:hover{background:rgba(255,255,255,.05);border-color:var(--line)}
.eprow .pth{font:12px ui-monospace,SFMono-Regular,Menlo,monospace;word-break:break-all;color:var(--ink)}
.eprow .fw{color:var(--muted);font-size:11px}
.eprow .hd{font-size:11.5px;color:var(--ink2);overflow:hidden}
.eprow .hd .f{color:var(--muted);font-size:10.5px}
.eprow .hd .callees{color:var(--out);font-size:10.5px;display:block;overflow:hidden;
  text-overflow:ellipsis;white-space:nowrap}
.eprow .in{color:var(--muted);font-size:11px;text-align:right;font-variant-numeric:tabular-nums}
.eprow .in b{color:var(--xs);font-weight:600}
.mth{font:10.5px/1 ui-monospace,SFMono-Regular,Menlo,monospace;padding:3px 0;
  border-radius:5px;text-align:center;border:1px solid var(--line);color:var(--ink2)}
.mth.GET{color:#7fd4f0;border-color:rgba(127,212,240,.35)}
.mth.POST{color:#8ee59a;border-color:rgba(142,229,154,.35)}
.mth.PUT{color:#e5c86a;border-color:rgba(229,200,106,.35)}
.mth.PATCH{color:#e5c86a;border-color:rgba(229,200,106,.35)}
.mth.DELETE{color:#f28b82;border-color:rgba(242,139,130,.35)}
.mth.ANY{color:var(--muted)}
.confb{font-size:10px;color:var(--muted)}
.confb.high{color:#8ee59a}
#apiempty{color:var(--ink2);padding:30px 0;text-align:center}
@media (max-width:900px){.eprow{grid-template-columns:56px 1fr;grid-auto-rows:auto}
  .eprow .in{text-align:left}}
@media (max-width:760px){#side{width:220px}#detail{width:260px}}
</style>
</head>
<body>
<canvas id="gl"></canvas>
<noscript><div class="ns">This map is rendered with JavaScript (fully offline
— the file is self-contained and makes no network requests). Enable
JavaScript for this local file to view it.</div></noscript>
<div id="fallback"><div>This browser has no WebGL support, which the 3D map
needs.<br>The file is self-contained — try opening it in another browser.</div></div>
<div id="empty"><div>No functions in the index.<br>Run
<b>gigagraph index</b> in a source tree first.</div></div>

<div id="side">
  <div class="card">
    <h1 id="title">code map</h1>
    <div id="stats"></div>
    <div id="capnote"></div>
  </div>
  <div class="card">
    <input id="search" type="text" placeholder="search functions &amp; endpoints&hellip;" autocomplete="off" spellcheck="false">
    <div id="results"></div>
  </div>
  <div class="card">
    <div class="ctl"><span class="k">view</span>
      <div class="seg" id="viewseg">
        <button data-v="map" class="on">3d map</button>
        <button data-v="api">api surface</button>
      </div>
    </div>
    <div class="ctl"><span class="k">color</span>
      <div class="seg" id="colorseg">
        <button data-v="lang" class="on">language</button>
        <button data-v="dir">directory</button>
      </div>
    </div>
    <div class="ctl"><span class="k">edges</span>
      <div class="seg" id="edgeseg">
        <button data-v="focus" class="on">focus</button>
        <button data-v="all">all</button>
        <button data-v="none">off</button>
      </div>
    </div>
    <div class="ctl"><span class="k">api flow</span>
      <div class="seg" id="flowseg">
        <button data-v="on" class="on">on</button>
        <button data-v="off">off</button>
      </div>
    </div>
    <div class="ctl"><span class="k">spin</span>
      <div class="seg" id="spinseg">
        <button data-v="on" class="on">on</button>
        <button data-v="off">off</button>
      </div>
    </div>
  </div>
  <div class="card" id="legend">
    <div class="lbl" id="legendtitle">languages</div>
    <div id="legendrows"></div>
    <div class="legfoot" id="legfoot"><span class="ring"></span><span></span></div>
  </div>
</div>

<div class="card" id="detail">
  <button id="close" title="close (esc)">&times;</button>
  <div id="detailbody"></div>
</div>
<div id="tip"></div>
<div id="help">drag rotate &middot; wheel zoom &middot; shift-drag pan &middot; click node to focus &middot; esc clears</div>

<div id="api">
  <div class="apihead">
    <h1 id="apititle">API surface</h1>
    <span class="sub" id="apistats"></span>
    <button id="tomap">3d map &rarr;</button>
  </div>
  <div id="matrixwrap" style="display:none">
    <div class="lbl">cross-service calls (client &rarr; endpoint matches)</div>
    <div id="matrix"></div>
    <div class="mxnote">rows call columns &middot; click a count to filter the list below &middot; static in-repo matches only — external clients are invisible</div>
  </div>
  <div class="apibar">
    <input id="apiq" type="text" placeholder="filter by path, handler, framework&hellip;" autocomplete="off" spellcheck="false">
    <select id="apisvc" style="display:none"><option value="">all services</option></select>
    <select id="apikind"><option value="">all kinds</option></select>
    <select id="apimeth"><option value="">all methods</option></select>
  </div>
  <div id="apirows"></div>
</div>

<script type="application/json" id="graph-data">__GRAPH_DATA__</script>
<script>
'use strict';
const data = JSON.parse(document.getElementById('graph-data').textContent);
const meta = data.meta, nodes = data.nodes, eps = data.eps || [], cc = data.cc || [];
const N = nodes.length, E = eps.length;
const SVCS = (meta.services || []).map(s => s.name);

/* Endpoints become pseudo-nodes with NEGATIVE ids so one byId map serves
   both entity kinds: epKey(eid) = -(eid+1). */
function epKey(eid){ return -(eid + 1); }
const epNodes = eps.map(e => Object.assign({}, e, {
  id: epKey(e.id), eid: e.id, isEp: true,
  name: (e.kind === 'http' ? e.method + ' ' : '') + e.path,
  qn: (e.framework || e.kind) + ' · ' + e.kind + (SVCS.length > 1 ? ' · ' + (SVCS[e.sv] || '') : ''),
  lang: e.kind, sz: e.m || 0,
  out: (e.h != null ? [e.h] : []), 'in': [], sim: []
}));
const all = nodes.concat(epNodes);
const TOTAL = all.length;
const byId = new Map(all.map(n => [n.id, n]));

/* Wire matches into both directions: ep['in'] = matched caller fns,
   fn.epOut = endpoints this function calls, fn.hEp = endpoints it serves. */
nodes.forEach(n => { n.epOut = []; n.hEp = []; });
epNodes.forEach(e => { if (e.h != null) { const h = byId.get(e.h); if (h) h.hEp.push(e.id); } });
cc.forEach(f => {
  const ep = byId.get(epKey(f.to));
  if (!ep) return;
  if (f.from != null && byId.get(f.from)) {
    if (ep['in'].length < 20 && ep['in'].indexOf(f.from) < 0) ep['in'].push(f.from);
    const src = byId.get(f.from);
    if (src.epOut.indexOf(ep.id) < 0) src.epOut.push(ep.id);
  }
});

document.title = 'code map — ' + (meta.root || '');
document.getElementById('title').textContent = 'code map · ' + (meta.root || '');
const langCount = Object.keys(meta.languages || {}).length;
document.getElementById('stats').textContent =
  meta.shown + ' functions · ' + langCount + ' language' + (langCount === 1 ? '' : 's') +
  (E ? ' · ' + E + ' endpoint' + (E === 1 ? '' : 's') : '') +
  (meta.multi ? ' · ' + SVCS.length + ' services' : '');
if (meta.capped || meta.eps_capped) {
  const cap = document.getElementById('capnote');
  cap.style.display = 'block';
  cap.textContent = (meta.capped ? 'Showing the ' + meta.shown + ' most connected of ' +
    meta.total + ' functions (payload cap). ' : '') +
    (meta.eps_capped ? 'Showing the ' + meta.eps_shown + ' most-called of ' +
    meta.eps_total + ' endpoints.' : '');
}
if (TOTAL === 0) document.getElementById('empty').style.display = 'grid';

/* Fixed language -> hue mapping (stable across projects). The first eight are
   a CVD-validated dark categorical order; identity is always ALSO carried by
   the legend, tooltip and detail panel text. */
const LANG_COLORS = {
  typescript:'#3987e5', rust:'#d95926', python:'#199e70', javascript:'#c98500',
  go:'#d55181', bash:'#008300', kotlin:'#9085e9', java:'#e66767',
  tsx:'#6da7ec', swift:'#ef8354', c:'#7b8fd9', cpp:'#e0578f', csharp:'#b168d8',
  php:'#c678bd', ruby:'#cc4759', sql:'#ad7f3d', prisma:'#5cc9ad',
  graphql:'#e87ba4', yaml:'#8896a5'
};
const FALLBACK_COLOR = '#9aa0a6';
const DIR_PALETTE = ['#3987e5','#d95926','#199e70','#c98500','#d55181','#008300',
  '#9085e9','#e66767','#6da7ec','#ef8354','#b168d8','#5cc9ad'];
const DIR_OTHER = '#7d8590';
/* Endpoint kind -> hue (diamond glyphs; independent of color mode). */
const KIND_COLORS = {
  http:'#e5a83a', graphql:'#e87ba4', grpc:'#9d8cf0',
  'json-rpc':'#b168d8', 'xml-rpc':'#b168d8', soap:'#8fb4d8', websocket:'#5cc9ad'
};
const KIND_FALLBACK = '#c9a227';
const EP_COUNT = meta.endpoints || 0;

function topdir(f){ const i = f.indexOf('/'); return i > 0 ? f.slice(0, i) : '(root)'; }
const dirCounts = new Map();
nodes.forEach(n => { const d = topdir(n.file); dirCounts.set(d, (dirCounts.get(d) || 0) + 1); });
const dirOrder = [...dirCounts.keys()].sort((a, b) =>
  (dirCounts.get(b) - dirCounts.get(a)) || (a < b ? -1 : 1));
const dirColor = new Map(dirOrder.map((d, i) => [d, i < DIR_PALETTE.length ? DIR_PALETTE[i] : DIR_OTHER]));

function hex2rgb(h){
  return [parseInt(h.slice(1,3),16)/255, parseInt(h.slice(3,5),16)/255, parseInt(h.slice(5,7),16)/255];
}
let colorMode = 'lang';
function keyOf(n){
  if (n.isEp) return '◆ ' + n.kind;
  return colorMode === 'lang' ? n.lang : topdir(n.file);
}
function colorForKey(k){
  if (k.slice(0, 2) === '◆ ') return KIND_COLORS[k.slice(2)] || KIND_FALLBACK;
  return colorMode === 'lang' ? (LANG_COLORS[k] || FALLBACK_COLOR) : (dirColor.get(k) || DIR_OTHER);
}
function colorOf(n){
  if (n.isEp) return KIND_COLORS[n.kind] || KIND_FALLBACK;
  return colorMode === 'lang' ? (LANG_COLORS[n.lang] || FALLBACK_COLOR)
                              : (dirColor.get(topdir(n.file)) || DIR_OTHER);
}
function esc(s){
  return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
}
function base(f){ const i = f.lastIndexOf('/'); return i >= 0 ? f.slice(i + 1) : f; }
function svcName(i){ return SVCS[i] || '(root)'; }

/* ---------------- WebGL ---------------- */
const canvas = document.getElementById('gl');
const gl = canvas.getContext('webgl', { antialias: true, alpha: false });
if (!gl) { document.getElementById('fallback').style.display = 'grid'; throw new Error('no webgl'); }

function shader(type, src){
  const s = gl.createShader(type);
  gl.shaderSource(s, src); gl.compileShader(s);
  if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s));
  return s;
}
function program(vs, fs){
  const p = gl.createProgram();
  gl.attachShader(p, shader(gl.VERTEX_SHADER, vs));
  gl.attachShader(p, shader(gl.FRAGMENT_SHADER, fs));
  gl.linkProgram(p);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p));
  return p;
}

/* aFlag: 0 = plain fn, 1 = fn with endpoint-handler ring, 2 = endpoint
   (diamond glyph with white rim). */
const ptProg = program(
  'attribute vec3 aPos;attribute float aSize;attribute vec3 aColor;' +
  'attribute float aFlag;attribute float aAlpha;' +
  'uniform mat4 uMVP;uniform float uScale;' +
  'varying vec3 vColor;varying float vFlag;varying float vAlpha;' +
  'void main(){gl_Position=uMVP*vec4(aPos,1.0);' +
  'float w=max(gl_Position.w,0.1);' +
  'gl_PointSize=clamp(aSize*uScale/w,2.5,90.0);' +
  'vColor=aColor;vFlag=aFlag;vAlpha=aAlpha;}',
  'precision mediump float;' +
  'varying vec3 vColor;varying float vFlag;varying float vAlpha;' +
  'void main(){vec2 p=gl_PointCoord*2.0-1.0;' +
  'float d=(vFlag>1.5)?(abs(p.x)+abs(p.y)):length(p);' +
  'if(d>1.0)discard;' +
  'float core=1.0-smoothstep(0.55,0.72,d);' +
  'float halo=(1.0-smoothstep(0.0,1.0,d))*0.22;' +
  'vec3 col=vColor;float a=max(core,halo);' +
  'if(vFlag>0.5&&vFlag<1.5){float ring=smoothstep(0.68,0.76,d)*(1.0-smoothstep(0.9,0.99,d));' +
  'col=mix(col,vec3(1.0),ring);a=max(a,ring*0.95);}' +
  'if(vFlag>1.5){float rim=smoothstep(0.6,0.7,d)*(1.0-smoothstep(0.86,0.97,d));' +
  'col=mix(col,vec3(1.0),rim*0.85);a=max(a,rim*0.9);}' +
  'a*=vAlpha;if(a<0.012)discard;gl_FragColor=vec4(col,a);}');

const lnProg = program(
  'attribute vec3 aPos;attribute vec4 aColor;uniform mat4 uMVP;varying vec4 vColor;' +
  'void main(){gl_Position=uMVP*vec4(aPos,1.0);vColor=aColor;}',
  'precision mediump float;varying vec4 vColor;void main(){gl_FragColor=vColor;}');

const ptLoc = {
  aPos: gl.getAttribLocation(ptProg, 'aPos'),
  aSize: gl.getAttribLocation(ptProg, 'aSize'),
  aColor: gl.getAttribLocation(ptProg, 'aColor'),
  aFlag: gl.getAttribLocation(ptProg, 'aFlag'),
  aAlpha: gl.getAttribLocation(ptProg, 'aAlpha'),
  uMVP: gl.getUniformLocation(ptProg, 'uMVP'),
  uScale: gl.getUniformLocation(ptProg, 'uScale')
};
const lnLoc = {
  aPos: gl.getAttribLocation(lnProg, 'aPos'),
  aColor: gl.getAttribLocation(lnProg, 'aColor'),
  uMVP: gl.getUniformLocation(lnProg, 'uMVP')
};

/* Interleaved point data: x,y,z,size,r,g,b,flag,alpha (9 floats). */
const STRIDE = 9;
const ptData = new Float32Array(TOTAL * STRIDE);
let maxCallers = 1;
nodes.forEach(n => { if (n.sz > maxCallers) maxCallers = n.sz; });
let maxMatches = 1;
epNodes.forEach(e => { if (e.m > maxMatches) maxMatches = e.m; });
all.forEach((n, i) => {
  const o = i * STRIDE;
  ptData[o] = n.x; ptData[o + 1] = n.y; ptData[o + 2] = n.z;
  let s;
  if (n.isEp) {
    s = 6.5 + 5.0 * Math.sqrt((n.m || 0) / maxMatches);
    ptData[o + 7] = 2;
  } else {
    s = 3.4 + 11.0 * Math.sqrt(n.sz / maxCallers);
    if (n.ep) s = Math.max(s, 7.0);
    ptData[o + 7] = n.ep ? 1 : 0;
  }
  ptData[o + 3] = s;
  ptData[o + 8] = 1;
});
const ptBuf = gl.createBuffer();
function uploadPoints(){
  gl.bindBuffer(gl.ARRAY_BUFFER, ptBuf);
  gl.bufferData(gl.ARRAY_BUFFER, ptData, gl.DYNAMIC_DRAW);
}
function setColors(){
  all.forEach((n, i) => {
    const c = hex2rgb(colorOf(n));
    const o = i * STRIDE;
    ptData[o + 4] = c[0]; ptData[o + 5] = c[1]; ptData[o + 6] = c[2];
  });
  uploadPoints();
}

/* Edge buffer: x,y,z,r,g,b,a per vertex (7 floats). */
const lnBuf = gl.createBuffer();
let lnCount = 0;
const IN_COL = hex2rgb('#f78fb8'), OUT_COL = hex2rgb('#7fd4f0');
const ALL_COL = [0.62, 0.66, 0.72];
const XS_COL = hex2rgb('#f7c948'), SS_COL = hex2rgb('#7fd4f0');
const MAX_ALL_EDGES = 20000;
const MAX_ARCS = 4000;
const ARC_SEGS = 14;
function rebuildEdges(){
  const v = [];
  const push = (a, b, c, alpha) => {
    v.push(a.x, a.y, a.z, c[0], c[1], c[2], alpha,
           b.x, b.y, b.z, c[0], c[1], c[2], alpha);
  };
  /* Client-call arcs: quadratic bezier lifted above the chord so API flow
     reads differently from plain call edges. */
  const arc = (a, b, c, alpha) => {
    const d = Math.hypot(b.x - a.x, b.y - a.y, b.z - a.z);
    const lift = 4 + d * 0.22;
    const mx = (a.x + b.x) / 2, my = (a.y + b.y) / 2 + lift, mz = (a.z + b.z) / 2;
    let px = a.x, py = a.y, pz = a.z;
    for (let i = 1; i <= ARC_SEGS; i++) {
      const t = i / ARC_SEGS, u = 1 - t;
      const x = u * u * a.x + 2 * u * t * mx + t * t * b.x;
      const y = u * u * a.y + 2 * u * t * my + t * t * b.y;
      const z = u * u * a.z + 2 * u * t * mz + t * t * b.z;
      v.push(px, py, pz, c[0], c[1], c[2], alpha, x, y, z, c[0], c[1], c[2], alpha);
      px = x; py = y; pz = z;
    }
  };
  if (edgeMode !== 'none') {
    if (focusId !== null) {
      const f = byId.get(focusId);
      if (!f.isEp) {
        f.out.forEach(id => { const t = byId.get(id); if (t) push(f, t, OUT_COL, 0.85); });
        f['in'].forEach(id => { const t = byId.get(id); if (t) push(t, f, IN_COL, 0.85); });
      }
    } else if (edgeMode === 'all') {
      let e = 0;
      outer:
      for (const n of nodes) {
        for (const id of n.out) {
          const t = byId.get(id);
          if (!t) continue;
          push(n, t, ALL_COL, 0.06);
          if (++e >= MAX_ALL_EDGES) break outer;
        }
      }
    }
  }
  if (flowsOn) {
    const touches = f => {
      if (focusId === null) return true;
      const ek = epKey(f.to);
      return f.from === focusId || ek === focusId ||
        (byId.get(ek) || {}).h === focusId;
    };
    let n = 0;
    for (const f of cc) {
      if (!touches(f)) continue;
      const to = byId.get(epKey(f.to));
      if (!to) continue;
      const from = f.from != null ? byId.get(f.from) : null;
      if (!from) continue;
      const xs = f.fsv !== to.sv;
      arc(from, to, xs ? XS_COL : SS_COL,
          focusId !== null ? 0.9 : (xs ? 0.55 : 0.28));
      if (++n >= MAX_ARCS) break;
    }
    /* Endpoint -> handler anchor lines. */
    for (const e of epNodes) {
      if (e.h == null) continue;
      if (focusId !== null && focusId !== e.id && focusId !== e.h) continue;
      const h = byId.get(e.h);
      if (!h) continue;
      push(e, h, hex2rgb(KIND_COLORS[e.kind] || KIND_FALLBACK),
           focusId !== null ? 0.85 : 0.3);
    }
  }
  lnCount = v.length / 7;
  gl.bindBuffer(gl.ARRAY_BUFFER, lnBuf);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(v), gl.DYNAMIC_DRAW);
}

/* ---------------- Camera ---------------- */
let cx = 0, cy = 0, cz = 0;
all.forEach(n => { cx += n.x; cy += n.y; cz += n.z; });
if (TOTAL) { cx /= TOTAL; cy /= TOTAL; cz /= TOTAL; }
let R = 1;
all.forEach(n => {
  const d = Math.hypot(n.x - cx, n.y - cy, n.z - cz);
  if (d > R) R = d;
});
const cam = { yaw: 0.55, pitch: 0.32, dist: R * 2.4 + 2, target: [cx, cy, cz] };
const dist0 = cam.dist;
let dpr = 1, cw = 2, chh = 2;
function resize(){
  dpr = Math.min(window.devicePixelRatio || 1, 2);
  cw = canvas.clientWidth || window.innerWidth;
  chh = canvas.clientHeight || window.innerHeight;
  canvas.width = Math.max(2, Math.round(cw * dpr));
  canvas.height = Math.max(2, Math.round(chh * dpr));
}
window.addEventListener('resize', resize);
resize();

function persp(fovy, aspect, near, far){
  const f = 1 / Math.tan(fovy / 2), nf = 1 / (near - far);
  return [f / aspect, 0, 0, 0, 0, f, 0, 0, 0, 0, (far + near) * nf, -1, 0, 0, 2 * far * near * nf, 0];
}
function lookAt(e, c, up){
  let zx = e[0] - c[0], zy = e[1] - c[1], zz = e[2] - c[2];
  const zl = Math.hypot(zx, zy, zz) || 1; zx /= zl; zy /= zl; zz /= zl;
  let xx = up[1] * zz - up[2] * zy, xy = up[2] * zx - up[0] * zz, xz = up[0] * zy - up[1] * zx;
  const xl = Math.hypot(xx, xy, xz) || 1; xx /= xl; xy /= xl; xz /= xl;
  const yx = zy * xz - zz * xy, yy = zz * xx - zx * xz, yz = zx * xy - zy * xx;
  return [xx, yx, zx, 0, xy, yy, zy, 0, xz, yz, zz, 0,
    -(xx * e[0] + xy * e[1] + xz * e[2]),
    -(yx * e[0] + yy * e[1] + yz * e[2]),
    -(zx * e[0] + zy * e[1] + zz * e[2]), 1];
}
function mul4(a, b){
  const o = new Array(16);
  for (let c = 0; c < 4; c++) for (let r = 0; r < 4; r++)
    o[c * 4 + r] = a[r] * b[c * 4] + a[4 + r] * b[c * 4 + 1] + a[8 + r] * b[c * 4 + 2] + a[12 + r] * b[c * 4 + 3];
  return o;
}
let view = null, mvp = null;
function updateMatrices(){
  const cp = Math.cos(cam.pitch), sp = Math.sin(cam.pitch);
  const sy = Math.sin(cam.yaw), cyw = Math.cos(cam.yaw);
  const eye = [cam.target[0] + cam.dist * cp * sy,
               cam.target[1] + cam.dist * sp,
               cam.target[2] + cam.dist * cp * cyw];
  view = lookAt(eye, cam.target, [0, 1, 0]);
  const near = Math.max(cam.dist / 500, 0.05), far = cam.dist * 20 + R * 6;
  mvp = mul4(persp(0.9, cw / chh, near, far), view);
}

/* ---------------- State ---------------- */
let focusId = null, focusNbr = new Set(), hoverId = null;
let searchIds = null, hiddenKeys = new Set();
let edgeMode = 'focus', spin = true, flowsOn = true;
let fly = null;
let viewMode = 'map';

function alphaOf(n){
  if (hiddenKeys.has(keyOf(n))) return 0;
  let a = 1;
  if (searchIds && !searchIds.has(n.id)) a = 0.06;
  if (focusId !== null) {
    if (n.id === focusId) a = 1;
    else if (focusNbr.has(n.id)) a = a < 0.5 ? 0.45 : 0.95;
    else a = Math.min(a, 0.05);
  }
  return a;
}
function setAlphas(){
  all.forEach((n, i) => { ptData[i * STRIDE + 8] = alphaOf(n); });
  uploadPoints();
}

function neighborsOf(n){
  const s = new Set([...n.out, ...n['in']]);
  (n.epOut || []).forEach(id => s.add(id));
  (n.hEp || []).forEach(id => s.add(id));
  if (n.isEp) {
    /* Also pull in the handler's immediate callees so an endpoint's
       neighborhood shows the code behind it. */
    if (n.h != null) {
      const h = byId.get(n.h);
      if (h) h.out.forEach(id => s.add(id));
    }
  }
  return s;
}
function setFocus(id){
  const n = byId.get(id);
  if (!n) return;
  focusId = id;
  focusNbr = neighborsOf(n);
  fly = { from: cam.target.slice(), to: [n.x, n.y, n.z], t: 0 };
  renderDetail(n);
  setAlphas(); rebuildEdges();
}
function clearFocus(){
  focusId = null; focusNbr.clear();
  document.getElementById('detail').style.display = 'none';
  setAlphas(); rebuildEdges();
}

/* ---------------- View toggle ---------------- */
function setView(v){
  viewMode = v;
  document.getElementById('api').style.display = v === 'api' ? 'block' : 'none';
  document.getElementById('side').style.display = v === 'api' ? 'none' : 'flex';
  document.getElementById('help').style.display = v === 'api' ? 'none' : 'block';
  document.getElementById('detail').style.display =
    v === 'api' ? 'none' : (focusId !== null ? 'block' : 'none');
  document.querySelectorAll('#viewseg button').forEach(b =>
    b.classList.toggle('on', b.dataset.v === v));
  if (v === 'api') renderApi();
}
document.getElementById('tomap').addEventListener('click', () => setView('map'));

/* ---------------- HUD ---------------- */
function chip(color, diamond){
  return '<span class="' + (diamond ? 'dia' : 'dot') + '" style="background:' + color + '"></span>';
}
function itemRow(id, extraClass){
  const n = byId.get(id);
  if (!n) return '';
  return '<div class="item ' + (extraClass || '') + '" data-id="' + n.id + '">' +
    chip(colorOf(n), !!n.isEp) + '<span>' + esc(n.name) + '</span>' +
    '<span class="f">' + esc(base(n.file)) + ':' + n.line + '</span></div>';
}
function handlerLabel(e){
  if (e.h != null) return itemRow(e.h);
  if (e.hn) return '<div class="item plain"><span>' + esc(e.hn) + '</span>' +
    '<span class="f">' + esc(base(e.hf || '')) + ':' + (e.hl || 0) + '</span></div>';
  return '<div class="item plain"><span class="f">handler not statically resolved</span></div>';
}
function renderDetail(n){
  const el = document.getElementById('detail');
  const bodyOf = (title, ids, cls) => ids.length
    ? '<div class="sect"><div class="lbl">' + title + '</div>' +
      ids.map(id => itemRow(id, cls)).join('') + '</div>'
    : '';
  let html;
  if (n.isEp) {
    const calls = cc.filter(f => f.to === n.eid);
    const callRows = calls.slice(0, 12).map(f => {
      if (f.from != null && byId.get(f.from)) return itemRow(f.from);
      return '<div class="item plain"><span>' + esc(f.fname || '(top level)') + '</span>' +
        '<span class="f">' + esc(base(f.file)) + ':' + f.line + '</span></div>';
    }).join('') + (calls.length > 12 ? '<div class="item plain"><span class="f">… ' +
      (calls.length - 12) + ' more</span></div>' : '');
    const h = n.h != null ? byId.get(n.h) : null;
    html =
      '<h2>' + esc(n.name) + '</h2>' +
      '<div class="qn">' + esc(n.qn) + '</div>' +
      '<div class="meta">' + chip(colorOf(n), true) +
      '<span>' + esc(n.kind) + '</span><span>·</span>' +
      '<span>' + esc(n.framework || '') + '</span><span>·</span>' +
      '<span>' + esc(n.conf) + ' confidence</span><span>·</span>' +
      '<span>' + esc(n.file) + ':' + n.line + '</span></div>' +
      '<div class="sect"><div class="lbl">handler</div>' + handlerLabel(n) +
      (h ? '<div class="qn">' + esc(h.qn) + '</div>' : '') + '</div>' +
      (h && h.sig ? '<code>' + esc(h.sig) + '</code>' : '') +
      (h ? bodyOf('handler calls → <span style="color:var(--out)">●</span>', h.out.slice(0, 8)) : '') +
      (calls.length ? '<div class="sect"><div class="lbl">client calls in (' + calls.length +
        ')</div>' + callRows + '</div>' : '');
  } else {
    html =
      '<h2>' + esc(n.name) + (n.ep ? ' <span title="API endpoint handler">◎</span>' : '') + '</h2>' +
      '<div class="qn">' + esc(n.qn) + '</div>' +
      '<div class="meta">' + chip(LANG_COLORS[n.lang] || FALLBACK_COLOR) +
      '<span>' + esc(n.lang) + '</span><span>·</span>' +
      '<span>' + esc(n.file) + ':' + n.line + '</span><span>·</span>' +
      '<span>' + n.sz + ' caller site' + (n.sz === 1 ? '' : 's') + '</span></div>' +
      (n.sig ? '<code>' + esc(n.sig) + '</code>' : '') +
      bodyOf('serves endpoint <span style="color:var(--xs)">◆</span>', n.hEp) +
      bodyOf('calls endpoints <span style="color:var(--xs)">◆</span>', n.epOut) +
      bodyOf('callers ← <span style="color:var(--in)">●</span>', n['in']) +
      bodyOf('callees → <span style="color:var(--out)">●</span>', n.out) +
      bodyOf('similar (structural)', n.sim);
  }
  document.getElementById('detailbody').innerHTML = html;
  el.style.display = 'block';
}
document.getElementById('detail').addEventListener('click', e => {
  const item = e.target.closest('.item');
  if (item && item.dataset.id !== undefined) setFocus(+item.dataset.id);
});
document.getElementById('close').addEventListener('click', clearFocus);

const tip = document.getElementById('tip');
function showTip(n, mx, my){
  if (n.isEp) {
    const xsIn = cc.filter(f => f.to === n.eid && f.fsv !== n.sv).length;
    tip.innerHTML = '<div class="t">◆ ' + esc(n.name) + '</div>' +
      '<div class="m">' + esc(n.framework || '') + ' · ' + esc(n.kind) + ' · ' +
      esc(n.conf) + ' · ' + esc(n.file) + ':' + n.line + '</div>' +
      '<div class="sim">' + n.m + ' client call' + (n.m === 1 ? '' : 's') +
      (xsIn ? ' (' + xsIn + ' cross-service)' : '') +
      (n.h != null && byId.get(n.h) ? ' · handler ' + esc(byId.get(n.h).name) : '') + '</div>';
  } else {
    const sims = n.sim.map(id => byId.get(id)).filter(Boolean).map(s => esc(s.name));
    tip.innerHTML = '<div class="t">' + esc(n.name) +
      (n.ep ? ' ◎ endpoint handler' : '') + '</div>' +
      (n.sig ? '<div class="s">' + esc(n.sig.length > 140 ? n.sig.slice(0, 140) + '…' : n.sig) + '</div>' : '') +
      '<div class="m">' + esc(n.file) + ':' + n.line + ' · ' + esc(n.lang) +
      ' · ' + n['in'].length + ' callers · ' + n.out.length + ' callees' +
      (n.epOut.length ? ' · calls ' + n.epOut.length + ' endpoint' + (n.epOut.length === 1 ? '' : 's') : '') + '</div>' +
      (sims.length ? '<div class="sim">similar: ' + sims.join(', ') + '</div>' : '');
  }
  tip.style.display = 'block';
  const bw = tip.offsetWidth, bh = tip.offsetHeight;
  tip.style.left = Math.min(mx + 14, cw - bw - 8) + 'px';
  tip.style.top = Math.min(my + 12, chh - bh - 8) + 'px';
}
function hideTip(){ tip.style.display = 'none'; }

function renderLegend(){
  const counts = new Map();
  all.forEach(n => { const k = keyOf(n); counts.set(k, (counts.get(k) || 0) + 1); });
  const keys = [...counts.keys()].sort((a, b) =>
    (counts.get(b) - counts.get(a)) || (a < b ? -1 : 1));
  document.getElementById('legendtitle').textContent =
    (colorMode === 'lang' ? 'languages' : 'top-level directories') + (E ? ' + endpoints' : '');
  document.getElementById('legendrows').innerHTML = keys.map(k => {
    const isKind = k.slice(0, 2) === '◆ ';
    return '<div class="lg' + (hiddenKeys.has(k) ? ' off' : '') + '" data-k="' + esc(k) + '">' +
      chip(colorForKey(k), isKind) + '<span class="n">' + esc(k) + '</span>' +
      '<span class="c">' + counts.get(k) + '</span></div>';
  }).join('');
  document.querySelector('#legfoot span:last-child').textContent =
    '◆ = API endpoint · white ring = handler' + (EP_COUNT ? ' (' + EP_COUNT + ')' : '') +
    ' · click a row to hide';
}
document.getElementById('legendrows').addEventListener('click', e => {
  const row = e.target.closest('.lg');
  if (!row) return;
  const k = row.dataset.k;
  if (hiddenKeys.has(k)) hiddenKeys.delete(k); else hiddenKeys.add(k);
  renderLegend(); setAlphas();
});

function segWire(id, fn){
  const seg = document.getElementById(id);
  seg.addEventListener('click', e => {
    const b = e.target.closest('button');
    if (!b) return;
    seg.querySelectorAll('button').forEach(x => x.classList.toggle('on', x === b));
    fn(b.dataset.v);
  });
}
segWire('viewseg', v => setView(v));
segWire('colorseg', v => {
  colorMode = v; hiddenKeys.clear();
  setColors(); setAlphas(); renderLegend();
});
segWire('edgeseg', v => { edgeMode = v; rebuildEdges(); });
segWire('flowseg', v => { flowsOn = v === 'on'; rebuildEdges(); });
segWire('spinseg', v => { spin = v === 'on'; });
function setSpin(on){
  spin = on;
  document.querySelectorAll('#spinseg button').forEach(b =>
    b.classList.toggle('on', (b.dataset.v === 'on') === on));
}

/* ---------------- Search ---------------- */
const searchEl = document.getElementById('search');
const resultsEl = document.getElementById('results');
let matches = [];
searchEl.addEventListener('input', () => {
  const q = searchEl.value.trim().toLowerCase();
  if (!q) {
    searchIds = null; matches = []; resultsEl.style.display = 'none';
    setAlphas(); return;
  }
  matches = all.filter(n =>
    n.name.toLowerCase().includes(q) || n.qn.toLowerCase().includes(q) ||
    n.file.toLowerCase().includes(q));
  searchIds = new Set(matches.map(n => n.id));
  resultsEl.innerHTML = matches.slice(0, 25).map(n =>
    '<div class="row" data-id="' + n.id + '"><span>' + chip(colorOf(n), !!n.isEp) + ' ' + esc(n.name) +
    '</span><span class="f">' + esc(base(n.file)) + ':' + n.line + '</span></div>').join('') +
    (matches.length > 25 ? '<div class="row"><span class="f">… ' +
      (matches.length - 25) + ' more</span></div>' : '') +
    (matches.length === 0 ? '<div class="row"><span class="f">no matches</span></div>' : '');
  resultsEl.style.display = 'block';
  setAlphas();
});
resultsEl.addEventListener('click', e => {
  const row = e.target.closest('.row');
  if (row && row.dataset.id !== undefined) setFocus(+row.dataset.id);
});
searchEl.addEventListener('keydown', e => {
  if (e.key === 'Enter' && matches.length) setFocus(matches[0].id);
  if (e.key === 'Escape') { searchEl.value = ''; searchEl.dispatchEvent(new Event('input')); searchEl.blur(); }
});
window.addEventListener('keydown', e => {
  if (e.key === 'Escape' && document.activeElement !== searchEl &&
      document.activeElement !== apiQ) {
    if (viewMode === 'api') { setView('map'); return; }
    if (focusId !== null) clearFocus();
    else if (searchIds) { searchEl.value = ''; searchEl.dispatchEvent(new Event('input')); }
  }
  if (e.key === '/' && document.activeElement !== searchEl &&
      document.activeElement !== apiQ) {
    e.preventDefault();
    (viewMode === 'api' ? apiQ : searchEl).focus();
  }
});

/* ---------------- API surface panel ---------------- */
const apiQ = document.getElementById('apiq');
const apiSvc = document.getElementById('apisvc');
const apiKind = document.getElementById('apikind');
const apiMeth = document.getElementById('apimeth');
let flowFilter = null; /* {from, to} service indices */

document.getElementById('apititle').textContent = 'API surface · ' + (meta.root || '');
document.getElementById('apistats').textContent =
  meta.eps_shown + (meta.eps_capped ? ' of ' + meta.eps_total : '') + ' endpoint' +
  (meta.eps_total === 1 ? '' : 's') +
  ' · ' + meta.matches + ' matched client call' + (meta.matches === 1 ? '' : 's') +
  (meta.multi ? ' · ' + SVCS.length + ' services' : '');

/* Filter dropdowns, populated from the data (stable order). */
if (SVCS.length > 1) {
  apiSvc.style.display = '';
  (meta.services || []).forEach((s, i) => {
    if (!s.endpoints) return;
    const o = document.createElement('option');
    o.value = String(i); o.textContent = s.name + ' (' + s.endpoints + ')';
    apiSvc.appendChild(o);
  });
}
[...new Set(eps.map(e => e.kind))].sort().forEach(k => {
  const o = document.createElement('option'); o.value = k; o.textContent = k;
  apiKind.appendChild(o);
});
[...new Set(eps.map(e => e.method))].sort().forEach(m => {
  const o = document.createElement('option'); o.value = m; o.textContent = m;
  apiMeth.appendChild(o);
});
[apiQ, apiSvc, apiKind, apiMeth].forEach(el =>
  el.addEventListener('input', renderApiRows));

/* Service-to-service flow matrix from the correlated matches. */
const flows = new Map(); /* 'from>to' -> count */
cc.forEach(f => {
  const to = byId.get(epKey(f.to));
  if (!to) return;
  const k = f.fsv + '>' + to.sv;
  flows.set(k, (flows.get(k) || 0) + 1);
});
function renderMatrix(){
  const wrap = document.getElementById('matrixwrap');
  if (!meta.multi || flows.size === 0) { wrap.style.display = 'none'; return; }
  wrap.style.display = 'block';
  const parts = new Set();
  flows.forEach((_, k) => { const [a, b] = k.split('>'); parts.add(+a); parts.add(+b); });
  const idx = [...parts].sort((a, b) => a - b);
  let max = 1;
  flows.forEach(v => { if (v > max) max = v; });
  let h = '<table class="mx"><tr><th>calls &rarr;</th>' +
    idx.map(i => '<th>' + esc(svcName(i)) + '</th>').join('') + '</tr>';
  for (const r of idx) {
    h += '<tr><th>' + esc(svcName(r)) + '</th>';
    for (const c of idx) {
      const n = flows.get(r + '>' + c) || 0;
      const sel = flowFilter && flowFilter.from === r && flowFilter.to === c;
      const bg = n ? 'background:rgba(247,201,72,' + (0.06 + 0.30 * n / max).toFixed(2) + ')' : '';
      h += '<td class="' + (n ? 'n' : 'zero') + (sel ? ' sel' : '') +
        '" data-from="' + r + '" data-to="' + c + '" style="' + bg + '">' +
        (n || '·') + '</td>';
    }
    h += '</tr>';
  }
  h += '</table>';
  document.getElementById('matrix').innerHTML = h;
}
document.getElementById('matrix').addEventListener('click', e => {
  const td = e.target.closest('td.n');
  if (!td) return;
  const from = +td.dataset.from, to = +td.dataset.to;
  flowFilter = (flowFilter && flowFilter.from === from && flowFilter.to === to)
    ? null : { from, to };
  renderMatrix(); renderApiRows();
});

function calleeSummary(e){
  if (e.h == null) return '';
  const h = byId.get(e.h);
  if (!h || !h.out.length) return '';
  const names = h.out.slice(0, 4).map(id => byId.get(id)).filter(Boolean).map(c => esc(c.name));
  const more = h.out.length > 4 ? ' +' + (h.out.length - 4) : '';
  return '<span class="callees">&rarr; ' + names.join(', ') + more + '</span>';
}
function epMatchesFilters(e){
  if (apiSvc.value !== '' && e.sv !== +apiSvc.value) return false;
  if (apiKind.value !== '' && e.kind !== apiKind.value) return false;
  if (apiMeth.value !== '' && e.method !== apiMeth.value) return false;
  if (flowFilter) {
    if (e.sv !== flowFilter.to) return false;
    if (!cc.some(f => f.to === e.id && f.fsv === flowFilter.from)) return false;
  }
  const q = apiQ.value.trim().toLowerCase();
  if (q) {
    const h = e.h != null ? byId.get(e.h) : null;
    const hay = (e.path + ' ' + e.method + ' ' + e.framework + ' ' + e.kind + ' ' +
      e.file + ' ' + (h ? h.name + ' ' + h.qn : (e.hn || ''))).toLowerCase();
    if (!hay.includes(q)) return false;
  }
  return true;
}
function renderApiRows(){
  const box = document.getElementById('apirows');
  if (E === 0) {
    box.innerHTML = '<div id="apiempty">No API endpoints detected in this index.<br>' +
      '<span style="font-size:11.5px;color:var(--muted)">Endpoint detection covers HTTP routes ' +
      '(express, flask, spring, gin, rails, …) plus GraphQL / gRPC / SOAP / RPC operations.</span></div>';
    return;
  }
  const list = eps.filter(epMatchesFilters);
  if (!list.length) {
    box.innerHTML = '<div id="apiempty">No endpoints match the current filters.</div>';
    return;
  }
  /* Group by service, stable order (service index, then endpoint id). */
  const groups = new Map();
  list.forEach(e => {
    if (!groups.has(e.sv)) groups.set(e.sv, []);
    groups.get(e.sv).push(e);
  });
  const svs = [...groups.keys()].sort((a, b) => a - b);
  box.innerHTML = svs.map(sv => {
    const rows = groups.get(sv).map(e => {
      const h = e.h != null ? byId.get(e.h) : null;
      const hd = h
        ? esc(h.name) + ' <span class="f">' + esc(h.file) + ':' + h.line + '</span>'
        : (e.hn ? esc(e.hn) + ' <span class="f">' + esc(e.hf || '') + ':' + (e.hl || 0) + '</span>'
                : '<span class="f">handler unresolved</span>');
      const xsIn = cc.filter(f => f.to === e.id && f.fsv !== e.sv).length;
      return '<div class="eprow" data-eid="' + e.id + '">' +
        '<span class="mth ' + esc(e.method) + '">' + esc(e.method) + '</span>' +
        '<span class="pth">' + esc(e.path) + '</span>' +
        '<span class="fw">' + esc(e.framework || '') + ' · ' + esc(e.kind) +
        ' · <span class="confb ' + esc(e.conf) + '">' + esc(e.conf) + '</span></span>' +
        '<span class="hd">' + hd + calleeSummary(e) + '</span>' +
        '<span class="in">' + (e.m ? e.m + ' in' : '·') +
        (xsIn ? ' <b>' + xsIn + '×svc</b>' : '') + '</span>' +
        '</div>';
    }).join('');
    const st = (meta.services || [])[sv] || {};
    return '<div class="svcgrp"><div class="svch"><span class="nm">' + esc(svcName(sv)) +
      '</span><span class="ct">' + groups.get(sv).length + ' endpoint' +
      (groups.get(sv).length === 1 ? '' : 's') +
      (st.functions ? ' · ' + st.functions + ' functions' : '') + '</span></div>' +
      rows + '</div>';
  }).join('');
}
document.getElementById('apirows').addEventListener('click', e => {
  const row = e.target.closest('.eprow');
  if (!row) return;
  setView('map');
  setFocus(epKey(+row.dataset.eid));
});
function renderApi(){ renderMatrix(); renderApiRows(); }

/* ---------------- Interaction ---------------- */
let dragging = false, dragMoved = 0, panMode = false, lastX = 0, lastY = 0, downT = 0;
let mouseX = -1, mouseY = -1, wantPick = false;
canvas.addEventListener('contextmenu', e => e.preventDefault());
canvas.addEventListener('pointerdown', e => {
  dragging = true; dragMoved = 0; panMode = e.shiftKey || e.button === 2 || e.button === 1;
  lastX = e.clientX; lastY = e.clientY; downT = performance.now();
  setSpin(false);
  canvas.setPointerCapture(e.pointerId);
  canvas.style.cursor = 'grabbing';
});
canvas.addEventListener('pointermove', e => {
  mouseX = e.clientX; mouseY = e.clientY; wantPick = true;
  if (!dragging) return;
  const dx = e.clientX - lastX, dy = e.clientY - lastY;
  lastX = e.clientX; lastY = e.clientY;
  dragMoved += Math.abs(dx) + Math.abs(dy);
  if (panMode) {
    const s = cam.dist * 0.0016;
    cam.target[0] -= (view[0] * dx - view[1] * dy) * s;
    cam.target[1] -= (view[4] * dx - view[5] * dy) * s;
    cam.target[2] -= (view[8] * dx - view[9] * dy) * s;
    fly = null;
  } else {
    cam.yaw -= dx * 0.005;
    cam.pitch = Math.min(1.52, Math.max(-1.52, cam.pitch + dy * 0.005));
  }
});
canvas.addEventListener('pointerup', e => {
  dragging = false;
  canvas.style.cursor = hoverId !== null ? 'pointer' : 'grab';
  if (dragMoved < 5 && performance.now() - downT < 500 && e.button === 0) {
    const id = pick(e.clientX, e.clientY);
    if (id !== null) setFocus(id); else if (focusId !== null) clearFocus();
  }
});
canvas.addEventListener('pointerleave', () => { hoverId = null; hideTip(); });
canvas.addEventListener('wheel', e => {
  e.preventDefault();
  cam.dist = Math.min(Math.max(cam.dist * Math.exp(e.deltaY * 0.0012), R * 0.06 + 0.2), R * 14 + 20);
}, { passive: false });

function pick(mx, my){
  if (!mvp) return null;
  let best = null, bestD = 1e9;
  const scl = 1.35 * dist0; /* matches uScale without dpr */
  for (let i = 0; i < TOTAL; i++) {
    const o = i * STRIDE;
    if (ptData[o + 8] < 0.05) continue; /* hidden / heavily dimmed */
    const x = ptData[o], y = ptData[o + 1], z = ptData[o + 2];
    const w = mvp[3] * x + mvp[7] * y + mvp[11] * z + mvp[15];
    if (w <= 0) continue;
    const sx = ((mvp[0] * x + mvp[4] * y + mvp[8] * z + mvp[12]) / w * 0.5 + 0.5) * cw;
    const sy = (0.5 - (mvp[1] * x + mvp[5] * y + mvp[9] * z + mvp[13]) / w * 0.5) * chh;
    const r = Math.min(Math.max(ptData[o + 3] * scl / w, 2.5), 90) / 2;
    const d = Math.hypot(sx - mx, sy - my);
    if (d < Math.max(r + 3, 7) && d < bestD) { bestD = d; best = all[i].id; }
  }
  return best;
}

/* ---------------- Render loop ---------------- */
setColors(); setAlphas(); rebuildEdges(); renderLegend();
if (meta.mode === 'api') setView('api');
gl.disable(gl.DEPTH_TEST);
gl.enable(gl.BLEND);
gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
gl.clearColor(0x0f / 255, 0x11 / 255, 0x15 / 255, 1);

let lastT = performance.now();
function frame(t){
  const dt = Math.min(t - lastT, 100); lastT = t;
  if (spin && !dragging) cam.yaw += dt * 0.00010;
  if (fly) {
    fly.t = Math.min(fly.t + dt / 450, 1);
    const k = fly.t < 0.5 ? 2 * fly.t * fly.t : 1 - Math.pow(-2 * fly.t + 2, 2) / 2;
    for (let i = 0; i < 3; i++) cam.target[i] = fly.from[i] + (fly.to[i] - fly.from[i]) * k;
    if (fly.t >= 1) fly = null;
  }
  updateMatrices();
  gl.viewport(0, 0, canvas.width, canvas.height);
  gl.clear(gl.COLOR_BUFFER_BIT);

  if (lnCount > 0) {
    gl.useProgram(lnProg);
    gl.uniformMatrix4fv(lnLoc.uMVP, false, mvp);
    gl.bindBuffer(gl.ARRAY_BUFFER, lnBuf);
    gl.enableVertexAttribArray(lnLoc.aPos);
    gl.vertexAttribPointer(lnLoc.aPos, 3, gl.FLOAT, false, 28, 0);
    gl.enableVertexAttribArray(lnLoc.aColor);
    gl.vertexAttribPointer(lnLoc.aColor, 4, gl.FLOAT, false, 28, 12);
    gl.drawArrays(gl.LINES, 0, lnCount);
  }
  if (TOTAL > 0) {
    gl.useProgram(ptProg);
    gl.uniformMatrix4fv(ptLoc.uMVP, false, mvp);
    gl.uniform1f(ptLoc.uScale, 1.35 * dist0 * dpr);
    gl.bindBuffer(gl.ARRAY_BUFFER, ptBuf);
    gl.enableVertexAttribArray(ptLoc.aPos);
    gl.vertexAttribPointer(ptLoc.aPos, 3, gl.FLOAT, false, 36, 0);
    gl.enableVertexAttribArray(ptLoc.aSize);
    gl.vertexAttribPointer(ptLoc.aSize, 1, gl.FLOAT, false, 36, 12);
    gl.enableVertexAttribArray(ptLoc.aColor);
    gl.vertexAttribPointer(ptLoc.aColor, 3, gl.FLOAT, false, 36, 16);
    gl.enableVertexAttribArray(ptLoc.aFlag);
    gl.vertexAttribPointer(ptLoc.aFlag, 1, gl.FLOAT, false, 36, 28);
    gl.enableVertexAttribArray(ptLoc.aAlpha);
    gl.vertexAttribPointer(ptLoc.aAlpha, 1, gl.FLOAT, false, 36, 32);
    gl.drawArrays(gl.POINTS, 0, TOTAL);
  }

  if (wantPick && !dragging && viewMode === 'map') {
    wantPick = false;
    const id = pick(mouseX, mouseY);
    if (id !== hoverId) {
      hoverId = id;
      canvas.style.cursor = id !== null ? 'pointer' : 'grab';
      if (id === null) hideTip();
    }
    if (hoverId !== null) showTip(byId.get(hoverId), mouseX, mouseY);
  }
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
</script>
</body>
</html>
"####;
