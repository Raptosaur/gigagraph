//! In-memory semantic vector index. Two signals per function, both always on:
//! - structural: feature bags hashed into fixed-size signed vectors
//!   (feature-hashing trick), IDF-weighted, L2-normalized;
//! - semantic: the bag's identifier/callee/type/doc words embedded with the
//!   compiled-in distilled static model (see `crate::embed`).
//!
//! Search is brute-force blended cosine over both, in parallel.

use rayon::prelude::*;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

pub const DIMS: usize = 256;
pub const SEM_DIMS: usize = crate::embed::DIMS;

/// Fixed blend for top-k scoring: structural cosine dominates (call shape,
/// AST, control flow are the primary signal), the distilled semantic cosine
/// pulls synonym-named twins together. Deliberately not configurable — every
/// index everywhere scores identically.
pub const STRUCTURAL_WEIGHT: f32 = 0.6;
pub const SEMANTIC_WEIGHT: f32 = 0.4;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct VectorIndex {
    pub dims: u32,
    /// Row-major, functions * dims, each row L2-normalized (or all-zero).
    pub data: Vec<f32>,
    /// feature hash -> document frequency; only df >= 2 entries are kept
    /// (rare features get the default IDF).
    pub df: FxHashMap<u64, u32>,
    pub n_docs: u32,
    /// Row-major semantic matrix, functions * SEM_DIMS, each row
    /// L2-normalized (or all-zero). Serialized with the index.
    #[serde(default)]
    pub sem: Vec<f32>,
}

fn feature_hash(feat: &str) -> u64 {
    let mut h = rustc_hash::FxHasher::default();
    feat.hash(&mut h);
    h.finish()
}

impl VectorIndex {
    pub fn build(feature_bags: &[FxHashMap<String, u32>]) -> VectorIndex {
        let n_docs = feature_bags.len() as u32;

        // Document frequency per feature (hashed).
        let df: FxHashMap<u64, u32> = feature_bags
            .par_iter()
            .fold(FxHashMap::default, |mut acc: FxHashMap<u64, u32>, bag| {
                for feat in bag.keys() {
                    *acc.entry(feature_hash(feat)).or_insert(0) += 1;
                }
                acc
            })
            .reduce(FxHashMap::default, |mut a, b| {
                for (k, v) in b {
                    *a.entry(k).or_insert(0) += v;
                }
                a
            });
        let df: FxHashMap<u64, u32> = df.into_iter().filter(|&(_, v)| v >= 2).collect();

        let mut index = VectorIndex {
            dims: DIMS as u32,
            data: vec![0.0; feature_bags.len() * DIMS],
            df,
            n_docs,
            sem: vec![0.0; feature_bags.len() * SEM_DIMS],
        };

        let df_ref = &index.df;
        let rows: Vec<Vec<f32>> = feature_bags
            .par_iter()
            .map(|bag| embed_bag(bag, df_ref, n_docs))
            .collect();
        for (i, row) in rows.into_iter().enumerate() {
            index.data[i * DIMS..(i + 1) * DIMS].copy_from_slice(&row);
        }
        let sem_rows: Vec<Vec<f32>> = feature_bags
            .par_iter()
            .map(semantic_embed_bag)
            .collect();
        for (i, row) in sem_rows.into_iter().enumerate() {
            index.sem[i * SEM_DIMS..(i + 1) * SEM_DIMS].copy_from_slice(&row);
        }
        index
    }

    pub fn embed(&self, bag: &FxHashMap<String, u32>) -> Vec<f32> {
        embed_bag(bag, &self.df, self.n_docs)
    }

    pub fn vector_of(&self, id: u32) -> Option<&[f32]> {
        let start = id as usize * DIMS;
        self.data.get(start..start + DIMS)
    }

    /// Semantic row for a function; `None` when out of range (or when the
    /// index predates the semantic matrix — callers pass `&[]` then).
    pub fn sem_vector_of(&self, id: u32) -> Option<&[f32]> {
        let start = id as usize * SEM_DIMS;
        self.sem.get(start..start + SEM_DIMS)
    }

    /// Top-k blended similarity against all rows:
    /// `STRUCTURAL_WEIGHT * structural_cosine + SEMANTIC_WEIGHT *
    /// semantic_cosine`. A `sem_query` of the wrong length (e.g. `&[]`)
    /// scores the semantic term as zero. `exclude` is dropped from results
    /// (usually the query function itself).
    pub fn top_k(
        &self,
        query: &[f32],
        sem_query: &[f32],
        k: usize,
        exclude: Option<u32>,
    ) -> Vec<(u32, f32)> {
        if query.len() != DIMS || self.n_docs == 0 {
            return Vec::new();
        }
        let use_sem = sem_query.len() == SEM_DIMS && self.sem.len() == self.data.len() / DIMS * SEM_DIMS;
        let mut scored: Vec<(u32, f32)> = self
            .data
            .par_chunks(DIMS)
            .enumerate()
            .filter_map(|(i, row)| {
                if exclude == Some(i as u32) {
                    return None;
                }
                let structural: f32 = row.iter().zip(query).map(|(a, b)| a * b).sum();
                let semantic: f32 = if use_sem {
                    self.sem[i * SEM_DIMS..(i + 1) * SEM_DIMS]
                        .iter()
                        .zip(sem_query)
                        .map(|(a, b)| a * b)
                        .sum()
                } else {
                    0.0
                };
                let score = STRUCTURAL_WEIGHT * structural + SEMANTIC_WEIGHT * semantic;
                (score > 0.0).then_some((i as u32, score))
            })
            .collect();
        if scored.is_empty() || k == 0 {
            return Vec::new();
        }
        let k = k.min(scored.len());
        // Score desc, then id asc: ties resolve the same way regardless of
        // partition order or k.
        let cmp = |a: &(u32, f32), b: &(u32, f32)| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0));
        scored.select_nth_unstable_by(k - 1, cmp);
        scored.truncate(k);
        scored.sort_by(cmp);
        scored
    }
}

/// Semantic document for a function: the human-word-bearing feature-bag keys
/// (identifiers, callee names, typed-local types, doc words), embedded with
/// the compiled-in static model. Keys are sorted so the mean-pool order — and
/// thus the floating-point result — is deterministic.
pub fn semantic_embed_bag(bag: &FxHashMap<String, u32>) -> Vec<f32> {
    let mut words: Vec<&str> = bag
        .keys()
        .filter_map(|k| {
            k.strip_prefix("id:")
                .or_else(|| k.strip_prefix("call:"))
                .or_else(|| k.strip_prefix("ty:"))
                .or_else(|| k.strip_prefix("doc:"))
        })
        .collect();
    words.sort_unstable();
    crate::embed::embed(&words.join(" "))
}

fn embed_bag(bag: &FxHashMap<String, u32>, df: &FxHashMap<u64, u32>, n_docs: u32) -> Vec<f32> {
    let mut v = vec![0.0f32; DIMS];
    for (feat, &tf) in bag {
        let h = feature_hash(feat);
        let dfi = df.get(&h).copied().unwrap_or(1);
        let idf = (((n_docs + 1) as f32) / ((dfi + 1) as f32)).ln() + 1.0;
        let w = (1.0 + (tf as f32).ln()) * idf;
        let dim = (h % DIMS as u64) as usize;
        let sign = if h & (1 << 63) == 0 { 1.0 } else { -1.0 };
        v[dim] += sign * w;
    }
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}
