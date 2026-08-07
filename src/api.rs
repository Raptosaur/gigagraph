//! Tool implementations shared by the MCP server and the debug CLI.

use crate::endpoints;
use crate::extract;
use crate::graph::GigaGraph;
use crate::impact;
use crate::indexer::{self, Index};
use crate::lang;
use crate::touches;
use crate::types::{Confidence, Lang, Resolution};
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::path::PathBuf;

pub struct AppState {
    pub root: PathBuf,
    pub index: Option<Index>,
    /// The post-handshake background index warm has been kicked off (clients
    /// may send `initialize` more than once; the warm runs once).
    pub warm_started: bool,
    /// In-flight background LSP enrichment: (tree fingerprint it was started
    /// for, channel the enriched index arrives on).
    lsp_pending: Option<(u64, std::sync::mpsc::Receiver<Index>)>,
    /// Fingerprint for which enrichment already finished (or was found
    /// unavailable) this process — don't respawn until the tree changes.
    lsp_done_fp: Option<u64>,
}

impl AppState {
    pub fn new(root: PathBuf) -> AppState {
        AppState {
            root,
            index: None,
            warm_started: false,
            lsp_pending: None,
            lsp_done_fp: None,
        }
    }

    /// Post-initialization sync: build (or incrementally refresh) the index
    /// on a background thread so the first real tool call finds a warm,
    /// persisted cache instead of paying the cold-build latency. The thread
    /// shares nothing with the serve loop — it only writes the on-disk cache
    /// (atomically), which `ensure_index` then loads by fingerprint. If a
    /// tool call races the warm, both build the same content; the duplicate
    /// work is bounded by one cold build.
    pub fn warm_in_background(&mut self) {
        if self.warm_started {
            return;
        }
        self.warm_started = true;
        let root = self.root.clone();
        std::thread::spawn(move || {
            if let Err(e) = indexer::build_index(&root, false) {
                eprintln!("gigagraph: post-handshake sync failed: {e:#}");
            }
        });
    }

    /// Loads or builds the index, and transparently re-indexes (incrementally)
    /// whenever any indexable file was added, removed, or modified since the
    /// index was built — every tool answers against the current tree. The
    /// staleness probe is stat-only (no file contents are read); a persisted
    /// fingerprint lets fresh processes skip rebuilding an up-to-date index.
    fn ensure_index(&mut self) -> Result<&Index> {
        let fp = indexer::tree_fingerprint(&self.root);
        if self.index.is_none() {
            self.index = indexer::load_index(&self.root);
        }
        let fresh = self
            .index
            .as_ref()
            .is_some_and(|ix| ix.stats.tree_fingerprint == fp);
        if !fresh {
            self.index = Some(indexer::build_index(&self.root, false)?);
        }
        self.maybe_lsp_enrich(fp);
        Ok(self.index.as_ref().unwrap())
    }

    /// Optional LSP enrichment, fully asynchronous: when the current index is
    /// fresh but not yet enriched and a language server is auto-detected,
    /// spawn one background pass (mirrors `warm_in_background` — it never
    /// blocks a query). A finished pass is adopted on the next tool call; on
    /// repos with no detectable server this is two stat calls once per tree
    /// state, then nothing.
    fn maybe_lsp_enrich(&mut self, fp: u64) {
        let Some(ix) = self.index.as_ref() else { return };
        if ix.stats.tree_fingerprint != fp {
            return;
        }
        if ix.stats.lsp_enriched {
            self.lsp_pending = None;
            return;
        }
        if let Some((pending_fp, rx)) = &self.lsp_pending {
            if *pending_fp == fp {
                match rx.try_recv() {
                    Ok(enriched) => {
                        if enriched.stats.tree_fingerprint == fp {
                            self.index = Some(enriched);
                        }
                        self.lsp_pending = None;
                        self.lsp_done_fp = Some(fp);
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        // Thread ended without a result (no provider after
                        // all, tree moved, enrichment abandoned).
                        self.lsp_pending = None;
                        self.lsp_done_fp = Some(fp);
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
                return;
            }
            // Pending pass belongs to an older tree; let it finish and be
            // ignored (its fingerprint check makes adoption safe anyway).
            self.lsp_pending = None;
        }
        if self.lsp_done_fp == Some(fp) {
            return;
        }
        if !crate::lsp::available(&self.root) {
            self.lsp_done_fp = Some(fp);
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let root = self.root.clone();
        std::thread::spawn(move || {
            if let Some(enriched) = crate::lsp::enrich_root(&root, fp) {
                let _ = tx.send(enriched);
            }
        });
        self.lsp_pending = Some((fp, rx));
    }

    pub fn dispatch(&mut self, tool: &str, args: &Value) -> Result<Value> {
        match tool {
            "index_project" => self.index_project(args),
            "index_stats" => self.index_stats(),
            "search_functions" => self.search_functions(args),
            "get_function" => self.get_function(args),
            "get_callers" => self.get_callers(args),
            "get_callees" => self.get_callees(args),
            "find_similar" => self.find_similar(args),
            "call_path" => self.call_path(args),
            "file_overview" => self.file_overview(args),
            "list_packages" => self.list_packages(args),
            "list_endpoints" => self.list_endpoints(args),
            "find_endpoint_callers" => self.find_endpoint_callers(args),
            "get_endpoint" => self.get_endpoint(args),
            "list_client_calls" => self.list_client_calls(args),
            "unreferenced_endpoints" => self.unreferenced_endpoints(args),
            "unreferenced_functions" => self.unreferenced_functions(args),
            "blast_radius" => self.blast_radius(args),
            "affected_tests" => self.affected_tests(args),
            "bridge_map" => self.bridge_map(args),
            "visualize" => self.visualize(args),
            "record_touch" => self.record_touch(args),
            "recent_touches" => self.recent_touches(args),
            _ => bail!("unknown tool: {tool}"),
        }
    }

    /// Blast radius of a change: everything that can transitively reach the
    /// seed function(s) over resolved call edges, correlated endpoint<-client
    /// pairs, and the RN bridge — plus the endpoints and tests in that set.
    fn blast_radius(&mut self, args: &Value) -> Result<Value> {
        let max_depth = args
            .get("max_depth")
            .and_then(Value::as_u64)
            .map(|v| v.clamp(1, 50) as u32)
            .unwrap_or(10);
        let limit = limit_arg(args, 100);
        let index = self.ensure_index()?;
        let g = &index.graph;
        let seeds = impact_seeds(g, args)?;
        let res = impact::blast_radius(g, &seeds, max_depth);

        let mut rows: Vec<(u32, &impact::Impacted)> =
            res.impacted.iter().map(|(&id, imp)| (id, imp)).collect();
        rows.sort_by_key(|&(id, imp)| (imp.depth, imp.confidence == Confidence::Heuristic, id));
        let mut by_depth: std::collections::BTreeMap<u32, usize> = Default::default();
        for (_, imp) in &rows {
            *by_depth.entry(imp.depth).or_default() += 1;
        }
        let functions: Vec<Value> = rows
            .iter()
            .take(limit)
            .map(|&(id, imp)| {
                let f = &g.functions[id as usize];
                json!({
                    "id": format!("fn:{id}"),
                    "qualified_name": f.qualified_name,
                    "file": g.file_of(id).path,
                    "lines": format!("{}-{}", f.start_line, f.end_line),
                    "depth": imp.depth,
                    "confidence": conf_str(imp.confidence),
                    "via": imp.via,
                    "pulled_in_at": format!(
                        "{}:{}",
                        g.files[imp.site.0 as usize].path, imp.site.1
                    ),
                })
            })
            .collect();

        // Endpoints whose handler sits inside the blast (seed handlers at
        // depth 0): the outward-facing surface a change can break.
        let in_set = |id: u32| -> Option<u32> {
            if seeds.contains(&id) {
                Some(0)
            } else {
                res.impacted.get(&id).map(|i| i.depth)
            }
        };
        let affected_endpoints: Vec<Value> = g
            .endpoints
            .endpoints
            .iter()
            .filter_map(|e| {
                let depth = e.handler.and_then(in_set)?;
                let mut v = endpoint_json(g, e);
                v["depth"] = json!(depth);
                Some(v)
            })
            .collect();

        let tests = impact::affected_tests(g, &seeds, &res);
        let seed_summaries: Vec<Value> = seeds
            .iter()
            .take(20)
            .map(|&s| function_summary(g, s))
            .collect();
        Ok(json!({
            "seeds": seed_summaries,
            "seed_count": seeds.len(),
            "max_depth": max_depth,
            "total_impacted": res.impacted.len(),
            "by_depth": by_depth
                .into_iter()
                .map(|(d, n)| (d.to_string(), json!(n)))
                .collect::<serde_json::Map<String, Value>>(),
            "truncated": res.truncated,
            "functions": functions,
            "affected_endpoints": affected_endpoints,
            "affected_test_count": tests.len(),
            "note": "Static closure over resolved call edges, endpoint correlations, and the RN bridge. Dynamic dispatch, reflection, and external/cross-repo callers are invisible; `heuristic` rows mean at least one uncertain edge on the path. Use affected_tests for the test list.",
        }))
    }

    /// Which tests can a change to this function/file dirty? The impacted
    /// subset of the blast radius that is a test (or lives in a test file),
    /// grouped by file — the file is the practical re-run unit.
    fn affected_tests(&mut self, args: &Value) -> Result<Value> {
        let max_depth = args
            .get("max_depth")
            .and_then(Value::as_u64)
            .map(|v| v.clamp(1, 50) as u32)
            .unwrap_or(20);
        let limit = limit_arg(args, 200);
        let index = self.ensure_index()?;
        let g = &index.graph;
        let seeds = impact_seeds(g, args)?;
        let res = impact::blast_radius(g, &seeds, max_depth);
        let tests = impact::affected_tests(g, &seeds, &res);

        // Group by file, keep the shallowest depth per file.
        let mut files: Vec<(u32, u32, Vec<&(u32, u32, Confidence)>)> = Vec::new();
        for t in tests.iter().take(limit) {
            let fid = g.functions[t.0 as usize].file_id;
            match files.iter_mut().find(|(f, ..)| *f == fid) {
                Some((_, min_depth, list)) => {
                    *min_depth = (*min_depth).min(t.1);
                    list.push(t);
                }
                None => files.push((fid, t.1, vec![t])),
            }
        }
        files.sort_by_key(|&(_, d, _)| d);
        let file_rows: Vec<Value> = files
            .iter()
            .map(|(fid, min_depth, list)| {
                let tests: Vec<Value> = list
                    .iter()
                    .map(|(id, depth, conf)| {
                        let f = &g.functions[*id as usize];
                        json!({
                            "id": format!("fn:{id}"),
                            "name": f.name,
                            "lines": format!("{}-{}", f.start_line, f.end_line),
                            "depth": depth,
                            "confidence": conf_str(*conf),
                        })
                    })
                    .collect();
                json!({
                    "file": g.files[*fid as usize].path,
                    "min_depth": min_depth,
                    "tests": tests,
                })
            })
            .collect();
        Ok(json!({
            "seed_count": seeds.len(),
            "total_affected_tests": tests.len(),
            "truncated": res.truncated || tests.len() > limit,
            "files": file_rows,
            "note": "Tests reached over static call edges (plus endpoint/bridge correlations). Helpers defined in test files count as tests — the FILE is the re-run unit. Tests that reach the change only through dynamic dispatch, fixtures, or data files are not listed; an empty result narrows the run, it does not prove no test is affected.",
        }))
    }

    fn index_project(&mut self, args: &Value) -> Result<Value> {
        // Root is fixed at launch (--root); clients may not repoint the
        // server at arbitrary filesystem paths.
        let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
        let index = indexer::build_index(&self.root, force)?;
        let stats = serde_json::to_value(&index.stats)?;
        self.index = Some(index);
        Ok(json!({ "indexed": true, "root": self.root.to_string_lossy(), "stats": stats }))
    }

    fn index_stats(&mut self) -> Result<Value> {
        let root = self.root.to_string_lossy().to_string();
        let index = self.ensure_index()?;
        Ok(json!({ "root": root, "stats": serde_json::to_value(&index.stats)? }))
    }

    fn search_functions(&mut self, args: &Value) -> Result<Value> {
        let query = required_str(args, "query")?.to_string();
        let limit = limit_arg(args, 20);
        let index = self.ensure_index()?;
        let g = &index.graph;
        let q = query.to_lowercase();

        let mut scored: Vec<(i32, u32)> = g
            .functions
            .iter()
            .filter_map(|f| {
                let name = f.name.to_lowercase();
                let qname = f.qualified_name.to_lowercase();
                let score = if name == q {
                    100
                } else if name.starts_with(&q) {
                    80
                } else if name.contains(&q) {
                    60
                } else if qname.contains(&q) {
                    40
                } else if subsequence_match(&name, &q) {
                    20
                } else {
                    return None;
                };
                let penalty = if f.is_toplevel { 15 } else { 0 };
                Some((score - penalty, f.id))
            })
            .collect();
        scored.sort_by_key(|&(s, id)| (-s, id));
        scored.truncate(limit);

        let results: Vec<Value> = scored
            .iter()
            .map(|&(score, id)| {
                let mut v = function_summary(g, id);
                v["match_score"] = json!(score);
                v
            })
            .collect();
        Ok(json!({ "query": query, "results": results }))
    }

    fn get_function(&mut self, args: &Value) -> Result<Value> {
        let target = required_str(args, "function")?.to_string();
        let index = self.ensure_index()?;
        let g = &index.graph;
        let id = resolve_function_ref(g, &target)?;
        let f = &g.functions[id as usize];

        let callees: Vec<Value> = g.calls_by_caller[id as usize]
            .iter()
            .map(|&i| call_site_json(g, i))
            .collect();
        let caller_sites = collect_callers(g, id, 20);
        Ok(json!({
            "function": function_summary(g, id),
            "param_count": f.param_count,
            "containing_type": f.containing_type,
            "packages_used": g.packages_used(id),
            "calls": callees,
            "callers": caller_sites,
        }))
    }

    fn get_callers(&mut self, args: &Value) -> Result<Value> {
        let target = required_str(args, "function")?.to_string();
        let limit = limit_arg(args, 50);
        let index = self.ensure_index()?;
        let g = &index.graph;
        let id = resolve_function_ref(g, &target)?;
        let callers = collect_callers(g, id, limit);
        Ok(json!({
            "function": function_summary(g, id),
            "callers": callers,
        }))
    }

    fn get_callees(&mut self, args: &Value) -> Result<Value> {
        let target = required_str(args, "function")?.to_string();
        let limit = limit_arg(args, 100);
        let index = self.ensure_index()?;
        let g = &index.graph;
        let id = resolve_function_ref(g, &target)?;
        let callees: Vec<Value> = g.calls_by_caller[id as usize]
            .iter()
            .take(limit)
            .map(|&i| call_site_json(g, i))
            .collect();
        Ok(json!({
            "function": function_summary(g, id),
            "calls": callees,
        }))
    }

    fn find_similar(&mut self, args: &Value) -> Result<Value> {
        let limit = limit_arg(args, 10);
        let function = args
            .get("function")
            .and_then(Value::as_str)
            .map(String::from);
        let snippet = args
            .get("snippet")
            .and_then(Value::as_str)
            .map(String::from);
        let language = args
            .get("language")
            .and_then(Value::as_str)
            .map(String::from);
        let index = self.ensure_index()?;
        let g = &index.graph;

        let (query_vec, sem_vec, exclude, subject) = if let Some(target) = function {
            let id = resolve_function_ref(g, &target)?;
            let v = index
                .vectors
                .vector_of(id)
                .ok_or_else(|| anyhow!("no vector for function {id}"))?
                .to_vec();
            let s = index
                .vectors
                .sem_vector_of(id)
                .map(|s| s.to_vec())
                .unwrap_or_default();
            (v, s, Some(id), json!({ "function": function_summary(g, id) }))
        } else if let Some(code) = snippet {
            let lang_name =
                language.ok_or_else(|| anyhow!("`language` is required with `snippet`"))?;
            let lang = Lang::from_name(&lang_name)
                .ok_or_else(|| anyhow!("unknown language: {lang_name}"))?;
            let spec = lang::spec_for_lang(lang)
                .ok_or_else(|| anyhow!("unsupported language: {lang_name}"))?;
            let extracted =
                extract::extract(spec, &code).ok_or_else(|| anyhow!("failed to parse snippet"))?;
            // Prefer the first real function; fall back to top-level scope.
            let func = extracted
                .functions
                .iter()
                .find(|f| !f.is_toplevel)
                .or_else(|| extracted.functions.first())
                .ok_or_else(|| anyhow!("no function found in snippet"))?;
            // The same Tier-1 enrichment the index build applies (verb
            // buckets, subwords, typed locals) — the snippet must be embedded
            // in the same feature space as the indexed functions.
            let mut bag = func.features.clone();
            crate::verbs::augment_bag(&mut bag, &func.locals);
            (
                index.vectors.embed(&bag),
                crate::vector::semantic_embed_bag(&bag),
                None,
                json!({ "snippet_function": func.name }),
            )
        } else {
            bail!("provide either `function` or `snippet` (+`language`)");
        };

        let hits = index.vectors.top_k(&query_vec, &sem_vec, limit + 10, exclude);
        let results: Vec<Value> = hits
            .into_iter()
            .filter(|&(id, _)| !g.functions[id as usize].is_toplevel)
            .take(limit)
            .map(|(id, score)| {
                let mut v = function_summary(g, id);
                v["similarity"] = json!((score * 1000.0).round() / 1000.0);
                v
            })
            .collect();
        Ok(json!({ "subject": subject, "results": results }))
    }

    fn call_path(&mut self, args: &Value) -> Result<Value> {
        let from = required_str(args, "from")?.to_string();
        let to = required_str(args, "to")?.to_string();
        let max_depth = args
            .get("max_depth")
            .and_then(Value::as_u64)
            .unwrap_or(10)
            .min(32) as usize;
        let index = self.ensure_index()?;
        let g = &index.graph;
        let src = resolve_function_ref(g, &from)?;
        let dst = resolve_function_ref(g, &to)?;

        // BFS over resolved internal call edges.
        let mut prev: rustc_hash::FxHashMap<u32, (u32, u32)> = rustc_hash::FxHashMap::default();
        let mut queue = std::collections::VecDeque::new();
        let mut depth_of: rustc_hash::FxHashMap<u32, usize> = rustc_hash::FxHashMap::default();
        queue.push_back(src);
        depth_of.insert(src, 0);
        let mut found = src == dst;
        while let Some(cur) = queue.pop_front() {
            if found {
                break;
            }
            let d = depth_of[&cur];
            if d >= max_depth {
                continue;
            }
            for &ci in &g.calls_by_caller[cur as usize] {
                if let Resolution::Internal { callee, .. } = &g.calls[ci as usize].resolution {
                    if !depth_of.contains_key(callee) {
                        depth_of.insert(*callee, d + 1);
                        prev.insert(*callee, (cur, ci));
                        if *callee == dst {
                            found = true;
                            break;
                        }
                        queue.push_back(*callee);
                    }
                }
            }
        }

        if !found {
            return Ok(json!({
                "found": false,
                "from": function_summary(g, src),
                "to": function_summary(g, dst),
                "max_depth": max_depth,
            }));
        }
        let mut steps: Vec<Value> = Vec::new();
        let mut cur = dst;
        while cur != src {
            let (p, ci) = prev[&cur];
            let call = &g.calls[ci as usize];
            steps.push(json!({
                "caller": g.functions[p as usize].qualified_name,
                "callee": g.functions[cur as usize].qualified_name,
                "at": format!("{}:{}", g.file_of(p).path, call.line),
            }));
            cur = p;
        }
        steps.reverse();
        Ok(json!({
            "found": true,
            "from": function_summary(g, src),
            "to": function_summary(g, dst),
            "steps": steps,
        }))
    }

    fn file_overview(&mut self, args: &Value) -> Result<Value> {
        let path = required_str(args, "path")?.to_string();
        let index = self.ensure_index()?;
        let g = &index.graph;
        let file_id = resolve_file_ref(g, &path)?;
        let file = &g.files[file_id as usize];
        let imports: Vec<Value> = file
            .imports
            .iter()
            .map(|imp| {
                json!({
                    "path": imp.path,
                    "names": imp.names,
                    "line": imp.line,
                    "external_package": imp.external_package,
                    "resolved_file": imp.resolved_file.map(|id| g.files[id as usize].path.clone()),
                })
            })
            .collect();
        let functions: Vec<Value> = g
            .functions
            .iter()
            .filter(|f| f.file_id == file_id)
            .map(|f| function_summary(g, f.id))
            .collect();
        Ok(json!({
            "path": file.path,
            "language": file.language.name(),
            "package": file.package,
            "imports": imports,
            "functions": functions,
        }))
    }

    fn list_packages(&mut self, args: &Value) -> Result<Value> {
        let limit = limit_arg(args, 100);
        let index = self.ensure_index()?;
        let g = &index.graph;
        let mut rows: Vec<(String, usize, usize)> = g
            .package_calls
            .iter()
            .map(|(pkg, idxs)| {
                let callers: rustc_hash::FxHashSet<u32> =
                    idxs.iter().map(|&i| g.calls[i as usize].caller).collect();
                (pkg.clone(), idxs.len(), callers.len())
            })
            .collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        rows.truncate(limit);
        let packages: Vec<Value> = rows
            .into_iter()
            .map(|(pkg, calls, callers)| {
                json!({ "package": pkg, "call_sites": calls, "calling_functions": callers })
            })
            .collect();
        Ok(json!({ "packages": packages }))
    }

    fn list_endpoints(&mut self, args: &Value) -> Result<Value> {
        let limit = limit_arg(args, 100);
        let method = args
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_string);
        let path_q = args
            .get("path")
            .and_then(Value::as_str)
            .map(str::to_lowercase);
        let framework = args
            .get("framework")
            .and_then(Value::as_str)
            .map(str::to_string);
        let kind = args
            .get("kind")
            .and_then(Value::as_str)
            .map(|k| {
                endpoints::ApiKind::from_name(k)
                    .ok_or_else(|| anyhow!("unknown kind: {k} (http/soap/xml-rpc/json-rpc/grpc/graphql)"))
            })
            .transpose()?;
        let index = self.ensure_index()?;
        let g = &index.graph;
        let mut rows = Vec::new();
        for e in &g.endpoints.endpoints {
            if let Some(k) = kind {
                if e.kind != k {
                    continue;
                }
            }
            if let Some(m) = &method {
                if !e.method.as_str().eq_ignore_ascii_case(m) {
                    continue;
                }
            }
            if let Some(q) = &path_q {
                if !e.path_norm.contains(q.as_str())
                    && !e.path_raw.to_lowercase().contains(q.as_str())
                {
                    continue;
                }
            }
            if let Some(f) = &framework {
                if &e.framework != f {
                    continue;
                }
            }
            rows.push(endpoint_json(g, e));
            if rows.len() >= limit {
                break;
            }
        }
        Ok(json!({ "endpoints": rows, "total_detected": g.endpoints.endpoints.len() }))
    }

    fn find_endpoint_callers(&mut self, args: &Value) -> Result<Value> {
        let raw = required_str(args, "path")?.to_string();
        let method = args
            .get("method")
            .and_then(Value::as_str)
            .and_then(endpoints::HttpMethod::from_name);
        let index = self.ensure_index()?;
        let g = &index.graph;
        let norm =
            endpoints::normalize_path(&raw).ok_or_else(|| anyhow!("unusable path: {raw}"))?;
        let mut results = Vec::new();
        for e in &g.endpoints.endpoints {
            if !endpoints::paths_unify(&e.path_norm, &norm) {
                continue;
            }
            if let Some(m) = method {
                if !endpoints::HttpMethod::compatible(e.method, m) {
                    continue;
                }
            }
            let callers: Vec<Value> = g
                .endpoints
                .matches
                .iter()
                .filter(|(_, eid, _)| *eid == e.id)
                .map(|(cid, _, conf)| {
                    let c = &g.endpoints.client_calls[*cid as usize];
                    let mut v = client_json(g, c);
                    v["match_confidence"] = json!(conf_str(*conf));
                    v
                })
                .collect();
            let mut v = endpoint_json(g, e);
            v["callers"] = json!(callers);
            results.push(v);
        }
        Ok(json!({
            "query": { "path": raw, "normalized": norm, "method": method.map(|m| m.as_str()) },
            "endpoints": results,
            "note": "Callers are statically detected in-repo HTTP calls; external/mobile/cross-repo clients are invisible."
        }))
    }

    fn get_endpoint(&mut self, args: &Value) -> Result<Value> {
        let target = required_str(args, "endpoint")?.to_string();
        let index = self.ensure_index()?;
        let g = &index.graph;
        let id: u32 = target
            .strip_prefix("ep:")
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| anyhow!("endpoint reference must be `ep:<id>` (from list_endpoints)"))?;
        let e = g
            .endpoints
            .endpoints
            .get(id as usize)
            .ok_or_else(|| anyhow!("no endpoint ep:{id}"))?;
        let mut v = endpoint_json(g, e);
        if let Some(h) = e.handler {
            v["handler"] = function_summary(g, h);
        }
        let callers: Vec<Value> = g
            .endpoints
            .matches
            .iter()
            .filter(|(_, eid, _)| *eid == e.id)
            .map(|(cid, _, conf)| {
                let c = &g.endpoints.client_calls[*cid as usize];
                let mut cv = client_json(g, c);
                cv["match_confidence"] = json!(conf_str(*conf));
                cv
            })
            .collect();
        v["callers"] = json!(callers);
        Ok(v)
    }

    fn list_client_calls(&mut self, args: &Value) -> Result<Value> {
        let limit = limit_arg(args, 100);
        let unmatched_only = args
            .get("unmatched")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let library = args
            .get("library")
            .and_then(Value::as_str)
            .map(str::to_string);
        let index = self.ensure_index()?;
        let g = &index.graph;
        let matched: rustc_hash::FxHashSet<u32> =
            g.endpoints.matches.iter().map(|(cid, _, _)| *cid).collect();
        let mut rows = Vec::new();
        for c in &g.endpoints.client_calls {
            if unmatched_only && matched.contains(&c.id) {
                continue;
            }
            if let Some(l) = &library {
                if &c.library != l {
                    continue;
                }
            }
            let mut v = client_json(g, c);
            let eps: Vec<String> = g
                .endpoints
                .matches
                .iter()
                .filter(|(cid, _, _)| *cid == c.id)
                .map(|(_, eid, _)| format!("ep:{eid}"))
                .collect();
            v["matched_endpoints"] = json!(eps);
            rows.push(v);
            if rows.len() >= limit {
                break;
            }
        }
        Ok(json!({ "client_calls": rows, "total_detected": g.endpoints.client_calls.len() }))
    }

    fn unreferenced_endpoints(&mut self, args: &Value) -> Result<Value> {
        let limit = limit_arg(args, 100);
        let index = self.ensure_index()?;
        let g = &index.graph;
        let referenced: rustc_hash::FxHashSet<u32> =
            g.endpoints.matches.iter().map(|(_, eid, _)| *eid).collect();
        let rows: Vec<Value> = g
            .endpoints
            .endpoints
            .iter()
            .filter(|e| !referenced.contains(&e.id))
            .take(limit)
            .map(|e| endpoint_json(g, e))
            .collect();
        Ok(json!({
            "unreferenced": rows,
            "note": "No in-repo caller found. External clients (mobile apps, other repos, third parties) are invisible to static indexing — absence of callers here is not proof an endpoint is dead."
        }))
    }

    fn visualize(&mut self, _args: &Value) -> Result<Value> {
        let out = self.root.join(".gigagraph").join("map.html");
        let index = self.ensure_index()?;
        let shown = index
            .graph
            .functions
            .iter()
            .filter(|f| !f.is_toplevel)
            .count();
        crate::viz::write_html_for_index(index, &out)?;
        let abs = out.canonicalize().unwrap_or(out);
        Ok(json!({
            "path": abs.to_string_lossy(),
            "functions": shown,
            "note": "Self-contained interactive 3D code map (no network needed). Tell the user to open this file in a browser."
        }))
    }

    fn record_touch(&mut self, args: &Value) -> Result<Value> {
        let files: Vec<String> = args
            .get("files")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("missing required argument `files` (array of paths)"))?
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        let why = required_str(args, "why")?.to_string();
        let agent = args
            .get("agent")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let outcome = touches::record_touch(&self.root, &files, &why, agent)?;
        let file_counts: Value = outcome
            .file_counts
            .iter()
            .map(|(f, n)| (f.clone(), json!(n)))
            .collect::<serde_json::Map<String, Value>>()
            .into();
        Ok(json!({
            "recorded": touch_json(&outcome.entry),
            "total_entries": outcome.total_entries,
            "file_counts": file_counts,
            "caps": { "global": touches::MAX_GLOBAL, "per_file": touches::MAX_PER_FILE },
        }))
    }

    fn recent_touches(&mut self, args: &Value) -> Result<Value> {
        let file = args.get("file").and_then(Value::as_str).map(str::to_string);
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .map(|v| v.clamp(1, 50) as usize)
            .unwrap_or(10);
        let entries = touches::recent(&self.root, file.as_deref(), limit)?;
        let rows: Vec<Value> = entries.iter().map(touch_json).collect();
        Ok(json!({
            "file": file,
            "touches": rows,
            "note": "Agent-/hook-reported edit history (newest first), not authoritative — `git log` is the real history; this ring adds the WHY and covers uncommitted work."
        }))
    }

    fn unreferenced_functions(&mut self, args: &Value) -> Result<Value> {
        let limit = limit_arg(args, 50);
        let language = args
            .get("language")
            .and_then(Value::as_str)
            .and_then(Lang::from_name);
        let index = self.ensure_index()?;
        let g = &index.graph;

        let handler_ids: rustc_hash::FxHashSet<u32> = g
            .endpoints
            .endpoints
            .iter()
            .filter_map(|e| e.handler)
            .collect();

        let mut strong: Vec<Value> = Vec::new();
        let mut public_unref: Vec<Value> = Vec::new();
        let mut excluded: rustc_hash::FxHashMap<&'static str, u32> = Default::default();

        for f in &g.functions {
            if f.is_toplevel {
                continue;
            }
            if let Some(l) = language {
                if f.language != l {
                    continue;
                }
            }
            if g.called_names.contains(&f.name) || g.referenced_names.contains(&f.name) {
                continue; // referenced somewhere, by some mechanism
            }
            let reason = if f.has_decorations {
                Some("decorated")
            } else if handler_ids.contains(&f.id) {
                Some("endpoint_handler")
            } else {
                entry_point_reason(&f.name)
            };
            if let Some(r) = reason {
                *excluded.entry(r).or_insert(0) += 1;
                continue;
            }
            let mut row = function_summary(g, f.id);
            if f.is_exported || visibility_sniff(&f.signature) {
                row["why_demoted"] = json!("public/exported — external callers invisible");
                if public_unref.len() < limit {
                    public_unref.push(row);
                }
            } else if strong.len() < limit {
                strong.push(row);
            }
        }

        Ok(json!({
            "strong_candidates": strong,
            "public_unreferenced": public_unref,
            "excluded_counts": excluded,
            "caveats": "Static, single-repo view. Cross-language dispatch (JNI, RN bridge, reflection, codegen registration) is invisible. A name referenced anywhere — even at unresolved call sites — is treated as alive. strong_candidates is a REVIEW QUEUE, not a delete list; public_unreferenced only means no in-repo caller."
        }))
    }
}

impl AppState {
    fn bridge_map(&mut self, args: &Value) -> Result<Value> {
        let limit = limit_arg(args, 100);
        let module_filter = args
            .get("module")
            .and_then(Value::as_str)
            .map(str::to_string);
        let index = self.ensure_index()?;
        let g = &index.graph;
        let b = &g.bridge;

        let mut natives: Vec<Value> = Vec::new();
        for n in &b.natives {
            if let Some(m) = &module_filter {
                if &n.module != m && &n.module_alias != m {
                    continue;
                }
            }
            let callers: Vec<Value> = b
                .matches
                .iter()
                .filter(|(_, nid, _)| *nid == n.id)
                .map(|(cid, _, conf)| {
                    let c = &b.calls[*cid as usize];
                    json!({
                        "caller": function_summary(g, c.caller),
                        "at": format!("{}:{}", g.files[c.file_id as usize].path, c.line),
                        "module_named": c.module,
                        "confidence": conf_str(*conf),
                    })
                })
                .collect();
            natives.push(json!({
                "module": n.module,
                "js_alias": n.module_alias,
                "method": n.method,
                "mechanism": n.mechanism,
                "implementation": function_summary(g, n.function),
                "js_callers": callers,
            }));
            if natives.len() >= limit {
                break;
            }
        }

        let matched_calls: rustc_hash::FxHashSet<u32> =
            b.matches.iter().map(|(cid, _, _)| *cid).collect();
        let unmatched_js: Vec<Value> = b
            .calls
            .iter()
            .filter(|c| !matched_calls.contains(&c.id))
            .take(limit)
            .map(|c| {
                json!({
                    "module": c.module,
                    "method": c.method,
                    "caller": function_summary(g, c.caller),
                    "at": format!("{}:{}", g.files[c.file_id as usize].path, c.line),
                })
            })
            .collect();

        Ok(json!({
            "natives": natives,
            "unmatched_js_calls": unmatched_js,
            "note": "Name-based cross-language correlation. Modules registering a custom getName() differ from their class name and may not match; Swift modules need attribute capture; unmatched JS calls may target modules from node_modules (not indexed)."
        }))
    }
}

/// Names that frameworks/runtimes invoke without a visible call site.
fn entry_point_reason(name: &str) -> Option<&'static str> {
    if name == "main" {
        return Some("main");
    }
    if name.starts_with("__") && name.ends_with("__") && name.len() > 4 {
        return Some("dunder");
    }
    if name.starts_with("Java_") {
        return Some("jni_export");
    }
    let follows = |prefix: &str| {
        name.strip_prefix(prefix)
            .and_then(|r| r.chars().next())
            .is_some_and(|c| c.is_uppercase() || c == '_')
    };
    if follows("on") || follows("handle") {
        return Some("callback_convention");
    }
    if name.starts_with("test_") || name.starts_with("Test") || name.ends_with("_test") {
        return Some("test_convention");
    }
    None
}

/// Signature text suggests external visibility (library API surface).
fn visibility_sniff(signature: &str) -> bool {
    let s = signature;
    s.starts_with("pub ")
        || s.starts_with("export ")
        || s.contains("public ")
        || s.contains("extern ")
        || s.starts_with("open ")
        || s.contains("@objc")
}

// ---- helpers ----

/// Relative path, or unique `/`-suffix, to file id.
fn resolve_file_ref(g: &GigaGraph, path: &str) -> Result<u32> {
    g.path_index
        .get(path)
        .copied()
        .or_else(|| {
            let suffix = format!("/{path}");
            let mut hit = None;
            for f in &g.files {
                if f.path.ends_with(&suffix) || f.path == path {
                    if hit.is_some() {
                        return None;
                    }
                    hit = Some(f.id);
                }
            }
            hit
        })
        .ok_or_else(|| anyhow!("file not found (or ambiguous suffix): {path}"))
}

/// Seed set for impact queries: one function (`function`) or every function
/// defined in a file (`file`).
fn impact_seeds(g: &GigaGraph, args: &Value) -> Result<Vec<u32>> {
    if let Some(f) = args.get("function").and_then(Value::as_str) {
        return Ok(vec![resolve_function_ref(g, f)?]);
    }
    if let Some(p) = args.get("file").and_then(Value::as_str) {
        let fid = resolve_file_ref(g, p)?;
        let fns: Vec<u32> = g
            .functions
            .iter()
            .filter(|f| f.file_id == fid)
            .map(|f| f.id)
            .collect();
        if fns.is_empty() {
            bail!("no functions indexed in {p}");
        }
        return Ok(fns);
    }
    bail!("provide `function` (fn ref) or `file` (path) to seed the analysis")
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("missing required argument `{key}`"))
}

fn limit_arg(args: &Value, default: usize) -> usize {
    args.get("limit")
        .and_then(Value::as_u64)
        .map(|v| v.clamp(1, 500) as usize)
        .unwrap_or(default)
}

fn subsequence_match(haystack: &str, needle: &str) -> bool {
    let mut it = haystack.chars();
    needle.chars().all(|c| it.any(|h| h == c))
}

fn touch_json(t: &touches::Touch) -> Value {
    json!({
        "ts": t.ts,
        "files": t.files,
        "why": t.why,
        "agent": t.agent,
    })
}

fn conf_str(c: Confidence) -> &'static str {
    match c {
        Confidence::High => "high",
        Confidence::Heuristic => "heuristic",
    }
}

fn endpoint_json(g: &GigaGraph, e: &endpoints::Endpoint) -> Value {
    let file = &g.files[e.file_id as usize];
    json!({
        "id": format!("ep:{}", e.id),
        "kind": e.kind.as_str(),
        "method": e.method.as_str(),
        "path": e.path_raw,
        "normalized": e.path_norm,
        "framework": e.framework,
        "at": format!("{}:{}", file.path, e.line),
        "handler": e.handler.map(|h| g.functions[h as usize].qualified_name.clone()),
        "confidence": conf_str(e.confidence),
    })
}

fn client_json(g: &GigaGraph, c: &endpoints::ClientCall) -> Value {
    let file = &g.files[c.file_id as usize];
    json!({
        "id": format!("client:{}", c.id),
        "kind": c.kind.as_str(),
        "method": c.method.as_str(),
        "url": c.url_raw,
        "normalized": c.path_norm,
        "library": c.library,
        "caller": function_summary(g, c.caller),
        "at": format!("{}:{}", file.path, c.line),
    })
}

pub fn function_summary(g: &GigaGraph, id: u32) -> Value {
    let f = &g.functions[id as usize];
    json!({
        "id": format!("fn:{id}"),
        "name": f.name,
        "qualified_name": f.qualified_name,
        "language": f.language.name(),
        "file": g.file_of(id).path,
        "lines": format!("{}-{}", f.start_line, f.end_line),
        "signature": f.signature,
    })
}

fn call_site_json(g: &GigaGraph, call_idx: u32) -> Value {
    let call = &g.calls[call_idx as usize];
    let caller_file = g.file_of(call.caller);
    let mut v = json!({
        "name": call.name,
        "receiver": call.receiver,
        "at": format!("{}:{}", caller_file.path, call.line),
        "args": call.arg_count,
    });
    match &call.resolution {
        Resolution::Internal {
            callee,
            confidence,
            ambiguous_with,
        } => {
            v["resolved"] = json!("internal");
            v["callee"] = json!(g.functions[*callee as usize].qualified_name);
            v["callee_id"] = json!(format!("fn:{callee}"));
            // "lsp" = a real language server confirmed this edge (strictly
            // stronger than the static "high").
            v["confidence"] = json!(if g.lsp_confirmed.contains(&call_idx) {
                "lsp"
            } else {
                match confidence {
                    Confidence::High => "high",
                    Confidence::Heuristic => "heuristic",
                }
            });
            if !ambiguous_with.is_empty() {
                v["ambiguous_with"] = json!(
                    ambiguous_with
                        .iter()
                        .map(|id| g.functions[*id as usize].qualified_name.clone())
                        .collect::<Vec<_>>()
                );
            }
        }
        Resolution::External { package } => {
            v["resolved"] = json!("external");
            v["package"] = json!(package);
        }
        Resolution::Unresolved => {
            v["resolved"] = json!("unresolved");
        }
    }
    v
}

fn collect_callers(g: &GigaGraph, id: u32, limit: usize) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    if let Some(idxs) = g.callers_of.get(&id) {
        for &i in idxs.iter().take(limit) {
            let call = &g.calls[i as usize];
            let mut v = function_summary(g, call.caller);
            v["call_at"] = json!(format!("{}:{}", g.file_of(call.caller).path, call.line));
            out.push(v);
        }
    }
    // Ambiguous mentions: calls resolved elsewhere that list `id` as a
    // plausible alternative.
    if out.len() < limit {
        for (i, call) in g.calls.iter().enumerate() {
            if let Resolution::Internal { ambiguous_with, .. } = &call.resolution {
                if ambiguous_with.contains(&id) {
                    let mut v = function_summary(g, call.caller);
                    v["call_at"] = json!(format!("{}:{}", g.file_of(call.caller).path, call.line));
                    v["ambiguous"] = json!(true);
                    out.push(v);
                    if out.len() >= limit {
                        break;
                    }
                }
            }
            let _ = i;
        }
    }
    out
}

/// Resolves a user-supplied function reference: `fn:<id>`, exact qualified
/// name, or a simple name (must be unambiguous — otherwise the error lists
/// candidates).
pub fn resolve_function_ref(g: &GigaGraph, target: &str) -> Result<u32> {
    if let Some(id_str) = target.strip_prefix("fn:") {
        let id: u32 = id_str
            .parse()
            .map_err(|_| anyhow!("bad function id: {target}"))?;
        if (id as usize) < g.functions.len() {
            return Ok(id);
        }
        bail!("function id out of range: {target}");
    }
    if let Some(ids) = g.qname_index.get(target) {
        if ids.len() == 1 {
            return Ok(ids[0]);
        }
        bail!(
            "qualified name matches {} overloads; use an id: {}",
            ids.len(),
            ids.iter()
                .map(|id| format!("fn:{id}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if let Some(ids) = g.name_index.get(target) {
        let real: Vec<u32> = ids
            .iter()
            .copied()
            .filter(|&id| !g.functions[id as usize].is_toplevel)
            .collect();
        match real.len() {
            0 => {}
            1 => return Ok(real[0]),
            _ => bail!(
                "name `{target}` is ambiguous ({} matches): {}",
                real.len(),
                real.iter()
                    .take(10)
                    .map(|&id| { format!("fn:{id} ({})", g.functions[id as usize].qualified_name) })
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        }
    }
    bail!("function not found: {target} (try search_functions)")
}
