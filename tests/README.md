modified_at: 2026-03-23 07:07 MSK
Ручная сверка guide/docs: 2026-03-23 07:07 MSK

# Tests

Этот каталог удерживает тестовый contour `Amai`.

Сейчас baseline такой:
- unit tests живут рядом с Rust modules;
- integration/smoke проверки выполняются через runtime commands из `docs/OPERATIONS.md`;
- локальный proof-cycle теперь состоит из нескольких отдельных контуров:

```bash
./scripts/proof_local.sh
./scripts/proof_hardening.sh
./scripts/proof_performance.sh
./scripts/proof_accuracy.sh
./scripts/proof_load.sh
./scripts/proof_stress_scale.sh
./scripts/proof_profiles.sh
./scripts/proof_install_auto.sh
./scripts/proof_benchmark_matrix.sh
./scripts/proof_external_benchmark_env.sh
./scripts/proof_external_benchmark_adapter.sh
./scripts/proof_mcp_task_matrix.sh
./scripts/proof_memory_task_matrix.sh
./scripts/proof_token_benchmark.sh
./scripts/proof_token_benchmark_suite.sh
./scripts/proof_cold_benchmark.sh
./scripts/proof_observability.sh
./scripts/proof_mcp.sh
./scripts/proof_hostile.sh
./scripts/proof_text_compare.sh
```

Отдельно materialized Rust-native verification commands:
- `cargo run -- verify benchmark ...`
- `cargo run -- verify cold-path --manifest config/cold_benchmark_manifest.toml ...`
- `cargo run -- verify accuracy ...`
- `cargo run -- verify load ...`
- `cargo run -- verify token-benchmark ...`
- `cargo run -- verify token-benchmark-suite ...`
- `cargo run -- verify text-compare ...`
- `cargo run -- verify mcp ...`
- `cargo run -- verify memory-matrix --matrix letta_memory_local ...`
- `cargo run -- verify hostile ...`
- `cargo run --release -- observe guardrails`
- `cargo run -- benchmark list`
- `cargo run -- benchmark coverage`
- `cargo run -- benchmark explain --benchmark live_mcpbench`
- `cargo run -- benchmark external-check`
- `cargo run -- benchmark external-explain --benchmark vectordbbench`
- `cargo run -- benchmark external-datasets`
- `cargo run -- benchmark external-download --dataset dbpedia_openai_1000k_angular`
- `cargo run -- benchmark external-plan --benchmark vectordbbench`
- `cargo run -- benchmark external-adapter --benchmark vectordbbench --dataset dbpedia_openai_1000k_angular`
- `cargo run -- benchmark external-harvest --benchmark vectordbbench --dataset dbpedia_openai_1000k_angular`
- `cargo run -- verify mcp-matrix --matrix live_mcpbench_local ...`
- `cargo run -- verify mcp-matrix --matrix mcp_universe_local ...`

Для `external-adapter` важный инвариант теперь такой:
- `run_external.sh` должен быть реальным безопасным launch-path, а не `echo`-заглушкой;
- для `VectorDBBench` это теперь означает `Rust HDF5 -> Parquet conversion + upstream qdrantlocal launch`;
- для `VectorDBBench/QdrantLocal` launch-path теперь дополнительно обязан:
  - очищать старый `vectordbbench-results` перед повторным run;
  - materialize-ить local compatibility patch для `timeout = 600`;
  - считать итог честно по `result label`, а не только по `exit code`;
- для `ann-benchmarks` это означает `.venv + requirements + install.py + run.py`;
- если upstream держит canonical launch path в `disabled: true`, `Amai` обязан выдавать `blocked_upstream_disabled`, а не ложный `prepared`;
- `external-harvest` должен уметь одной короткой командой показать `summary/report/script/status/log`, а после завершения ещё и поднять реальные `result_*.json`;
- ложный `docker compose up` без явного compose-файла больше не считается допустимым proof-path.

Когда появятся отдельные integration tests с поднимаемым stack fixture, они materialize-ятся именно здесь.
