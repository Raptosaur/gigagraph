//! Change-impact analysis ("blast radius"): the transitive caller closure of
//! a set of seed functions, following resolved call edges UPWARD, jumping
//! across correlated boundaries (HTTP/RPC endpoint <- its in-repo clients,
//! RN bridge native <- its JS call sites), and deriving which tests the
//! change can dirty.
//!
//! Everything here is static: dynamic dispatch, reflection, and external
//! callers stay invisible, and any heuristic edge on a path caps that whole
//! path's confidence at Heuristic. The result is a review queue ordered by
//! distance, not a proof of safety.

use crate::graph::GigaGraph;
use crate::types::{Confidence, Resolution};
use rustc_hash::FxHashMap;
use std::collections::VecDeque;

/// Cap on visited nodes: a hub function in a big monorepo can pull in half
/// the graph; past this the answer is "everything", not a useful list.
const NODE_CAP: usize = 5_000;

#[derive(Debug, Clone, Copy)]
pub struct Impacted {
    pub depth: u32,
    pub confidence: Confidence,
    /// Edge family that pulled the function in: "call", "ambiguous-call",
    /// "endpoint-client", "bridge".
    pub via: &'static str,
    /// Call/client site of the pulling edge: (file_id, line).
    pub site: (u32, u32),
}

#[derive(Debug, Default)]
pub struct ImpactResult {
    /// fn id -> how it was reached. Seeds are NOT included.
    pub impacted: FxHashMap<u32, Impacted>,
    pub truncated: bool,
}

fn worse(a: Confidence, b: Confidence) -> Confidence {
    if a == Confidence::High && b == Confidence::High {
        Confidence::High
    } else {
        Confidence::Heuristic
    }
}

struct Edge {
    to: u32,
    confidence: Confidence,
    via: &'static str,
    site: (u32, u32),
}

/// Transitive caller closure from `seeds`, breadth-first, up to `max_depth`
/// edges. A node reached first through a Heuristic path and later through a
/// High path is upgraded (and re-expanded) — confidence is the best
/// achievable, depth belongs to the path that achieved it.
pub fn blast_radius(g: &GigaGraph, seeds: &[u32], max_depth: u32) -> ImpactResult {
    // ---- Reverse adjacency: callee -> callers ----
    let mut rev: FxHashMap<u32, Vec<Edge>> = FxHashMap::default();
    for call in &g.calls {
        if let Resolution::Internal {
            callee,
            confidence,
            ambiguous_with,
        } = &call.resolution
        {
            let site = (g.functions[call.caller as usize].file_id, call.line);
            rev.entry(*callee).or_default().push(Edge {
                to: call.caller,
                confidence: *confidence,
                via: "call",
                site,
            });
            for amb in ambiguous_with {
                rev.entry(*amb).or_default().push(Edge {
                    to: call.caller,
                    confidence: Confidence::Heuristic,
                    via: "ambiguous-call",
                    site,
                });
            }
        }
    }
    // ---- Boundary jumps ----
    // Endpoint handler -> callers of correlated in-repo client calls.
    for (cid, eid, match_conf) in &g.endpoints.matches {
        let e = &g.endpoints.endpoints[*eid as usize];
        let Some(handler) = e.handler else { continue };
        let c = &g.endpoints.client_calls[*cid as usize];
        rev.entry(handler).or_default().push(Edge {
            to: c.caller,
            confidence: worse(*match_conf, worse(e.confidence, c.confidence)),
            via: "endpoint-client",
            site: (c.file_id, c.line),
        });
    }
    // RN bridge native -> JS call sites.
    for (cid, nid, match_conf) in &g.bridge.matches {
        let n = &g.bridge.natives[*nid as usize];
        let c = &g.bridge.calls[*cid as usize];
        rev.entry(n.function).or_default().push(Edge {
            to: c.caller,
            confidence: worse(*match_conf, c.confidence),
            via: "bridge",
            site: (c.file_id, c.line),
        });
    }

    // ---- BFS with confidence upgrade ----
    let mut result = ImpactResult::default();
    let mut state: FxHashMap<u32, (u32, Confidence)> = FxHashMap::default();
    let mut queue: VecDeque<u32> = VecDeque::new();
    for &s in seeds {
        state.insert(s, (0, Confidence::High));
        queue.push_back(s);
    }
    let seed_set: FxHashMap<u32, ()> = seeds.iter().map(|&s| (s, ())).collect();

    while let Some(cur) = queue.pop_front() {
        let (depth, conf) = state[&cur];
        if depth >= max_depth {
            continue;
        }
        let Some(edges) = rev.get(&cur) else { continue };
        for edge in edges {
            if seed_set.contains_key(&edge.to) {
                continue;
            }
            let next_conf = worse(conf, edge.confidence);
            let update = match state.get(&edge.to) {
                None => state.len() < NODE_CAP + seeds.len(),
                Some((_, old)) => *old == Confidence::Heuristic && next_conf == Confidence::High,
            };
            if state.len() >= NODE_CAP + seeds.len() && !state.contains_key(&edge.to) {
                result.truncated = true;
                continue;
            }
            if update {
                state.insert(edge.to, (depth + 1, next_conf));
                result.impacted.insert(
                    edge.to,
                    Impacted {
                        depth: depth + 1,
                        confidence: next_conf,
                        via: edge.via,
                        site: edge.site,
                    },
                );
                queue.push_back(edge.to);
            }
        }
    }
    result
}

/// Impacted (or seed) test functions, i.e. what to re-run. Returns
/// (fn id, depth, confidence) sorted by depth then id; depth 0 = the seed
/// itself is a test.
pub fn affected_tests(
    g: &GigaGraph,
    seeds: &[u32],
    result: &ImpactResult,
) -> Vec<(u32, u32, Confidence)> {
    let mut out: Vec<(u32, u32, Confidence)> = Vec::new();
    for &s in seeds {
        if g.functions[s as usize].is_test {
            out.push((s, 0, Confidence::High));
        }
    }
    for (&id, imp) in &result.impacted {
        if g.functions[id as usize].is_test {
            out.push((id, imp.depth, imp.confidence));
        }
    }
    out.sort_by_key(|&(id, depth, _)| (depth, id));
    out
}
