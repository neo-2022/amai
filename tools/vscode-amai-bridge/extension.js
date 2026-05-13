const vscode = require("vscode");
const fs = require("fs/promises");
const path = require("path");
const packageJson = require("./package.json");

const COMMAND_ID = "amaiVscodeBridge.openCleanChat";
const OPEN_SIDEBAR_COMMAND_ID = "amaiVscodeBridge.openWorkspaceSidebarChat";
const OPEN_PANEL_COMMAND_ID = "amaiVscodeBridge.openWorkspacePanelChat";
const FOCUS_VIEW_COMMAND_ID = "amaiVscodeBridge.focusSidebarView";
const OPEN_MANAGED_REPO_COMMAND_ID = "amaiVscodeBridge.openManagedRepoWorkspace";
const OPEN_OPENAI_EXTENSION_COMMAND_ID = "amaiVscodeBridge.openOpenAiExtension";
const RELOAD_WINDOW_COMMAND_ID = "amaiVscodeBridge.reloadWindow";
const VIEW_ID = "amai.sidebar";
const EXTENSION_URI_AUTHORITY = "amai.amai-vscode-bridge";
const EXTENSION_VERSION = packageJson.version;
const REQUIRED_CODEX_COMMANDS = [
  "chatgpt.openSidebar",
  "chatgpt.newChat",
  "chatgpt.newCodexPanel",
];

function publicBridgeIdentity() {
  return {
    authority: EXTENSION_URI_AUTHORITY,
    command_id: COMMAND_ID,
    version: EXTENSION_VERSION,
    capabilities: {
      ui_cleanup: true,
      visible_surface: true,
    },
  };
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function normalizeString(value) {
  if (typeof value !== "string") {
    return null;
  }
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function normalizeBoolean(value, defaultValue) {
  if (typeof value !== "string") {
    return defaultValue;
  }
  switch (value.trim().toLowerCase()) {
    case "1":
    case "true":
    case "yes":
      return true;
    case "0":
    case "false":
    case "no":
      return false;
    default:
      return defaultValue;
  }
}

function paramsToPayload(params) {
  return {
    promptFile: normalizeString(params.get("prompt_file")),
    promptText: normalizeString(params.get("prompt_text")),
    resultFile: normalizeString(params.get("result_file")),
    repoRoot: normalizeString(params.get("repo_root")),
    target: normalizeString(params.get("target")) || "sidebar",
    autoSubmit: normalizeBoolean(params.get("auto_submit"), true),
  };
}

function isBridgeUriLike(value) {
  return typeof value === "string" && value.includes(`${EXTENSION_URI_AUTHORITY}/open-clean-chat`);
}

function tabMatchesBridgeUri(tab, uriText) {
  if (!tab) {
    return false;
  }
  if (isBridgeUriLike(tab.label)) {
    return true;
  }
  const input = tab.input;
  const candidates = [
    input?.uri?.toString?.(true),
    input?.modified?.toString?.(true),
    input?.original?.toString?.(true),
  ];
  return candidates.some((value) => value === uriText || isBridgeUriLike(value));
}

function summarizeTabInput(input) {
  if (!input) {
    return {
      kind: null,
      uri: null,
      original_uri: null,
      modified_uri: null,
    };
  }
  return {
    kind: input.constructor?.name ?? null,
    uri: input.uri?.toString?.(true) ?? null,
    original_uri: input.original?.toString?.(true) ?? null,
    modified_uri: input.modified?.toString?.(true) ?? null,
  };
}

function summarizeTab(tab, activeTab) {
  if (!tab) {
    return null;
  }
  return {
    label: normalizeString(tab.label),
    is_active: Boolean(activeTab && tab === activeTab),
    is_dirty: Boolean(tab.isDirty),
    is_pinned: Boolean(tab.isPinned),
    is_preview: Boolean(tab.isPreview),
    input: summarizeTabInput(tab.input),
  };
}

function collectVisibleSurfaceState(uriText) {
  const groups = [];
  let totalTabs = 0;
  let bridgeTabCount = 0;
  let nonBridgeTabCount = 0;
  const nonBridgeTabLabels = [];

  for (const group of vscode.window.tabGroups.all) {
    const activeTab = group.activeTab ?? null;
    const tabs = group.tabs.map((tab) => {
      totalTabs += 1;
      const isBridgeTab = tabMatchesBridgeUri(tab, uriText);
      if (isBridgeTab) {
        bridgeTabCount += 1;
      } else {
        nonBridgeTabCount += 1;
        const label = normalizeString(tab.label);
        if (label) {
          nonBridgeTabLabels.push(label);
        }
      }
      return {
        ...summarizeTab(tab, activeTab),
        is_bridge_tab: isBridgeTab,
      };
    });
    groups.push({
      is_active: Boolean(vscode.window.tabGroups.activeTabGroup === group),
      view_column: group.viewColumn ?? null,
      tab_count: tabs.length,
      tabs,
    });
  }

  const activeTextEditorUri = vscode.window.activeTextEditor?.document?.uri?.toString?.(true) ?? null;
  const activeTab = vscode.window.tabGroups.activeTabGroup?.activeTab ?? null;
  return {
    active_text_editor_uri: activeTextEditorUri,
    active_tab: summarizeTab(activeTab, activeTab),
    bridge_tab_count: bridgeTabCount,
    group_count: groups.length,
    non_bridge_tab_count: nonBridgeTabCount,
    non_bridge_tab_labels: nonBridgeTabLabels,
    tab_groups: groups,
    total_tab_count: totalTabs,
  };
}

function summarizeVisibleSurfaceDelta(before, after) {
  const beforeLabels = new Set(before?.non_bridge_tab_labels ?? []);
  const afterLabels = new Set(after?.non_bridge_tab_labels ?? []);
  const addedLabels = [...afterLabels].filter((label) => !beforeLabels.has(label));
  const removedLabels = [...beforeLabels].filter((label) => !afterLabels.has(label));
  return {
    active_tab_label_before: before?.active_tab?.label ?? null,
    active_tab_label_after: after?.active_tab?.label ?? null,
    active_tab_kind_before: before?.active_tab?.input?.kind ?? null,
    active_tab_kind_after: after?.active_tab?.input?.kind ?? null,
    group_count_before: before?.group_count ?? 0,
    group_count_after: after?.group_count ?? 0,
    total_tab_count_before: before?.total_tab_count ?? 0,
    total_tab_count_after: after?.total_tab_count ?? 0,
    bridge_tab_count_before: before?.bridge_tab_count ?? 0,
    bridge_tab_count_after: after?.bridge_tab_count ?? 0,
    non_bridge_tab_count_before: before?.non_bridge_tab_count ?? 0,
    non_bridge_tab_count_after: after?.non_bridge_tab_count ?? 0,
    non_bridge_tab_labels_added: addedLabels,
    non_bridge_tab_labels_removed: removedLabels,
    new_non_bridge_surface_detected: addedLabels.length > 0,
  };
}

function collectBridgeUiState(uriText) {
  let matchingTabCount = 0;
  for (const group of vscode.window.tabGroups.all) {
    for (const tab of group.tabs) {
      if (tabMatchesBridgeUri(tab, uriText)) {
        matchingTabCount += 1;
      }
    }
  }
  const activeUri = vscode.window.activeTextEditor?.document?.uri?.toString?.(true) ?? null;
  const activeEditorMatchesBridgeUri =
    activeUri === uriText || isBridgeUriLike(activeUri);
  return {
    active_editor_matches_bridge_uri: activeEditorMatchesBridgeUri,
    active_editor_uri: activeEditorMatchesBridgeUri ? activeUri : null,
    matching_tab_count: matchingTabCount,
  };
}

async function writeResultFile(resultFile, payload) {
  if (!resultFile) {
    return;
  }
  await fs.mkdir(require("path").dirname(resultFile), { recursive: true });
  await fs.writeFile(resultFile, `${JSON.stringify(payload, null, 2)}\n`, "utf8");
}

async function getCodexSurfaceState() {
  const commands = await vscode.commands.getCommands(true);
  const missingCommands = REQUIRED_CODEX_COMMANDS.filter(
    (command) => !commands.includes(command)
  );
  return {
    available: missingCommands.length === 0,
    missingCommands,
  };
}

function formatCodexSurfaceError(surfaceState) {
  const missingCommand = surfaceState?.missingCommands?.[0] ?? "chatgpt.openSidebar";
  return [
    "Amai bridge не нашёл готовую чат-интеграцию в VS Code.",
    `Не хватает команды: ${missingCommand}.`,
    "Что сделать:",
    "1. Установите и включите совместимое chat-расширение.",
    "2. Перезапустите или Reload Window в VS Code / Codium.",
    "3. Затем снова откройте Amai sidebar.",
  ].join(" ");
}

async function ensureCodexCommandsAvailable() {
  const surfaceState = await getCodexSurfaceState();
  if (!surfaceState.available) {
    throw new Error(formatCodexSurfaceError(surfaceState));
  }
}

async function pathExists(targetPath) {
  try {
    await fs.access(targetPath);
    return true;
  } catch {
    return false;
  }
}

async function hasAmaiServerConfig(targetPath) {
  if (!(await pathExists(targetPath))) {
    return false;
  }
  try {
    const raw = await fs.readFile(targetPath, "utf8");
    return /"amai"\s*:/.test(raw);
  } catch {
    return false;
  }
}

async function collectInstallReadiness() {
  const repoRoot = currentWorkspaceRepoRoot();
  const codexSurface = await getCodexSurfaceState();
  const managedRepoRoot = path.join(
    process.env.HOME || "",
    ".local",
    "share",
    "amai",
    "repo"
  );
  const managedRepoInstalled = await pathExists(managedRepoRoot);
  const userMcpCandidates = [
    path.join(process.env.HOME || "", ".config", "Code", "User", "mcp.json"),
    path.join(process.env.HOME || "", ".config", "VSCodium", "User", "mcp.json"),
    path.join(process.env.HOME || "", ".vscode-oss", "User", "mcp.json"),
  ];
  const userMcpConfigured = (
    await Promise.all(userMcpCandidates.map((file) => hasAmaiServerConfig(file)))
  ).some(Boolean);
  const workspaceMcpConfig =
    repoRoot !== null ? path.join(repoRoot, ".vscode", "mcp.json") : null;
  const workspaceMcpConfigured = workspaceMcpConfig
    ? await hasAmaiServerConfig(workspaceMcpConfig)
    : false;
  const mcpConfigured = userMcpConfigured || workspaceMcpConfigured;
  const workspaceMatchesManagedRepo =
    repoRoot !== null && managedRepoInstalled && repoRoot === managedRepoRoot;
  return {
    codexSurface,
    managedRepoInstalled,
    managedRepoRoot,
    repoRoot,
    workspaceMcpConfig,
    userMcpCandidates,
    userMcpConfigured,
    mcpConfigured,
    workspaceMcpConfigured,
    workspaceMatchesManagedRepo,
  };
}

function renderStatusBadge(ok, text) {
  const tone = ok ? "ok" : "warn";
  return `<div class="status-row ${tone}">${ok ? "OK" : "!"} ${text}</div>`;
}

async function closeTransientUriEditors(uriText) {
  if (!normalizeString(uriText)) {
    return {
      active_editor_matches_bridge_uri_after: false,
      active_editor_matches_bridge_uri_before: false,
      active_editor_uri_after: null,
      active_editor_uri_before: null,
      closed_active_editor: false,
      close_attempts: 0,
      closed_tab_candidates_total: 0,
      matching_tabs_after: 0,
      matching_tabs_before: 0,
      skipped_reason: "source_uri_missing",
      success: false,
      uri_cleanup_requested: false,
    };
  }

  const before = collectBridgeUiState(uriText);
  let closeAttempts = 0;
  let closedActiveEditor = false;
  let closedTabCandidatesTotal = 0;

  const closeMatchingActiveEditor = async () => {
    const active = vscode.window.activeTextEditor;
    const activeUri = active?.document?.uri?.toString?.(true);
    if (activeUri === uriText || isBridgeUriLike(activeUri)) {
      await vscode.commands.executeCommand("workbench.action.closeActiveEditor");
      return true;
    }
    return false;
  };

  for (let attempt = 0; attempt < 8; attempt += 1) {
    closeAttempts += 1;
    let closedSomething = false;
    const tabsToClose = [];
    for (const group of vscode.window.tabGroups.all) {
      for (const tab of group.tabs) {
        if (tabMatchesBridgeUri(tab, uriText)) {
          tabsToClose.push(tab);
        }
      }
    }

    if (tabsToClose.length > 0) {
      closedTabCandidatesTotal += tabsToClose.length;
      await vscode.window.tabGroups.close(tabsToClose, true);
      closedSomething = true;
    }

    if (await closeMatchingActiveEditor()) {
      closedActiveEditor = true;
      closedSomething = true;
    }

    if (!closedSomething) {
      break;
    }
    await sleep(120);
  }

  const after = collectBridgeUiState(uriText);
  return {
    active_editor_matches_bridge_uri_after: after.active_editor_matches_bridge_uri,
    active_editor_matches_bridge_uri_before: before.active_editor_matches_bridge_uri,
    active_editor_uri_after: after.active_editor_uri,
    active_editor_uri_before: before.active_editor_uri,
    closed_active_editor: closedActiveEditor,
    close_attempts: closeAttempts,
    closed_tab_candidates_total: closedTabCandidatesTotal,
    matching_tabs_after: after.matching_tab_count,
    matching_tabs_before: before.matching_tab_count,
    skipped_reason: null,
    success:
      after.matching_tab_count === 0 && !after.active_editor_matches_bridge_uri,
    uri_cleanup_requested: true,
  };
}

async function executeBridgeLaunch({ target, promptText, autoSubmit }) {
  await ensureCodexCommandsAvailable();
  if (target === "panel") {
    await vscode.commands.executeCommand("chatgpt.newCodexPanel");
  } else {
    await vscode.commands.executeCommand("chatgpt.openSidebar");
    await sleep(250);
    await vscode.commands.executeCommand("chatgpt.newChat");
  }
  await sleep(450);
  await vscode.commands.executeCommand("type", { text: promptText });
  if (autoSubmit) {
    await sleep(120);
    await vscode.commands.executeCommand("type", { text: "\n" });
  }
}

async function openCleanChat(input, sourceUriText = null) {
  const startedAt = new Date().toISOString();
  const promptFile = normalizeString(input?.promptFile);
  const inlinePromptText = normalizeString(input?.promptText);
  const resultFile = normalizeString(input?.resultFile);
  const repoRoot = normalizeString(input?.repoRoot);
  const target = normalizeString(input?.target) || "sidebar";
  const autoSubmit = input?.autoSubmit !== false;

  try {
    let promptText = inlinePromptText;
    if (!promptText) {
      if (!promptFile) {
        throw new Error("prompt_file or promptText is required");
      }
      promptText = (await fs.readFile(promptFile, "utf8")).trim();
    }
    if (!promptText) {
      throw new Error(promptFile ? `prompt_file is blank: ${promptFile}` : "promptText is blank");
    }

    const visibleSurfaceBeforeLaunch = collectVisibleSurfaceState(sourceUriText);
    await writeResultFile(resultFile, {
      status: "launch_started",
      started_at: startedAt,
      prompt_file: promptFile,
      prompt_text_inline: Boolean(inlinePromptText),
      repo_root: repoRoot,
      target,
      auto_submit: autoSubmit,
      prompt_chars: promptText.length,
      public_bridge: publicBridgeIdentity(),
      visible_surface: {
        before_launch: visibleSurfaceBeforeLaunch,
      },
      commands: target === "panel"
        ? ["chatgpt.newCodexPanel", "type"]
        : ["chatgpt.openSidebar", "chatgpt.newChat", "type"],
    });

    await executeBridgeLaunch({ target, promptText, autoSubmit });
    const uiCleanup = await closeTransientUriEditors(sourceUriText);
    await sleep(150);
    const visibleSurfaceAfterLaunch = collectVisibleSurfaceState(sourceUriText);

    await writeResultFile(resultFile, {
      status: "launch_requested",
      started_at: startedAt,
      completed_at: new Date().toISOString(),
      prompt_file: promptFile,
      prompt_text_inline: Boolean(inlinePromptText),
      repo_root: repoRoot,
      target,
      auto_submit: autoSubmit,
      prompt_chars: promptText.length,
      public_bridge: publicBridgeIdentity(),
      ui_cleanup: uiCleanup,
      visible_surface: {
        before_launch: visibleSurfaceBeforeLaunch,
        after_launch: visibleSurfaceAfterLaunch,
        delta: summarizeVisibleSurfaceDelta(
          visibleSurfaceBeforeLaunch,
          visibleSurfaceAfterLaunch
        ),
      },
      commands: target === "panel"
        ? ["chatgpt.newCodexPanel", "type"]
        : ["chatgpt.openSidebar", "chatgpt.newChat", "type"],
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    await writeResultFile(resultFile, {
      status: "launch_failed",
      started_at: startedAt,
      completed_at: new Date().toISOString(),
      prompt_file: promptFile,
      prompt_text_inline: Boolean(inlinePromptText),
      repo_root: repoRoot,
      target,
      auto_submit: autoSubmit,
      error: message,
      public_bridge: publicBridgeIdentity(),
    });
    throw error;
  }
}

function currentWorkspaceRepoRoot() {
  const folder = vscode.workspace.workspaceFolders?.[0];
  return normalizeString(folder?.uri?.fsPath) ?? null;
}

function currentWorkspacePrompt(target) {
  const repoRoot = currentWorkspaceRepoRoot();
  const repoLine = repoRoot
    ? `Workspace repo root: ${repoRoot}`
    : "Workspace repo root is unavailable.";
  const targetLine =
    target === "panel"
      ? "Open the clean Amai session in a separate panel."
      : "Open the clean Amai session in the sidebar.";
  return [
    "Amai clean session bootstrap.",
    repoLine,
    targetLine,
    "If this workspace contains startup instructions or AGENTS.md, follow them before tool use.",
  ].join("\n");
}

async function launchWorkspaceChat(target) {
  await openCleanChat({
    promptText: currentWorkspacePrompt(target),
    repoRoot: currentWorkspaceRepoRoot(),
    target,
    autoSubmit: false,
  });
}

async function openManagedRepoWorkspace() {
  const readiness = await collectInstallReadiness();
  if (readiness.managedRepoInstalled !== true || !readiness.managedRepoRoot) {
    await vscode.window.showErrorMessage(
      "Amai install не найден. Сначала установите Amai, затем откройте проект."
    );
    return;
  }
  await vscode.commands.executeCommand(
    "vscode.openFolder",
    vscode.Uri.file(readiness.managedRepoRoot),
    {
      forceNewWindow: false,
    }
  );
}

async function openOpenAiExtension() {
  await vscode.commands.executeCommand(
    "workbench.extensions.search",
    "@id:openai.chatgpt"
  );
  await vscode.commands.executeCommand(
    "workbench.view.extensions"
  );
}

async function runWorkspaceLaunch(target) {
  try {
    await launchWorkspaceChat(target);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    await vscode.window.showErrorMessage(`Amai launch failed: ${message}`);
    throw error;
  }
}

class AmaiSidebarViewProvider {
  constructor(extensionUri) {
    this.extensionUri = extensionUri;
    this.view = null;
  }

  async resolveWebviewView(webviewView) {
    this.view = webviewView;
    webviewView.webview.options = {
      enableCommandUris: true,
    };
    const readiness = await collectInstallReadiness();
    webviewView.webview.html = this.renderHtml(webviewView.webview, readiness);
  }

  renderHtml(webview, readiness) {
    const repoRoot = readiness?.repoRoot ?? "not detected";
    const identity = publicBridgeIdentity();
    const codexReady = readiness?.codexSurface?.available === true;
    const installReady = readiness?.managedRepoInstalled === true;
    const workspaceReady = readiness?.workspaceMcpConfigured === true;
    const mcpReady = readiness?.mcpConfigured === true;
    const workspaceMatchesManagedRepo = readiness?.workspaceMatchesManagedRepo === true;
    const sidebarCommandUri = codexReady ? `command:${OPEN_SIDEBAR_COMMAND_ID}` : null;
    const panelCommandUri = codexReady ? `command:${OPEN_PANEL_COMMAND_ID}` : null;
    const managedRepoCommandUri = `command:${OPEN_MANAGED_REPO_COMMAND_ID}`;
    const openAiExtensionCommandUri = `command:${OPEN_OPENAI_EXTENSION_COMMAND_ID}`;
    const reloadWindowCommandUri = `command:${RELOAD_WINDOW_COMMAND_ID}`;
    const mcpStatus = mcpReady
      ? "MCP-конфиг Amai найден"
      : "Проверьте MCP-конфиг Amai в профиле VS Code/Codium или в .vscode/mcp.json";
    const installStatus = installReady
      ? "Локальная установка Amai найдена"
      : "Сначала установите само приложение Amai";
    const codexStatus = codexReady
      ? "Чат-интеграция VS Code доступна"
      : "Сначала установите и включите совместимое chat-расширение";
    const installHint = installReady
      ? `<p class="hint">Amai repo: <code>${readiness?.managedRepoRoot ?? "~/.local/share/amai/repo"}</code>.</p>`
      : `<p class="hint">Похоже, локальная установка Amai ещё не найдена по пути <code>${readiness?.managedRepoRoot ?? "~/.local/share/amai/repo"}</code>.</p>`;
    const workspaceHint = workspaceReady
      ? (workspaceMatchesManagedRepo
          ? `<p class="hint">Текущий проект уже привязан к локальной установке Amai.</p>`
          : `<p class="hint">Проектный .vscode/mcp.json найден и готов.</p>`)
      : (mcpReady
          ? `<p class="hint">Используется user-level MCP-конфиг, поэтому Amai доступен в любом проекте.</p>`
          : `<p class="hint">После install проверьте user-level MCP-конфиг или .vscode/mcp.json и сделайте Reload Window.</p>`);
    const codexHint = codexReady
      ? ""
      : `<p class="hint">Без совместимой чат-интеграции bridge-кнопки не смогут открыть чат Amai.</p>`;
    const actionPrimary = codexReady && mcpReady
      ? `<a class="action-button" href="${sidebarCommandUri}">Открыть в Sidebar</a>`
      : `<span class="action-button disabled">Сначала закройте шаги установки ниже</span>`;
    const actionSecondary = codexReady && mcpReady
      ? `<a class="action-button secondary" href="${panelCommandUri}">Открыть в Panel</a>`
      : `<span class="action-button secondary disabled">Панель недоступна</span>`;
    const nextSteps = [
      `<li><strong>1.</strong> Установите Amai одной командой из README.</li>`,
      `<li><strong>2.</strong> Откройте любой рабочий проект в VS Code/Codium.</li>`,
      `<li><strong>3.</strong> Сделайте Reload Window.</li>`,
      `<li><strong>4.</strong> Убедитесь, что включено совместимое chat-расширение.</li>`,
      `<li><strong>5.</strong> Затем нажмите кнопку открытия Amai.</li>`,
    ].join("");
    return `<!DOCTYPE html>
<html lang="ru">
  <head>
    <meta charset="UTF-8" />
    <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline';" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <style>
      body {
        font-family: var(--vscode-font-family);
        color: var(--vscode-foreground);
        padding: 12px;
      }
      .card {
        border: 1px solid var(--vscode-panel-border);
        border-radius: 10px;
        padding: 12px;
        background: var(--vscode-sideBar-background);
      }
      h2 {
        margin: 0 0 8px;
        font-size: 16px;
      }
      p, li {
        line-height: 1.4;
      }
      .meta {
        color: var(--vscode-descriptionForeground);
        font-size: 12px;
        word-break: break-word;
      }
      .actions {
        display: grid;
        gap: 8px;
        margin-top: 12px;
      }
      .action-button {
        display: block;
        text-align: center;
        text-decoration: none;
        border: 0;
        border-radius: 8px;
        padding: 10px 12px;
        cursor: pointer;
        color: var(--vscode-button-foreground);
        background: var(--vscode-button-background);
      }
      .action-button.secondary {
        color: var(--vscode-button-secondaryForeground);
        background: var(--vscode-button-secondaryBackground);
      }
      .action-button.disabled {
        cursor: default;
        opacity: 0.65;
        pointer-events: none;
      }
      .status-list {
        display: grid;
        gap: 8px;
        margin-top: 12px;
      }
      .status-row {
        border-radius: 8px;
        padding: 8px 10px;
        font-size: 12px;
      }
      .status-row.ok {
        background: color-mix(in srgb, var(--vscode-testing-iconPassed) 16%, transparent);
      }
      .status-row.warn {
        background: color-mix(in srgb, var(--vscode-list-warningForeground) 14%, transparent);
      }
      .hint {
        color: var(--vscode-descriptionForeground);
        font-size: 12px;
        margin-top: 8px;
      }
      .steps {
        margin: 12px 0 0;
        padding-left: 18px;
      }
      .steps li {
        margin: 6px 0;
      }
      .helper-actions {
        display: grid;
        gap: 8px;
        margin-top: 12px;
      }
      .helper-link {
        color: var(--vscode-textLink-foreground);
        text-decoration: none;
      }
      code {
        font-family: var(--vscode-editor-font-family);
      }
    </style>
  </head>
  <body>
    <div class="card">
      <h2>Amai</h2>
      <p>Этот extension добавляет bridge и кнопки Amai в VS Code, но сам по себе не заменяет полную установку приложения.</p>
      <div class="meta">Bridge: ${identity.authority}@${identity.version}</div>
      <div class="meta">Workspace: ${repoRoot}</div>
      <div class="status-list">
        ${renderStatusBadge(installReady, installStatus)}
        ${renderStatusBadge(mcpReady, mcpStatus)}
        ${renderStatusBadge(codexReady, codexStatus)}
      </div>
      ${installHint}
      ${workspaceHint}
      ${codexHint}
      <ol class="steps">
        ${nextSteps}
      </ol>
      <div class="helper-actions">
        <a class="helper-link" href="${managedRepoCommandUri}">Открыть Amai Repo</a>
        <a class="helper-link" href="${reloadWindowCommandUri}">Reload Window</a>
        <a class="helper-link" href="${openAiExtensionCommandUri}">Открыть chat-расширение</a>
      </div>
      <div class="actions">
        ${actionPrimary}
        ${actionSecondary}
      </div>
    </div>
  </body>
</html>`;
  }
}

function activate(context) {
  const viewProvider = new AmaiSidebarViewProvider(context.extensionUri);
  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider(VIEW_ID, viewProvider)
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(COMMAND_ID, async (input) => {
      await openCleanChat(input ?? {});
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(OPEN_SIDEBAR_COMMAND_ID, async () => {
      await runWorkspaceLaunch("sidebar");
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(OPEN_PANEL_COMMAND_ID, async () => {
      await runWorkspaceLaunch("panel");
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(FOCUS_VIEW_COMMAND_ID, async () => {
      await vscode.commands.executeCommand(`${VIEW_ID}.focus`);
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(OPEN_MANAGED_REPO_COMMAND_ID, async () => {
      await openManagedRepoWorkspace();
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(OPEN_OPENAI_EXTENSION_COMMAND_ID, async () => {
      await openOpenAiExtension();
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(RELOAD_WINDOW_COMMAND_ID, async () => {
      await vscode.commands.executeCommand("workbench.action.reloadWindow");
    })
  );

  context.subscriptions.push(
    vscode.window.registerUriHandler({
      handleUri: async (uri) => {
        await openCleanChat(
          paramsToPayload(new URLSearchParams(uri.query)),
          uri.toString(true)
        );
      },
    })
  );
}

function deactivate() {}

module.exports = {
  activate,
  deactivate,
};
