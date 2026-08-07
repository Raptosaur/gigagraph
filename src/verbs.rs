//! Identifier normalization for the similarity vectors: camelCase/snake_case
//! word splitting and a static verb -> semantic-bucket table, so
//! `fetchUser`, `get_user`, and `loadUser` all contribute a shared `vb:READ`
//! feature (alongside, never replacing, their raw `id:`/`call:` features).
//!
//! Always on, no configuration — every index gets identical behavior.

use rustc_hash::FxHashMap;

/// Distinct `w:` subword features added per function — keeps pathological
/// identifier soups from flooding the bag.
const MAX_SUBWORDS: usize = 48;

/// Splits an identifier (or free text) into lowercase words: breaks on
/// non-alphanumerics, lower->upper camel boundaries, acronym ends
/// (`parseJSONBody` -> parse, json, body), and alpha<->digit transitions.
pub fn split_words(s: &str) -> Vec<String> {
    #[derive(Clone, Copy, PartialEq)]
    enum K {
        Lower,
        Upper,
        Digit,
        Other,
    }
    let kind = |c: char| {
        if c.is_lowercase() {
            K::Lower
        } else if c.is_uppercase() {
            K::Upper
        } else if c.is_ascii_digit() {
            K::Digit
        } else {
            K::Other
        }
    };
    let chars: Vec<char> = s.chars().collect();
    let mut words = Vec::new();
    let mut cur = String::new();
    for i in 0..chars.len() {
        let c = chars[i];
        let k = kind(c);
        if k == K::Other {
            if !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if !cur.is_empty() {
            let prev = kind(chars[i - 1]);
            let boundary = match (prev, k) {
                (K::Lower, K::Upper) => true,
                (K::Digit, K::Lower | K::Upper) | (K::Lower | K::Upper, K::Digit) => true,
                // Acronym end: `JSONBody` — break between N and B.
                (K::Upper, K::Lower) => {
                    if i >= 2 && kind(chars[i - 2]) == K::Upper {
                        let tail: String = cur.pop().into_iter().collect();
                        if !cur.is_empty() {
                            words.push(std::mem::take(&mut cur));
                        }
                        cur = tail;
                    }
                    false
                }
                _ => false,
            };
            if boundary {
                words.push(std::mem::take(&mut cur));
            }
        }
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
}

/// ~200 common code verbs -> ~30 semantic buckets. Static and deliberately
/// unconfigurable: the whole point is that every repo, every user, every
/// index agrees on what `fetch` means.
pub fn verb_bucket(word: &str) -> Option<&'static str> {
    Some(match word {
        "fetch" | "get" | "load" | "retrieve" | "read" | "find" | "query" | "lookup" | "list"
        | "scan" | "search" | "peek" | "view" | "resolve" => "READ",
        "create" | "insert" | "add" | "save" | "store" | "persist" | "write" | "put" | "append"
        | "record" => "WRITE",
        "delete" | "remove" | "drop" | "clear" | "purge" | "destroy" | "erase" | "evict"
        | "unlink" | "discard" => "DELETE",
        "check" | "validate" | "verify" | "assert" | "ensure" | "expect" | "confirm" | "guard"
        | "is" | "has" | "can" | "should" | "contains" | "matches" | "equals" | "exists" => {
            "CHECK"
        }
        "send" | "emit" | "publish" | "dispatch" | "notify" | "broadcast" | "post" | "trigger"
        | "fire" | "signal" | "announce" => "EMIT",
        "parse" | "decode" | "deserialize" | "unmarshal" | "unserialize" | "unpack"
        | "extract" | "interpret" => "PARSE",
        "format" | "encode" | "serialize" | "marshal" | "stringify" | "escape" | "pack" => {
            "FORMAT"
        }
        "update" | "set" | "modify" | "patch" | "mutate" | "change" | "edit" | "replace"
        | "assign" | "toggle" | "increment" | "decrement" | "bump" => "UPDATE",
        "init" | "initialize" | "setup" | "configure" | "register" | "bootstrap" | "install"
        | "prepare" | "provision" | "seed" => "INIT",
        "handle" | "process" | "execute" | "run" | "apply" | "invoke" | "perform" | "do"
        | "call" | "exec" => "EXEC",
        "auth" | "login" | "logout" | "authenticate" | "authorize" | "signin" | "signout"
        | "signup" => "AUTH",
        "count" | "sum" | "aggregate" | "reduce" | "calc" | "compute" | "calculate"
        | "average" | "score" | "measure" | "total" | "tally" => "COMPUTE",
        "open" | "connect" | "listen" | "dial" | "accept" | "subscribe" => "CONNECT",
        "close" | "disconnect" | "shutdown" | "stop" | "cancel" | "abort" | "terminate"
        | "kill" | "dispose" | "teardown" | "unsubscribe" | "halt" => "CLOSE",
        "start" | "begin" | "launch" | "spawn" | "resume" | "restart" => "START",
        "copy" | "clone" | "duplicate" | "snapshot" => "COPY",
        "merge" | "join" | "combine" | "concat" | "union" | "zip" => "MERGE",
        "filter" | "select" | "pick" | "exclude" | "omit" | "reject" | "prune" | "dedupe"
        | "distinct" => "FILTER",
        "sort" | "order" | "rank" | "arrange" => "SORT",
        "map" | "transform" | "convert" | "cast" | "normalize" | "translate" | "migrate"
        | "adapt" | "coerce" | "flatten" | "invert" | "reverse" | "wrap" | "unwrap" => {
            "TRANSFORM"
        }
        "wait" | "sleep" | "delay" | "poll" | "retry" | "block" | "await" | "defer"
        | "debounce" | "throttle" | "timeout" => "WAIT",
        "log" | "trace" | "debug" | "warn" | "report" | "audit" => "LOG",
        "render" | "draw" | "paint" | "display" | "show" | "print" | "plot" | "visualize"
        | "highlight" => "SHOW",
        "test" | "mock" | "stub" | "fake" | "spy" | "bench" | "benchmark" | "fuzz" => "TEST",
        "lock" | "acquire" | "release" | "unlock" => "LOCK",
        "hash" | "sign" | "encrypt" | "decrypt" | "digest" | "cipher" | "hmac" => "CRYPTO",
        "upload" | "download" | "sync" | "push" | "pull" | "import" | "export" => "XFER",
        "build" | "make" | "generate" | "produce" | "derive" | "compile" | "assemble"
        | "construct" | "new" => "BUILD",
        "bind" | "attach" | "mount" | "wire" | "inject" | "hook" | "link" => "BIND",
        "split" | "tokenize" | "chunk" | "slice" | "partition" | "segment" | "divide" => "SPLIT",
        _ => return None,
    })
}

/// Semantic bucket for a whole identifier: its leading word, mapped through
/// the verb table (`fetchUserById` -> READ; `UserRepository` -> None).
pub fn bucket_for_name(name: &str) -> Option<&'static str> {
    let words = split_words(name);
    verb_bucket(words.first()?)
}

/// In-place Tier-1 enrichment of a finished feature bag:
/// - `vb:<BUCKET>` for every `call:`/`id:` feature whose leading word is a
///   known verb (raw features are kept — these are additions);
/// - `w:<word>` for the remaining (non-verb) subwords, lowercased, len 3..=24,
///   capped at `MAX_SUBWORDS` distinct words per function;
/// - `ty:<T>` for each typed local/param binding.
///
/// Deterministic: source keys are processed in sorted order so the subword
/// cap always keeps the same words.
pub fn augment_bag(bag: &mut FxHashMap<String, u32>, locals: &[(String, String)]) {
    let mut sources: Vec<(&str, u32)> = bag
        .iter()
        .filter_map(|(k, &tf)| {
            k.strip_prefix("call:")
                .or_else(|| k.strip_prefix("id:"))
                .map(|name| (name, tf))
        })
        .collect();
    sources.sort_unstable();

    let mut adds: FxHashMap<String, u32> = FxHashMap::default();
    let mut subwords = 0usize;
    for (name, tf) in sources {
        let words = split_words(name);
        for (i, w) in words.iter().enumerate() {
            if i == 0
                && let Some(bucket) = verb_bucket(w)
            {
                *adds.entry(format!("vb:{bucket}")).or_insert(0) += tf;
                continue;
            }
            if w.len() < 3 || w.len() > 24 || w.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let key = format!("w:{w}");
            match adds.get_mut(&key) {
                Some(c) => *c += tf,
                None if subwords < MAX_SUBWORDS => {
                    adds.insert(key, tf);
                    subwords += 1;
                }
                None => {}
            }
        }
    }
    for (_, ty) in locals {
        *adds.entry(format!("ty:{ty}")).or_insert(0) += 1;
    }
    for (k, v) in adds {
        *bag.entry(k).or_insert(0) += v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_camel_snake_and_acronyms() {
        assert_eq!(split_words("fetchUserById"), ["fetch", "user", "by", "id"]);
        assert_eq!(split_words("get_user_name"), ["get", "user", "name"]);
        assert_eq!(split_words("parseJSONBody"), ["parse", "json", "body"]);
        assert_eq!(split_words("HTTPClient"), ["http", "client"]);
        assert_eq!(split_words("base64Encode"), ["base", "64", "encode"]);
        assert_eq!(split_words("__init__"), ["init"]);
        assert_eq!(split_words(""), Vec::<String>::new());
    }

    #[test]
    fn verbs_map_to_buckets() {
        for (verb, bucket) in [
            ("fetch", "READ"),
            ("get", "READ"),
            ("load", "READ"),
            ("create", "WRITE"),
            ("save", "WRITE"),
            ("delete", "DELETE"),
            ("purge", "DELETE"),
            ("validate", "CHECK"),
            ("emit", "EMIT"),
            ("parse", "PARSE"),
            ("serialize", "FORMAT"),
            ("update", "UPDATE"),
            ("bootstrap", "INIT"),
            ("handle", "EXEC"),
            ("login", "AUTH"),
            ("compute", "COMPUTE"),
            ("connect", "CONNECT"),
            ("shutdown", "CLOSE"),
            ("spawn", "START"),
            ("clone", "COPY"),
            ("merge", "MERGE"),
            ("filter", "FILTER"),
            ("sort", "SORT"),
            ("transform", "TRANSFORM"),
            ("poll", "WAIT"),
            ("warn", "LOG"),
            ("render", "SHOW"),
            ("mock", "TEST"),
            ("unlock", "LOCK"),
            ("encrypt", "CRYPTO"),
            ("download", "XFER"),
            ("generate", "BUILD"),
            ("inject", "BIND"),
            ("tokenize", "SPLIT"),
        ] {
            assert_eq!(verb_bucket(verb), Some(bucket), "verb {verb}");
        }
        assert_eq!(verb_bucket("user"), None);
        assert_eq!(verb_bucket(""), None);
    }

    #[test]
    fn synonyms_share_a_bucket() {
        assert_eq!(bucket_for_name("fetchUser"), bucket_for_name("loadUser"));
        assert_eq!(bucket_for_name("fetchUser"), bucket_for_name("get_user"));
        assert_ne!(bucket_for_name("fetchUser"), bucket_for_name("deleteUser"));
        assert_eq!(bucket_for_name("UserRepository"), None);
    }

    #[test]
    fn augment_adds_without_replacing() {
        let mut bag: FxHashMap<String, u32> = FxHashMap::default();
        bag.insert("call:fetchUser".into(), 2);
        bag.insert("id:parseConfig".into(), 1);
        bag.insert("ast:call_expression".into(), 3);
        let locals = vec![("db".to_string(), "Database".to_string())];
        augment_bag(&mut bag, &locals);

        // Raw features survive.
        assert_eq!(bag.get("call:fetchUser"), Some(&2));
        assert_eq!(bag.get("id:parseConfig"), Some(&1));
        // Verb buckets carry the source tf.
        assert_eq!(bag.get("vb:READ"), Some(&2));
        assert_eq!(bag.get("vb:PARSE"), Some(&1));
        // Non-verb subwords appear once each.
        assert_eq!(bag.get("w:user"), Some(&2));
        assert_eq!(bag.get("w:config"), Some(&1));
        // Typed locals become ty: features.
        assert_eq!(bag.get("ty:Database"), Some(&1));
        // AST features are not word-split.
        assert!(!bag.contains_key("w:expression"));
    }
}
