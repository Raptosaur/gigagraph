//! Distilled static text embeddings, compiled into the binary.
//!
//! The artifact `embed/potion-base-2M.bin` is converted (by
//! `embed/distill.py`, committed alongside it) from the MIT-licensed
//! Model2Vec model `minishlab/potion-base-2M`
//! (<https://huggingface.co/minishlab/potion-base-2M>), a static distillation
//! of `baai/bge-base-en-v1.5`: 29528 WordPiece tokens x 64 dims, int8 with a
//! per-token scale (~2.2 MB, mean per-row cosine vs the f32 original
//! 0.99998). Lookup + mean-pool + L2-normalize — microseconds per call, no
//! runtime downloads, no configuration.
//!
//! Tokenization is greedy longest-match against the embedded WordPiece vocab
//! (root tokens first, `##` continuations after), applied to lowercased
//! camelCase/snake_case-split words — a faithful-enough reproduction of the
//! BERT tokenizer for identifier-shaped input.

use rustc_hash::FxHashMap;
use std::sync::OnceLock;

/// Embedding dimensions of the compiled-in model.
pub const DIMS: usize = 64;

/// Hard cap on tokens pooled per document; documents are distilled feature
/// bags, so this is generous.
const MAX_TOKENS: usize = 2048;

static ARTIFACT: &[u8] = include_bytes!("embed/potion-base-2M.bin");

struct Model {
    /// Per-token dequantization scale, one per vocab row.
    scales: Vec<f32>,
    /// Row-major int8 vectors (vocab * DIMS), reinterpreted per element.
    quant: &'static [u8],
    /// Word-initial tokens -> row.
    roots: FxHashMap<&'static str, u32>,
    /// `##`-continuation tokens (prefix stripped) -> row.
    conts: FxHashMap<&'static str, u32>,
    max_root_len: usize,
    max_cont_len: usize,
}

fn model() -> &'static Model {
    static MODEL: OnceLock<Model> = OnceLock::new();
    MODEL.get_or_init(parse_artifact)
}

fn parse_artifact() -> Model {
    let b = ARTIFACT;
    assert_eq!(&b[0..4], b"GGE1", "embed artifact magic mismatch");
    let vocab = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
    let dims = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
    assert_eq!(dims, DIMS, "embed artifact dims mismatch");
    let mut off = 12;
    let scales: Vec<f32> = (0..vocab)
        .map(|i| f32::from_le_bytes(b[off + i * 4..off + i * 4 + 4].try_into().unwrap()))
        .collect();
    off += vocab * 4;
    let quant = &b[off..off + vocab * dims];
    off += vocab * dims;

    let mut roots = FxHashMap::default();
    let mut conts = FxHashMap::default();
    let (mut max_root_len, mut max_cont_len) = (0, 0);
    for row in 0..vocab as u32 {
        let len = u16::from_le_bytes(b[off..off + 2].try_into().unwrap()) as usize;
        off += 2;
        let tok = std::str::from_utf8(&b[off..off + len]).expect("embed artifact utf8 token");
        off += len;
        match tok.strip_prefix("##") {
            Some(rest) if !rest.is_empty() => {
                max_cont_len = max_cont_len.max(rest.len());
                conts.insert(rest, row);
            }
            _ => {
                if !tok.is_empty() {
                    max_root_len = max_root_len.max(tok.len());
                    roots.insert(tok, row);
                }
            }
        }
    }
    Model {
        scales,
        quant,
        roots,
        conts,
        max_root_len,
        max_cont_len,
    }
}

/// Greedy longest-match WordPiece over one lowercased word. An unmatched
/// leading character is skipped (stays in root mode); after the first match,
/// only `##` continuations apply.
fn tokenize_word(m: &Model, word: &str, rows: &mut Vec<u32>) {
    let mut start = 0;
    let mut first = true;
    while start < word.len() && rows.len() < MAX_TOKENS {
        let (table, max_len) = if first {
            (&m.roots, m.max_root_len)
        } else {
            (&m.conts, m.max_cont_len)
        };
        // Char-boundary end positions within the length budget, longest first.
        let mut matched = None;
        let mut end = word.len().min(start + max_len);
        while end > start {
            if word.is_char_boundary(end)
                && let Some(&row) = table.get(&word[start..end])
            {
                matched = Some((end, row));
                break;
            }
            end -= 1;
        }
        match matched {
            Some((end, row)) => {
                rows.push(row);
                start = end;
                first = false;
            }
            None => {
                // Skip one char; keep root mode until something matches.
                start += word[start..].chars().next().map_or(1, |c| c.len_utf8());
            }
        }
    }
}

/// Embeds free text: split into words (camelCase/snake_case aware,
/// lowercased), WordPiece-tokenize, mean-pool the static token vectors,
/// L2-normalize. Returns an all-zero vector for text with no known tokens.
/// Fully deterministic.
pub fn embed(text: &str) -> Vec<f32> {
    let m = model();
    let mut rows: Vec<u32> = Vec::new();
    for word in crate::verbs::split_words(text) {
        if rows.len() >= MAX_TOKENS {
            break;
        }
        tokenize_word(m, &word, &mut rows);
    }
    let mut v = vec![0.0f32; DIMS];
    for &r in &rows {
        let scale = m.scales[r as usize];
        let base = r as usize * DIMS;
        for (j, x) in v.iter_mut().enumerate() {
            *x += (m.quant[base + j] as i8) as f32 * scale;
        }
    }
    if !rows.is_empty() {
        let n = rows.len() as f32;
        for x in &mut v {
            *x /= n;
        }
    }
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

/// Cosine similarity of two same-length vectors (helper for tests/callers;
/// `embed` output is already L2-normalized, so this is a dot product for
/// non-zero vectors).
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_parses() {
        let m = model();
        assert_eq!(m.scales.len() * DIMS, m.quant.len());
        assert!(m.roots.contains_key("user"), "common word in vocab");
        assert!(m.max_root_len > 0 && m.max_cont_len > 0);
    }

    #[test]
    fn embeddings_are_normalized_and_deterministic() {
        let a = embed("fetch user profile");
        let norm: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "norm {norm}");
        assert_eq!(a, embed("fetch user profile"));
        assert_eq!(embed(""), vec![0.0; DIMS]);
    }

    #[test]
    fn semantic_neighbors_beat_strangers() {
        let fetch_user = embed("fetchUser");
        let load_user = embed("loadUser");
        let parse_config = embed("parseConfig");
        let near = cosine(&fetch_user, &load_user);
        let far = cosine(&fetch_user, &parse_config);
        assert!(
            near > far,
            "fetchUser~loadUser ({near}) should beat fetchUser~parseConfig ({far})"
        );
    }
}
