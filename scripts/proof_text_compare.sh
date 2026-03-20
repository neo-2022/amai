#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

cargo run --quiet -- verify text-compare \
  --project project_alpha \
  --namespace review \
  --retrieval-mode local_plus_related \
  --cases-file fixtures/text_compare_cases.jsonl >/tmp/amai-text-compare.out

grep -q '"text_compare"' /tmp/amai-text-compare.out
grep -q '"mean_precision"' /tmp/amai-text-compare.out
grep -q '"hybrid"' /tmp/amai-text-compare.out
grep -q '"lexical_only"' /tmp/amai-text-compare.out
grep -q '"semantic_only"' /tmp/amai-text-compare.out
grep -q '"mean_hybrid_savings_factor_vs_naive"' /tmp/amai-text-compare.out

rm -f /tmp/amai-text-compare.out

echo "proof_text_compare: ok"
