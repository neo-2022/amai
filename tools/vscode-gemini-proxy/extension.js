const vscode = require("vscode");
const https = require("https");
const fs = require("fs/promises");
const path = require("path");
const { exec } = require("child_process");

const MODEL_CHOICES = [
  "gemini-2.5-flash",
  "gemini-2.5-pro",
  "gemini-2.0-flash",
  "gemini-2.0-flash-lite",
  "gemini-flash-latest",
  "gemini-pro-latest",
];

const DEFAULT_MAX_CONTEXT_CHARS = 20000;
const DEFAULT_MAX_SEARCH_RESULTS = 20;
const DEFAULT_COMMAND_TIMEOUT_MS = 20000;
const DEFAULT_COMMAND_MAX_BUFFER = 1024 * 1024;

function normalizeBaseUrl(raw) {
  return raw.replace(/\/+$/, "");
}

function buildRequestUrl(baseUrl, model, apiKey, useV1) {
  const version = useV1 ? "v1" : "v1beta";
  const path = `/${version}/models/${encodeURIComponent(model)}:generateContent?key=${encodeURIComponent(
    apiKey
  )}`;
  return `${normalizeBaseUrl(baseUrl)}${path}`;
}

function httpPostJson(url, body) {
  return new Promise((resolve, reject) => {
    const payload = JSON.stringify(body);
    const parsed = new URL(url);
    const options = {
      method: "POST",
      hostname: parsed.hostname,
      path: parsed.pathname + parsed.search,
      port: parsed.port || 443,
      headers: {
        "Content-Type": "application/json",
        "Content-Length": Buffer.byteLength(payload),
      },
    };
    const req = https.request(options, (res) => {
      let data = "";
      res.setEncoding("utf8");
      res.on("data", (chunk) => {
        data += chunk;
      });
      res.on("end", () => {
        if (res.statusCode && res.statusCode >= 200 && res.statusCode < 300) {
          try {
            resolve(JSON.parse(data));
          } catch (error) {
            reject(new Error(`Failed to parse JSON: ${error.message}`));
          }
          return;
        }
        reject(new Error(`HTTP ${res.statusCode}: ${data}`));
      });
    });
    req.on("error", reject);
    req.write(payload);
    req.end();
  });
}

function httpGetJson(url) {
  return new Promise((resolve, reject) => {
    const parsed = new URL(url);
    const options = {
      method: "GET",
      hostname: parsed.hostname,
      path: parsed.pathname + parsed.search,
      port: parsed.port || 443,
    };
    const req = https.request(options, (res) => {
      let data = "";
      res.setEncoding("utf8");
      res.on("data", (chunk) => {
        data += chunk;
      });
      res.on("end", () => {
        if (res.statusCode && res.statusCode >= 200 && res.statusCode < 300) {
          try {
            resolve(JSON.parse(data));
          } catch (error) {
            reject(new Error(`Failed to parse JSON: ${error.message}`));
          }
          return;
        }
        reject(new Error(`HTTP ${res.statusCode}: ${data}`));
      });
    });
    req.on("error", reject);
    req.end();
  });
}

async function fetchModelList(baseUrl, apiKey, useV1) {
  const version = useV1 ? "v1" : "v1beta";
  const url = `${normalizeBaseUrl(baseUrl)}/${version}/models?key=${encodeURIComponent(apiKey)}`;
  const payload = await httpGetJson(url);
  const models = payload?.models || [];
  return models
    .filter((model) =>
      (model.supportedGenerationMethods || []).includes("generateContent")
    )
    .filter((model) => {
      const modalities = Array.isArray(model.supportedResponseModalities)
        ? model.supportedResponseModalities
        : [];
      if (modalities.length === 0) {
        return true;
      }
      return modalities.includes("TEXT");
    })
    .map((model) => (model.name || "").replace(/^models\//, ""))
    .filter(Boolean);
}

function truncateText(text, limit) {
  if (!text || text.length <= limit) {
    return text;
  }
  return `${text.slice(0, limit)}\n\n[TRUNCATED to ${limit} chars]`;
}

function getWorkspaceRoot() {
  const folders = vscode.workspace.workspaceFolders;
  if (!folders || folders.length === 0) {
    return null;
  }
  return folders[0].uri.fsPath;
}

function resolvePath(inputPath, allowExternalPaths) {
  if (!inputPath || !inputPath.trim()) {
    throw new Error("Path is empty.");
  }
  const root = getWorkspaceRoot();
  if (!root) {
    throw new Error("Workspace root is not available.");
  }
  const candidate = path.isAbsolute(inputPath)
    ? inputPath
    : path.join(root, inputPath);
  const normalized = path.normalize(candidate);
  if (!allowExternalPaths && !normalized.startsWith(root)) {
    throw new Error("Path is outside workspace root.");
  }
  return normalized;
}

async function runCommand(command, options) {
  if (!command || !command.trim()) {
    throw new Error("Command is empty.");
  }
  const root = getWorkspaceRoot();
  if (!root) {
    throw new Error("Workspace root is not available.");
  }
  const timeoutMs = options.commandTimeoutMs || DEFAULT_COMMAND_TIMEOUT_MS;
  const maxBuffer = options.commandMaxBuffer || DEFAULT_COMMAND_MAX_BUFFER;
  const cwd = options.cwd || root;
  return new Promise((resolve, reject) => {
    exec(
      command,
      { cwd, timeout: timeoutMs, maxBuffer },
      (error, stdout, stderr) => {
        if (error) {
          const message = stderr || stdout || error.message;
          reject(new Error(message.trim() || "Command failed."));
          return;
        }
        resolve((stdout || "").trim());
      }
    );
  });
}

async function executeToolCall(toolCall, cfg) {
  const allowCommands = !!cfg.get("allowCommands");
  const allowFileRead = !!cfg.get("allowFileRead");
  const allowFileWrite = !!cfg.get("allowFileWrite");
  const allowExternalPaths = !!cfg.get("allowExternalPaths");
  const commandTimeoutMs = Number(cfg.get("commandTimeoutMs")) || DEFAULT_COMMAND_TIMEOUT_MS;
  const commandMaxBuffer = Number(cfg.get("commandMaxBuffer")) || DEFAULT_COMMAND_MAX_BUFFER;
  const root = getWorkspaceRoot();

  if (!toolCall || typeof toolCall !== "object") {
    throw new Error("Invalid tool call.");
  }
  switch (toolCall.tool) {
    case "run": {
      if (!allowCommands) {
        throw new Error("Commands are disabled by settings.");
      }
      const cwd = toolCall.cwd
        ? resolvePath(toolCall.cwd, allowExternalPaths)
        : root;
      return await runCommand(toolCall.command, {
        cwd,
        commandTimeoutMs,
        commandMaxBuffer,
      });
    }
    case "read_file": {
      if (!allowFileRead) {
        throw new Error("File read is disabled by settings.");
      }
      const target = resolvePath(toolCall.path, allowExternalPaths);
      const data = await fs.readFile(target, "utf8");
      return truncateText(
        data,
        Number(cfg.get("maxContextChars")) || DEFAULT_MAX_CONTEXT_CHARS
      );
    }
    case "write_file": {
      if (!allowFileWrite) {
        throw new Error("File write is disabled by settings.");
      }
      const target = resolvePath(toolCall.path, allowExternalPaths);
      const content = toolCall.content ?? "";
      await fs.writeFile(target, content, "utf8");
      return `Wrote ${content.length} bytes to ${target}`;
    }
    case "list_dir": {
      if (!allowFileRead) {
        throw new Error("Directory listing is disabled by settings.");
      }
      const target = resolvePath(toolCall.path || ".", allowExternalPaths);
      const entries = await fs.readdir(target, { withFileTypes: true });
      return entries
        .map((entry) => (entry.isDirectory() ? `[DIR] ${entry.name}` : `[FILE] ${entry.name}`))
        .join("\n");
    }
    case "search": {
      if (!allowFileRead) {
        throw new Error("Project search is disabled by settings.");
      }
      const query = toolCall.query || "";
      const maxResults =
        Number(cfg.get("maxSearchResults")) || DEFAULT_MAX_SEARCH_RESULTS;
      const matches = await searchProject(query, maxResults);
      return matches.length ? matches.join("\n") : "No matches.";
    }
    default:
      throw new Error(`Unknown tool: ${toolCall.tool}`);
  }
}

function parseToolCallFromText(text) {
  if (!text || typeof text !== "string") {
    return null;
  }
  const match = text.match(/TOOL_CALL:\\s*(\\{[\\s\\S]*?\\})/);
  if (!match) {
    return null;
  }
  try {
    return JSON.parse(match[1]);
  } catch {
    return null;
  }
}

async function getActiveSelection() {
  const editor = vscode.window.activeTextEditor;
  if (!editor) {
    return null;
  }
  const selection = editor.selection;
  if (selection.isEmpty) {
    return null;
  }
  const text = editor.document.getText(selection);
  if (!text || !text.trim()) {
    return null;
  }
  const path = editor.document.uri.fsPath;
  return { path, text };
}

async function getActiveFileContent() {
  const editor = vscode.window.activeTextEditor;
  if (!editor) {
    return null;
  }
  const text = editor.document.getText();
  if (!text || !text.trim()) {
    return null;
  }
  const path = editor.document.uri.fsPath;
  return { path, text };
}

async function searchProject(query, maxResults) {
  if (!query || !query.trim()) {
    return [];
  }
  const results = [];
  const pattern = query.trim();
  await vscode.workspace.findTextInFiles(
    { pattern },
    { maxResults },
    (result) => {
      if (results.length >= maxResults) {
        return;
      }
      const rel = vscode.workspace.asRelativePath(result.uri);
      const preview = result.preview?.text || "";
      results.push(`${rel}: ${preview.trim()}`);
    }
  );
  return results;
}

async function buildContextBlock(options) {
  if (!options) {
    return "";
  }
  const cfg = vscode.workspace.getConfiguration("geminiProxy");
  const maxChars = Number(cfg.get("maxContextChars")) || DEFAULT_MAX_CONTEXT_CHARS;
  const maxSearch = Number(cfg.get("maxSearchResults")) || DEFAULT_MAX_SEARCH_RESULTS;
  const sections = [];

  if (options.includeSelection) {
    const selection = await getActiveSelection();
    if (selection) {
      const rel = vscode.workspace.asRelativePath(selection.path);
      sections.push(
        `### Selection (${rel})\n${truncateText(selection.text, maxChars)}`
      );
    }
  }

  if (options.includeFile) {
    const file = await getActiveFileContent();
    if (file) {
      const rel = vscode.workspace.asRelativePath(file.path);
      sections.push(`### File (${rel})\n${truncateText(file.text, maxChars)}`);
    }
  }

  if (options.searchQuery) {
    const matches = await searchProject(options.searchQuery, maxSearch);
    if (matches.length > 0) {
      sections.push(`### Project search (${options.searchQuery})\n${matches.join("\n")}`);
    }
  }

  return sections.join("\n\n");
}

async function askGemini() {
  const cfg = vscode.workspace.getConfiguration("geminiProxy");
  const baseUrl = cfg.get("baseUrl");
  const apiKey = cfg.get("apiKey");
  const model = cfg.get("model");
  const useV1 = cfg.get("useV1");

  if (!apiKey || !apiKey.trim()) {
    vscode.window.showErrorMessage("Gemini Proxy: set geminiProxy.apiKey in settings.");
    return;
  }
  if (!baseUrl || !baseUrl.trim()) {
    vscode.window.showErrorMessage("Gemini Proxy: set geminiProxy.baseUrl in settings.");
    return;
  }

  const prompt = await vscode.window.showInputBox({
    title: "Gemini Proxy: Ask",
    prompt: "Введите запрос для Gemini",
  });
  if (!prompt || !prompt.trim()) {
    return;
  }

  const requestUrl = buildRequestUrl(baseUrl, model, apiKey, useV1);
  const payload = {
    contents: [{ parts: [{ text: prompt }] }],
  };

  try {
    const response = await httpPostJson(requestUrl, payload);
    const text =
      response?.candidates?.[0]?.content?.parts?.map((p) => p.text).join("") ??
      "Пустой ответ";
    const doc = await vscode.workspace.openTextDocument({
      content: text,
      language: "markdown",
    });
    await vscode.window.showTextDocument(doc, { preview: true });
  } catch (error) {
    vscode.window.showErrorMessage(`Gemini Proxy error: ${error.message}`);
  }
}

function openChat() {
  const panel = vscode.window.createWebviewPanel(
    "geminiProxyChat",
    "Gemini Proxy Chat",
    vscode.ViewColumn.Beside,
    {
      enableScripts: true,
      retainContextWhenHidden: true,
    }
  );

  const cfg = vscode.workspace.getConfiguration("geminiProxy");
  const baseUrl = cfg.get("baseUrl");
  const model = cfg.get("model");
  const useV1 = cfg.get("useV1");

  panel.webview.html = renderChatHtml({
    baseUrl,
    model,
    useV1,
    models: MODEL_CHOICES,
  });

  panel.webview.onDidReceiveMessage(async (msg) => {
    if (!msg || typeof msg !== "object") {
      return;
    }
    if (msg.type === "saveSettings") {
      await cfg.update("baseUrl", msg.baseUrl, vscode.ConfigurationTarget.Global);
      await cfg.update("model", msg.model, vscode.ConfigurationTarget.Global);
      await cfg.update("useV1", !!msg.useV1, vscode.ConfigurationTarget.Global);
      panel.webview.postMessage({ type: "settingsSaved" });
      return;
    }
    if (msg.type === "refreshModels") {
      const latest = vscode.workspace.getConfiguration("geminiProxy");
      const apiKey = latest.get("apiKey");
      if (!apiKey || !apiKey.trim()) {
        panel.webview.postMessage({
          type: "error",
          message: "Gemini API key is not set. Set geminiProxy.apiKey in Settings.",
        });
        return;
      }
      try {
        const models = await fetchModelList(
          msg.baseUrl || latest.get("baseUrl"),
          apiKey,
          !!msg.useV1
        );
        const current = msg.model || latest.get("model");
        const selected = models.includes(current) ? current : models[0];
        panel.webview.postMessage({
          type: "models",
          models,
          selected,
        });
      } catch (error) {
        panel.webview.postMessage({ type: "error", message: error.message });
      }
      return;
    }
    if (msg.type === "sendPrompt") {
      const latest = vscode.workspace.getConfiguration("geminiProxy");
      const apiKey = latest.get("apiKey");
      if (!apiKey || !apiKey.trim()) {
        panel.webview.postMessage({
          type: "error",
          message: "Gemini API key is not set. Set geminiProxy.apiKey in Settings.",
        });
        return;
      }
      const requestUrl = buildRequestUrl(
        msg.baseUrl || latest.get("baseUrl"),
        msg.model || latest.get("model"),
        apiKey,
        !!msg.useV1
      );
      const contextBlock = await buildContextBlock(msg.contextOptions);
      const promptText = msg.prompt || "";
      const composed = contextBlock
        ? `Контекст проекта:\n${contextBlock}\n\nЗапрос:\n${promptText}`
        : promptText;
      const payload = {
        contents: [{ parts: [{ text: composed }] }],
      };
      try {
        const response = await httpPostJson(requestUrl, payload);
        const text =
          response?.candidates?.[0]?.content?.parts?.map((p) => p.text).join("") ??
          "Пустой ответ";
        panel.webview.postMessage({ type: "response", text });
        const toolCall = parseToolCallFromText(text);
        if (toolCall) {
          const allowAuto = !!latest.get("allowAutoToolCalls");
          if (!allowAuto) {
            const choice = await vscode.window.showWarningMessage(
              `Gemini просит tool-call: ${toolCall.tool}. Выполнить?`,
              "Да",
              "Нет"
            );
            if (choice !== "Да") {
              return;
            }
          }
          try {
            const output = await executeToolCall(toolCall, latest);
            panel.webview.postMessage({
              type: "toolResult",
              tool: toolCall.tool,
              output,
            });
          } catch (error) {
            panel.webview.postMessage({
              type: "error",
              message: `Tool failed: ${error.message}`,
            });
          }
        }
      } catch (error) {
        panel.webview.postMessage({ type: "error", message: error.message });
      }
    }
    if (msg.type === "runTool") {
      const latest = vscode.workspace.getConfiguration("geminiProxy");
      try {
        const output = await executeToolCall(msg.toolCall, latest);
        panel.webview.postMessage({
          type: "toolResult",
          tool: msg.toolCall.tool,
          output,
        });
      } catch (error) {
        panel.webview.postMessage({ type: "error", message: error.message });
      }
    }
  });
}

function renderChatHtml({ baseUrl, model, useV1, models }) {
  const options = models
    .map(
      (name) =>
        `<option value="${name}" ${name === model ? "selected" : ""}>${name}</option>`
    )
    .join("");
  const useV1Checked = useV1 ? "checked" : "";
  return `<!doctype html>
<html lang="ru">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <style>
      body { font-family: "Segoe UI", sans-serif; margin: 0; background: #0f141a; color: #e5ecf2; }
      header { padding: 12px 16px; border-bottom: 1px solid #1f2933; display: flex; gap: 12px; align-items: center; }
      main { display: grid; grid-template-rows: 1fr auto; height: calc(100vh - 54px); }
      .row { display: flex; gap: 8px; align-items: center; }
      input, select, textarea { background: #121923; color: #e5ecf2; border: 1px solid #233041; border-radius: 6px; padding: 6px 8px; }
      textarea { width: 100%; height: 88px; resize: vertical; }
      button { background: #2f6df6; border: none; color: white; padding: 8px 12px; border-radius: 6px; cursor: pointer; }
      button.secondary { background: #334155; }
      #log { padding: 16px; overflow: auto; }
      .msg { margin-bottom: 12px; white-space: pre-wrap; }
      .msg.user { color: #9bd6ff; }
      .msg.ai { color: #d6f5d6; }
      .msg.error { color: #ff9b9b; }
      .msg.tool { color: #facc15; }
      footer { padding: 12px 16px; border-top: 1px solid #1f2933; display: grid; gap: 8px; }
      .toolbox { border: 1px solid #233041; border-radius: 6px; padding: 8px; display: grid; gap: 8px; }
    </style>
  </head>
  <body>
    <header>
      <div class="row">
        <label>Model</label>
        <select id="model">${options}</select>
        <button id="refresh" class="secondary">Refresh</button>
      </div>
      <div class="row">
        <label>Base URL</label>
        <input id="baseUrl" size="36" value="${baseUrl || ""}" />
      </div>
      <div class="row">
        <label>Use /v1</label>
        <input id="useV1" type="checkbox" ${useV1Checked} />
      </div>
      <button id="save">Save</button>
    </header>
    <main>
      <div id="log"></div>
      <footer>
        <textarea id="prompt" placeholder="Напиши запрос..."></textarea>
        <div class="row">
          <label><input id="ctxSelection" type="checkbox" /> Selection</label>
          <label><input id="ctxFile" type="checkbox" /> File</label>
          <label><input id="ctxSearch" type="checkbox" /> Search</label>
          <input id="ctxQuery" placeholder="поиск по проекту" size="24" />
        </div>
        <div class="toolbox">
          <div class="row">
            <label>Cmd</label>
            <input id="toolCmd" placeholder="rg -n \"foo\" ." size="40" />
            <button id="toolRun" class="secondary">Run</button>
          </div>
          <div class="row">
            <label>Read</label>
            <input id="toolReadPath" placeholder="path/to/file" size="34" />
            <button id="toolRead" class="secondary">Read</button>
          </div>
          <div class="row">
            <label>List</label>
            <input id="toolListPath" placeholder="path/to/dir" size="34" />
            <button id="toolList" class="secondary">List</button>
          </div>
          <div class="row">
            <label>Write</label>
            <input id="toolWritePath" placeholder="path/to/file" size="26" />
          </div>
          <textarea id="toolWriteContent" placeholder="content to write"></textarea>
          <button id="toolWrite" class="secondary">Write</button>
        </div>
        <div class="row">
          <button id="send">Send</button>
          <button id="clear" class="secondary">Clear</button>
        </div>
      </footer>
    </main>
    <script>
      const vscode = acquireVsCodeApi();
      const log = document.getElementById("log");
      const prompt = document.getElementById("prompt");
      const model = document.getElementById("model");
      const baseUrl = document.getElementById("baseUrl");
      const useV1 = document.getElementById("useV1");
      const ctxSelection = document.getElementById("ctxSelection");
      const ctxFile = document.getElementById("ctxFile");
      const ctxSearch = document.getElementById("ctxSearch");
      const ctxQuery = document.getElementById("ctxQuery");
      const toolCmd = document.getElementById("toolCmd");
      const toolReadPath = document.getElementById("toolReadPath");
      const toolListPath = document.getElementById("toolListPath");
      const toolWritePath = document.getElementById("toolWritePath");
      const toolWriteContent = document.getElementById("toolWriteContent");

      function addMessage(text, cls) {
        const div = document.createElement("div");
        div.className = "msg " + cls;
        div.textContent = text;
        log.appendChild(div);
        log.scrollTop = log.scrollHeight;
      }

      document.getElementById("save").addEventListener("click", () => {
        vscode.postMessage({
          type: "saveSettings",
          baseUrl: baseUrl.value,
          model: model.value,
          useV1: useV1.checked,
        });
      });

      document.getElementById("send").addEventListener("click", () => {
        const text = prompt.value.trim();
        if (!text) return;
        addMessage(text, "user");
        prompt.value = "";
        vscode.postMessage({
          type: "sendPrompt",
          prompt: text,
          baseUrl: baseUrl.value,
          model: model.value,
          useV1: useV1.checked,
          contextOptions: {
            includeSelection: ctxSelection.checked,
            includeFile: ctxFile.checked,
            searchQuery: ctxSearch.checked ? ctxQuery.value : "",
          },
        });
      });

      document.getElementById("toolRun").addEventListener("click", () => {
        const command = toolCmd.value.trim();
        if (!command) return;
        vscode.postMessage({ type: "runTool", toolCall: { tool: "run", command } });
      });

      document.getElementById("toolRead").addEventListener("click", () => {
        const pathValue = toolReadPath.value.trim();
        if (!pathValue) return;
        vscode.postMessage({ type: "runTool", toolCall: { tool: "read_file", path: pathValue } });
      });

      document.getElementById("toolList").addEventListener("click", () => {
        const pathValue = toolListPath.value.trim() || ".";
        vscode.postMessage({ type: "runTool", toolCall: { tool: "list_dir", path: pathValue } });
      });

      document.getElementById("toolWrite").addEventListener("click", () => {
        const pathValue = toolWritePath.value.trim();
        if (!pathValue) return;
        const content = toolWriteContent.value;
        vscode.postMessage({
          type: "runTool",
          toolCall: { tool: "write_file", path: pathValue, content },
        });
      });

      document.getElementById("clear").addEventListener("click", () => {
        log.innerHTML = "";
      });

      document.getElementById("refresh").addEventListener("click", () => {
        vscode.postMessage({
          type: "refreshModels",
          baseUrl: baseUrl.value,
          model: model.value,
          useV1: useV1.checked,
        });
      });

      window.addEventListener("message", (event) => {
        const msg = event.data;
        if (msg.type === "response") {
          addMessage(msg.text, "ai");
        } else if (msg.type === "toolResult") {
          addMessage(`[tool:${msg.tool}]\\n${msg.output}`, "tool");
        } else if (msg.type === "error") {
          addMessage(msg.message, "error");
        } else if (msg.type === "settingsSaved") {
          addMessage("Settings saved.", "ai");
        } else if (msg.type === "models") {
          const current = msg.selected || model.value;
          model.innerHTML = "";
          (msg.models || []).forEach((name) => {
            const option = document.createElement("option");
            option.value = name;
            option.textContent = name;
            if (name === current) option.selected = true;
            model.appendChild(option);
          });
          if (!model.value && (msg.models || []).length > 0) {
            model.value = msg.models[0];
          }
          addMessage("Model list refreshed.", "ai");
        }
      });
    </script>
  </body>
</html>`;
}

function activate(context) {
  const disposable = vscode.commands.registerCommand("geminiProxy.ask", askGemini);
  const openChatCommand = vscode.commands.registerCommand("geminiProxy.openChat", openChat);
  context.subscriptions.push(disposable, openChatCommand);
}

module.exports = {
  activate,
};
