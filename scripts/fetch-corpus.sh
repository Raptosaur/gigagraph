#!/usr/bin/env bash
# Clone the real-world validation corpus described by tests/corpus.json.
#
#   scripts/fetch-corpus.sh                 # fetch every repo
#   scripts/fetch-corpus.sh axum googletest # fetch a subset by name
#   scripts/fetch-corpus.sh --report        # index what is present, print
#                                           # measured counts (for setting floors)
#   scripts/fetch-corpus.sh --list          # show the manifest
#
# Clones are shallow and pinned to the manifest's `rev`, and land in the
# gitignored directory named by the manifest (`validation/corpus` by default).
# Nothing here runs in CI by default: the corpus tests are `#[ignore]`d and
# skip themselves when a clone is absent.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manifest="$repo_root/tests/corpus.json"

python_bin="$(command -v python3 || true)"
if [ -z "$python_bin" ]; then
  echo "fetch-corpus: python3 is required to read the manifest" >&2
  exit 1
fi

# field <name> <key>  -> value from the manifest
field() {
  "$python_bin" - "$manifest" "$1" "$2" <<'PY'
import json, sys
manifest, name, key = sys.argv[1], sys.argv[2], sys.argv[3]
data = json.load(open(manifest))
if name == "":
    print(data[key])
else:
    for r in data["repos"]:
        if r["name"] == name:
            print(r[key])
            break
PY
}

names() {
  "$python_bin" - "$manifest" <<'PY'
import json, sys
for r in json.load(open(sys.argv[1]))["repos"]:
    print(r["name"])
PY
}

corpus_dir="$repo_root/$(field "" dir)"

case "${1:-}" in
--list)
  "$python_bin" - "$manifest" <<'PY'
import json, sys
data = json.load(open(sys.argv[1]))
print(f"corpus dir: {data['dir']}")
for r in data["repos"]:
    print(f"  {r['name']:22} {r['repo']} @ {r['rev'][:12]}")
    print(f"  {'':22} {r['why']}")
PY
  exit 0
  ;;
--report)
  shift
  echo "measuring $corpus_dir (floors in tests/corpus.json should sit ~30% below these)"
  for name in $(names); do
    dir="$corpus_dir/$name"
    [ -d "$dir" ] || continue
    stats=$(cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -- \
      query --root "$dir" index_stats '{}')
    tests=$(cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -- \
      query --root "$dir" list_tests '{"limit":1}')
    eps=$(cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -- \
      query --root "$dir" list_endpoints '{"limit":1}')
    "$python_bin" - "$name" "$stats" "$tests" "$eps" <<'PY'
import json, sys
name, stats, tests, eps = sys.argv[1:5]
s, t, e = json.loads(stats)["stats"], json.loads(tests), json.loads(eps)
fw = ", ".join(f"{k}:{v}" for k, v in sorted(t.get("frameworks", {}).items()))
print(f"  {name:22} files={s.get('files')} functions={s.get('functions')} "
      f"endpoints={e.get('total_detected')} cases={t.get('total_cases')}")
print(f"  {'':22} languages={sorted(s.get('functions_by_language', {}))}")
print(f"  {'':22} frameworks={fw or '-'}")
tot = s.get("resolved_internal", 0) + s.get("resolved_external", 0) + s.get("unresolved", 0)
pct = (s.get("resolved_internal", 0) + s.get("resolved_external", 0)) * 100 // tot if tot else 0
print(f"  {'':22} resolution={pct}%  (suggested min_resolution_pct: {max(1, pct - 10)})")
PY
  done
  exit 0
  ;;
esac

wanted=("$@")
if [ ${#wanted[@]} -eq 0 ]; then
  mapfile -t wanted < <(names)
fi

mkdir -p "$corpus_dir"
for name in "${wanted[@]}"; do
  url="$(field "$name" repo)"
  rev="$(field "$name" rev)"
  if [ -z "$url" ]; then
    echo "fetch-corpus: unknown repo '$name' (see --list)" >&2
    exit 1
  fi
  dir="$corpus_dir/$name"

  if [ -d "$dir/.git" ]; then
    current="$(git -C "$dir" rev-parse HEAD)"
    if [ "$current" = "$rev" ]; then
      echo "== $name already at $rev"
      continue
    fi
    echo "== $name updating $current -> $rev"
  else
    echo "== $name cloning $url"
    rm -rf "$dir"
    git init --quiet "$dir"
    git -C "$dir" remote add origin "$url"
  fi

  # Fetch just the pinned commit; fall back to a shallow default-branch clone
  # for servers that refuse by-sha fetches.
  if git -C "$dir" fetch --quiet --depth 1 origin "$rev" 2>/dev/null; then
    git -C "$dir" checkout --quiet FETCH_HEAD
  else
    echo "   (server refused fetch-by-sha; falling back to default branch)"
    git -C "$dir" fetch --quiet --depth 50 origin
    git -C "$dir" checkout --quiet "$rev" 2>/dev/null ||
      git -C "$dir" checkout --quiet FETCH_HEAD
  fi
  echo "   $(git -C "$dir" rev-parse --short HEAD) $(du -sh "$dir" | cut -f1)"
done

echo
echo "corpus ready in $corpus_dir"
echo "run: cargo test --test corpus_test -- --ignored --nocapture"
