use crate::config::{self, AppConfig};
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;
use sysinfo::{Disks, System};

#[derive(Debug, Clone, Deserialize)]
struct InstallState {
    package_version: String,
    repo_revision: String,
    client_key: String,
    client_config: String,
    stack_profile: String,
    installed_at_epoch_seconds: u64,
}

pub fn browser_base_url(bind: &str) -> String {
    let Some((host, port)) = bind.rsplit_once(':') else {
        return format!("http://{bind}");
    };
    let normalized_host = match host {
        "0.0.0.0" => "127.0.0.1".to_string(),
        "::" | "[::]" => "[::1]".to_string(),
        _ => host.to_string(),
    };
    format!("http://{normalized_host}:{port}")
}

pub fn brand_mark_svg() -> &'static str {
    include_str!("../brand/amai_mark.svg")
}

pub fn brand_lockup_svg() -> &'static str {
    include_str!("../brand/amai_lockup.svg")
}

pub fn favicon_ico() -> &'static [u8] {
    include_bytes!("../brand/favicon.ico")
}

pub fn render_html(refresh_ms: u64) -> String {
    const TEMPLATE: &str = r#"<!doctype html>
<html lang="ru">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Amai Human Dashboard</title>
  <link rel="icon" type="image/svg+xml" href="/brand/amai_mark.svg?v=__ASSET_VERSION__">
  <link rel="icon" href="/favicon.ico?v=__ASSET_VERSION__" sizes="any">
  <link rel="shortcut icon" href="/favicon.ico?v=__ASSET_VERSION__">
  <style>
    :root {
      color-scheme: light;
      --bg: linear-gradient(180deg, #f5f1e7 0%, #f3f6ef 45%, #eef4f6 100%);
      --paper: rgba(255, 252, 247, 0.92);
      --ink: #1e2a2f;
      --muted: #55666d;
      --accent: #0d6b6f;
      --accent-soft: rgba(13, 107, 111, 0.11);
      --pass: #1d7c5b;
      --pass-soft: rgba(29, 124, 91, 0.12);
      --alert: #b96d10;
      --alert-soft: rgba(185, 109, 16, 0.12);
      --critical: #b6382b;
      --critical-soft: rgba(182, 56, 43, 0.12);
      --unknown: #61717a;
      --unknown-soft: rgba(97, 113, 122, 0.12);
      --shadow: 0 18px 44px rgba(28, 43, 49, 0.12);
      --border: rgba(30, 42, 47, 0.10);
      --surface: rgba(255, 255, 255, 0.72);
      --surface-raised: rgba(255, 255, 255, 0.78);
      --surface-solid: rgba(255, 255, 255, 0.82);
      --surface-border: rgba(30, 42, 47, 0.08);
      --hero-glow: rgba(13, 107, 111, 0.11);
      --error-border: rgba(182, 56, 43, 0.18);
    }

    * { box-sizing: border-box; }
    body {
      margin: 0;
      min-height: 100vh;
      background: var(--bg);
      color: var(--ink);
      font-family: "IBM Plex Sans", "Segoe UI", "Helvetica Neue", sans-serif;
    }

    a { color: var(--accent); }

    .shell {
      max-width: 1280px;
      margin: 0 auto;
      padding: 18px 20px 40px;
    }

    .hero {
      display: grid;
      grid-template-columns: minmax(0, 1.2fr) minmax(280px, 0.8fr);
      gap: 14px;
      align-items: start;
      margin-bottom: 14px;
    }

    .panel {
      background: var(--paper);
      border: 1px solid var(--border);
      border-radius: 24px;
      box-shadow: var(--shadow);
      backdrop-filter: blur(14px);
    }

    .hero-main {
      padding: 18px 20px;
      position: relative;
      overflow: hidden;
      display: grid;
      gap: 14px;
    }

    .brand-line {
      display: flex;
      align-items: center;
      gap: 10px;
      flex-wrap: wrap;
    }

    .brand-mark {
      width: 34px;
      height: 34px;
      border-radius: 10px;
      flex: 0 0 auto;
      box-shadow: 0 10px 26px rgba(11, 16, 32, 0.12);
    }

    .hero-main::after {
      content: "";
      position: absolute;
      inset: auto -80px -120px auto;
      width: 180px;
      height: 180px;
      background: radial-gradient(circle, var(--hero-glow) 0%, rgba(13, 107, 111, 0) 70%);
      pointer-events: none;
    }

    .eyebrow {
      display: inline-flex;
      align-items: center;
      gap: 10px;
      padding: 8px 14px;
      border-radius: 999px;
      background: var(--accent-soft);
      color: var(--accent);
      font-size: 13px;
      font-weight: 700;
      letter-spacing: 0.04em;
      text-transform: uppercase;
    }

    h1 {
      margin: 12px 0 8px;
      font-size: clamp(24px, 3vw, 34px);
      line-height: 1.02;
      letter-spacing: -0.04em;
    }

    .lead {
      margin: 0;
      max-width: 60ch;
      color: var(--muted);
      font-size: 13px;
      line-height: 1.4;
    }

    .hero-cards {
      display: grid;
      grid-template-columns: repeat(3, minmax(0, 1fr));
      gap: 12px;
    }

    .hero-metric-card {
      padding: 14px 16px;
      border-radius: 18px;
    }

    .hero-metric-card .card-top {
      margin-bottom: 4px;
      align-items: center;
    }

    .hero-metric-card .card-title {
      font-size: 14px;
    }

    .hero-metric-card .card-value {
      margin: 8px 0 6px;
      font-size: clamp(22px, 3vw, 32px);
      line-height: 0.98;
    }

    .hero-metric-card .card-note {
      font-size: 13px;
      line-height: 1.4;
    }

    .hero-side {
      padding: 16px;
      display: flex;
      flex-direction: column;
      gap: 10px;
    }

    .status-pill {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 7px 12px;
      border-radius: 999px;
      font-size: 13px;
      font-weight: 700;
      width: fit-content;
    }

    .status-pill.pass { background: var(--pass-soft); color: var(--pass); }
    .status-pill.alert { background: var(--alert-soft); color: var(--alert); }
    .status-pill.critical { background: var(--critical-soft); color: var(--critical); }
    .status-pill.unknown { background: var(--unknown-soft); color: var(--unknown); }

    .side-block {
      padding: 12px 14px;
      border-radius: 18px;
      background: var(--surface);
      border: 1px solid var(--surface-border);
    }

    .side-block h2,
    .section h2 {
      margin: 0 0 8px;
      font-size: 18px;
      letter-spacing: -0.02em;
    }

    .kv {
      margin: 0;
      display: grid;
      gap: 8px;
    }

    .kv div {
      display: flex;
      justify-content: space-between;
      gap: 14px;
      font-size: 14px;
      color: var(--muted);
    }

    .kv strong {
      color: var(--ink);
      font-weight: 700;
      text-align: right;
    }

    .section {
      padding: 18px;
      margin-bottom: 14px;
    }

    .cards {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
      gap: 14px;
    }

    .metric-card,
    .service-card,
    .glossary-card,
    .link-card {
      padding: 18px;
      border-radius: 20px;
      border: 1px solid var(--surface-border);
      background: var(--surface-raised);
    }

    .metric-card.pass,
    .service-card.pass { background: linear-gradient(180deg, rgba(29, 124, 91, 0.10), var(--surface-solid)); }
    .metric-card.alert,
    .service-card.alert { background: linear-gradient(180deg, rgba(185, 109, 16, 0.10), var(--surface-solid)); }
    .metric-card.critical,
    .service-card.critical { background: linear-gradient(180deg, rgba(182, 56, 43, 0.10), var(--surface-solid)); }
    .metric-card.unknown,
    .service-card.unknown { background: linear-gradient(180deg, rgba(97, 113, 122, 0.10), var(--surface-solid)); }

    .card-top {
      display: flex;
      justify-content: space-between;
      align-items: start;
      gap: 12px;
      margin-bottom: 8px;
    }

    .card-title {
      margin: 0;
      font-size: 15px;
      color: var(--muted);
      font-weight: 700;
    }

    .card-value {
      margin: 10px 0 8px;
      font-size: clamp(24px, 4vw, 36px);
      letter-spacing: -0.04em;
      font-weight: 800;
      line-height: 0.95;
    }

    .card-note {
      margin: 0;
      color: var(--muted);
      font-size: 14px;
      line-height: 1.5;
    }

    .service-headline {
      margin: 0 0 10px;
      font-size: 22px;
      line-height: 1.1;
      letter-spacing: -0.03em;
    }

    .detail-list,
    .warning-list,
    .glossary-list,
    .next-list,
    .link-list {
      margin: 0;
      padding-left: 18px;
      display: grid;
      gap: 8px;
      color: var(--muted);
    }

    .muted {
      color: var(--muted);
      font-size: 14px;
      line-height: 1.6;
    }

    .error-banner {
      display: none;
      padding: 16px 18px;
      border-radius: 18px;
      background: var(--critical-soft);
      color: var(--critical);
      font-weight: 700;
      margin-bottom: 18px;
      border: 1px solid var(--error-border);
    }

    .link-disabled {
      color: var(--muted);
      font-weight: 700;
      text-decoration: none;
      cursor: default;
    }

    code {
      font-family: "IBM Plex Mono", "JetBrains Mono", "SFMono-Regular", monospace;
      font-size: 0.92em;
    }

    @media (prefers-color-scheme: dark) {
      :root {
        color-scheme: dark;
        --bg: radial-gradient(circle at top, #163338 0%, #10191f 36%, #0a0f13 100%);
        --paper: rgba(14, 20, 24, 0.92);
        --ink: #eef4f7;
        --muted: #a5b7bf;
        --accent: #79d2c5;
        --accent-soft: rgba(121, 210, 197, 0.14);
        --pass: #7fd8ae;
        --pass-soft: rgba(79, 158, 122, 0.22);
        --alert: #f4c06a;
        --alert-soft: rgba(185, 109, 16, 0.24);
        --critical: #ff8f82;
        --critical-soft: rgba(182, 56, 43, 0.24);
        --unknown: #b2bfca;
        --unknown-soft: rgba(97, 113, 122, 0.24);
        --shadow: 0 22px 56px rgba(0, 0, 0, 0.34);
        --border: rgba(238, 244, 247, 0.08);
        --surface: rgba(17, 25, 30, 0.78);
        --surface-raised: rgba(17, 25, 30, 0.88);
        --surface-solid: rgba(20, 30, 36, 0.94);
        --surface-border: rgba(238, 244, 247, 0.08);
        --hero-glow: rgba(121, 210, 197, 0.18);
        --error-border: rgba(255, 143, 130, 0.30);
      }
    }

    @media (max-width: 900px) {
      .hero {
        grid-template-columns: 1fr;
      }

      .hero-cards {
        grid-template-columns: 1fr;
      }
    }
  </style>
</head>
<body>
  <div class="shell">
    <div id="error-banner" class="error-banner"></div>
    <section class="hero">
      <div class="panel hero-main">
        <div class="brand-line">
          <img class="brand-mark" src="/brand/amai_mark.svg" alt="Amai">
          <div class="eyebrow">Amai Human Dashboard</div>
        </div>
        <h1>Amai: живая польза на одной странице.</h1>
        <p class="lead">
          Здесь сразу видно главное: сколько токенов уже сэкономлено, насколько быстро отвечает
          <code>Amai</code>, не течёт ли один проект в другой и всё ли в порядке у внутренних сервисов.
        </p>
        <div class="hero-cards" id="hero-cards"></div>
      </div>
      <aside class="panel hero-side">
        <div id="summary-status"></div>
        <div class="side-block">
          <h2>Коротко</h2>
          <div class="kv" id="headline-kv"></div>
        </div>
        <div class="side-block">
          <h2>Быстрые ссылки</h2>
          <ul class="link-list" id="quick-links"></ul>
        </div>
      </aside>
    </section>

    <section class="panel section">
      <h2>Скорость и защита контекста</h2>
      <p class="muted">
        Здесь собраны уже не общие слова, а рабочие инженерные цифры: быстрый повторный запрос,
        первый запрос без прогрева и контроль того, что контекст одного проекта не протекает в другой.
      </p>
      <div class="cards" id="top-cards"></div>
    </section>

    <section class="panel section">
      <h2>Машина и установка</h2>
      <p class="muted">
        Этот блок нужен не для инженеров, а чтобы вы сразу видели: на каком железе сейчас всё
        работает и к какому клиенту уже привязана установка.
      </p>
      <div class="cards" id="machine-cards"></div>
    </section>

    <section class="panel section">
      <h2>Внутренние сервисы</h2>
      <p class="muted">
        Ниже показано, что происходит внутри: база метаданных, семантический поиск, очередь
        событий и точность изоляции.
      </p>
      <div class="cards" id="service-cards"></div>
    </section>

    <section class="panel section">
      <h2>Если есть проблемы</h2>
      <div id="warnings-wrap"></div>
    </section>

    <section class="panel section">
      <h2>Что означают слова на этой странице</h2>
      <div class="cards" id="glossary-cards"></div>
    </section>
  </div>

  <script>
    const REFRESH_MS = __REFRESH_MS__;
    const errorBanner = document.getElementById("error-banner");

    function statusClass(status) {
      return ["pass", "alert", "critical", "unknown"].includes(status) ? status : "unknown";
    }

    function clearNode(node) {
      while (node.firstChild) {
        node.removeChild(node.firstChild);
      }
    }

    function textNode(tag, className, text) {
      const element = document.createElement(tag);
      if (className) {
        element.className = className;
      }
      element.textContent = text;
      return element;
    }

    function renderSummary(meta, headline) {
      const summary = document.getElementById("summary-status");
      clearNode(summary);
      const pill = textNode("div", `status-pill ${statusClass(headline.status)}`, headline.status_label);
      summary.appendChild(pill);

      const kv = document.getElementById("headline-kv");
      clearNode(kv);
      const rows = [
        ["Stack", meta.stack_name],
        ["Версия", meta.package_version],
        ["Главный KPI", headline.token_title],
        ["Сейчас", `${headline.token_value} (${headline.token_scope})`],
        ["Обновление", headline.captured_at],
        ["Автообновление", `${meta.refresh_seconds} сек.`],
      ];
      rows.forEach(([label, value]) => {
        const row = document.createElement("div");
        row.appendChild(textNode("span", "", label));
        row.appendChild(textNode("strong", "", value || "ещё нет данных"));
        kv.appendChild(row);
      });
    }

    function renderLinks(links) {
      const list = document.getElementById("quick-links");
      clearNode(list);
      links.forEach((entry) => {
        const li = document.createElement("li");
        if (entry.url) {
          const link = document.createElement("a");
          link.href = entry.url;
          link.textContent = entry.label;
          link.target = "_blank";
          link.rel = "noreferrer";
          li.appendChild(link);
        } else {
          li.appendChild(textNode("span", "link-disabled", entry.label));
        }
        if (entry.note) {
          li.appendChild(textNode("span", "muted", ` — ${entry.note}`));
        }
        list.appendChild(li);
      });
    }

    function renderCards(containerId, cards, kind) {
      const container = document.getElementById(containerId);
      clearNode(container);
      cards.forEach((card) => {
        const element = document.createElement("article");
        element.className = `${kind} ${statusClass(card.status)}`;

        const top = document.createElement("div");
        top.className = "card-top";
        top.appendChild(textNode("p", "card-title", card.title));
        top.appendChild(textNode("div", `status-pill ${statusClass(card.status)}`, card.status_label));
        element.appendChild(top);

        const valueClass = kind === "service-card" ? "service-headline" : "card-value";
        element.appendChild(textNode("p", valueClass, card.value));
        element.appendChild(textNode("p", "card-note", card.note));

        if (card.details && card.details.length > 0) {
          const list = document.createElement("ul");
          list.className = "detail-list";
          card.details.forEach((detail) => {
            list.appendChild(textNode("li", "", detail));
          });
          element.appendChild(list);
        }

        container.appendChild(element);
      });
    }

    function renderWarnings(warnings) {
      const wrap = document.getElementById("warnings-wrap");
      clearNode(wrap);

      if (!warnings || warnings.length === 0) {
        wrap.appendChild(textNode("p", "muted", "Сейчас явных проблем не видно. Если ниже появится alert или critical, он окажется здесь простым русским текстом."));
        return;
      }

      const list = document.createElement("ul");
      list.className = "warning-list";
      warnings.forEach((warning) => {
        list.appendChild(textNode("li", "", warning));
      });
      wrap.appendChild(list);
    }

    function renderGlossary(glossary) {
      const container = document.getElementById("glossary-cards");
      clearNode(container);
      glossary.forEach((entry) => {
        const card = document.createElement("article");
        card.className = "glossary-card";
        card.appendChild(textNode("h3", "card-title", entry.term));
        card.appendChild(textNode("p", "card-note", entry.meaning));
        container.appendChild(card);
      });
    }

    function showError(message) {
      errorBanner.style.display = "block";
      errorBanner.textContent = message;
    }

    function hideError() {
      errorBanner.style.display = "none";
      errorBanner.textContent = "";
    }

    async function loadDashboard() {
      try {
        const response = await fetch("/api/dashboard", { cache: "no-store" });
        if (!response.ok) {
          throw new Error(`HTTP ${response.status}`);
        }
        const payload = await response.json();
        hideError();
        renderSummary(payload.meta, payload.headline);
        renderLinks(payload.links);
        renderCards("hero-cards", payload.hero_cards, "metric-card hero-metric-card");
        renderCards("top-cards", payload.top_cards, "metric-card");
        renderCards("machine-cards", payload.machine_cards, "metric-card");
        renderCards("service-cards", payload.service_cards, "service-card");
        renderWarnings(payload.warnings);
        renderGlossary(payload.glossary);
      } catch (error) {
        showError(`Не удалось обновить панель Amai: ${error.message}`);
      }
    }

    loadDashboard();
    setInterval(loadDashboard, REFRESH_MS);
  </script>
</body>
</html>
"#;

    TEMPLATE
        .replace("__REFRESH_MS__", &refresh_ms.to_string())
        .replace("__ASSET_VERSION__", env!("CARGO_PKG_VERSION"))
}

pub fn build_payload(
    cfg: &AppConfig,
    snapshot: &Value,
    bind: &str,
    refresh_ms: u64,
) -> Result<Value> {
    let repo_root = config::discover_repo_root(None)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    let install_state = load_install_state(&repo_root).unwrap_or(None);
    let machine = collect_machine_summary(&repo_root).ok();
    let base_url = browser_base_url(bind);
    let captured_at_epoch_ms = snapshot["captured_at_epoch_ms"]
        .as_u64()
        .unwrap_or_default();

    Ok(json!({
        "meta": {
            "stack_name": cfg.stack_name,
            "package_version": env!("CARGO_PKG_VERSION"),
            "captured_at_epoch_ms": captured_at_epoch_ms,
            "captured_at_label": human_timestamp(captured_at_epoch_ms),
            "refresh_ms": refresh_ms,
            "refresh_seconds": refresh_ms / 1000,
            "base_url": base_url,
        },
        "headline": build_headline(snapshot, captured_at_epoch_ms),
        "hero_cards": build_hero_cards(snapshot),
        "top_cards": build_top_cards(snapshot),
        "machine_cards": build_machine_cards(machine.as_ref(), install_state.as_ref()),
        "service_cards": build_service_cards(snapshot),
        "warnings": build_warnings(snapshot),
        "glossary": build_glossary(),
        "links": build_links(&base_url),
    }))
}

fn build_headline(snapshot: &Value, captured_at_epoch_ms: u64) -> Value {
    let pass = snapshot["sla"]["summary"]["pass"].as_u64().unwrap_or(0);
    let alert = snapshot["sla"]["summary"]["alert"].as_u64().unwrap_or(0);
    let critical = snapshot["sla"]["summary"]["critical"].as_u64().unwrap_or(0);
    let unknown = snapshot["sla"]["summary"]["unknown"].as_u64().unwrap_or(0);
    let token_headline = &snapshot["token_budget_report"]["token_budget_report"]["headline"];
    let status = if critical > 0 {
        "critical"
    } else if alert > 0 {
        "alert"
    } else if unknown > 0 {
        "unknown"
    } else {
        "pass"
    };
    json!({
        "status": status,
        "status_label": status_label(status),
        "captured_at": human_timestamp(captured_at_epoch_ms),
        "summary": format!("SLA сейчас: pass={pass}, alert={alert}, critical={critical}, unknown={unknown}"),
        "token_title": token_headline["title"].as_str().unwrap_or("ещё нет данных"),
        "token_value": format_percent(token_headline["value_percent"].as_f64()),
        "token_scope": token_headline["scope_label"].as_str().unwrap_or("ещё нет данных"),
    })
}

fn build_top_cards(snapshot: &Value) -> Vec<Value> {
    vec![
        card(
            "Hot retrieval p95",
            format_ms(snapshot["latest_retrieval_hot"]["benchmark"]["p95_ms"].as_f64()),
            "Это повторный запрос по уже прогретому быстрому кэшу.".to_string(),
            status_for_metric_prefix(snapshot, "retrieval.hot"),
        ),
        card(
            "Cold retrieval p95",
            format_ms(snapshot["latest_retrieval_cold"]["benchmark"]["p95_ms"].as_f64()),
            "Это первый запрос после старта или без готового прогрева.".to_string(),
            status_for_metric_prefix(snapshot, "retrieval.cold"),
        ),
        card(
            "Cross-project leakage",
            format_f64_count(snapshot["latest_retrieval_accuracy"]["accuracy_verification"]["cross_project_leakage"].as_f64()),
            "Должно быть ровно 0. Иначе один проект начал подтекать в другой.".to_string(),
            status_for_metric_prefix(snapshot, "accuracy.cross_project_leakage"),
        ),
    ]
}

fn build_hero_cards(snapshot: &Value) -> Vec<Value> {
    let report = &snapshot["token_budget_report"]["token_budget_report"];
    let current_session = &report["current_session"];
    let lifetime = &report["lifetime"];
    let rolling_window = &report["rolling_window"];
    let session_events_total = current_session["events_total"].as_u64().unwrap_or(0);
    let session_events = current_session["counted_events"].as_u64().unwrap_or(0);
    let session_saved = current_session["verified_effective_saved_tokens"].as_i64();
    let session_percent = current_session["verified_effective_savings_pct"].as_f64();
    let session_started = current_session["started_at_epoch_ms"].as_u64();
    let session_ended = current_session["ended_at_epoch_ms"].as_u64();
    let lifetime_events_total = lifetime["events_total"].as_u64().unwrap_or(0);
    let lifetime_events = lifetime["counted_events"].as_u64().unwrap_or(0);
    let lifetime_saved = lifetime["verified_effective_saved_tokens"].as_i64();
    let lifetime_percent = lifetime["verified_effective_savings_pct"].as_f64();
    let lifetime_started = lifetime["started_at_epoch_ms"].as_u64();
    let lifetime_ended = lifetime["ended_at_epoch_ms"].as_u64();
    let rolling_events_total = rolling_window["events_total"].as_u64().unwrap_or(0);
    let rolling_events = rolling_window["counted_events"].as_u64().unwrap_or(0);
    let rolling_saved = rolling_window["verified_effective_saved_tokens"].as_i64();
    let rolling_percent = rolling_window["verified_effective_savings_pct"].as_f64();
    let rolling_started = rolling_window["started_at_epoch_ms"].as_u64();
    let rolling_ended = rolling_window["ended_at_epoch_ms"].as_u64();
    let rolling_window_label = report["profile"]["display_name"]
        .as_str()
        .unwrap_or("рабочее окно");
    let rolling_recovery = rolling_window["median_recovery_tokens"].as_f64();
    let session_recovery = current_session["median_recovery_tokens"].as_f64();
    let lifetime_recovery = lifetime["median_recovery_tokens"].as_f64();
    let session_answer_rate = current_session["answer_like_rate"].as_f64();
    let session_answer_count = current_session["answer_like_counted_events"]
        .as_u64()
        .unwrap_or(0);
    let session_answer_percent = current_session["verified_answer_like_savings_pct"].as_f64();
    let rolling_answer_rate = rolling_window["answer_like_rate"].as_f64();
    let rolling_answer_count = rolling_window["answer_like_counted_events"]
        .as_u64()
        .unwrap_or(0);
    let rolling_answer_percent = rolling_window["verified_answer_like_savings_pct"].as_f64();
    let lifetime_answer_rate = lifetime["answer_like_rate"].as_f64();
    let lifetime_answer_count = lifetime["answer_like_counted_events"].as_u64().unwrap_or(0);
    let lifetime_answer_percent = lifetime["verified_answer_like_savings_pct"].as_f64();

    vec![
        card(
            "Экономия токенов за текущую сессию",
            format_signed_count(session_saved),
            if session_events > 0 {
                format!(
                    "Сессия здесь = непрерывная работа без паузы дольше 30 минут. Длительность: {}. Учтено реальных Amai-запросов без потери качества: {}. Проверенная реальная экономия по ним: {}. {}",
                    elapsed_since_epoch_label(session_started, session_ended),
                    format_u64(Some(session_events)),
                    format_percent(session_percent),
                    recovery_sentence(session_recovery)
                ) + &format!(
                    " До более строгого полезного ответа без лишнего доуточнения уже дотянулись: {} событий, {} от всей выборки, экономия по ним: {}.",
                    format_u64(Some(session_answer_count)),
                    format_percent(session_answer_rate),
                    format_percent(session_answer_percent)
                )
            } else if session_events_total > 0 {
                format!(
                    "В этой сессии уже есть Amai-запросы: {}. Но они ещё не дали проверенную выборку, поэтому главный KPI по сессии пока не накоплен.",
                    format_u64(Some(session_events_total))
                )
            } else {
                "В текущей непрерывной сессии Amai ещё не накопил ни одного учтённого запроса, поэтому реальную экономию пока рано показывать.".to_string()
            },
            savings_status(session_saved, session_events, session_events_total),
        ),
        card(
            "Экономия токенов за рабочее окно",
            format_signed_count(rolling_saved),
            if rolling_events > 0 {
                format!(
                    "Это текущее скользящее окно профиля {}. Период: {}. Учтено реальных Amai-запросов без потери качества: {}. Проверенная реальная экономия: {}. {}",
                    rolling_window_label,
                    elapsed_since_epoch_label(rolling_started, rolling_ended),
                    format_u64(Some(rolling_events)),
                    format_percent(rolling_percent),
                    recovery_sentence(rolling_recovery)
                ) + &format!(
                    " До более строгого полезного ответа без лишнего доуточнения уже дошли: {} событий, {} от окна, экономия по ним: {}.",
                    format_u64(Some(rolling_answer_count)),
                    format_percent(rolling_answer_rate),
                    format_percent(rolling_answer_percent)
                )
            } else if rolling_events_total > 0 {
                format!(
                    "В текущем рабочем окне уже есть Amai-запросы: {}. Но проверенная выборка ещё не собрана, поэтому реальную экономию за окно пока рано считать устойчивой.",
                    format_u64(Some(rolling_events_total))
                )
            } else {
                "В текущем рабочем окне Amai ещё не накопил учтённых запросов, поэтому здесь пока нет живой verified статистики.".to_string()
            },
            savings_status(rolling_saved, rolling_events, rolling_events_total),
        ),
        card(
            "Экономия токенов за всё время записи",
            format_signed_count(lifetime_saved),
            if lifetime_events > 0 {
                format!(
                    "Это итог с первого проверенного Amai-запроса в этой установке. Период: {}. Учтено реальных Amai-запросов без потери качества: {}. Проверенная реальная экономия: {}. {}",
                    elapsed_since_epoch_label(lifetime_started, lifetime_ended),
                    format_u64(Some(lifetime_events)),
                    format_percent(lifetime_percent),
                    recovery_sentence(lifetime_recovery)
                ) + &format!(
                    " До более строгого полезного ответа без лишнего доуточнения за всё время дошли: {} событий, {} от всей выборки, экономия по ним: {}.",
                    format_u64(Some(lifetime_answer_count)),
                    format_percent(lifetime_answer_rate),
                    format_percent(lifetime_answer_percent)
                )
            } else if lifetime_events_total > 0 {
                format!(
                    "После установки уже накоплены Amai-запросы: {}. Но проверенная выборка ещё не собрана, поэтому главный KPI пока не считается надёжным.",
                    format_u64(Some(lifetime_events_total))
                )
            } else {
                "После установки Amai ещё не накопил учтённых запросов, поэтому здесь пока нет итоговой живой статистики.".to_string()
            },
            savings_status(lifetime_saved, lifetime_events, lifetime_events_total),
        ),
    ]
}

fn build_machine_cards(
    machine: Option<&MachineSummary>,
    install_state: Option<&InstallState>,
) -> Vec<Value> {
    let mut cards = Vec::new();
    if let Some(machine) = machine {
        cards.push(card(
            "CPU",
            format!("{} потоков", machine.logical_cpus),
            machine.cpu_model.clone(),
            "pass",
        ));
        cards.push(card(
            "Оперативная память",
            format!("{:.2} GiB", machine.total_memory_gib),
            format!(
                "Свободно сейчас {:.2} GiB, тип: {}.",
                machine.available_memory_gib, machine.memory_type
            ),
            "pass",
        ));
        cards.push(card(
            "Свободный диск",
            format!("{:.2} GiB", machine.available_disk_gib),
            "Это запас под индексы, артефакты, кэш и сборки.".to_string(),
            "pass",
        ));
    } else {
        cards.push(card(
            "Машина",
            "ещё нет данных".to_string(),
            "Сводку по железу пока не удалось собрать автоматически.".to_string(),
            "unknown",
        ));
    }

    if let Some(install_state) = install_state {
        cards.push(card(
            "Установленный клиент",
            client_display_name(&install_state.client_key).to_string(),
            format!(
                "Профиль: {}. Config: {}.",
                install_state.stack_profile, install_state.client_config
            ),
            "pass",
        ));
        cards.push(card(
            "Сборка",
            install_state.package_version.clone(),
            format!(
                "Ревизия: {}. Установлено: {}.",
                install_state.repo_revision,
                human_epoch_seconds(install_state.installed_at_epoch_seconds)
            ),
            "pass",
        ));
    } else {
        cards.push(card(
            "Установка",
            "ещё не найдена".to_string(),
            "state/install_state.json пока не найден, поэтому панель не видит последнюю user-facing установку.".to_string(),
            "unknown",
        ));
    }
    cards
}

fn build_service_cards(snapshot: &Value) -> Vec<Value> {
    vec![
        service_card(
            "PostgreSQL",
            format_ms(snapshot["postgres"]["query_probe_p95_ms"].as_f64()),
            "Это база метаданных, policy, проектов и continuity-снимков.".to_string(),
            status_for_metric_prefix(snapshot, "postgres."),
            vec![
                format!(
                    "Загрузка соединений: {}.",
                    format_ratio_percent(snapshot["postgres"]["connection_usage_ratio"].as_f64())
                ),
                format!(
                    "TPS между snapshot-ами: {}.",
                    format_optional(snapshot["postgres"]["transactions_per_sec"].as_f64(), |v| format!("{v:.2}"))
                ),
                format!(
                    "Deadlocks: {}.",
                    format_f64_count(snapshot["postgres"]["deadlocks_total"].as_f64())
                ),
                format!(
                    "WAL throughput: {}.",
                    format_optional(snapshot["postgres"]["wal_bytes_per_sec"].as_f64(), human_bytes_per_sec)
                ),
            ],
        ),
        service_card(
            "Qdrant",
            format_ms(snapshot["latest_retrieval_cold"]["retrieval_runtime"]["stage_p95_ms"]["semantic_search_ms"].as_f64()),
            "Это семантический слой. Он ускоряет recall, но не заменяет source of truth.".to_string(),
            status_for_metric_prefix(snapshot, "qdrant."),
            vec![
                format!(
                    "Optimize queue: {}.",
                    format_f64_count(snapshot["qdrant"]["index_optimize_queue"].as_f64())
                ),
                format!(
                    "Update queue: {}.",
                    format_f64_count(snapshot["qdrant"]["update_queue_length"].as_f64())
                ),
                format!(
                    "Resident memory: {}.",
                    format_optional(snapshot["qdrant"]["memory_resident_bytes"].as_f64(), human_bytes)
                ),
            ],
        ),
        service_card(
            "NATS / JetStream",
            format_ms(snapshot["nats"]["publish_probe_p95_ms"].as_f64()),
            "Это event/work plane. Через него идут фоновые события и очереди, а не source of truth.".to_string(),
            status_for_metric_prefix(snapshot, "nats."),
            vec![
                format!(
                    "Consumer lag: {}.",
                    format_f64_count(snapshot["nats"]["consumer_lag_msgs"].as_f64())
                ),
                format!(
                    "JetStream disk usage: {}.",
                    format_ratio_percent(snapshot["nats"]["jetstream_disk_usage_ratio"].as_f64())
                ),
                format!(
                    "Connections: {}.",
                    format_f64_count(snapshot["nats"]["connections"].as_f64())
                ),
            ],
        ),
        service_card(
            "Точность и изоляция",
            format_ratio_percent(snapshot["latest_retrieval_accuracy"]["accuracy_verification"]["symbol_precision"].as_f64()),
            "Этот блок отвечает на главный вопрос: не течёт ли один проект в другой и насколько надёжно Amai попадает в нужный код.".to_string(),
            worst_status(
                status_for_metric_prefix(snapshot, "parser."),
                status_for_metric_prefix(snapshot, "accuracy."),
            ),
            vec![
                format!(
                    "Cross-project leakage: {}.",
                    format_f64_count(snapshot["latest_retrieval_accuracy"]["accuracy_verification"]["cross_project_leakage"].as_f64())
                ),
                format!(
                    "Semantic precision: {}.",
                    format_ratio_percent(snapshot["latest_retrieval_accuracy"]["accuracy_verification"]["semantic_precision"].as_f64())
                ),
                format!(
                    "Parser coverage: {}.",
                    format_ratio_percent(snapshot["latest_index_project"]["index_project"]["parser_coverage_ratio"].as_f64())
                ),
            ],
        ),
        service_card(
            "Нагрузка",
            format_optional(snapshot["latest_retrieval_load_hot"]["load_verification"]["qps"].as_f64(), |v| format!("{v:.2} qps")),
            "Здесь видно, выдерживает ли прогретый быстрый путь реальную конкуренцию, а не один удачный запрос.".to_string(),
            status_for_metric_prefix(snapshot, "load."),
            vec![
                format!(
                    "Hot error rate: {}.",
                    format_percent(snapshot["latest_retrieval_load_hot"]["load_verification"]["error_rate"].as_f64())
                ),
                format!(
                    "Cold retrieval p95: {}.",
                    format_ms(snapshot["latest_retrieval_cold"]["benchmark"]["p95_ms"].as_f64())
                ),
                format!(
                    "Hot retrieval p95: {}.",
                    format_ms(snapshot["latest_retrieval_hot"]["benchmark"]["p95_ms"].as_f64())
                ),
            ],
        ),
    ]
}

fn build_warnings(snapshot: &Value) -> Vec<String> {
    let mut warnings = Vec::new();
    for check in snapshot["sla"]["checks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|check| check["status"].as_str().unwrap_or("unknown") != "pass")
    {
        warnings.push(humanize_check(check));
    }
    warnings
}

fn build_glossary() -> Vec<Value> {
    vec![
        json!({
            "term": "Hot retrieval",
            "meaning": "Повторный запрос по уже прогретому кэшу. Именно здесь Amai показывает самые быстрые цифры."
        }),
        json!({
            "term": "Cold retrieval",
            "meaning": "Первый запрос после старта или без прогрева. Он всегда тяжелее и поэтому медленнее."
        }),
        json!({
            "term": "Cross-project leakage",
            "meaning": "Случай, когда контекст одного проекта просочился в другой. Для строгого режима это должно быть только 0."
        }),
        json!({
            "term": "Parser coverage",
            "meaning": "Насколько код был разобран структурно через AST, а не грубым текстовым fallback-поиском."
        }),
        json!({
            "term": "Token savings",
            "meaning": "Сколько токенов Amai сэкономил по сравнению с наивной подачей слишком большого объёма контекста."
        }),
        json!({
            "term": "SLA summary",
            "meaning": "Короткая сводка: сколько обязательных checks сейчас проходят, предупреждают или уже горят критически."
        }),
    ]
}

fn build_links(base_url: &str) -> Vec<Value> {
    let mut links = vec![
        json!({
            "label": "Raw dashboard JSON",
            "url": format!("{base_url}/api/dashboard"),
            "note": "Если хотите отдать эти же данные другой программе."
        }),
        json!({
            "label": "Raw snapshot JSON",
            "url": format!("{base_url}/api/snapshot"),
            "note": "Полный live snapshot без human-упаковки."
        }),
        json!({
            "label": "Prometheus metrics",
            "url": format!("{base_url}/metrics"),
            "note": "Инженерный слой для scrape и алертов."
        }),
        json!({
            "label": "Health JSON",
            "url": format!("{base_url}/healthz"),
            "note": "Быстрый health-check с тем же SLA-контуром."
        }),
    ];

    let prometheus_port = env::var("AMI_PROMETHEUS_PORT").unwrap_or_else(|_| "59090".to_string());
    let grafana_port = env::var("AMI_GRAFANA_PORT").unwrap_or_else(|_| "53000".to_string());
    let grafana_admin_user =
        env::var("AMI_GRAFANA_ADMIN_USER").unwrap_or_else(|_| "admin".to_string());
    let grafana_default_password = env::var("AMI_GRAFANA_ADMIN_PASSWORD")
        .map(|value| value == "admin_change_me")
        .unwrap_or(false);
    let prometheus_available = tcp_port_is_open("127.0.0.1", &prometheus_port);
    let grafana_available = tcp_port_is_open("127.0.0.1", &grafana_port);
    links.push(json!({
        "label": "Prometheus",
        "url": if prometheus_available { Value::from(monitoring_url(base_url, &prometheus_port)) } else { Value::Null },
        "note": if prometheus_available {
            "Глубокие live-метрики уже доступны."
        } else {
            "Monitoring profile сейчас не поднят. Сначала запустите ./scripts/monitoring_up.sh."
        }
    }));
    links.push(json!({
        "label": "Grafana",
        "url": if grafana_available { Value::from(monitoring_url(base_url, &grafana_port)) } else { Value::Null },
        "note": if grafana_available {
            if grafana_default_password {
                format!("Готовый инженерный dashboard уже доступен. Логин: {}. Пароль сейчас стандартный из .env: admin_change_me. Лучше сменить его в AMI_GRAFANA_ADMIN_PASSWORD.", grafana_admin_user)
            } else {
                format!("Готовый инженерный dashboard уже доступен. Логин: {}. Текущий пароль задан в .env через AMI_GRAFANA_ADMIN_PASSWORD.", grafana_admin_user)
            }
        } else {
            "Grafana поднимается вместе с monitoring profile. Сначала запустите ./scripts/monitoring_up.sh.".to_string()
        }
    }));
    links
}

fn monitoring_url(base_url: &str, port: &str) -> String {
    let (scheme, host) = parse_base_url_host(base_url);
    format!("{scheme}://{host}:{port}")
}

fn parse_base_url_host(base_url: &str) -> (&str, &str) {
    let (scheme, rest) = base_url.split_once("://").unwrap_or(("http", base_url));
    let host = rest
        .rsplit_once(':')
        .map(|(host, _)| host)
        .unwrap_or(rest)
        .trim_end_matches('/');
    (scheme, host)
}

fn tcp_port_is_open(host: &str, port: &str) -> bool {
    let Ok(addrs) = format!("{host}:{port}").to_socket_addrs() else {
        return false;
    };
    addrs
        .into_iter()
        .any(|addr| TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok())
}

fn load_install_state(repo_root: &Path) -> Result<Option<InstallState>> {
    let path = repo_root.join("state/install_state.json");
    if !path.is_file() {
        return Ok(None);
    }
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let state =
        serde_json::from_str(&content).context("failed to parse dashboard install state")?;
    Ok(Some(state))
}

#[derive(Debug, Clone)]
struct MachineSummary {
    cpu_model: String,
    logical_cpus: usize,
    total_memory_gib: f64,
    available_memory_gib: f64,
    memory_type: String,
    available_disk_gib: f64,
}

fn collect_machine_summary(repo_root: &Path) -> Result<MachineSummary> {
    let mut system = System::new_all();
    system.refresh_memory();
    let cpu_model = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "модель CPU не определена".to_string());
    let disks = Disks::new_with_refreshed_list();
    Ok(MachineSummary {
        cpu_model,
        logical_cpus: system.cpus().len(),
        total_memory_gib: bytes_to_gib(system.total_memory()),
        available_memory_gib: bytes_to_gib(system.available_memory()),
        memory_type: detect_memory_type()
            .unwrap_or_else(|| "система не дала определить автоматически".to_string()),
        available_disk_gib: disk_available_for_path(&disks, repo_root)
            .map(bytes_to_gib)
            .unwrap_or_default(),
    })
}

fn detect_memory_type() -> Option<String> {
    for (program, args) in [
        ("dmidecode", vec!["--type", "17"]),
        ("lshw", vec!["-class", "memory"]),
    ] {
        let output = std::process::Command::new(program)
            .args(args)
            .output()
            .ok()?;
        if !output.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        if let Some(found) = extract_memory_generation(&text) {
            return Some(found);
        }
    }
    None
}

fn extract_memory_generation(text: &str) -> Option<String> {
    for candidate in ["DDR5", "LPDDR5", "DDR4", "LPDDR4", "DDR3"] {
        if text.contains(candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

fn disk_available_for_path(disks: &Disks, path: &Path) -> Option<u64> {
    let canonical = path.canonicalize().ok()?;
    disks
        .iter()
        .filter(|disk| canonical.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .map(|disk| disk.available_space())
}

fn card(title: &str, value: String, note: String, status: &str) -> Value {
    json!({
        "title": title,
        "value": value,
        "note": note,
        "status": status,
        "status_label": status_label(status),
    })
}

fn service_card(
    title: &str,
    value: String,
    note: String,
    status: &str,
    details: Vec<String>,
) -> Value {
    json!({
        "title": title,
        "value": value,
        "note": note,
        "status": status,
        "status_label": status_label(status),
        "details": details,
    })
}

fn status_label(status: &str) -> &'static str {
    match status {
        "pass" => "в норме",
        "alert" => "внимание",
        "critical" => "критично",
        _ => "нет данных",
    }
}

fn savings_status(
    saved_tokens: Option<i64>,
    counted_events: u64,
    events_total: u64,
) -> &'static str {
    if counted_events == 0 {
        if events_total == 0 {
            "unknown"
        } else {
            "alert"
        }
    } else if saved_tokens.unwrap_or_default() < 0 {
        "alert"
    } else {
        "pass"
    }
}

fn recovery_sentence(median_recovery_tokens: Option<f64>) -> String {
    match median_recovery_tokens {
        Some(value) if value > 0.0 => {
            format!(
                "Медианный штраф на доуточнение: {} токенов.",
                value.round() as i64
            )
        }
        Some(_) => "Доуточнения пока не отъедали токены назад.".to_string(),
        None => "Штраф на доуточнение пока ещё не накоплен.".to_string(),
    }
}

fn status_for_metric_prefix(snapshot: &Value, prefix: &str) -> &'static str {
    let mut current: Option<&str> = None;
    for check in snapshot["sla"]["checks"].as_array().into_iter().flatten() {
        let metric = check["metric"].as_str().unwrap_or_default();
        if !metric.starts_with(prefix) {
            continue;
        }
        let status = check["status"].as_str().unwrap_or("unknown");
        current = Some(match current {
            Some(existing) => worst_status(existing, status),
            None => match status {
                "pass" => "pass",
                "alert" => "alert",
                "critical" => "critical",
                _ => "unknown",
            },
        });
    }
    current.unwrap_or("unknown")
}

fn worst_status(left: &str, right: &str) -> &'static str {
    if status_rank(left) >= status_rank(right) {
        match left {
            "pass" => "pass",
            "alert" => "alert",
            "critical" => "critical",
            _ => "unknown",
        }
    } else {
        match right {
            "pass" => "pass",
            "alert" => "alert",
            "critical" => "critical",
            _ => "unknown",
        }
    }
}

fn status_rank(status: &str) -> u8 {
    match status {
        "critical" => 4,
        "alert" => 3,
        "pass" => 2,
        "unknown" => 1,
        _ => 0,
    }
}

fn humanize_check(check: &Value) -> String {
    let metric = check["metric"].as_str().unwrap_or("unknown.metric");
    let status = status_label(check["status"].as_str().unwrap_or("unknown"));
    let value = match check["value"].as_f64() {
        Some(number) if metric.ends_with("_ratio") => format!("{:.2}%", number * 100.0),
        Some(number) if metric.contains("p95_ms") => format!("{number:.3} ms"),
        Some(number) => format!("{number:.3}"),
        None => "ещё нет данных".to_string(),
    };
    let explanation = match metric {
        "postgres.connection_usage_ratio" => "PostgreSQL использует слишком много соединений.",
        "postgres.query_probe_p95_ms" => "PostgreSQL отвечает медленнее, чем должен.",
        "postgres.replica_lag_seconds" => {
            "Отставание реплики PostgreSQL вышло за допустимый контур."
        }
        "postgres.deadlocks_total" => {
            "В PostgreSQL появились deadlock-и, а здесь ожидается строго 0."
        }
        "qdrant.index_optimize_queue" => "У Qdrant выросла очередь оптимизации индекса.",
        "qdrant.update_queue_length" => "У Qdrant растёт очередь обновлений.",
        "qdrant.search_stage_p95_ms" => "Семантический поиск в Qdrant стал заметно тяжелее.",
        "nats.publish_probe_p95_ms" => "NATS публикует события медленнее ожидаемого.",
        "nats.consumer_lag_msgs" => "У JetStream накопилось слишком много непрочитанных сообщений.",
        "nats.jetstream_disk_usage_ratio" => "JetStream слишком близко подошёл к лимиту диска.",
        "retrieval.cold_p95_ms" => "Первый запрос после старта стал слишком медленным.",
        "retrieval.hot_p95_ms" => "Быстрый повторный запрос больше не укладывается в stretch-goal.",
        "parser.coverage_ratio" => {
            "Слишком часто приходится падать в грубый текстовый fallback вместо AST-разбора."
        }
        "accuracy.cross_project_leakage" => {
            "Один проект начал подтекать в другой, а этого быть не должно."
        }
        "accuracy.symbol_precision" => "Попадание в нужные символы стало менее точным.",
        "accuracy.semantic_precision" => {
            "Семантический поиск стал реже попадать в правильные ответы."
        }
        "load.hot_qps" => "Горячий быстрый путь держит меньше QPS, чем обещано.",
        "load.hot_error_rate" => "Под нагрузкой появились ошибки на быстром пути.",
        _ => "Один из обязательных проверочных контуров вышел из своей нормы.",
    };
    format!("{explanation} Сейчас: {value}. Состояние: {status}.")
}

fn human_timestamp(epoch_ms: u64) -> String {
    if epoch_ms == 0 {
        return "ещё нет данных".to_string();
    }
    let secs = epoch_ms / 1000;
    format!("epoch {secs}")
}

fn human_epoch_seconds(epoch_seconds: u64) -> String {
    if epoch_seconds == 0 {
        return "ещё нет данных".to_string();
    }
    format!("epoch {epoch_seconds}")
}

fn client_display_name(key: &str) -> &str {
    match key {
        "vscode" => "VS Code",
        "cursor" => "Cursor",
        "codex" => "Codex",
        "claude-code" => "Claude Code",
        "claude-desktop" => "Claude Desktop",
        other => other,
    }
}

fn format_ms(value: Option<f64>) -> String {
    value
        .map(|number| format!("{number:.3} ms"))
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

fn format_ratio_percent(value: Option<f64>) -> String {
    value
        .map(|number| format!("{:.2}%", number * 100.0))
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

fn format_percent(value: Option<f64>) -> String {
    value
        .map(|number| format!("{number:.2}%"))
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

fn format_u64(value: Option<u64>) -> String {
    value
        .map(|number| number.to_string())
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

fn format_signed_count(value: Option<i64>) -> String {
    value
        .map(|number| number.to_string())
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

fn format_f64_count(value: Option<f64>) -> String {
    value
        .map(|number| format!("{number:.0}"))
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

fn format_optional<F>(value: Option<f64>, formatter: F) -> String
where
    F: FnOnce(f64) -> String,
{
    value
        .map(formatter)
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

fn human_bytes(value: f64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    if value >= GIB {
        format!("{:.2} GiB", value / GIB)
    } else if value >= MIB {
        format!("{:.2} MiB", value / MIB)
    } else if value >= KIB {
        format!("{:.2} KiB", value / KIB)
    } else {
        format!("{value:.0} B")
    }
}

fn human_bytes_per_sec(value: f64) -> String {
    format!("{}/s", human_bytes(value))
}

fn elapsed_since_epoch_label(start_epoch_ms: Option<u64>, end_epoch_ms: Option<u64>) -> String {
    let Some(start_epoch_ms) = start_epoch_ms.filter(|value| *value > 0) else {
        return "ещё нет данных".to_string();
    };
    let Some(end_epoch_ms) = end_epoch_ms.filter(|value| *value >= start_epoch_ms) else {
        return "ещё нет данных".to_string();
    };
    human_elapsed_ms(end_epoch_ms.saturating_sub(start_epoch_ms))
}

fn human_elapsed_ms(value_ms: u64) -> String {
    let total_minutes = value_ms / 60_000;
    if total_minutes < 1 {
        return "меньше минуты".to_string();
    }

    let days = total_minutes / (60 * 24);
    let hours = (total_minutes % (60 * 24)) / 60;
    let minutes = total_minutes % 60;
    let mut parts = Vec::new();

    if days > 0 {
        parts.push(format!("{days} дн."));
    }
    if hours > 0 {
        parts.push(format!("{hours} ч."));
    }
    if minutes > 0 {
        parts.push(format!("{minutes} мин."));
    }

    if parts.is_empty() {
        "меньше минуты".to_string()
    } else {
        parts.join(" ")
    }
}

fn bytes_to_gib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}

#[cfg(test)]
mod tests {
    use super::{browser_base_url, human_elapsed_ms, monitoring_url, worst_status};

    #[test]
    fn browser_url_rewrites_unspecified_v4() {
        assert_eq!(browser_base_url("0.0.0.0:9464"), "http://127.0.0.1:9464");
    }

    #[test]
    fn critical_status_wins() {
        assert_eq!(worst_status("pass", "critical"), "critical");
        assert_eq!(worst_status("alert", "unknown"), "alert");
        assert_eq!(worst_status("unknown", "pass"), "pass");
    }

    #[test]
    fn monitoring_url_reuses_dashboard_host() {
        assert_eq!(
            monitoring_url("http://demo-host:9464", "59090"),
            "http://demo-host:59090"
        );
    }

    #[test]
    fn elapsed_label_is_compact() {
        assert_eq!(human_elapsed_ms(30_000), "меньше минуты");
        assert_eq!(human_elapsed_ms(61_000), "1 мин.");
        assert_eq!(human_elapsed_ms(3_720_000), "1 ч. 2 мин.");
    }
}
