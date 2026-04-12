# Gemini Proxy Chat (Local)

Минимальное локальное расширение VS Code, которое отправляет запросы в Gemini через прокси
из `n8n-nodes-wildbots-gemini` (Cloudflare Worker).

## Установка (локально)

1. Откройте VS Code.
2. `Extensions` → `...` → `Install from VSIX` не нужно.
3. В `Extensions` выберите `Install from Location...` и укажите папку:
   `/home/art/agent-memory-index/tools/vscode-gemini-proxy`
4. Перезапустите VS Code, если попросит.

## Настройки

Откройте `Settings` и задайте:

- `geminiProxy.apiKey`: ключ Google AI Studio.
- `geminiProxy.baseUrl`: URL Cloudflare Worker (по умолчанию стоит публичный).
- `geminiProxy.model`: модель, например `gemini-2.5-flash`.
- `geminiProxy.useV1`: если нужно использовать `/v1/` вместо `/v1beta/`.
- `geminiProxy.maxContextChars`: лимит символов для прикладываемого контекста.
- `geminiProxy.maxSearchResults`: лимит результатов project-search.
- `geminiProxy.allowCommands`: разрешить запуск shell-команд.
- `geminiProxy.allowFileRead`: разрешить чтение файлов/листинг директорий.
- `geminiProxy.allowFileWrite`: разрешить запись файлов.
- `geminiProxy.allowExternalPaths`: разрешить доступ к путям вне workspace.
- `geminiProxy.commandTimeoutMs`: таймаут команд.
- `geminiProxy.commandMaxBuffer`: лимит вывода команд.
- `geminiProxy.allowAutoToolCalls`: авто-выполнение TOOL_CALL без подтверждения.

## Использование

### Чат-окно

Команда: `Gemini Proxy: Open Chat`.
Внутри окна можно:
- выбрать модель;
- поменять base URL;
- включить /v1;
- прикладывать контекст проекта (selection, file, search);
- запускать tool-операции (команды, чтение/запись файлов, листинг).
- отправлять сообщения.
Кнопка `Refresh` подгружает полный список доступных моделей прямо из Gemini API.

### Быстрый запрос

Команда: `Gemini Proxy: Ask`.

Ответ откроется в новой вкладке как markdown.
