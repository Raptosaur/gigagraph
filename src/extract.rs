//! Language-agnostic extraction: runs a LangSpec's query over one file and
//! produces functions, call sites, imports, and semantic feature bags.

use crate::lang::LangSpec;
use crate::types::{Import, Lang};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Parser, QueryCursor};

pub const TOPLEVEL_NAME: &str = "(toplevel)";
const MAX_SIGNATURE_LEN: usize = 160;
const MAX_IDENT_FEATURE_LEN: usize = 64;
const MAX_RECEIVER_LEN: usize = 48;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawCall {
    pub name: String,
    pub receiver: Option<String>,
    pub line: u32,
    pub arg_count: u16,
    pub start_byte: u32,
    pub end_byte: u32,
    /// Distilled literal/identifier arguments (capped); fuels endpoint and
    /// client-call detection without keeping full argument text.
    pub arg_lits: Vec<ArgLit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LitKind {
    Str,
    Ident,
}

/// One harvested argument: `index` is the top-level argument position; `key`
/// is the kwarg/object-field name when the value sat one level down
/// (`methods=["POST"]`, `{ method: "PUT" }`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArgLit {
    pub index: u16,
    pub key: Option<String>,
    pub kind: LitKind,
    pub text: String,
}

/// A decorator/annotation/attribute attached to a function or type
/// (`@app.route("/x")`, `@GetMapping("/x")`, `#[Route('/x')]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawDecoration {
    /// Dotted/qualified name as written, e.g. `app.route`, `GetMapping`.
    pub name: String,
    pub arg_lits: Vec<ArgLit>,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedFunction {
    pub name: String,
    pub start_line: u32,
    pub end_line: u32,
    pub start_byte: u32,
    pub end_byte: u32,
    pub signature: String,
    pub containing_type: Option<String>,
    pub param_count: u16,
    pub is_toplevel: bool,
    /// Definition sits under an export wrapper node (JS/TS `export`).
    pub is_exported: bool,
    pub calls: Vec<RawCall>,
    pub decorations: Vec<RawDecoration>,
    /// String literals this function returns (`return "Payments";`,
    /// Kotlin `= "Payments"`), capped — fuels RN getName() aliasing.
    pub ret_strs: Vec<String>,
    /// Semantic feature bag (feature string -> term frequency) used for the
    /// similarity vectors. Structural, not textual: callee names, identifiers,
    /// AST shape, control flow.
    pub features: FxHashMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedFile {
    pub language: Lang,
    pub package: Option<String>,
    pub imports: Vec<Import>,
    pub functions: Vec<ExtractedFunction>,
    /// Class/type-level decorations (`@Controller("users")`,
    /// `[Route("api/[controller]")]`): (type name, decoration).
    pub type_decorations: Vec<(String, RawDecoration)>,
    /// Single-assignment string constants (`const API = "/users"`,
    /// `static final String BASE = "/api"`): name -> literal.
    pub consts: Vec<(String, String)>,
}

struct Caps {
    func_def: Option<u32>,
    func_name: Option<u32>,
    func_params: Option<u32>,
    func_body: Option<u32>,
    call: Option<u32>,
    call_name: Option<u32>,
    call_recv: Option<u32>,
    call_args: Option<u32>,
    import: Option<u32>,
    import_path: Option<u32>,
    import_path_system: Option<u32>,
    import_name: Option<u32>,
    package_name: Option<u32>,
    deco: Option<u32>,
    deco_name: Option<u32>,
    deco_args: Option<u32>,
    deco_type: Option<u32>,
    const_name: Option<u32>,
    const_value: Option<u32>,
    ret_str: Option<u32>,
}

impl Caps {
    fn new(query: &tree_sitter::Query) -> Caps {
        let idx = |name: &str| query.capture_index_for_name(name);
        Caps {
            func_def: idx("func.def"),
            func_name: idx("func.name"),
            func_params: idx("func.params"),
            func_body: idx("func.body"),
            call: idx("call"),
            call_name: idx("call.name"),
            call_recv: idx("call.recv"),
            call_args: idx("call.args"),
            import: idx("import"),
            import_path: idx("import.path"),
            import_path_system: idx("import.path.system"),
            import_name: idx("import.name"),
            package_name: idx("package.name"),
            deco: idx("deco"),
            deco_name: idx("deco.name"),
            deco_args: idx("deco.args"),
            deco_type: idx("deco.type"),
            const_name: idx("const.name"),
            const_value: idx("const.value"),
            ret_str: idx("ret.str"),
        }
    }
}

/// A decoration seen during the match loop, with enough context to associate
/// it with its function after functions are collected.
struct DecoCand {
    deco: RawDecoration,
    end_byte: u32,
    /// Byte range of a `@func.def` captured in the same query match, if any —
    /// the strongest association (Java/C# annotations inside the definition).
    same_match_def: Option<(u32, u32)>,
    /// Decorated type name (`@deco.type`) for class-level decorations.
    type_name: Option<String>,
}

#[derive(Default)]
struct ImportBuilder {
    path: Option<String>,
    system: bool,
    names: Vec<String>,
    line: u32,
}

struct FuncCand<'t> {
    def: Node<'t>,
    name: String,
    params: Option<Node<'t>>,
    body: Option<Node<'t>>,
}

struct CallCand<'t> {
    node: Node<'t>,
    name: String,
    recv: Option<Node<'t>>,
    args: Option<Node<'t>>,
}

pub fn extract(spec: &LangSpec, source: &str) -> Option<ExtractedFile> {
    let mut parser = Parser::new();
    parser.set_language(&spec.language).ok()?;
    let tree = parser.parse(source, None)?;
    let root = tree.root_node();
    let bytes = source.as_bytes();
    let caps = Caps::new(&spec.query);

    let mut funcs: Vec<FuncCand> = Vec::new();
    let mut seen_funcs: FxHashSet<(u32, u32)> = FxHashSet::default();
    let mut decos: Vec<DecoCand> = Vec::new();
    let mut seen_decos: FxHashSet<(u32, u32)> = FxHashSet::default();
    let mut consts: Vec<(String, String)> = Vec::new();
    // (start byte, text) — attributed to the innermost function later.
    let mut ret_strs: Vec<(u32, String)> = Vec::new();
    let mut calls: Vec<CallCand> = Vec::new();
    let mut call_slots: FxHashMap<(u32, u32, u32), usize> = FxHashMap::default();
    let mut imports: FxHashMap<usize, ImportBuilder> = FxHashMap::default();
    let mut import_order: Vec<usize> = Vec::new();
    let mut package: Option<String> = None;

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&spec.query, root, bytes);
    while let Some(m) = matches.next() {
        let get = |want: Option<u32>| -> Option<Node> {
            want.and_then(|w| m.captures.iter().find(|c| c.index == w).map(|c| c.node))
        };

        if let Some(def) = get(caps.func_def) {
            if seen_funcs.insert((def.start_byte() as u32, def.end_byte() as u32)) {
                if let Some(name_node) = get(caps.func_name) {
                    funcs.push(FuncCand {
                        def,
                        name: node_text(name_node, source),
                        params: get(caps.func_params),
                        body: get(caps.func_body),
                    });
                }
            }
        }

        if let Some(call_node) = get(caps.call) {
            if let Some(name_node) = get(caps.call_name) {
                let key = (
                    call_node.start_byte() as u32,
                    call_node.end_byte() as u32,
                    name_node.start_byte() as u32,
                );
                let recv = get(caps.call_recv);
                match call_slots.get(&key) {
                    Some(&slot) => {
                        // Same call matched by several patterns (e.g. with and
                        // without a receiver clause) — keep the richer info.
                        if calls[slot].recv.is_none() {
                            calls[slot].recv = recv;
                        }
                        if calls[slot].args.is_none() {
                            calls[slot].args = get(caps.call_args);
                        }
                    }
                    None => {
                        call_slots.insert(key, calls.len());
                        calls.push(CallCand {
                            node: call_node,
                            name: node_text(name_node, source),
                            recv,
                            args: get(caps.call_args),
                        });
                    }
                }
            }
        }

        if let Some(import_node) = get(caps.import) {
            let id = import_node.id();
            let builder = imports.entry(id).or_insert_with(|| {
                import_order.push(id);
                ImportBuilder {
                    line: import_node.start_position().row as u32 + 1,
                    ..Default::default()
                }
            });
            if let Some(p) = get(caps.import_path) {
                builder.path = Some(strip_quotes(&node_text(p, source)));
            }
            if let Some(p) = get(caps.import_path_system) {
                builder.path = Some(strip_quotes(&node_text(p, source)));
                builder.system = true;
            }
            if let Some(n) = get(caps.import_name) {
                let t = node_text(n, source);
                if !builder.names.contains(&t) {
                    builder.names.push(t);
                }
            }
        }

        if let Some(deco_node) = get(caps.deco) {
            let key = (deco_node.start_byte() as u32, deco_node.end_byte() as u32);
            if seen_decos.insert(key) {
                if let Some(name_node) = get(caps.deco_name) {
                    decos.push(DecoCand {
                        deco: RawDecoration {
                            name: node_text(name_node, source),
                            arg_lits: get(caps.deco_args)
                                .map(|a| harvest_arg_lits(spec, a, source))
                                .unwrap_or_default(),
                            line: deco_node.start_position().row as u32 + 1,
                        },
                        end_byte: deco_node.end_byte() as u32,
                        same_match_def: get(caps.func_def)
                            .map(|d| (d.start_byte() as u32, d.end_byte() as u32)),
                        type_name: get(caps.deco_type).map(|t| node_text(t, source)),
                    });
                }
            }
        }

        if let (Some(n), Some(v)) = (get(caps.const_name), get(caps.const_value)) {
            if consts.len() < 200 {
                let value = strip_quotes(&node_text(v, source));
                if value.len() <= MAX_LIT_LEN {
                    consts.push((node_text(n, source), value));
                }
            }
        }

        if let Some(r) = get(caps.ret_str) {
            if ret_strs.len() < 200 {
                let text = strip_quotes(&node_text(r, source));
                if !text.is_empty() && text.len() <= 64 {
                    ret_strs.push((r.start_byte() as u32, text));
                }
            }
        }

        if package.is_none() {
            if let Some(p) = get(caps.package_name) {
                package = Some(node_text(p, source));
            }
        }
    }

    // Innermost-function assignment needs functions ordered by start byte.
    funcs.sort_by_key(|f| (f.def.start_byte(), std::cmp::Reverse(f.def.end_byte())));

    let mut out_funcs: Vec<ExtractedFunction> = funcs
        .iter()
        .map(|f| build_function(spec, f, source))
        .collect();

    // Prefix max of end bytes lets the containment scan bail out early.
    let mut prefix_max_end: Vec<u32> = Vec::with_capacity(funcs.len());
    let mut running = 0u32;
    for f in &funcs {
        running = running.max(f.def.end_byte() as u32);
        prefix_max_end.push(running);
    }

    let mut toplevel_calls: Vec<RawCall> = Vec::new();
    for c in &calls {
        let pos = c.node.start_byte() as u32;
        let raw = RawCall {
            name: c.name.clone(),
            receiver: c
                .recv
                .and_then(|r| sanitize_receiver(&node_text(r, source))),
            line: c.node.start_position().row as u32 + 1,
            arg_count: c.args.map(|a| count_named(a)).unwrap_or(0),
            start_byte: c.node.start_byte() as u32,
            end_byte: c.node.end_byte() as u32,
            arg_lits: c
                .args
                .map(|a| harvest_arg_lits(spec, a, source))
                .unwrap_or_default(),
        };
        // Walk backwards from the last function starting at/before `pos`;
        // the first one whose range contains the call is the innermost.
        let start_idx = funcs.partition_point(|f| f.def.start_byte() as u32 <= pos);
        let mut owner: Option<usize> = None;
        for i in (0..start_idx).rev() {
            if prefix_max_end[i] <= pos {
                break; // nothing earlier can contain this position
            }
            if funcs[i].def.end_byte() as u32 > pos {
                owner = Some(i);
                break;
            }
        }
        match owner {
            Some(i) => out_funcs[i].calls.push(raw),
            None => toplevel_calls.push(raw),
        }
    }

    // Returned string literals attach to their innermost function (same
    // containment scan as calls); toplevel ones are dropped.
    for (pos, text) in ret_strs {
        let start_idx = funcs.partition_point(|f| f.def.start_byte() as u32 <= pos);
        for i in (0..start_idx).rev() {
            if prefix_max_end[i] <= pos {
                break;
            }
            if funcs[i].def.end_byte() as u32 > pos {
                if out_funcs[i].ret_strs.len() < 2 {
                    out_funcs[i].ret_strs.push(text);
                }
                break;
            }
        }
    }

    if !toplevel_calls.is_empty() {
        let mut features = FxHashMap::default();
        for c in &toplevel_calls {
            *features.entry(format!("call:{}", c.name)).or_insert(0) += 1;
        }
        features.insert("ast:toplevel".to_string(), 1);
        out_funcs.push(ExtractedFunction {
            name: TOPLEVEL_NAME.to_string(),
            start_line: 1,
            end_line: root.end_position().row as u32 + 1,
            start_byte: 0,
            end_byte: root.end_byte() as u32,
            signature: TOPLEVEL_NAME.to_string(),
            containing_type: None,
            param_count: 0,
            is_toplevel: true,
            is_exported: false,
            calls: toplevel_calls,
            decorations: Vec::new(),
            ret_strs: Vec::new(),
            features,
        });
    }

    // Fold call names into each function's feature bag.
    for f in &mut out_funcs {
        if f.is_toplevel {
            continue;
        }
        let call_feats: Vec<String> = f.calls.iter().map(|c| format!("call:{}", c.name)).collect();
        for feat in call_feats {
            *f.features.entry(feat).or_insert(0) += 1;
        }
    }

    // Attach decorations: same-match definition wins; class-level decorations
    // go to the file; otherwise the nearest function starting after the
    // decoration ends (Python decorators, TS method decorators).
    let mut type_decorations: Vec<(String, RawDecoration)> = Vec::new();
    let mut orphan_decos: Vec<RawDecoration> = Vec::new();
    for d in decos {
        if let Some((s, e)) = d.same_match_def {
            if let Some(i) = funcs
                .iter()
                .position(|f| f.def.start_byte() as u32 == s && f.def.end_byte() as u32 == e)
            {
                out_funcs[i].decorations.push(d.deco);
                continue;
            }
        }
        if let Some(t) = d.type_name {
            type_decorations.push((t, d.deco));
            continue;
        }
        let idx = funcs.partition_point(|f| (f.def.start_byte() as u32) < d.end_byte);
        match out_funcs.get_mut(idx) {
            Some(f) => f.decorations.push(d.deco),
            // No following function (e.g. a trailing RCT_EXTERN_METHOD whose
            // body is macro wreckage): park on the synthetic toplevel.
            None => orphan_decos.push(d.deco),
        }
    }
    if !orphan_decos.is_empty() {
        match out_funcs.last_mut().filter(|f| f.is_toplevel) {
            Some(top) => top.decorations.extend(orphan_decos),
            None => out_funcs.push(ExtractedFunction {
                name: TOPLEVEL_NAME.to_string(),
                start_line: 1,
                end_line: root.end_position().row as u32 + 1,
                start_byte: 0,
                end_byte: root.end_byte() as u32,
                signature: TOPLEVEL_NAME.to_string(),
                containing_type: None,
                param_count: 0,
                is_toplevel: true,
                is_exported: false,
                calls: Vec::new(),
                decorations: orphan_decos,
                ret_strs: Vec::new(),
                features: FxHashMap::default(),
            }),
        }
    }

    let imports = import_order
        .into_iter()
        .filter_map(|id| {
            let b = imports.remove(&id)?;
            let path = b.path?;
            Some(Import {
                path,
                names: b.names,
                line: b.line,
                system: b.system,
                external_package: None,
                resolved_file: None,
            })
        })
        .collect();

    Some(ExtractedFile {
        language: spec.lang,
        package,
        imports,
        functions: out_funcs,
        type_decorations,
        consts,
    })
}

const MAX_ARG_LITS: usize = 8;
const MAX_LIT_LEN: usize = 200;

/// Distill string literals and identifier-ish arguments from a call's
/// argument-list node. Depth 1: direct children. Depth 2: kwarg / object-field
/// / array values, tagged with their key where one exists.
fn harvest_arg_lits(spec: &LangSpec, args: Node, source: &str) -> Vec<ArgLit> {
    let mut out = Vec::new();
    let mut cursor = args.walk();
    let children: Vec<Node> = args
        .children(&mut cursor)
        .filter(|c| c.is_named())
        .collect();
    for (i, child) in children.into_iter().enumerate() {
        if out.len() >= MAX_ARG_LITS {
            break;
        }
        let index = i as u16;
        if let Some(lit) = classify_lit(spec, child, source) {
            out.push(ArgLit {
                index,
                key: None,
                kind: lit.0,
                text: lit.1,
            });
            continue;
        }
        // One level down: keyed pairs (kwargs, object fields) and list items.
        let mut c2 = child.walk();
        let grandchildren: Vec<Node> = child.children(&mut c2).filter(|c| c.is_named()).collect();
        // Keyed shape: first named child is the key, value(s) follow
        // (python keyword_argument, JS pair, PHP named argument, C# name:).
        let keyed = child.kind().contains("argument")
            || child.kind().contains("pair")
            || child.kind().contains("field")
            || child.kind().contains("element_init");
        let key_text = if keyed && grandchildren.len() >= 2 {
            let k = grandchildren[0];
            let t = node_text(k, source);
            (t.len() <= 64 && !t.contains('\n')).then(|| strip_quotes(&t))
        } else {
            None
        };
        for gc in grandchildren
            .iter()
            .skip(if key_text.is_some() { 1 } else { 0 })
        {
            if out.len() >= MAX_ARG_LITS {
                break;
            }
            if let Some((kind, text)) = classify_lit(spec, *gc, source) {
                out.push(ArgLit {
                    index,
                    key: key_text.clone(),
                    kind,
                    text,
                });
            } else {
                // Depth 2 proper: object/dict/array values under the pair.
                let mut c3 = gc.walk();
                for ggc in gc.children(&mut c3).filter(|c| c.is_named()) {
                    if out.len() >= MAX_ARG_LITS {
                        break;
                    }
                    let inner_key = if key_text.is_some() {
                        key_text.clone()
                    } else {
                        let mut ck = ggc.walk();
                        let parts: Vec<Node> =
                            ggc.children(&mut ck).filter(|c| c.is_named()).collect();
                        if (ggc.kind() == "pair"
                            || ggc.kind().contains("argument")
                            || ggc.kind().contains("element_init"))
                            && parts.len() >= 2
                        {
                            let t = node_text(parts[0], source);
                            if let Some((kind, text)) = classify_lit(spec, parts[1], source) {
                                out.push(ArgLit {
                                    index,
                                    key: Some(strip_quotes(&t)),
                                    kind,
                                    text,
                                });
                            }
                            continue;
                        }
                        None
                    };
                    if let Some((kind, text)) = classify_lit(spec, ggc, source) {
                        out.push(ArgLit {
                            index,
                            key: inner_key,
                            kind,
                            text,
                        });
                    }
                }
            }
        }
    }
    out
}

fn classify_lit(spec: &LangSpec, node: Node, source: &str) -> Option<(LitKind, String)> {
    // Unwrap single-child wrapper nodes (C# attribute_argument, PHP
    // array_element_initializer) so the literal inside is reachable.
    let mut node = node;
    for _ in 0..2 {
        if spec.string_kinds.contains(&node.kind()) || node.named_child_count() != 1 {
            break;
        }
        node = node.named_child(0).unwrap();
    }
    let kind = node.kind();
    if spec.string_kinds.contains(&kind) {
        let text = strip_quotes(&node_text(node, source));
        return (text.len() <= MAX_LIT_LEN).then_some((LitKind::Str, text));
    }
    if spec.identifier_kinds.contains(&kind)
        || kind == "scoped_identifier"
        || kind == "attribute"
        || kind == "member_expression"
        || kind == "qualified_name"
        || kind == "selector_expression"
        || kind == "field_access"
    {
        let text = node_text(node, source);
        return (text.len() <= MAX_LIT_LEN && !text.contains('\n'))
            .then_some((LitKind::Ident, text));
    }
    None
}

/// First line of the declaration itself, skipping leading annotation /
/// attribute lines (`@Test`, `@available(...)`) — including multi-line
/// annotation argument lists. Falls back to the raw first line when the whole
/// definition starts with an annotation on the declaration line itself.
fn signature_line(sig_src: &str) -> &str {
    let paren_delta = |s: &str| {
        s.chars().fold(0i32, |d, c| match c {
            '(' => d + 1,
            ')' => d - 1,
            _ => d,
        })
    };
    let mut depth = 0i32;
    for line in sig_src.lines() {
        let t = line.trim();
        if depth > 0 {
            depth += paren_delta(t);
            continue;
        }
        if t.is_empty() {
            continue;
        }
        if t.starts_with('@') {
            depth += paren_delta(t);
            continue;
        }
        return t;
    }
    sig_src.lines().next().unwrap_or("").trim()
}

fn build_function(spec: &LangSpec, f: &FuncCand, source: &str) -> ExtractedFunction {
    let def = f.def;
    let params_node = f
        .params
        .or_else(|| f.body.and_then(|b| b.child_by_field_name("parameters")))
        .or_else(|| f.body.and_then(|b| b.child_by_field_name("parameter")))
        .or_else(|| def.child_by_field_name("parameters"));
    let mut param_count = params_node.map(|p| count_named(p)).unwrap_or(0);
    // A bare single-identifier arrow param (`x => ...`) has no list node.
    if param_count == 0 {
        if let Some(p) = params_node {
            if p.is_named() && p.kind().contains("identifier") {
                param_count = 1;
            }
        }
    }
    // C: `f(void)` parses as one parameter_declaration but means zero params.
    if spec.lang == Lang::C && param_count == 1 {
        if let Some(p) = params_node {
            if node_text(p, source).replace([' ', '(', ')'], "") == "void" {
                param_count = 0;
            }
        }
    }

    let sig_src = &source[def.start_byte()..def.end_byte()];
    let signature: String = signature_line(sig_src)
        .chars()
        .take(MAX_SIGNATURE_LEN)
        .collect();

    let mut features = FxHashMap::default();
    collect_features(spec, def, source, &mut features);
    let n = param_count.min(12);
    *features.entry(format!("arity:{n}")).or_insert(0) += 1;
    let lines = def
        .end_position()
        .row
        .saturating_sub(def.start_position().row)
        + 1;
    let bucket = (lines as f64).log2() as u32;
    *features.entry(format!("size:{bucket}")).or_insert(0) += 1;

    ExtractedFunction {
        name: f.name.clone(),
        start_line: def.start_position().row as u32 + 1,
        end_line: def.end_position().row as u32 + 1,
        start_byte: def.start_byte() as u32,
        end_byte: def.end_byte() as u32,
        signature,
        containing_type: containing_type(spec, def, source),
        param_count,
        is_toplevel: false,
        is_exported: is_export_wrapped(def),
        calls: Vec::new(),
        decorations: docblock_decorations(def, source),
        ret_strs: Vec::new(),
        features,
    }
}

/// Docblock annotations preceding a definition become decorations —
/// legacy Symfony/Doctrine-style routing (`/** @Route("/x",
/// methods={"GET"}) */`) predates PHP attributes. Only uppercase-initial
/// names followed by `(` qualify, which skips `@param`/`@return` noise.
fn docblock_decorations(def: Node, source: &str) -> Vec<RawDecoration> {
    let mut out = Vec::new();
    let mut prev = def.prev_named_sibling();
    let mut seen = 0;
    while let Some(p) = prev {
        if !p.kind().contains("comment") || seen >= 3 {
            break;
        }
        let text = node_text(p, source);
        if text.starts_with("/**") {
            parse_docblock_annotations(&text, p.start_position().row as u32 + 1, &mut out);
        }
        seen += 1;
        prev = p.prev_named_sibling();
    }
    out
}

fn parse_docblock_annotations(text: &str, line: u32, out: &mut Vec<RawDecoration>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() && out.len() < 8 {
        if bytes[i] != b'@' {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut j = start;
        while j < bytes.len()
            && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_' || bytes[j] == b'\\')
        {
            j += 1;
        }
        let name = &text[start..j];
        let simple = name.rsplit('\\').next().unwrap_or(name);
        if simple.is_empty()
            || !simple.chars().next().is_some_and(|c| c.is_uppercase())
            || j >= bytes.len()
            || bytes[j] != b'('
        {
            i = j.max(i + 1);
            continue;
        }
        // Balance parens to find the argument span.
        let mut depth = 0;
        let mut k = j;
        while k < bytes.len() {
            match bytes[k] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            k += 1;
        }
        let inner = &text[j + 1..k.min(text.len())];
        out.push(RawDecoration {
            name: simple.to_string(),
            arg_lits: parse_annotation_args(inner),
            line,
        });
        i = k + 1;
    }
}

/// `"/path", methods={"GET","POST"}, name="x"` -> ArgLits: keyless Str for
/// bare quoted values, keyed Strs for `key=`-prefixed ones (brace lists
/// yield one lit per element).
fn parse_annotation_args(inner: &str) -> Vec<ArgLit> {
    let mut lits = Vec::new();
    let mut key: Option<String> = None;
    let mut pending_key = String::new();
    let mut chars = inner.char_indices().peekable();
    let mut index: u16 = 0;
    let mut brace = 0i32;
    while let Some((_, c)) = chars.next() {
        match c {
            '"' | '\'' => {
                let mut s = String::new();
                for (_, c2) in chars.by_ref() {
                    if c2 == c {
                        break;
                    }
                    s.push(c2);
                }
                if !s.is_empty() && s.len() <= MAX_LIT_LEN && lits.len() < MAX_ARG_LITS {
                    lits.push(ArgLit {
                        index,
                        key: key.clone(),
                        kind: LitKind::Str,
                        text: s,
                    });
                }
                pending_key.clear();
            }
            '=' => {
                key = (!pending_key.is_empty()).then(|| pending_key.trim().to_string());
                pending_key.clear();
            }
            ',' => {
                // Outside braces a comma ends the keyed group; inside a
                // brace list the key persists across elements.
                if brace == 0 {
                    key = None;
                    index += 1;
                }
                pending_key.clear();
            }
            '{' => brace += 1,
            '}' => {
                brace -= 1;
                if brace <= 0 {
                    key = None;
                }
            }
            _ if c.is_alphanumeric() || c == '_' => pending_key.push(c),
            _ => {}
        }
    }
    lits
}

/// JS/TS `export function f()` wraps the definition in an export node; the
/// signature's first line never shows the keyword, so check ancestry (two
/// levels covers `export default`).
fn is_export_wrapped(def: Node) -> bool {
    let mut p = def.parent();
    for _ in 0..2 {
        match p {
            Some(n) if n.kind().starts_with("export") => return true,
            Some(n) => p = n.parent(),
            None => break,
        }
    }
    false
}

/// Iterative subtree walk collecting AST-shape, identifier, and control-flow
/// features. Iterative to survive pathologically deep trees (minified JS).
fn collect_features(
    spec: &LangSpec,
    def: Node,
    source: &str,
    features: &mut FxHashMap<String, u32>,
) {
    let mut loops = 0u32;
    let mut branches = 0u32;
    let mut flow_depth = 0u32;
    let mut max_flow_depth = 0u32;
    let mut doc_budget = MAX_DOC_TOKENS;
    let is_loop = |k: &str| spec.loop_kinds.contains(&k);
    let is_branch = |k: &str| spec.branch_kinds.contains(&k);
    let is_ident = |k: &str| spec.identifier_kinds.contains(&k);

    // Doc comments directly above the definition belong to it: walk backwards
    // over consecutive preceding comment siblings (`///` lines are separate
    // nodes in several grammars).
    let mut prev = def.prev_named_sibling();
    let mut leading = 0;
    while let Some(p) = prev {
        if !p.kind().contains("comment") || leading >= 10 {
            break;
        }
        add_doc_tokens(&node_text(p, source), features, &mut doc_budget);
        leading += 1;
        prev = p.prev_named_sibling();
    }
    // Python-style docstring: a bare string literal as the body's first
    // statement (harmlessly absent everywhere else).
    if let Some(body) = def.child_by_field_name("body") {
        if let Some(first) = body.named_child(0) {
            let s = if spec.string_kinds.contains(&first.kind()) {
                Some(first)
            } else {
                first
                    .named_child(0)
                    .filter(|c| spec.string_kinds.contains(&c.kind()))
            };
            if let Some(s) = s {
                add_doc_tokens(&node_text(s, source), features, &mut doc_budget);
            }
        }
    }

    let mut cursor = def.walk();
    'outer: loop {
        let node = cursor.node();
        if node.is_named() {
            let kind = node.kind();
            *features.entry(format!("ast:{kind}")).or_insert(0) += 1;
            if is_ident(kind) {
                let text = node_text(node, source);
                if !text.is_empty() && text.len() <= MAX_IDENT_FEATURE_LEN {
                    *features.entry(format!("id:{text}")).or_insert(0) += 1;
                }
            } else if kind.contains("comment") {
                add_doc_tokens(&node_text(node, source), features, &mut doc_budget);
            }
            if is_loop(kind) {
                loops += 1;
                flow_depth += 1;
                max_flow_depth = max_flow_depth.max(flow_depth);
            } else if is_branch(kind) {
                branches += 1;
                flow_depth += 1;
                max_flow_depth = max_flow_depth.max(flow_depth);
            }
        }
        if cursor.goto_first_child() {
            continue;
        }
        // Leaf reached: unwind, firing "leave" for each finished node.
        loop {
            let done = cursor.node();
            if done.is_named() {
                let kind = done.kind();
                if is_loop(kind) || is_branch(kind) {
                    flow_depth = flow_depth.saturating_sub(1);
                }
            }
            if done.id() == def.id() {
                break 'outer;
            }
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                break 'outer;
            }
        }
    }

    if loops > 0 {
        features.insert("flow:loops".to_string(), loops);
    }
    if branches > 0 {
        features.insert("flow:branches".to_string(), branches);
    }
    if max_flow_depth > 0 {
        features.insert(format!("flow:depth:{}", max_flow_depth.min(8)), 1);
    }
}

const MAX_DOC_TOKENS: usize = 40;

/// Comment/docstring words as low-weight `doc:` features. Purely lexical —
/// split on non-alphanumerics, lowercase, keep word-shaped tokens; IDF
/// weighting downstream buries ubiquitous words ("the", "returns") without a
/// stopword list. Each distinct token counts once per function (tf stays low,
/// so doc features nudge similarity rather than dominate it).
fn add_doc_tokens(text: &str, features: &mut FxHashMap<String, u32>, budget: &mut usize) {
    if *budget == 0 {
        return;
    }
    for tok in text.split(|c: char| !c.is_alphanumeric()) {
        if *budget == 0 {
            break;
        }
        if tok.len() < 3 || tok.len() > 24 || tok.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let lower = tok.to_ascii_lowercase();
        features.entry(format!("doc:{lower}")).or_insert(1);
        *budget -= 1;
    }
}

fn containing_type(spec: &LangSpec, def: Node, source: &str) -> Option<String> {
    let mut cur = def.parent();
    while let Some(node) = cur {
        for (kind, field) in spec.type_kinds {
            if node.kind() == *kind {
                if field.is_empty() {
                    // Positional fallback for grammars without a name field
                    // (ObjC @implementation): first identifier-ish child.
                    let mut c = node.walk();
                    if let Some(n) = node
                        .children(&mut c)
                        .find(|n| n.is_named() && n.kind().contains("identifier"))
                    {
                        return Some(node_text(n, source));
                    }
                } else if let Some(name) = node.child_by_field_name(field) {
                    return Some(node_text(name, source));
                }
            }
        }
        cur = node.parent();
    }
    None
}

fn node_text(node: Node, source: &str) -> String {
    source[node.start_byte()..node.end_byte()].to_string()
}

fn count_named(node: Node) -> u16 {
    let mut n = 0u16;
    let mut c = node.walk();
    for child in node.named_children(&mut c) {
        if child.kind() != "comment" {
            n = n.saturating_add(1);
        }
    }
    n
}

fn strip_quotes(s: &str) -> String {
    // Python/C# string prefixes (f"...", rb'...', $"...") sit outside the
    // quotes; skip one or two prefix letters only when a quote follows.
    let b = s.as_bytes();
    let is_pref = |c: u8| {
        matches!(
            c,
            b'f' | b'r' | b'b' | b'u' | b'F' | b'R' | b'B' | b'U' | b'$' | b'@'
        )
    };
    let is_quote = |c: u8| matches!(c, b'"' | b'\'');
    let start = if b.len() >= 2 && is_pref(b[0]) && is_quote(b[1]) {
        1
    } else if b.len() >= 3 && is_pref(b[0]) && is_pref(b[1]) && is_quote(b[2]) {
        2
    } else {
        0
    };
    s[start..]
        .trim_matches(|c| c == '"' || c == '\'' || c == '`' || c == '<' || c == '>')
        .to_string()
}

fn sanitize_receiver(s: &str) -> Option<String> {
    if s.is_empty() || s.len() > MAX_RECEIVER_LEN || s.contains('(') || s.contains('\n') {
        return None;
    }
    Some(s.to_string())
}
