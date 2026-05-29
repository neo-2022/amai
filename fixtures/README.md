modified_at: 2026-03-20 20:41 MSK
Ручная сверка guide/docs: 2026-03-20 20:41 MSK

# Fixtures

Этот каталог держит маленькие нейтральные fixture-проекты для hardening и recovery proof.

Они нужны, чтобы:
- проверять multi-project isolation;
- проверять controlled cross-project reading;
- не завязывать proof-контур на реальные продуктовые репозитории;
- воспроизводить smoke/hardening в новой среде.

Текущий набор:
- `project_alpha/`
- `project_beta/`
- `token_benchmark_queries.txt`
- `text_compare_cases.jsonl`

`text_compare_cases.jsonl` нужен для сравнительного retrieval/text contour:
- фиксирует query cases;
- задаёт ожидаемые project/path/term/symbol сигналы;
- позволяет честно сравнивать:
  - `hybrid`
  - `lexical-only`
  - `semantic-only`
  - и token budget против `naive scope`.
