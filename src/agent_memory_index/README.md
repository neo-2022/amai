# `src/agent_memory_index`

Этот каталог зарезервирован под будущий Rust-first bounded context для собственно
`agent memory index` домена `Amai`.

Сюда не нужно переносить всё подряд из `src/`.
Сюда не должны попадать:
- `dashboard`;
- `token_budget`;
- `observe`;
- `ops/bootstrap/deployment`;
- общие transport/storage adapters без Amai-domain ownership.

## Что сюда будет переноситься

Только owner-contained контуры, которые отвечают именно за индекс памяти агента
как продуктовый домен, а не за соседние эксплуатационные поверхности:

1. `project / namespace boundary ownership`
   - регистрация и удержание `repo_root -> project -> namespace` границ;
   - fail-closed правила против cross-project mixing;
   - source-of-truth orchestration вокруг project identity.

2. `lexical / exact / symbol retrieval planning`
   - Amai-first retrieval orchestration;
   - выбор lexical-before-semantic;
   - explainable retrieval plan surfaces для context recovery.

3. `context pack assembly`
   - сборка bounded context pack;
   - компоновка retrieval-result ownership без подмены truth vector-layer'ом;
   - handoff-ready packing logic именно как memory-index contour.

4. `memory-index explain / operator surfaces`
   - explain/debug surfaces, которые относятся именно к index/retrieval/domain
     логике `Amai`, а не к общему dashboard/ops слою.

## Что сюда переносить нельзя без отдельного решения

- generic SQL helpers;
- generic S3/Qdrant/NATS adapters;
- dashboard projection helpers;
- benchmark-only utilities;
- случайные legacy helpers только ради уменьшения размера файла.

## Migration law

Перенос в этот каталог допустим только если одновременно выполнены все условия:

1. у контура есть явный domain owner внутри `Amai`;
2. после выноса ownership станет понятнее, а не размажется;
3. tests едут вместе с owner-модулем;
4. новый каталог не превращается в "склад всего важного";
5. split не нарушает product law:
   - скорость;
   - точность;
   - качество;
   - правдивость.

Пока эти условия не выполнены, каталог может оставаться почти пустым.
Пустой каталог лучше, чем ложный bounded context.
