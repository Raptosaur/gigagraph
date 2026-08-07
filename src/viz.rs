//! Interactive 3D code map: functions are projected from their 256-dim
//! structural vectors to 3D via PCA (power iteration, deterministic), so
//! structurally similar code clusters together. The output is ONE
//! self-contained HTML file (raw WebGL, no CDN, works offline).

use crate::indexer::{self, Index};
use crate::types::Resolution;
use crate::vector::DIMS;
use anyhow::{Context, Result};
use rustc_hash::FxHashSet;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Payload cap: beyond this, only the most connected functions are kept.
const MAX_NODES: usize = 5000;
/// Per-node cap on direct caller / callee id lists.
const MAX_NEIGHBORS: usize = 20;
/// Similar functions listed per node.
const SIMILAR_K: usize = 5;

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
/// given index (no timestamps, seeded projection).
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

/// Builds the JSON payload embedded in the page: metadata plus one node per
/// (non-synthetic) function with 3D coordinates and capped neighbor lists.
fn build_payload(index: &Index) -> Value {
    let g = &index.graph;

    // Real functions only; `(toplevel)` synthetics are excluded from the map.
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
        // Top similar functions via the existing vector index.
        let sim: Vec<u32> = index
            .vectors
            .vector_of(id)
            .map(|v| index.vectors.top_k(v, SIMILAR_K + 15, Some(id)))
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
            "ep": endpoint_handlers.contains(&id),
            "sim": sim,
            "out": out_ids,
            "in": in_ids,
        }));
    }

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
        },
        "nodes": nodes,
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
// The page. One file, no network: raw WebGL point cloud + orbit camera.
// Language colors: the 8 most common languages use a CVD-validated dark
// categorical order; every color is also named in the legend / tooltip /
// detail panel, so identity never rides on hue alone.
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
  --accent:#3987e5; --in:#f78fb8; --out:#7fd4f0;
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
    <input id="search" type="text" placeholder="search functions&hellip;" autocomplete="off" spellcheck="false">
    <div id="results"></div>
  </div>
  <div class="card">
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

<script type="application/json" id="graph-data">__GRAPH_DATA__</script>
<script>
'use strict';
const data = JSON.parse(document.getElementById('graph-data').textContent);
const meta = data.meta, nodes = data.nodes, N = nodes.length;
const byId = new Map(nodes.map(n => [n.id, n]));

document.title = 'code map — ' + (meta.root || '');
document.getElementById('title').textContent = 'code map · ' + (meta.root || '');
const langCount = Object.keys(meta.languages || {}).length;
document.getElementById('stats').textContent =
  meta.shown + ' functions · ' + langCount + ' language' + (langCount === 1 ? '' : 's') +
  (meta.endpoints ? ' · ' + meta.endpoints + ' endpoints' : '');
if (meta.capped) {
  const cap = document.getElementById('capnote');
  cap.style.display = 'block';
  cap.textContent = 'Showing the ' + meta.shown + ' most connected of ' + meta.total +
    ' functions (payload cap).';
}
if (N === 0) document.getElementById('empty').style.display = 'grid';

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
function keyOf(n){ return colorMode === 'lang' ? n.lang : topdir(n.file); }
function colorOf(n){
  return colorMode === 'lang' ? (LANG_COLORS[n.lang] || FALLBACK_COLOR)
                              : (dirColor.get(topdir(n.file)) || DIR_OTHER);
}
function esc(s){
  return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
}
function base(f){ const i = f.lastIndexOf('/'); return i >= 0 ? f.slice(i + 1) : f; }

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
  'void main(){vec2 p=gl_PointCoord*2.0-1.0;float d=length(p);' +
  'if(d>1.0)discard;' +
  'float core=1.0-smoothstep(0.55,0.72,d);' +
  'float halo=(1.0-smoothstep(0.0,1.0,d))*0.22;' +
  'vec3 col=vColor;float a=max(core,halo);' +
  'if(vFlag>0.5){float ring=smoothstep(0.68,0.76,d)*(1.0-smoothstep(0.9,0.99,d));' +
  'col=mix(col,vec3(1.0),ring);a=max(a,ring*0.95);}' +
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
const ptData = new Float32Array(N * STRIDE);
let maxCallers = 1;
nodes.forEach(n => { if (n.sz > maxCallers) maxCallers = n.sz; });
nodes.forEach((n, i) => {
  const o = i * STRIDE;
  ptData[o] = n.x; ptData[o + 1] = n.y; ptData[o + 2] = n.z;
  let s = 3.4 + 11.0 * Math.sqrt(n.sz / maxCallers);
  if (n.ep) s = Math.max(s, 7.0);
  ptData[o + 3] = s;
  ptData[o + 7] = n.ep ? 1 : 0;
  ptData[o + 8] = 1;
});
const ptBuf = gl.createBuffer();
function uploadPoints(){
  gl.bindBuffer(gl.ARRAY_BUFFER, ptBuf);
  gl.bufferData(gl.ARRAY_BUFFER, ptData, gl.DYNAMIC_DRAW);
}
function setColors(){
  nodes.forEach((n, i) => {
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
const MAX_ALL_EDGES = 20000;
function rebuildEdges(){
  const v = [];
  const push = (a, b, c, alpha) => {
    v.push(a.x, a.y, a.z, c[0], c[1], c[2], alpha,
           b.x, b.y, b.z, c[0], c[1], c[2], alpha);
  };
  if (edgeMode !== 'none') {
    if (focusId !== null) {
      const f = byId.get(focusId);
      f.out.forEach(id => { const t = byId.get(id); if (t) push(f, t, OUT_COL, 0.85); });
      f['in'].forEach(id => { const t = byId.get(id); if (t) push(t, f, IN_COL, 0.85); });
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
  lnCount = v.length / 7;
  gl.bindBuffer(gl.ARRAY_BUFFER, lnBuf);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(v), gl.DYNAMIC_DRAW);
}

/* ---------------- Camera ---------------- */
let cx = 0, cy = 0, cz = 0;
nodes.forEach(n => { cx += n.x; cy += n.y; cz += n.z; });
if (N) { cx /= N; cy /= N; cz /= N; }
let R = 1;
nodes.forEach(n => {
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
let edgeMode = 'focus', spin = true;
let fly = null;

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
  nodes.forEach((n, i) => { ptData[i * STRIDE + 8] = alphaOf(n); });
  uploadPoints();
}

function setFocus(id){
  focusId = id;
  const n = byId.get(id);
  focusNbr = new Set([...n.out, ...n['in']]);
  fly = { from: cam.target.slice(), to: [n.x, n.y, n.z], t: 0 };
  renderDetail(n);
  setAlphas(); rebuildEdges();
}
function clearFocus(){
  focusId = null; focusNbr.clear();
  document.getElementById('detail').style.display = 'none';
  setAlphas(); rebuildEdges();
}

/* ---------------- HUD ---------------- */
function chip(color){
  return '<span class="dot" style="background:' + color + '"></span>';
}
function itemRow(id, extraClass){
  const n = byId.get(id);
  if (!n) return '';
  return '<div class="item ' + (extraClass || '') + '" data-id="' + n.id + '">' +
    chip(colorOf(n)) + '<span>' + esc(n.name) + '</span>' +
    '<span class="f">' + esc(base(n.file)) + ':' + n.line + '</span></div>';
}
function renderDetail(n){
  const el = document.getElementById('detail');
  const bodyOf = (title, ids, cls) => ids.length
    ? '<div class="sect"><div class="lbl">' + title + '</div>' +
      ids.map(id => itemRow(id, cls)).join('') + '</div>'
    : '';
  document.getElementById('detailbody').innerHTML =
    '<h2>' + esc(n.name) + (n.ep ? ' <span title="API endpoint handler">◎</span>' : '') + '</h2>' +
    '<div class="qn">' + esc(n.qn) + '</div>' +
    '<div class="meta">' + chip(LANG_COLORS[n.lang] || FALLBACK_COLOR) +
    '<span>' + esc(n.lang) + '</span><span>·</span>' +
    '<span>' + esc(n.file) + ':' + n.line + '</span><span>·</span>' +
    '<span>' + n.sz + ' caller site' + (n.sz === 1 ? '' : 's') + '</span></div>' +
    (n.sig ? '<code>' + esc(n.sig) + '</code>' : '') +
    bodyOf('callers ← <span style="color:var(--in)">●</span>', n['in']) +
    bodyOf('callees → <span style="color:var(--out)">●</span>', n.out) +
    bodyOf('similar (structural)', n.sim);
  el.style.display = 'block';
}
document.getElementById('detail').addEventListener('click', e => {
  const item = e.target.closest('.item');
  if (item) setFocus(+item.dataset.id);
});
document.getElementById('close').addEventListener('click', clearFocus);

const tip = document.getElementById('tip');
function showTip(n, mx, my){
  const sims = n.sim.map(id => byId.get(id)).filter(Boolean).map(s => esc(s.name));
  tip.innerHTML = '<div class="t">' + esc(n.name) +
    (n.ep ? ' ◎ endpoint' : '') + '</div>' +
    (n.sig ? '<div class="s">' + esc(n.sig.length > 140 ? n.sig.slice(0, 140) + '…' : n.sig) + '</div>' : '') +
    '<div class="m">' + esc(n.file) + ':' + n.line + ' · ' + esc(n.lang) +
    ' · ' + n['in'].length + ' callers · ' + n.out.length + ' callees</div>' +
    (sims.length ? '<div class="sim">similar: ' + sims.join(', ') + '</div>' : '');
  tip.style.display = 'block';
  const bw = tip.offsetWidth, bh = tip.offsetHeight;
  tip.style.left = Math.min(mx + 14, cw - bw - 8) + 'px';
  tip.style.top = Math.min(my + 12, chh - bh - 8) + 'px';
}
function hideTip(){ tip.style.display = 'none'; }

function renderLegend(){
  const counts = new Map();
  nodes.forEach(n => { const k = keyOf(n); counts.set(k, (counts.get(k) || 0) + 1); });
  const keys = [...counts.keys()].sort((a, b) =>
    (counts.get(b) - counts.get(a)) || (a < b ? -1 : 1));
  document.getElementById('legendtitle').textContent =
    colorMode === 'lang' ? 'languages' : 'top-level directories';
  document.getElementById('legendrows').innerHTML = keys.map(k => {
    const col = colorMode === 'lang' ? (LANG_COLORS[k] || FALLBACK_COLOR)
                                     : (dirColor.get(k) || DIR_OTHER);
    return '<div class="lg' + (hiddenKeys.has(k) ? ' off' : '') + '" data-k="' + esc(k) + '">' +
      chip(col) + '<span class="n">' + esc(k) + '</span>' +
      '<span class="c">' + counts.get(k) + '</span></div>';
  }).join('');
  document.querySelector('#legfoot span:last-child').textContent =
    'white ring = API endpoint' + (EP_COUNT ? ' (' + EP_COUNT + ')' : '') +
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
segWire('colorseg', v => {
  colorMode = v; hiddenKeys.clear();
  setColors(); setAlphas(); renderLegend();
});
segWire('edgeseg', v => { edgeMode = v; rebuildEdges(); });
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
  matches = nodes.filter(n =>
    n.name.toLowerCase().includes(q) || n.qn.toLowerCase().includes(q));
  searchIds = new Set(matches.map(n => n.id));
  resultsEl.innerHTML = matches.slice(0, 25).map(n =>
    '<div class="row" data-id="' + n.id + '"><span>' + chip(colorOf(n)) + ' ' + esc(n.name) +
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
  if (e.key === 'Escape' && document.activeElement !== searchEl) {
    if (focusId !== null) clearFocus();
    else if (searchIds) { searchEl.value = ''; searchEl.dispatchEvent(new Event('input')); }
  }
  if (e.key === '/' && document.activeElement !== searchEl) { e.preventDefault(); searchEl.focus(); }
});

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
  for (let i = 0; i < N; i++) {
    const o = i * STRIDE;
    if (ptData[o + 8] < 0.05) continue; /* hidden / heavily dimmed */
    const x = ptData[o], y = ptData[o + 1], z = ptData[o + 2];
    const w = mvp[3] * x + mvp[7] * y + mvp[11] * z + mvp[15];
    if (w <= 0) continue;
    const sx = ((mvp[0] * x + mvp[4] * y + mvp[8] * z + mvp[12]) / w * 0.5 + 0.5) * cw;
    const sy = (0.5 - (mvp[1] * x + mvp[5] * y + mvp[9] * z + mvp[13]) / w * 0.5) * chh;
    const r = Math.min(Math.max(ptData[o + 3] * scl / w, 2.5), 90) / 2;
    const d = Math.hypot(sx - mx, sy - my);
    if (d < Math.max(r + 3, 7) && d < bestD) { bestD = d; best = nodes[i].id; }
  }
  return best;
}

/* ---------------- Render loop ---------------- */
setColors(); setAlphas(); rebuildEdges(); renderLegend();
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
  if (N > 0) {
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
    gl.drawArrays(gl.POINTS, 0, N);
  }

  if (wantPick && !dragging) {
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
