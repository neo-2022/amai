# Memory Bench Runbook

This runbook prepares Amai for external memory benchmarks without introducing Python-only core paths.

## Benchmarks

- LongMemEval
- AMA-Bench
- MemoryAgentBench
- LoCoMo

## Prepare datasets and adapter workspaces

```bash
./scripts/proof_memory_external_benchmarks.sh
```

Notes:
- AMA-Bench requires manual dataset install from Hugging Face. Place a marker file at:
  `state/external-benchmarks/datasets/ama-bench.manual`
  after download.
- All other datasets are fetched via the external benchmark dataset catalog.

## Generate normalized cases for Amai

The proof script writes normalized JSONL cases into:

```
state/external-benchmarks/memory/<bench>/<dataset>/latest/cases.jsonl
state/external-benchmarks/memory/<bench>/<dataset>/latest/manifest.json
state/external-benchmarks/memory/<bench>/<dataset>/latest/requests.jsonl
```

Each JSONL line:

```json
{
  "bench": "longmemeval",
  "dataset": "longmemeval_s_cleaned",
  "case_id": "case-0001",
  "question": "...",
  "context": "...",
  "answer": "...",
  "metadata": { "...": "..." }
}
```

Requests file format (for model runtime):

```json
{
  "case_id": "case-0001",
  "prompt": "You are Amai... Answer:",
  "context": "...",
  "question": "..."
}
```

## Run Amai evaluation

This repo prepares normalized cases and requests, but model runtime is external. After you have model outputs, store predictions as JSONL:

```json
{
  "case_id": "case-0001",
  "predicted_answer": "..."
}
```

Score with:

```bash
cargo run -- benchmark external-memory-score --cases <cases.jsonl> --predictions <predictions.jsonl> --output <score.json>
```

Scoring is a baseline exact/contains/abstention heuristic until official upstream scorers are added.
