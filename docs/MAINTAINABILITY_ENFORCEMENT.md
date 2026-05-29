# Maintainability / Supportability / Evolvability / Anti-Hardcoding Gate For Amai

## Зачем нужен этот документ

Локальная canonical-копия стандарта теперь живёт внутри репозитория:
- `docs/standards/MAINTAINABILITY_SUPPORTABILITY_EVOLVABILITY_ANTI_HARDCODING_STANDARD.md`

Но для `Amai` одного внешнего документа недостаточно.

Нужен project-local binding, чтобы любой агент понимал:
- когда этот стандарт обязателен;
- чем он проверяется;
- что именно нельзя выпускать дальше;
- что делать, если текущий код уже не полностью соответствует этому стандарту.

Этот документ не заменяет сам стандарт.
Он materialize-ит локальную canonical-копию стандарта как обязательный operational gate для `Amai`.

## Честная текущая реальность

В проекте уже есть зоны, которые частично нарушают этот стандарт.

Это значит:
- нельзя делать вид, что проект уже идеально соответствует стандарту;
- нельзя откладывать этот стандарт “на потом”;
- нельзя пускать новые изменения по тому же плохому паттерну.

Правильная позиция такая:
- legacy-отклонения признаются честно;
- новые значимые изменения обязаны проходить через этот gate;
- старые нарушения должны уменьшаться, а не разрастаться.

## Стандарт нельзя игнорировать

Для `Amai` этот стандарт не является советом или факультативной рекомендацией.

Любой агент обязан:
- не противоречить этому стандарту ни в одном содержательном изменении;
- не оправдывать отклонение фразой `это слишком маленькая правка`;
- не выпускать изменение, которое явно нарушает standard law, даже если формальный full gate для этого изменения не запускался.

Важное различие только одно:
- для stage-based, архитектурных, schema/policy/truth-sensitive и critical-zone изменений обязателен полный machine-readable gate через `./scripts/maintainability_gate.sh --json`;
- для значимого обновления `docs/IMPLEMENTATION_STATUS.md` обязателен свежий machine-readable sync guard через `./scripts/implementation_status_sync_guard.sh --json`;
- для закрытия checkbox значимого этапа обязателен свежий machine-readable closure guard через `./scripts/maintainability_stage_close_guard.sh --json`;
- для маленьких purely local изменений допустим condensed path без отдельного gate-run;
- но condensed path не означает, что стандарт можно игнорировать.

Простое правило:
- full gate может быть не нужен для микроправки;
- сам стандарт обязателен всегда.

## Non-regression binding для Amai

Для `Amai` maintainability gate обязан сохранять product-law проекта:
- скорость;
- точность;
- качество;
- правдивость.

Это не дополнительные пожелания, а release/promotion law.

Значит:
- нельзя считать изменение хорошим, если оно улучшило только одну ось;
- нельзя принимать локальную оптимизацию, если она тихо ухудшила соседний контур;
- нельзя выпускать новый слой, если он не умеет доказать сохранение baseline.

Особенно запрещено:
- покупать экономию памяти ценой правды;
- покупать скорость ценой точности;
- покупать удобство ценой качества;
- покупать богатую память ценой ложного восстановления контекста.

Если изменение не доказывает non-regression по этим законам,
оно не готово к release, promotion или stage-close.

## Когда gate обязателен

Maintainability gate обязателен, если изменение:
- затрагивает stage-based implementation;
- меняет core runtime;
- меняет schema/contracts;
- меняет policy / truth / provenance / scope rules;
- меняет retrieval / continuity / restore / task graph / procedural memory;
- меняет observability / evidence / audit / recovery semantics;
- добавляет новый config / registry / source-of-truth;
- добавляет новый hardcoded rule или новый change-safety риск;
- делает заметный refactor критической зоны.

Если изменение маленькое и локальное, gate можно пройти быстро.
Но пропускать его совсем нельзя, если затронута критическая зона.

## Главные fail-closed вопросы

Перед тем как считать значимое изменение готовым, агент обязан ответить:

1. Какой домен меняется?
2. Где source of truth для этого поведения?
3. Не подменяется ли truth projection-слоем?
4. Не живёт ли правило, которое должно быть в config/registry/contract/policy, прямо в коде?
5. Не появился ли новый hidden hardcode?
6. Кто owner этой зоны:
   - кода;
   - тестов;
   - документации;
   - rollback/recovery semantics?
7. Какие тестовые слои обязательны:
   - unit;
   - contract;
   - integration;
   - e2e;
   - replay/regression;
   - load/soak;
   - hostile/fault/degradation;
   - drill/tabletop, если риск это требует?
8. Какой rollback path?
9. Какой recovery path?
10. Какие docs / checklists / contracts / runbooks нужно обновить?
11. Не стал ли change impact шире, чем планировалось?
12. Не ухудшились ли:
   - maintainability;
   - supportability;
   - evolvability;
   - change safety?
13. Не улучшили ли одну ось проекта ценой тихой деградации другой:
   - speed;
   - accuracy;
   - quality;
   - truth?
14. Не стал ли baseline менее честным:
   - из-за partial measurement;
   - из-за misleading dashboard/projection surface;
   - из-за подмены system aggregate красивым best-case?

Если на любой из этих вопросов нет честного ответа, gate считается не пройденным.

## Anti-hardcoding binding для Amai

Для `Amai` особенно запрещено:
- хардкодить изменяемые policy/routing/threshold/rule значения в `if/else`, `match`, helpers и UI;
- дублировать один и тот же rule в нескольких местах;
- прятать environment-specific или project-specific значения в коде, если они должны жить в:
  - config;
  - contract;
  - registry;
  - policy layer;
  - source-of-truth document.

Допустимо:
- compile-time invariants;
- protocol markers;
- жёсткие архитектурные константы, которые действительно не являются управляемыми правилами.

Простое правило:
- если значение может поменяться без переписывания архитектуры, это кандидат в source-of-truth слой, а не в hardcode.

## Что агент обязан проверить для Amai

### 1. Truth / Projection

Нужно проверить:
- не начинает ли projection вести себя как truth;
- не подменяется ли backend/domain enforcement UI-слоем;
- не показывается ли derived state как подтверждённая истина.
- не выдаётся ли лучший или последний удобный результат за честную оценку системы.

### 2. Ownership / Searchability

Нужно проверить:
- понятно ли, где менять этот контур;
- понятно ли, где его tests;
- понятно ли, где rollback/recovery;
- сможет ли другой инженер найти эту зону без устной традиции.

### 3. Testability

Нужно проверить:
- какие test layers обязательны именно для этого риска;
- не пытается ли change обойти нужный test layer более слабым.
- не подменяется ли полный stage/proof bundle одним удобным blocking-proof.

### 4. Release / Rollback / Recovery

Нужно проверить:
- можно ли включить change безопасно;
- есть ли kill-switch / rollback path;
- не ломается ли recoverability и observability.

### 5. Documentation Drift

Нужно проверить:
- если поменялся контракт, поменялись ли docs;
- если поменялся workflow, поменялись ли onboarding/status/gates документы;
- не появились ли конкурирующие “почти канонические” объяснения.

## Как этот gate использовать в проекте

Для значимого изменения порядок такой:

1. Пройти обычный project preflight:
   - `./scripts/agent_preflight.sh --json`
2. Пройти maintainability gate:
   - `./scripts/maintainability_gate.sh --json`
2a. Если `docs/IMPLEMENTATION_STATUS.md` обновляется как часть значимого изменения, подтвердить status sync guard:
   - `./scripts/implementation_status_sync_guard.sh --json`
2b. Перед закрытием checkbox значимого этапа подтвердить closure guard:
   - `./scripts/maintainability_stage_close_guard.sh --json`
3. Сверить изменение с:
   - `docs/IMPLEMENTATION_STATUS.md`
   - `docs/IMPLEMENTATION_GATES.md`
   - `docs/AMAI_GLOBAL_MEMORY_ROADMAP.md`
   - этим документом
4. Реализовать изменение
5. Прогнать обязательные benchmark/proof/harness для этапа
6. Если появились maintainability/supportability regressions:
   - сначала root-cause;
   - потом fix/recovery;
   - потом повторная проверка
7. Обновить docs/status/handoff

## Что считается нарушением gate

Нарушением считается, если:
- source-of-truth стал менее явным;
- изменение добавило новый скрытый hardcode;
- стало труднее найти нужную зону;
- rollback/recovery стали менее ясными;
- обязательные test layers не определены;
- change review существует только “в голове” и не отражён в docs/status/gates/handoff;
- `docs/IMPLEMENTATION_STATUS.md` обновлён для значимого изменения без passing `implementation_status_sync_guard`;
- checkbox значимого этапа поставлен без passing `maintainability_stage_close_guard`;
- baseline одной из 4 осей проекта тихо просел, даже если локальный результат “зелёный”;
- change улучшил одну ось за счёт скрытой деградации другой;
- projection/UI/benchmark surface стал менее правдивым, чем underlying contour;
- новый агент больше не сможет безопасно понять, как сопровождать эту часть проекта.

## Короткая формула для Amai

Хорошее изменение для `Amai`:
- не только работает;
- не только проходит proof;
- но и остаётся понятным, локализуемым, тестируемым, откатываемым, восстанавливаемым и не хардкодит изменяемые правила.

Если этого нет, изменение ещё не готово.
