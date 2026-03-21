use crate::config::{self, AppConfig};
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use sysinfo::{Disks, System};
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

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

    .card-source {
      margin-top: 8px;
      color: var(--accent);
      font-size: 12px;
      font-weight: 700;
      letter-spacing: 0.02em;
    }

    .metric-rows {
      margin: 14px 0 0;
      padding: 0;
      list-style: none;
      display: grid;
      gap: 8px;
    }

    .metric-row {
      display: grid;
      grid-template-columns: minmax(0, 1fr) auto;
      gap: 12px;
      align-items: start;
      padding-top: 8px;
      border-top: 1px solid var(--surface-border);
    }

    .metric-label {
      color: var(--muted);
      font-size: 13px;
      line-height: 1.35;
      display: inline-flex;
      align-items: center;
      gap: 6px;
      flex-wrap: wrap;
    }

    .metric-row-value {
      color: var(--ink);
      font-size: 13px;
      line-height: 1.35;
      font-weight: 700;
      text-align: right;
    }

    .has-tooltip {
      position: relative;
      display: inline-block;
      cursor: help;
      text-decoration: underline dotted rgba(13, 107, 111, 0.45);
      text-underline-offset: 3px;
    }

    .has-tooltip::after {
      content: attr(data-tip);
      position: absolute;
      left: 50%;
      bottom: calc(100% + 10px);
      transform: translateX(-50%) translateY(4px);
      min-width: 220px;
      max-width: 320px;
      padding: 10px 12px;
      border-radius: 12px;
      background: rgba(8, 13, 17, 0.96);
      color: #f7fafc;
      font-size: 12px;
      line-height: 1.45;
      text-transform: none;
      letter-spacing: normal;
      white-space: normal;
      opacity: 0;
      pointer-events: none;
      box-shadow: 0 18px 42px rgba(0, 0, 0, 0.28);
      transition: opacity 0.14s ease, transform 0.14s ease;
      z-index: 20;
    }

    .has-tooltip:hover::after,
    .has-tooltip:focus-visible::after {
      opacity: 1;
      transform: translateX(-50%) translateY(0);
    }

    .compare-card {
      padding: 20px;
      border-radius: 20px;
      border: 1px solid var(--surface-border);
      background: var(--surface-raised);
      display: grid;
      gap: 16px;
    }

    .compare-card.pass { background: linear-gradient(180deg, rgba(29, 124, 91, 0.10), var(--surface-solid)); }
    .compare-card.alert { background: linear-gradient(180deg, rgba(185, 109, 16, 0.10), var(--surface-solid)); }
    .compare-card.critical { background: linear-gradient(180deg, rgba(182, 56, 43, 0.10), var(--surface-solid)); }
    .compare-card.unknown { background: linear-gradient(180deg, rgba(97, 113, 122, 0.10), var(--surface-solid)); }

    .compare-head {
      display: flex;
      justify-content: space-between;
      align-items: start;
      gap: 12px;
    }

    .compare-grid {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 14px;
    }

    .compare-metric {
      border: 1px solid var(--surface-border);
      border-radius: 18px;
      background: var(--surface);
      padding: 16px;
      display: grid;
      gap: 6px;
    }

    .compare-metric-label {
      margin: 0;
      color: var(--muted);
      font-size: 14px;
      font-weight: 700;
    }

    .compare-metric-value {
      margin: 0;
      font-size: clamp(24px, 4vw, 34px);
      line-height: 0.95;
      font-weight: 800;
      letter-spacing: -0.04em;
    }

    .compare-metric-note {
      margin: 0;
      color: var(--muted);
      font-size: 13px;
      line-height: 1.45;
    }

    .compare-table-wrap {
      overflow-x: auto;
    }

    .compare-table {
      width: 100%;
      border-collapse: collapse;
      font-size: 13px;
      line-height: 1.35;
    }

    .compare-table th,
    .compare-table td {
      padding: 10px 12px;
      border-top: 1px solid var(--surface-border);
      text-align: right;
      vertical-align: top;
      white-space: nowrap;
    }

    .compare-table th:first-child,
    .compare-table td:first-child {
      text-align: left;
      white-space: normal;
    }

    .compare-table thead th {
      color: var(--muted);
      font-weight: 700;
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

      .compare-grid {
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
        <h1>Amai: понятная картина без гадания.</h1>
        <p class="lead">
          Здесь каждая цифра подписана честно: это живой поток прямо сейчас, последний проверочный
          прогон или текущий системный probe. Для каждой метрики есть <code>Эталон</code>,
          <code>Сейчас</code> и объяснение, что именно она означает.
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
      <h2>Живой Поток Сейчас</h2>
      <p class="muted">
        Это именно текущая живая сессия. Здесь нет старых benchmark-цифр: только потоковые метрики,
        которые меняются по мере новых запросов и автообновляются на странице.
      </p>
      <div class="cards" id="top-cards"></div>
    </section>

    <section class="panel section">
      <h2>Последние Честные Проверки</h2>
      <p class="muted">
        Эти цифры не потоковые. Здесь лежат последние сохранённые отдельные проверки:
        нагрузка быстрого пути, полный холодный прогон и проверка точности с изоляцией.
        Они нужны, чтобы сравнивать систему с её целями на повторяемых измерениях.
      </p>
      <div class="cards" id="benchmark-cards"></div>
    </section>

    <section class="panel section">
      <h2>Сервисы Прямо Сейчас</h2>
      <p class="muted">
        Это живые системные показатели: база метаданных, векторный слой и очередь событий.
        Если какая-то строка берётся не из live probe, а из последнего измеренного прогона,
        это будет подписано явно.
      </p>
      <div class="cards" id="service-cards"></div>
    </section>

    <section class="panel section">
      <h2>Машина И Установка</h2>
      <p class="muted">
        Здесь видно, на каком железе сейчас всё работает и к какому клиенту уже привязана установка.
      </p>
      <div class="cards" id="machine-cards"></div>
    </section>

    <section class="panel section">
      <h2>Если есть проблемы</h2>
      <div id="warnings-wrap"></div>
    </section>

    <section class="panel section">
      <h2>Что Означают Термины И Метрики</h2>
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

    function labelWithTooltip(text, tooltip, className = "metric-label") {
      const wrap = document.createElement("span");
      wrap.className = tooltip ? `${className} has-tooltip` : className;
      if (tooltip) {
        wrap.tabIndex = 0;
        wrap.setAttribute("data-tip", tooltip);
      }
      wrap.textContent = text;
      return wrap;
    }

    function renderCompareCard(container, card) {
      const element = document.createElement("article");
      element.className = `compare-card ${statusClass(card.status)}`;

      const head = document.createElement("div");
      head.className = "compare-head";
      head.appendChild(labelWithTooltip(card.title, card.title_tooltip, "card-title"));
      head.appendChild(textNode("div", `status-pill ${statusClass(card.status)}`, card.status_label));
      element.appendChild(head);

      if (card.source_label) {
        element.appendChild(textNode("p", "card-source", card.source_label));
      }
      element.appendChild(textNode("p", "card-note", card.note));

      if (card.metrics && card.metrics.length > 0) {
        const compareGrid = document.createElement("div");
        compareGrid.className = "compare-grid";
        card.metrics.forEach((metric) => {
          const metricCard = document.createElement("section");
          metricCard.className = "compare-metric";
          metricCard.appendChild(labelWithTooltip(metric.label, metric.tooltip, "compare-metric-label"));
          metricCard.appendChild(textNode("p", "compare-metric-value", metric.value));
          metricCard.appendChild(textNode("p", "compare-metric-note", metric.note));
          compareGrid.appendChild(metricCard);
        });
        element.appendChild(compareGrid);
      }

      const tableWrap = document.createElement("div");
      tableWrap.className = "compare-table-wrap";
      const table = document.createElement("table");
      table.className = "compare-table";

      const thead = document.createElement("thead");
      const headRow = document.createElement("tr");
      card.table.columns.forEach((column) => {
        const th = document.createElement("th");
        th.appendChild(labelWithTooltip(column.label, column.tooltip, ""));
        headRow.appendChild(th);
      });
      thead.appendChild(headRow);
      table.appendChild(thead);

      const tbody = document.createElement("tbody");
      card.table.rows.forEach((row) => {
        const tr = document.createElement("tr");
        const labelCell = document.createElement("td");
        labelCell.appendChild(labelWithTooltip(row.label, row.tooltip, ""));
        tr.appendChild(labelCell);
        row.values.forEach((value) => {
          tr.appendChild(textNode("td", "", value));
        });
        tbody.appendChild(tr);
      });
      table.appendChild(tbody);
      tableWrap.appendChild(table);
      element.appendChild(tableWrap);

      container.appendChild(element);
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
        if (card.kind === "live_compare" || card.kind === "compare_table") {
          renderCompareCard(container, card);
          return;
        }
        const element = document.createElement("article");
        element.className = `${kind} ${statusClass(card.status)}`;

        const top = document.createElement("div");
        top.className = "card-top";
        top.appendChild(labelWithTooltip(card.title, card.title_tooltip));
        top.appendChild(textNode("div", `status-pill ${statusClass(card.status)}`, card.status_label));
        element.appendChild(top);

        const valueClass = kind === "service-card" ? "service-headline" : "card-value";
        element.appendChild(textNode("p", valueClass, card.value));
        if (card.source_label) {
          element.appendChild(textNode("p", "card-source", card.source_label));
        }
        element.appendChild(textNode("p", "card-note", card.note));

        if (card.rows && card.rows.length > 0) {
          const list = document.createElement("ul");
          list.className = "metric-rows";
          card.rows.forEach((row) => {
            const item = document.createElement("li");
            item.className = "metric-row";
            item.appendChild(labelWithTooltip(row.label, row.tooltip));
            item.appendChild(textNode("span", "metric-row-value", row.value));
            list.appendChild(item);
          });
          element.appendChild(list);
        }

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
        renderCards("benchmark-cards", payload.benchmark_cards, "metric-card");
        renderCards("service-cards", payload.service_cards, "service-card");
        renderCards("machine-cards", payload.machine_cards, "metric-card");
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
        "benchmark_cards": build_benchmark_cards(snapshot),
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
    vec![live_latency_compare_card(snapshot)]
}

fn build_benchmark_cards(snapshot: &Value) -> Vec<Value> {
    let hot_load = &snapshot["latest_retrieval_load_hot"]["load_verification"];
    let cold_contour = &snapshot["latest_cold_path_benchmark"]["cold_benchmark"];
    let accuracy = &snapshot["latest_retrieval_accuracy"]["accuracy_verification"];
    let thresholds = &snapshot["thresholds"];
    let hot_load_sample_count = hot_load["success_count"]
        .as_u64()
        .zip(hot_load["error_count"].as_u64())
        .map(|(success, errors)| success + errors);

    vec![
        compare_table_card(
            "Быстрый путь под нагрузкой",
            "Это benchmark-карточка. Здесь нет живой телеметрии текущей сессии: показан только последний сохранённый hot-load прогон по прогретому быстрому пути."
                .to_string(),
            hot_load_benchmark_status(hot_load, thresholds),
            Some(source_label(
                "Источник: benchmark. Последний сохранённый hot-load verification snapshot; live-данные страницы сюда не подмешиваются",
                hot_load["captured_at_epoch_ms"].as_u64(),
            )),
            Some("Это отдельный проверочный прогон прогретого пути. Он показывает, какую нагрузку выдержал Amai в benchmark-режиме, а не то, что происходит прямо сейчас в чате.".to_string()),
            vec![
                compare_table_row(
                    "QPS",
                    "Сколько запросов в секунду выдержал быстрый прогретый путь в отдельном benchmark-прогоне.",
                    compare_pair(
                        format_threshold_at_least(
                            thresholds["load"]["hot_qps"].get("target").and_then(Value::as_f64),
                            "qps",
                            0,
                        ),
                        format_optional(hot_load["qps"].as_f64(), |v| format!("{v:.2} qps")),
                    ),
                ),
                compare_table_row(
                    "P50",
                    "Медиана hot benchmark. Обычный уровень задержки в отдельном нагрузочном прогоне.",
                    compare_pair(
                        format_threshold_at_most(
                            thresholds["load"]["hot_benchmark_table"]["target_p50_ms"]
                                .as_f64(),
                            "ms",
                            3,
                        ),
                        format_ms(hot_load["p50_ms"].as_f64()),
                    ),
                ),
                compare_table_row(
                    "P95",
                    "Тяжёлый хвост hot benchmark. Почти все прогретые ответы должны укладываться в эту границу.",
                    compare_pair(
                        format_threshold_at_most(
                            thresholds["load"]["hot_benchmark_table"]["target_p95_ms"]
                                .as_f64(),
                            "ms",
                            3,
                        ),
                        format_ms(hot_load["p95_ms"].as_f64()),
                    ),
                ),
                compare_table_row(
                    "P99",
                    "Редкие тяжёлые выбросы в отдельном hot-load benchmark.",
                    compare_pair(
                        format_threshold_at_most(
                            thresholds["load"]["hot_benchmark_table"]["target_p99_ms"]
                                .as_f64(),
                            "ms",
                            3,
                        ),
                        format_ms(hot_load["p99_ms"].as_f64()),
                    ),
                ),
                compare_table_row(
                    "Max",
                    "Самый тяжёлый одиночный запрос в последнем hot-load benchmark.",
                    compare_pair(
                        format_threshold_at_most(
                            thresholds["load"]["hot_benchmark_table"]["target_max_ms"]
                                .as_f64(),
                            "ms",
                            3,
                        ),
                        format_ms(hot_load["max_ms"].as_f64()),
                    ),
                ),
                compare_table_row(
                    "Error rate",
                    "Доля ошибок в отдельном hot-load benchmark. Здесь целевой уровень должен быть нулевым.",
                    compare_pair(
                        format_threshold_at_most(
                            thresholds["load"]["hot_error_rate"].get("target").and_then(Value::as_f64),
                            "%",
                            2,
                        ),
                        format_percent(hot_load["error_rate"].as_f64()),
                    ),
                ),
                compare_table_row(
                    "Workers",
                    "Сколько параллельных worker-ов участвовало в benchmark-прогоне.",
                    compare_pair(
                        format_threshold_at_least(
                            thresholds["load"]["hot_benchmark_table"]["target_workers"]
                                .as_f64(),
                            "",
                            0,
                        ),
                        format_u64(hot_load["workers"].as_u64()),
                    ),
                ),
                compare_table_row(
                    "Выборка",
                    "Сколько отдельных запросов вошло в benchmark. Это не живая сессия, а размер сохранённого проверочного прогона.",
                    compare_pair(
                        format_threshold_at_least(
                            thresholds["load"]["hot_benchmark_table"]["target_sample_count"]
                                .as_f64(),
                            "",
                            0,
                        ),
                        format_u64(hot_load_sample_count),
                    ),
                ),
            ],
        ),
        card_with_rows(
            "Полный холодный прогон",
            format_ms(cold_contour["machine_readable_summary"]["p95"].as_f64()),
            "Это последний честный end-to-end cold benchmark по реальным репозиториям и query slices.".to_string(),
            cold_contour_status(snapshot),
            Some(source_label(
                "Источник: benchmark. Последний сохранённый end-to-end cold benchmark; live-данные страницы сюда не подмешиваются",
                cold_contour["captured_at_epoch_ms"].as_u64(),
            )),
            Some("Cold contour меряет полный путь policy -> retrieval -> ranking -> pack assembly -> provenance -> orchestration.".to_string()),
            vec![
                metric_row(
                    "Эталон cold P95",
                    format_ms(cold_contour["profile"]["target_p95_ms"].as_f64()),
                    Some("Цель для p95 в полном cold end-to-end пути."),
                ),
                metric_row(
                    "Сейчас cold P95",
                    format_ms(cold_contour["machine_readable_summary"]["p95"].as_f64()),
                    Some("Фактический p95 последнего end-to-end cold benchmark."),
                ),
                metric_row(
                    "Эталон cold P99",
                    format_ms(cold_contour["profile"]["target_p99_ms"].as_f64()),
                    Some("Цель для p99 в полном cold end-to-end пути."),
                ),
                metric_row(
                    "Сейчас cold P99",
                    format_ms(cold_contour["machine_readable_summary"]["p99"].as_f64()),
                    Some("Фактический p99 последнего end-to-end cold benchmark."),
                ),
                metric_row(
                    "Эталон cold Max",
                    format_ms(cold_contour["profile"]["target_max_ms"].as_f64()),
                    Some("Самый большой выброс в cold benchmark должен оставаться в этой границе."),
                ),
                metric_row(
                    "Сейчас cold Max",
                    format_ms(cold_contour["machine_readable_summary"]["max"].as_f64()),
                    Some("Самый большой выброс в последнем cold benchmark."),
                ),
                metric_row(
                    "Эталон precision",
                    format_ratio_percent(cold_contour["profile"]["min_precision"].as_f64()),
                    Some("Доля выданного контекста, которая оказалась релевантной."),
                ),
                metric_row(
                    "Сейчас precision",
                    format_ratio_percent(cold_contour["machine_readable_summary"]["precision"].as_f64()),
                    Some("Фактическая точность последнего cold benchmark."),
                ),
                metric_row(
                    "Эталон recall",
                    format_ratio_percent(cold_contour["profile"]["min_recall"].as_f64()),
                    Some("Насколько полно система нашла нужные целевые данные."),
                ),
                metric_row(
                    "Сейчас recall",
                    format_ratio_percent(cold_contour["machine_readable_summary"]["recall"].as_f64()),
                    Some("Фактическая полнота последнего cold benchmark."),
                ),
                metric_row(
                    "Эталон hit rate",
                    format_ratio_percent(cold_contour["profile"]["min_target_hit_rate"].as_f64()),
                    Some("Доля запросов, где система действительно попала в нужную цель."),
                ),
                metric_row(
                    "Сейчас hit rate",
                    format_ratio_percent(cold_contour["machine_readable_summary"]["hit_rate"].as_f64()),
                    Some("Фактическая доля успешных попаданий в цели в последнем cold benchmark."),
                ),
                metric_row(
                    "Сейчас выборка",
                    format_u64(cold_contour["machine_readable_summary"]["sample_count"].as_u64()),
                    Some("Сколько cold-запросов вошло в итоговый benchmark."),
                ),
                metric_row(
                    "Сейчас repo count",
                    format_u64(cold_contour["machine_readable_summary"]["repo_count"].as_u64()),
                    Some("Сколько разных репозиториев вошло в последний cold benchmark."),
                ),
                metric_row(
                    "Сейчас query slices",
                    format_u64(cold_contour["machine_readable_summary"]["query_slice_count"].as_u64()),
                    Some("Сколько разных типов запросов покрывает последний cold benchmark."),
                ),
                metric_row(
                    "Сейчас duration",
                    format_optional(
                        cold_contour["machine_readable_summary"]["duration"].as_f64(),
                        |value| format!("{value:.2} сек."),
                    ),
                    Some("Сколько длился полный последний cold benchmark."),
                ),
            ],
        ),
        card_with_rows(
            "Точность и изоляция",
            format_f64_count(accuracy["cross_project_leakage"].as_f64()),
            "Этот блок не потоковый: он показывает последний сохранённый accuracy/isolation verification contour.".to_string(),
            worst_status(
                status_for_metric_prefix(snapshot, "accuracy.cross_project_leakage"),
                worst_status(
                    status_for_metric_prefix(snapshot, "accuracy.symbol_precision"),
                    status_for_metric_prefix(snapshot, "accuracy.semantic_precision"),
                ),
            ),
            Some(source_label(
                "Источник: benchmark. Последний сохранённый retrieval accuracy verification; live-данные страницы сюда не подмешиваются",
                accuracy["captured_at_epoch_ms"].as_u64(),
            )),
            Some("Проверка точности и изоляции показывает, не течёт ли один проект в другой и насколько точно Amai попадает в нужные символы и семантику.".to_string()),
            vec![
                metric_row(
                    "Эталон leakage",
                    "0".to_string(),
                    Some("Для строгой проектной изоляции утечки между проектами должны быть равны нулю."),
                ),
                metric_row(
                    "Сейчас leakage",
                    format_f64_count(accuracy["cross_project_leakage"].as_f64()),
                    Some("Фактическое число утечек между проектами в последнем verification contour."),
                ),
                metric_row(
                    "Эталон symbol precision",
                    format_ratio_percent(thresholds["accuracy"]["symbol_precision"]["target"].as_f64()),
                    Some("Насколько точно retrieval попадает в нужные символы, функции и сущности."),
                ),
                metric_row(
                    "Сейчас symbol precision",
                    format_ratio_percent(accuracy["symbol_precision"].as_f64()),
                    Some("Фактическая точность по символам в последнем verification contour."),
                ),
                metric_row(
                    "Эталон semantic precision",
                    format_ratio_percent(thresholds["accuracy"]["semantic_precision"]["target"].as_f64()),
                    Some("Насколько точно семантический слой попадает в правильный контекст."),
                ),
                metric_row(
                    "Сейчас semantic precision",
                    format_ratio_percent(accuracy["semantic_precision"].as_f64()),
                    Some("Фактическая семантическая точность в последнем verification contour."),
                ),
            ],
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
        cards.push(card_with_rows(
            "CPU",
            format!("{} потоков", machine.logical_cpus),
            machine.cpu_model.clone(),
            "pass",
            None,
            Some("Статический профиль процессора плюс живые текущие показатели загрузки и температуры.".to_string()),
            vec![
                metric_row(
                    "Сейчас нагрузка",
                    format_optional(machine.cpu_usage_percent, |value| format!("{value:.1}%")),
                    Some("Живая текущая загрузка CPU по всей системе."),
                ),
                metric_row(
                    "Сейчас температура",
                    format_optional(machine.cpu_temperature_celsius, format_celsius),
                    Some("Текущая температура CPU по датчику Tctl."),
                ),
                metric_row(
                    "Сейчас максимум частоты",
                    format_optional(machine.cpu_max_mhz, |value| format!("{value:.0} MHz")),
                    Some("Максимальная частота процессора, которую система сообщает прямо сейчас."),
                ),
            ],
        ));
        cards.push(card_with_rows(
            "Оперативная память",
            format!("{:.2} GiB", machine.total_memory_gib),
            format!(
                "Тип: {}. Скорость: {}.",
                machine.memory_type, machine.memory_speed_label
            ),
            "pass",
            None,
            Some(
                "Статический профиль RAM плюс живые показатели текущего использования.".to_string(),
            ),
            vec![
                metric_row(
                    "Сейчас тип",
                    machine.memory_type.clone(),
                    Some("Автоматически определённый тип оперативной памяти."),
                ),
                metric_row(
                    "Сейчас скорость",
                    machine.memory_speed_label.clone(),
                    Some("Автоматически определённая скорость оперативной памяти."),
                ),
                metric_row(
                    "Сейчас занято",
                    format!("{:.2} GiB", machine.used_memory_gib),
                    Some("Сколько оперативной памяти занято прямо сейчас."),
                ),
                metric_row(
                    "Сейчас свободно",
                    format!("{:.2} GiB", machine.available_memory_gib),
                    Some("Сколько оперативной памяти система считает доступной прямо сейчас."),
                ),
                metric_row(
                    "Сейчас usage",
                    format_optional(machine.memory_used_percent, |value| format!("{value:.1}%")),
                    Some("Текущая доля занятой оперативной памяти."),
                ),
                metric_row(
                    "Сейчас swap",
                    format!(
                        "{:.2} / {:.2} GiB",
                        machine.swap_used_gib, machine.swap_total_gib
                    ),
                    Some("Использование swap прямо сейчас."),
                ),
            ],
        ));
        cards.push(card_with_rows(
            "Основной диск",
            machine.disk_kind.clone(),
            format!(
                "Устройство: {}. Модель: {}.",
                machine.disk_device.as_deref().unwrap_or("ещё нет данных"),
                machine.disk_model
            ),
            "pass",
            None,
            Some("Статические характеристики основного диска плюс живая температура и текущая нагрузка ввода-вывода.".to_string()),
            vec![
                metric_row(
                    "Сейчас объём",
                    format!("{:.2} GiB", machine.disk_total_gib),
                    Some("Полный размер основного диска."),
                ),
                metric_row(
                    "Сейчас свободно",
                    format!("{:.2} GiB", machine.disk_available_gib),
                    Some("Сколько свободного места осталось на основном диске."),
                ),
                metric_row(
                    "Сейчас usage",
                    format_optional(machine.disk_used_percent, |value| format!("{value:.1}%")),
                    Some("Текущая доля занятого пространства на основном диске."),
                ),
                metric_row(
                    "Сейчас нагрузка",
                    format_optional(machine.disk_busy_percent, |value| format!("{value:.1}%")),
                    Some("Насколько диск был занят операциями ввода-вывода между двумя последними refresh панели."),
                ),
                metric_row(
                    "Сейчас чтение",
                    format_optional(machine.disk_read_mib_per_sec, |value| format!("{value:.2} MiB/s")),
                    Some("Текущая скорость чтения между двумя последними refresh панели."),
                ),
                metric_row(
                    "Сейчас запись",
                    format_optional(machine.disk_write_mib_per_sec, |value| format!("{value:.2} MiB/s")),
                    Some("Текущая скорость записи между двумя последними refresh панели."),
                ),
                metric_row(
                    "Сейчас температура",
                    format_optional(machine.disk_temperature_celsius, format_celsius),
                    Some("Температура NVMe/SSD по живому датчику."),
                ),
                metric_row(
                    "Сейчас firmware",
                    machine.disk_firmware.clone(),
                    Some("Версия прошивки основного диска."),
                ),
            ],
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
        card_with_rows(
            "PostgreSQL",
            format_ms(snapshot["postgres"]["query_probe_p95_ms"].as_f64()),
            "Живой probe базы метаданных, policy, проектов и continuity-снимков.".to_string(),
            combine_statuses(&[
                status_for_metric_name(snapshot, "postgres.query_probe_p95_ms"),
                status_for_metric_name(snapshot, "postgres.connection_usage_ratio"),
                status_for_metric_name(snapshot, "postgres.replica_lag_seconds"),
                status_for_metric_name(snapshot, "postgres.deadlocks_total"),
            ]),
            Some("Источник: живой PostgreSQL probe, обновляется на каждом refresh dashboard".to_string()),
            Some("PostgreSQL probe — это короткий живой замер базы метаданных, а не исторический benchmark.".to_string()),
            vec![
                metric_row(
                    "Эталон probe P95",
                    format_ms(snapshot["thresholds"]["postgres"]["query_probe_p95_ms"]["target"].as_f64()),
                    Some("Целевой p95 для короткого живого PostgreSQL probe."),
                ),
                metric_row(
                    "Сейчас probe P95",
                    format_ms(snapshot["postgres"]["query_probe_p95_ms"].as_f64()),
                    Some("Фактический p95 живого PostgreSQL probe на этом refresh."),
                ),
                metric_row(
                    "Эталон usage",
                    format_ratio_percent(snapshot["thresholds"]["postgres"]["connection_usage_ratio"]["target"].as_f64()),
                    Some("Желаемая доля занятых соединений PostgreSQL."),
                ),
                metric_row(
                    "Сейчас usage",
                    format_ratio_percent(snapshot["postgres"]["connection_usage_ratio"].as_f64()),
                    Some("Фактическая доля занятых соединений прямо сейчас."),
                ),
                metric_row(
                    "Сейчас TPS",
                    format_optional(snapshot["postgres"]["transactions_per_sec"].as_f64(), |v| format!("{v:.2}")),
                    Some("Сколько транзакций в секунду база делает между snapshot-ами."),
                ),
                metric_row(
                    "Сейчас WAL throughput",
                    format_optional(snapshot["postgres"]["wal_bytes_per_sec"].as_f64(), human_bytes_per_sec),
                    Some("Скорость записи журнала WAL между snapshot-ами."),
                ),
            ],
        ),
        card_with_rows(
            "Qdrant Amai live",
            format_optional(snapshot["qdrant"]["memory_resident_bytes"].as_f64(), human_bytes),
            "Живые системные показатели векторного слоя. Здесь показаны только действительно живые системные числа, а не исторический search-benchmark.".to_string(),
            combine_statuses(&[
                status_for_metric_name(snapshot, "qdrant.index_optimize_queue"),
                status_for_metric_name(snapshot, "qdrant.update_queue_length"),
            ]),
            Some("Источник: live Qdrant /metrics Amai, обновляется на каждом refresh dashboard".to_string()),
            Some("Qdrant — векторный слой. Он помогает recall, но не является source of truth для continuity или кода.".to_string()),
            vec![
                metric_row(
                    "Эталон optimize queue",
                    format_f64_count(snapshot["thresholds"]["qdrant"]["optimize_queue"]["target"].as_f64()),
                    Some("Целевой максимум очереди оптимизации индекса."),
                ),
                metric_row(
                    "Сейчас optimize queue",
                    format_f64_count(snapshot["qdrant"]["index_optimize_queue"].as_f64()),
                    Some("Текущая очередь оптимизации индекса Qdrant."),
                ),
                metric_row(
                    "Эталон update queue",
                    format_f64_count(snapshot["thresholds"]["qdrant"]["update_queue_length"]["target"].as_f64()),
                    Some("Желаемая длина очереди обновлений Qdrant."),
                ),
                metric_row(
                    "Сейчас update queue",
                    format_f64_count(snapshot["qdrant"]["update_queue_length"].as_f64()),
                    Some("Текущая длина очереди обновлений Qdrant."),
                ),
                metric_row(
                    "Сейчас resident memory",
                    format_optional(snapshot["qdrant"]["memory_resident_bytes"].as_f64(), human_bytes),
                    Some("Объём памяти, который Qdrant держит в resident state прямо сейчас."),
                ),
                metric_row(
                    "Сейчас points",
                    format_f64_count(snapshot["qdrant"]["points_count"].as_f64()),
                    Some("Сколько точек сейчас лежит в активной кодовой коллекции Qdrant."),
                ),
                metric_row(
                    "Сейчас segments",
                    format_f64_count(snapshot["qdrant"]["segments_count"].as_f64()),
                    Some("Сколько сегментов сейчас держит Qdrant. Много мелких сегментов может быть признаком будущей оптимизации."),
                ),
            ],
        ),
        benchmark_qdrant_live_card(snapshot),
        card_with_rows(
            "NATS / JetStream",
            format_ms(snapshot["nats"]["publish_probe_p95_ms"].as_f64()),
            "Живой probe очереди событий и фонового work plane.".to_string(),
            combine_statuses(&[
                status_for_metric_name(snapshot, "nats.publish_probe_p95_ms"),
                status_for_metric_name(snapshot, "nats.consumer_lag_msgs"),
                status_for_metric_name(snapshot, "nats.jetstream_disk_usage_ratio"),
            ]),
            Some("Источник: живой NATS/JetStream probe, обновляется на каждом refresh dashboard".to_string()),
            Some("NATS / JetStream — event и work plane для фоновых событий и очередей.".to_string()),
            vec![
                metric_row(
                    "Эталон publish P95",
                    format_ms(snapshot["thresholds"]["nats"]["publish_probe_p95_ms"]["target"].as_f64()),
                    Some("Целевой p95 для живого publish probe."),
                ),
                metric_row(
                    "Сейчас publish P95",
                    format_ms(snapshot["nats"]["publish_probe_p95_ms"].as_f64()),
                    Some("Фактический p95 для живого publish probe на этом refresh."),
                ),
                metric_row(
                    "Эталон lag",
                    format_f64_count(snapshot["thresholds"]["nats"]["consumer_lag_msgs"]["target"].as_f64()),
                    Some("Желаемый максимум непрочитанных сообщений."),
                ),
                metric_row(
                    "Сейчас lag",
                    format_f64_count(snapshot["nats"]["consumer_lag_msgs"].as_f64()),
                    Some("Текущая consumer lag в JetStream."),
                ),
                metric_row(
                    "Эталон disk usage",
                    format_ratio_percent(snapshot["thresholds"]["nats"]["jetstream_disk_usage_ratio"]["target"].as_f64()),
                    Some("Желаемая доля занятого диска JetStream."),
                ),
                metric_row(
                    "Сейчас disk usage",
                    format_ratio_percent(snapshot["nats"]["jetstream_disk_usage_ratio"].as_f64()),
                    Some("Текущая доля занятого диска JetStream."),
                ),
            ],
        ),
    ]
}

fn benchmark_qdrant_live_card(snapshot: &Value) -> Value {
    let benchmark = &snapshot["benchmark_qdrant"];
    let configured = benchmark["configured"].as_bool().unwrap_or(false);
    let available = benchmark["available"].as_bool().unwrap_or(false);
    let active = benchmark["active"].as_bool().unwrap_or(false);
    let from_last_success = benchmark["from_last_success"].as_bool().unwrap_or(false);
    let status = if !configured {
        "unknown"
    } else if !active || !available {
        "alert"
    } else {
        combine_statuses(&[
            status_at_most_or_equal(
                benchmark["index_optimize_queue"].as_f64(),
                snapshot["thresholds"]["qdrant"]["optimize_queue"]["target"].as_f64(),
            ),
            status_at_most_or_equal(
                benchmark["update_queue_length"].as_f64(),
                snapshot["thresholds"]["qdrant"]["update_queue_length"]["target"].as_f64(),
            ),
        ])
    };
    let value = if available || from_last_success {
        format_optional(benchmark["memory_resident_bytes"].as_f64(), human_bytes)
    } else if configured {
        "ещё нет данных".to_string()
    } else {
        "не настроено".to_string()
    };
    let note = if active && available {
        "Живые системные показатели отдельного Qdrant, который сейчас занят внешним benchmark-прогоном. Эти числа не смешиваются с Amai live.".to_string()
    } else if !active && available {
        "Тест сейчас не запущен. Показаны последние доступные системные показатели отдельного benchmark-Qdrant, чтобы вы не теряли картину после остановки прогона.".to_string()
    } else if from_last_success {
        "Показаны последние сохранённые результаты внешнего benchmark-Qdrant. Новый тест сейчас не запущен, но последние успешные числа сохранены для сравнения.".to_string()
    } else if configured {
        "Отдельный benchmark-Qdrant настроен, но тест сейчас не запущен. Значения появятся после первого успешного прогона.".to_string()
    } else {
        "Отдельный benchmark-Qdrant ещё не настроен. Когда внешний прогон будет идти через выделенный инстанс, здесь появятся его живые системные числа.".to_string()
    };
    let source_label = if active && available {
        Some(format!(
            "Источник: live Qdrant /metrics внешнего бенча ({}), обновляется на каждом refresh dashboard",
            benchmark["http_url"].as_str().unwrap_or("unknown")
        ))
    } else if !active && available {
        Some(format!(
            "Источник: последние доступные live Qdrant /metrics внешнего бенча ({}). Тест сейчас не запущен.",
            benchmark["http_url"].as_str().unwrap_or("unknown")
        ))
    } else if from_last_success {
        Some(format!(
            "Источник: последние сохранённые live Qdrant /metrics внешнего бенча ({}). Тест сейчас не запущен.",
            benchmark["http_url"].as_str().unwrap_or("unknown")
        ))
    } else {
        Some(
            "Источник: отдельный benchmark-Qdrant. Эта карточка никогда не берёт данные из Amai live."
                .to_string(),
        )
    };
    let rows = vec![
        metric_row(
            "Эталон optimize queue",
            format_f64_count(snapshot["thresholds"]["qdrant"]["optimize_queue"]["target"].as_f64()),
            Some("Целевой максимум очереди оптимизации индекса для внешнего benchmark-Qdrant."),
        ),
        metric_row(
            "Сейчас optimize queue",
            format_f64_count(benchmark["index_optimize_queue"].as_f64()),
            Some("Текущая очередь оптимизации индекса у внешнего benchmark-Qdrant."),
        ),
        metric_row(
            "Эталон update queue",
            format_f64_count(
                snapshot["thresholds"]["qdrant"]["update_queue_length"]["target"].as_f64(),
            ),
            Some("Желаемая длина очереди обновлений у внешнего benchmark-Qdrant."),
        ),
        metric_row(
            "Сейчас update queue",
            format_f64_count(benchmark["update_queue_length"].as_f64()),
            Some("Текущая длина очереди обновлений у внешнего benchmark-Qdrant."),
        ),
        metric_row(
            "Сейчас resident memory",
            format_optional(benchmark["memory_resident_bytes"].as_f64(), human_bytes),
            Some("Объём памяти, который отдельный benchmark-Qdrant держит прямо сейчас."),
        ),
        metric_row(
            "Сейчас points",
            format_f64_count(benchmark["points_count"].as_f64()),
            Some("Сколько точек сейчас загружено во внешний benchmark-Qdrant."),
        ),
        metric_row(
            "Сейчас segments",
            format_f64_count(benchmark["segments_count"].as_f64()),
            Some("Сколько сегментов сейчас держит внешний benchmark-Qdrant."),
        ),
    ];
    let status_label_override = if configured && !active {
        Some("тест не запущен".to_string())
    } else {
        None
    };
    json!({
        "title": "Qdrant внешнего бенча",
        "value": value,
        "note": note,
        "status": status,
        "status_label": status_label_override.unwrap_or_else(|| status_label(status).to_string()),
        "source_label": source_label,
        "title_tooltip": Some("Это отдельный инстанс Qdrant для внешнего benchmark-прогона. Он не должен смешиваться с основным Qdrant Amai.".to_string()),
        "rows": rows,
    })
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
            "term": "P50 / P95 / P99 / Max",
            "meaning": "P50 — середина выборки. P95 — почти все запросы, кроме тяжёлого хвоста. P99 — ещё более строгий хвост. Max — самый тяжёлый одиночный выброс."
        }),
        json!({
            "term": "QPS",
            "meaning": "Сколько запросов в секунду выдержала система в отдельном нагрузочном прогоне. Это не live поток страницы, а результат последнего benchmark."
        }),
        json!({
            "term": "Recall",
            "meaning": "Насколько полно система нашла всё нужное. Если recall низкий, часть правильного ответа просто не была найдена."
        }),
        json!({
            "term": "Precision",
            "meaning": "Насколько чисто система попала в нужный контекст. Если precision низкий, система тянет лишнее и шумное."
        }),
        json!({
            "term": "Hit rate",
            "meaning": "Доля запросов, где Amai реально попал в нужную цель: файл, символ, документ или нужный фрагмент контекста."
        }),
        json!({
            "term": "Fallback rate",
            "meaning": "Как часто системе пришлось отходить на запасной путь, потому что основной retrieval или ranking не справился сам."
        }),
        json!({
            "term": "Cross-project leakage",
            "meaning": "Случай, когда контекст одного проекта просочился в другой. Для строгого режима это должно быть только 0."
        }),
        json!({
            "term": "Live probe",
            "meaning": "Короткий живой системный замер, который пересчитывается прямо при refresh панели. Это не исторический snapshot и не benchmark."
        }),
        json!({
            "term": "Cold contour",
            "meaning": "Честный end-to-end cold benchmark: policy, retrieval, ranking, pack assembly, provenance и orchestration вместе, а не по отдельности."
        }),
        json!({
            "term": "Resident memory",
            "meaning": "Объём памяти, который сервис реально держит в RAM прямо сейчас, а не просто зарезервировал теоретически."
        }),
        json!({
            "term": "Semantic search",
            "meaning": "Поиск по смысловой близости, а не по точному совпадению слов. Полезен для recall, но не заменяет lexical/source-of-truth слой."
        }),
        json!({
            "term": "Token savings",
            "meaning": "Сколько токенов Amai сэкономил по сравнению с реалистичным baseline-путём без потери качества."
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
    cpu_usage_percent: Option<f64>,
    cpu_temperature_celsius: Option<f64>,
    cpu_max_mhz: Option<f64>,
    total_memory_gib: f64,
    available_memory_gib: f64,
    used_memory_gib: f64,
    memory_used_percent: Option<f64>,
    memory_type: String,
    memory_speed_label: String,
    swap_total_gib: f64,
    swap_used_gib: f64,
    disk_device: Option<String>,
    disk_model: String,
    disk_kind: String,
    disk_total_gib: f64,
    disk_available_gib: f64,
    disk_used_percent: Option<f64>,
    disk_busy_percent: Option<f64>,
    disk_read_mib_per_sec: Option<f64>,
    disk_write_mib_per_sec: Option<f64>,
    disk_temperature_celsius: Option<f64>,
    disk_firmware: String,
}

#[derive(Debug, Clone)]
struct CpuLoadSample {
    total: u64,
    idle: u64,
}

#[derive(Debug, Clone)]
struct DiskIoSample {
    device: String,
    read_sectors: u64,
    write_sectors: u64,
    io_millis: u64,
    captured_at_ms: u64,
}

#[derive(Debug, Clone)]
struct DiskLiveStats {
    busy_percent: Option<f64>,
    read_mib_per_sec: Option<f64>,
    write_mib_per_sec: Option<f64>,
}

static CPU_LOAD_CACHE: OnceLock<Mutex<Option<CpuLoadSample>>> = OnceLock::new();
static DISK_IO_CACHE: OnceLock<Mutex<Option<DiskIoSample>>> = OnceLock::new();
static CPU_MAX_MHZ_CACHE: OnceLock<Option<f64>> = OnceLock::new();

fn collect_machine_summary(repo_root: &Path) -> Result<MachineSummary> {
    let mut system = System::new_all();
    system.refresh_memory();
    let cpu_model = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "модель CPU не определена".to_string());
    let cpu_max_mhz = read_lscpu_numeric_field("CPU max MHz:");
    let disks = Disks::new_with_refreshed_list();
    let total_memory_gib = bytes_to_gib(system.total_memory());
    let available_memory_gib = bytes_to_gib(system.available_memory());
    let used_memory_gib = (total_memory_gib - available_memory_gib).max(0.0);
    let memory_used_percent = percentage_from_parts(used_memory_gib, total_memory_gib);
    let swap_total_gib = bytes_to_gib(system.total_swap());
    let swap_used_gib = bytes_to_gib(system.used_swap());
    let (memory_type, memory_speed_label) = detect_memory_characteristics();
    let disk_device = detect_disk_device_for_path(repo_root);
    let disk_space = disk_space_for_path(&disks, repo_root);
    let disk_total_gib = disk_space
        .map(|(total, _)| bytes_to_gib(total))
        .unwrap_or_default();
    let disk_available_gib = disk_space
        .map(|(_, available)| bytes_to_gib(available))
        .unwrap_or_default();
    let disk_used_percent = percentage_from_parts(
        (disk_total_gib - disk_available_gib).max(0.0),
        disk_total_gib,
    );
    let disk_model = disk_device
        .as_deref()
        .and_then(read_disk_model)
        .unwrap_or_else(|| "модель диска не определена".to_string());
    let disk_kind = disk_device
        .as_deref()
        .map(detect_disk_kind)
        .unwrap_or_else(|| "тип диска не определён".to_string());
    let disk_live = disk_device.as_deref().and_then(read_disk_live_stats);
    let disk_firmware = disk_device
        .as_deref()
        .and_then(read_disk_firmware)
        .unwrap_or_else(|| "ещё нет данных".to_string());
    let disk_temperature_celsius = disk_device.as_deref().and_then(read_disk_temperature);
    Ok(MachineSummary {
        cpu_model,
        logical_cpus: system.cpus().len(),
        cpu_usage_percent: read_cpu_usage_percent(),
        cpu_temperature_celsius: read_hwmon_temperature("k10temp", "Tctl"),
        cpu_max_mhz,
        total_memory_gib,
        available_memory_gib,
        used_memory_gib,
        memory_used_percent,
        memory_type,
        memory_speed_label,
        swap_total_gib,
        swap_used_gib,
        disk_device,
        disk_model,
        disk_kind,
        disk_total_gib,
        disk_available_gib,
        disk_used_percent,
        disk_busy_percent: disk_live.as_ref().and_then(|live| live.busy_percent),
        disk_read_mib_per_sec: disk_live.as_ref().and_then(|live| live.read_mib_per_sec),
        disk_write_mib_per_sec: disk_live.as_ref().and_then(|live| live.write_mib_per_sec),
        disk_temperature_celsius,
        disk_firmware,
    })
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn percentage_from_parts(value: f64, total: f64) -> Option<f64> {
    if total <= 0.0 {
        None
    } else {
        Some((value / total) * 100.0)
    }
}

fn read_cpu_usage_percent() -> Option<f64> {
    let (total, idle) = read_cpu_totals()?;
    let cache = CPU_LOAD_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().ok()?;
    let previous = guard.replace(CpuLoadSample { total, idle });
    let previous = previous?;
    let delta_total = total.saturating_sub(previous.total);
    if delta_total == 0 {
        return None;
    }
    let delta_idle = idle.saturating_sub(previous.idle);
    Some(((delta_total.saturating_sub(delta_idle)) as f64 / delta_total as f64) * 100.0)
}

fn read_cpu_totals() -> Option<(u64, u64)> {
    let line = fs::read_to_string("/proc/stat")
        .ok()?
        .lines()
        .next()?
        .to_string();
    let mut parts = line.split_whitespace();
    if parts.next()? != "cpu" {
        return None;
    }
    let values = parts
        .filter_map(|part| part.parse::<u64>().ok())
        .collect::<Vec<_>>();
    if values.len() < 5 {
        return None;
    }
    let total = values.iter().sum();
    let idle = values[3].saturating_add(values[4]);
    Some((total, idle))
}

fn read_lscpu_numeric_field(prefix: &str) -> Option<f64> {
    if prefix == "CPU max MHz:" {
        return CPU_MAX_MHZ_CACHE
            .get_or_init(|| read_lscpu_numeric_field_uncached(prefix))
            .to_owned();
    }
    read_lscpu_numeric_field_uncached(prefix)
}

fn read_lscpu_numeric_field_uncached(prefix: &str) -> Option<f64> {
    let output = Command::new("lscpu").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text
        .lines()
        .find(|line| line.trim_start().starts_with(prefix))?;
    let digits = line
        .split(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
        .find(|part| !part.is_empty())?;
    digits.parse::<f64>().ok()
}

fn read_hwmon_temperature(chip_name: &str, label: &str) -> Option<f64> {
    let hwmon_root = Path::new("/sys/class/hwmon");
    for entry in fs::read_dir(hwmon_root).ok()?.flatten() {
        let path = entry.path();
        let name = fs::read_to_string(path.join("name")).ok()?;
        if name.trim() != chip_name {
            continue;
        }
        for index in 1..=6 {
            let label_path = path.join(format!("temp{index}_label"));
            let input_path = path.join(format!("temp{index}_input"));
            if !input_path.is_file() {
                continue;
            }
            let current_label = fs::read_to_string(&label_path)
                .ok()
                .map(|text| text.trim().to_string())
                .unwrap_or_else(|| format!("temp{index}"));
            if current_label != label {
                continue;
            }
            let raw = fs::read_to_string(&input_path).ok()?;
            let milli_celsius = raw.trim().parse::<f64>().ok()?;
            return Some(milli_celsius / 1000.0);
        }
    }
    None
}

fn detect_memory_characteristics() -> (String, String) {
    for (program, args) in [
        ("sudo", vec!["-n", "dmidecode", "--type", "17"]),
        ("dmidecode", vec!["--type", "17"]),
        ("lshw", vec!["-class", "memory"]),
    ] {
        let output = Command::new(program).args(args).output().ok();
        let Some(output) = output else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let memory_type = extract_memory_generation(&text)
            .unwrap_or_else(|| "система не дала определить автоматически".to_string());
        let memory_speed = extract_memory_speed(&text)
            .map(|value| format!("{value} MT/s"))
            .unwrap_or_else(|| "не удалось определить автоматически".to_string());
        return (memory_type, memory_speed);
    }
    (
        "система не дала определить автоматически".to_string(),
        "не удалось определить автоматически".to_string(),
    )
}

fn extract_memory_speed(text: &str) -> Option<u64> {
    for line in text.lines() {
        let line = line.trim();
        if !(line.contains("Speed:")
            || line.contains("Configured Memory Speed:")
            || line.contains("clock:"))
        {
            continue;
        }
        let digits = line
            .split(|ch: char| !ch.is_ascii_digit())
            .find(|part| !part.is_empty())?;
        if let Ok(value) = digits.parse::<u64>() {
            return Some(value);
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

fn disk_space_for_path(disks: &Disks, path: &Path) -> Option<(u64, u64)> {
    let canonical = path.canonicalize().ok()?;
    disks
        .iter()
        .filter(|disk| canonical.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .map(|disk| (disk.total_space(), disk.available_space()))
}

fn detect_disk_device_for_path(path: &Path) -> Option<String> {
    let output = Command::new("df")
        .arg("--output=source")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let source = text
        .lines()
        .skip(1)
        .find(|line| !line.trim().is_empty())?
        .trim();
    normalize_block_device_name(source)
}

fn normalize_block_device_name(source: &str) -> Option<String> {
    let source = source.strip_prefix("/dev/").unwrap_or(source).trim();
    if source.is_empty() {
        return None;
    }
    if let Some((base, suffix)) = source.rsplit_once('p')
        && suffix.chars().all(|ch| ch.is_ascii_digit())
    {
        return Some(base.to_string());
    }
    let trimmed = source.trim_end_matches(|ch: char| ch.is_ascii_digit());
    if trimmed.is_empty() {
        Some(source.to_string())
    } else {
        Some(trimmed.to_string())
    }
}

fn read_disk_model(device: &str) -> Option<String> {
    fs::read_to_string(format!("/sys/class/block/{device}/device/model"))
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn read_disk_firmware(device: &str) -> Option<String> {
    fs::read_to_string(format!("/sys/class/block/{device}/device/firmware_rev"))
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn detect_disk_kind(device: &str) -> String {
    if device.starts_with("nvme") {
        return "NVMe SSD".to_string();
    }
    let rotational = fs::read_to_string(format!("/sys/class/block/{device}/queue/rotational"))
        .ok()
        .map(|text| text.trim() == "1");
    match rotational {
        Some(false) => "SSD".to_string(),
        Some(true) => "HDD".to_string(),
        None => "тип диска не определён".to_string(),
    }
}

fn read_disk_temperature(device: &str) -> Option<f64> {
    let device_model = read_disk_model(device);
    let device_serial = fs::read_to_string(format!("/sys/class/block/{device}/device/serial"))
        .ok()
        .map(|text| text.trim().to_string());
    for entry in fs::read_dir("/sys/class/hwmon").ok()?.flatten() {
        let path = entry.path();
        let name = fs::read_to_string(path.join("name")).ok()?;
        if name.trim() != "nvme" {
            continue;
        }
        let model_matches = device_model.as_deref().is_some_and(|expected| {
            fs::read_to_string(path.join("device/model"))
                .ok()
                .map(|actual| actual.trim() == expected)
                .unwrap_or(false)
        });
        let serial_matches = device_serial.as_deref().is_some_and(|expected| {
            fs::read_to_string(path.join("device/serial"))
                .ok()
                .map(|actual| actual.trim() == expected)
                .unwrap_or(false)
        });
        if !model_matches && !serial_matches {
            continue;
        }
        let raw = fs::read_to_string(path.join("temp1_input")).ok()?;
        let milli_celsius = raw.trim().parse::<f64>().ok()?;
        return Some(milli_celsius / 1000.0);
    }
    None
}

fn read_disk_live_stats(device: &str) -> Option<DiskLiveStats> {
    let (read_sectors, write_sectors, io_millis) = read_disk_counters(device)?;
    let captured_at_ms = now_epoch_ms();
    let cache = DISK_IO_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().ok()?;
    let previous = guard.replace(DiskIoSample {
        device: device.to_string(),
        read_sectors,
        write_sectors,
        io_millis,
        captured_at_ms,
    });
    let previous = previous?;
    if previous.device != device {
        return None;
    }
    let delta_ms = captured_at_ms.saturating_sub(previous.captured_at_ms);
    if delta_ms == 0 {
        return None;
    }
    let delta_seconds = delta_ms as f64 / 1000.0;
    let read_bytes = read_sectors
        .saturating_sub(previous.read_sectors)
        .saturating_mul(512);
    let write_bytes = write_sectors
        .saturating_sub(previous.write_sectors)
        .saturating_mul(512);
    let busy_percent =
        ((io_millis.saturating_sub(previous.io_millis)) as f64 / delta_ms as f64) * 100.0;
    Some(DiskLiveStats {
        busy_percent: Some(busy_percent.min(100.0)),
        read_mib_per_sec: Some(read_bytes as f64 / 1024.0 / 1024.0 / delta_seconds),
        write_mib_per_sec: Some(write_bytes as f64 / 1024.0 / 1024.0 / delta_seconds),
    })
}

fn read_disk_counters(device: &str) -> Option<(u64, u64, u64)> {
    let line = fs::read_to_string("/proc/diskstats")
        .ok()?
        .lines()
        .find(|line| line.split_whitespace().nth(2) == Some(device))?
        .to_string();
    let fields = line.split_whitespace().collect::<Vec<_>>();
    if fields.len() < 14 {
        return None;
    }
    let read_sectors = fields[5].parse::<u64>().ok()?;
    let write_sectors = fields[9].parse::<u64>().ok()?;
    let io_millis = fields[12].parse::<u64>().ok()?;
    Some((read_sectors, write_sectors, io_millis))
}

fn card(title: &str, value: String, note: String, status: &str) -> Value {
    card_with_rows(title, value, note, status, None, None, Vec::new())
}

fn live_latency_compare_card(snapshot: &Value) -> Value {
    let hot = latency_slice(snapshot, "hot");
    let cold = latency_slice(snapshot, "cold");
    let hot_sample_count = hot
        .and_then(|slice| slice["sample_count"].as_u64())
        .unwrap_or_default();
    let cold_sample_count = cold
        .and_then(|slice| slice["sample_count"].as_u64())
        .unwrap_or_default();
    let hot_has_data = hot_sample_count > 0;
    let cold_has_data = cold_sample_count > 0;
    let hot_thresholds = live_latency_thresholds(snapshot, "hot");
    let cold_thresholds = live_latency_thresholds(snapshot, "cold");
    let hot_status = if hot_has_data {
        status_from_threshold(
            hot.and_then(|slice| slice["p95_latency_ms"].as_f64()),
            hot_thresholds.0,
            hot_thresholds.1,
            hot_thresholds.2,
        )
    } else {
        "unknown"
    };
    let cold_status = if cold_has_data {
        status_from_threshold(
            cold.and_then(|slice| slice["p95_latency_ms"].as_f64()),
            cold_thresholds.0,
            cold_thresholds.1,
            cold_thresholds.2,
        )
    } else {
        "unknown"
    };
    let hot_targets = live_latency_table_targets(snapshot, "hot");
    let cold_targets = live_latency_table_targets(snapshot, "cold");

    json!({
        "kind": "live_compare",
        "title": "Как Amai отвечает сейчас",
        "title_tooltip": "Это живое сравнение двух пользовательских режимов: повторный запрос по уже прогретому кэшу и первый запрос без прогрева. Здесь нет benchmark-снимков — только текущая сессия.",
        "status": combine_statuses(&[hot_status, cold_status]),
        "status_label": status_label(combine_statuses(&[hot_status, cold_status])),
        "source_label": "Источник: живая выборка текущей сессии, обновляется при новых запросах. Benchmark-данные сюда не подмешиваются.",
        "note": "Сверху показана медиана, то есть обычный уровень ответа в каждом режиме. Ниже — одна общая таблица, чтобы сравнить повторный и первый запрос без дублирования отдельных карточек.",
        "metrics": [
            {
                "label": "Повторный запрос",
                "tooltip": "Это уже прогретый путь: пользователь повторяет похожий запрос, а Amai не стартует с пустого места.",
                "value": if hot_has_data {
                    format_ms(hot.and_then(|slice| slice["p50_latency_ms"].as_f64()))
                } else {
                    "ещё нет данных".to_string()
                },
                "note": if hot_has_data {
                    format!(
                        "Статус: {}. Живая выборка: {}.",
                        status_label(hot_status),
                        format_u64(Some(hot_sample_count))
                    )
                } else {
                    "В этой сессии ещё не накопилась живая hot-выборка.".to_string()
                }
            },
            {
                "label": "Первый запрос",
                "tooltip": "Это первый запрос без fast-cache и без прогрева. Он всегда тяжелее и лучше показывает реальную цену холодного старта.",
                "value": if cold_has_data {
                    format_ms(cold.and_then(|slice| slice["p50_latency_ms"].as_f64()))
                } else {
                    "ещё нет данных".to_string()
                },
                "note": if cold_has_data {
                    format!(
                        "Статус: {}. Живая выборка: {}.",
                        status_label(cold_status),
                        format_u64(Some(cold_sample_count))
                    )
                } else {
                    "В этой сессии ещё не накопилась живая cold-выборка.".to_string()
                }
            }
        ],
        "table": {
            "columns": [
                { "label": "Режим", "tooltip": "Какой путь сейчас сравниваем: прогретый повторный запрос или первый холодный запрос." },
                { "label": "P50", "tooltip": "Медиана. Это обычный уровень ответа, который пользователь видит чаще всего." },
                { "label": "P95", "tooltip": "Тяжёлый хвост. Почти все запросы должны укладываться в эту границу." },
                { "label": "P99", "tooltip": "Ещё более строгий хвост. Показывает редкие тяжёлые выбросы." },
                { "label": "Max", "tooltip": "Самый тяжёлый одиночный запрос в текущей живой выборке." },
                { "label": "Выборка", "tooltip": "Сколько живых запросов уже вошло в расчёт." }
            ],
            "rows": [
                {
                    "label": "Повторный запрос — эталон",
                    "tooltip": "Это фиксированные цели для прогретого повторного запроса. Они не зависят от текущей сессии и всегда должны быть заполнены.",
                    "values": target_values(&hot_targets)
                },
                {
                    "label": "Повторный запрос — сейчас",
                    "tooltip": "Текущая живая hot-выборка этой сессии.",
                    "values": compare_values(hot, hot_sample_count)
                },
                {
                    "label": "Первый запрос — эталон",
                    "tooltip": "Это фиксированные цели для первого запроса без прогрева. Они не зависят от текущей сессии и всегда должны быть заполнены.",
                    "values": target_values(&cold_targets)
                },
                {
                    "label": "Первый запрос — сейчас",
                    "tooltip": "Текущая живая cold-выборка этой сессии.",
                    "values": compare_values(cold, cold_sample_count)
                }
            ]
        }
    })
}

fn compare_table_card(
    title: &str,
    note: String,
    status: &str,
    source_label: Option<String>,
    title_tooltip: Option<String>,
    rows: Vec<Value>,
) -> Value {
    json!({
        "kind": "compare_table",
        "title": title,
        "note": note,
        "status": status,
        "status_label": status_label(status),
        "source_label": source_label,
        "title_tooltip": title_tooltip,
        "metrics": [],
        "table": {
            "columns": [
                { "label": "Метрика", "tooltip": "Что именно измерялось в этом проверочном прогоне." },
                { "label": "Эталон", "tooltip": "Фиксированная целевая планка. Она не зависит от текущей сессии и не меняется от запроса к запросу." },
                { "label": "Тестовые данные", "tooltip": "Фактический результат последнего сохранённого benchmark-прогона." }
            ],
            "rows": rows,
        }
    })
}

fn compare_table_row(label: &str, tooltip: &str, values: Vec<String>) -> Value {
    json!({
        "label": label,
        "tooltip": tooltip,
        "values": values,
    })
}

fn compare_pair(target: String, current: String) -> Vec<String> {
    vec![target, current]
}

fn card_with_rows(
    title: &str,
    value: String,
    note: String,
    status: &str,
    source_label: Option<String>,
    title_tooltip: Option<String>,
    rows: Vec<Value>,
) -> Value {
    json!({
        "title": title,
        "value": value,
        "note": note,
        "status": status,
        "status_label": status_label(status),
        "source_label": source_label,
        "title_tooltip": title_tooltip,
        "rows": rows,
    })
}

fn metric_row(label: &str, value: String, tooltip: Option<&str>) -> Value {
    json!({
        "label": label,
        "value": value,
        "tooltip": tooltip,
    })
}

fn hot_load_benchmark_status(hot_load: &Value, thresholds: &Value) -> &'static str {
    let qps_status = status_strict_more_than(
        hot_load["qps"].as_f64(),
        thresholds["load"]["hot_qps"]["target"].as_f64(),
    );
    let error_status = status_at_most_or_equal(
        hot_load["error_rate"].as_f64(),
        thresholds["load"]["hot_error_rate"]["target"].as_f64(),
    );
    let p50_status = status_strict_less_than(
        hot_load["p50_ms"].as_f64(),
        thresholds["load"]["hot_benchmark_table"]["target_p50_ms"].as_f64(),
    );
    let p95_status = status_strict_less_than(
        hot_load["p95_ms"].as_f64(),
        thresholds["load"]["hot_benchmark_table"]["target_p95_ms"].as_f64(),
    );
    let p99_status = status_strict_less_than(
        hot_load["p99_ms"].as_f64(),
        thresholds["load"]["hot_benchmark_table"]["target_p99_ms"].as_f64(),
    );
    let max_status = status_strict_less_than(
        hot_load["max_ms"].as_f64(),
        thresholds["load"]["hot_benchmark_table"]["target_max_ms"].as_f64(),
    );
    let workers_status = status_strict_more_than(
        hot_load["workers"].as_f64(),
        thresholds["load"]["hot_benchmark_table"]["target_workers"].as_f64(),
    );
    let sample_count = hot_load["success_count"]
        .as_u64()
        .zip(hot_load["error_count"].as_u64())
        .map(|(success, errors)| (success + errors) as f64);
    let sample_status = status_strict_more_than(
        sample_count,
        thresholds["load"]["hot_benchmark_table"]["target_sample_count"].as_f64(),
    );
    combine_statuses(&[
        qps_status,
        error_status,
        p50_status,
        p95_status,
        p99_status,
        max_status,
        workers_status,
        sample_status,
    ])
}

fn status_strict_less_than(current: Option<f64>, target: Option<f64>) -> &'static str {
    match (current, target) {
        (Some(current), Some(target)) if current < target => "pass",
        (Some(_), Some(_)) => "critical",
        _ => "unknown",
    }
}

fn status_strict_more_than(current: Option<f64>, target: Option<f64>) -> &'static str {
    match (current, target) {
        (Some(current), Some(target)) if current > target => "pass",
        (Some(_), Some(_)) => "critical",
        _ => "unknown",
    }
}

fn status_at_most_or_equal(current: Option<f64>, target: Option<f64>) -> &'static str {
    match (current, target) {
        (Some(current), Some(target)) if current <= target => "pass",
        (Some(_), Some(_)) => "critical",
        _ => "unknown",
    }
}

fn compare_values(slice: Option<&Value>, sample_count: u64) -> Vec<String> {
    if sample_count == 0 {
        return vec![
            "ещё нет данных".to_string(),
            "ещё нет данных".to_string(),
            "ещё нет данных".to_string(),
            "ещё нет данных".to_string(),
            "0".to_string(),
        ];
    }
    vec![
        format_ms(slice.and_then(|value| value["p50_latency_ms"].as_f64())),
        format_ms(slice.and_then(|value| value["p95_latency_ms"].as_f64())),
        format_ms(slice.and_then(|value| value["p99_latency_ms"].as_f64())),
        format_ms(slice.and_then(|value| value["max_latency_ms"].as_f64())),
        format_u64(Some(sample_count)),
    ]
}

#[derive(Debug, Clone, Copy)]
struct LiveLatencyTableTargets {
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
    sample_count: u64,
}

fn live_latency_table_targets(snapshot: &Value, state: &str) -> LiveLatencyTableTargets {
    let thresholds = if state == "hot" {
        &snapshot["thresholds"]["retrieval"]["hot_live_table"]
    } else {
        &snapshot["thresholds"]["retrieval"]["cold_live_table"]
    };
    LiveLatencyTableTargets {
        p50_ms: thresholds["target_p50_ms"].as_f64().unwrap_or(0.0),
        p95_ms: thresholds["target_p95_ms"].as_f64().unwrap_or(0.0),
        p99_ms: thresholds["target_p99_ms"].as_f64().unwrap_or(0.0),
        max_ms: thresholds["target_max_ms"].as_f64().unwrap_or(0.0),
        sample_count: thresholds["target_sample_count"].as_u64().unwrap_or(0),
    }
}

fn target_values(targets: &LiveLatencyTableTargets) -> Vec<String> {
    vec![
        format_target_ms("<", targets.p50_ms),
        format_target_ms("<", targets.p95_ms),
        format_target_ms("<", targets.p99_ms),
        format_target_ms("<", targets.max_ms),
        format_target_u64(">", targets.sample_count),
    ]
}

fn status_label(status: &str) -> &'static str {
    match status {
        "pass" => "в норме",
        "alert" => "внимание",
        "critical" => "критично",
        _ => "нет данных",
    }
}

fn live_latency_thresholds(snapshot: &Value, state: &str) -> (f64, f64, f64) {
    let key = if state == "hot" {
        "hot_live_p95_ms"
    } else {
        "cold_live_p95_ms"
    };
    let thresholds = &snapshot["thresholds"]["retrieval"][key];
    (
        thresholds["target"].as_f64().unwrap_or(0.0),
        thresholds["alert"].as_f64().unwrap_or(f64::INFINITY),
        thresholds["critical"].as_f64().unwrap_or(f64::INFINITY),
    )
}

fn status_from_threshold(
    value: Option<f64>,
    target: f64,
    alert: f64,
    _critical: f64,
) -> &'static str {
    let Some(value) = value else {
        return "unknown";
    };
    if value <= target {
        "pass"
    } else if value <= alert {
        "alert"
    } else {
        "critical"
    }
}

fn cold_contour_status(snapshot: &Value) -> &'static str {
    match snapshot["latest_cold_path_benchmark"]["cold_benchmark"]["executive_summary"]["verdict"]
        .as_str()
    {
        Some("TARGET MET") => "pass",
        Some("PARTIALLY MET") => "alert",
        Some("NOT MET") => "critical",
        _ => "unknown",
    }
}

fn latency_slice<'a>(snapshot: &'a Value, state: &str) -> Option<&'a Value> {
    snapshot["token_budget_report"]["token_budget_report"]["current_session"]["latency_slices"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|slice| slice["state"].as_str() == Some(state))
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

fn status_for_metric_name(snapshot: &Value, metric_name: &str) -> &'static str {
    snapshot["sla"]["checks"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|check| check["metric"].as_str() == Some(metric_name))
        .and_then(|check| check["status"].as_str())
        .and_then(normalize_status)
        .unwrap_or("unknown")
}

fn combine_statuses(statuses: &[&str]) -> &'static str {
    statuses
        .iter()
        .copied()
        .filter_map(normalize_status)
        .reduce(worst_status)
        .unwrap_or("unknown")
}

fn normalize_status(status: &str) -> Option<&'static str> {
    match status {
        "pass" => Some("pass"),
        "alert" => Some("alert"),
        "critical" => Some("critical"),
        "unknown" => Some("unknown"),
        _ => None,
    }
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
        Some(number) if metric.ends_with("_ms") => format!("{number:.3} ms"),
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
        "load.hot_p50_ms" => "Обычная hot-задержка в benchmark-прогоне стала выше целевой планки.",
        "load.hot_p95_ms" => "Тяжёлый хвост hot benchmark стал выше обещанной границы.",
        "load.hot_p99_ms" => "Редкие тяжёлые выбросы в hot benchmark стали слишком большими.",
        "load.hot_max_ms" => "Самый тяжёлый запрос в hot benchmark вышел за безопасную границу.",
        "load.hot_error_rate" => "Под нагрузкой появились ошибки на быстром пути.",
        "load.hot_workers" => "Последний hot benchmark был прогнан слишком слабой параллельностью.",
        "load.hot_sample_count" => {
            "Последний hot benchmark собран на слишком маленькой выборке, чтобы ему доверять."
        }
        _ => "Один из обязательных проверочных контуров вышел из своей нормы.",
    };
    format!("{explanation} Сейчас: {value}. Состояние: {status}.")
}

fn human_timestamp(epoch_ms: u64) -> String {
    if epoch_ms == 0 {
        return "ещё нет данных".to_string();
    }
    let nanos = (epoch_ms as i128) * 1_000_000;
    let Ok(offset) = UtcOffset::from_hms(3, 0, 0) else {
        return "ещё нет данных".to_string();
    };
    let Ok(datetime) = OffsetDateTime::from_unix_timestamp_nanos(nanos) else {
        return "ещё нет данных".to_string();
    };
    let format = format_description!("[year]-[month]-[day] [hour]:[minute]:[second] MSK");
    datetime
        .to_offset(offset)
        .format(&format)
        .unwrap_or_else(|_| "ещё нет данных".to_string())
}

fn human_epoch_seconds(epoch_seconds: u64) -> String {
    if epoch_seconds == 0 {
        return "ещё нет данных".to_string();
    }
    human_timestamp(epoch_seconds.saturating_mul(1000))
}

fn source_label(prefix: &str, epoch_ms: Option<u64>) -> String {
    match epoch_ms.filter(|value| *value > 0) {
        Some(epoch_ms) => format!("{prefix}. Измерено: {}.", human_timestamp(epoch_ms)),
        None => prefix.to_string(),
    }
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

fn format_threshold_at_most(value: Option<f64>, unit: &str, decimals: usize) -> String {
    format_threshold_value(value, "<", unit, decimals)
}

fn format_threshold_at_least(value: Option<f64>, unit: &str, decimals: usize) -> String {
    format_threshold_value(value, ">", unit, decimals)
}

fn format_threshold_value(
    value: Option<f64>,
    operator: &str,
    unit: &str,
    decimals: usize,
) -> String {
    match value {
        Some(number) if unit.is_empty() => {
            format!("{operator} {}", format_decimal(number, decimals))
        }
        Some(number) if unit == "%" => {
            format!("{operator} {}%", format_decimal(number, decimals))
        }
        Some(number) => format!("{operator} {} {unit}", format_decimal(number, decimals)),
        None => "ещё нет данных".to_string(),
    }
}

fn format_decimal(value: f64, decimals: usize) -> String {
    format!("{value:.prec$}", prec = decimals)
}

fn format_u64(value: Option<u64>) -> String {
    value
        .map(|number| number.to_string())
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

fn format_target_ms(operator: &str, value: f64) -> String {
    format!("{operator} {value:.3} ms")
}

fn format_target_u64(operator: &str, value: u64) -> String {
    format!("{operator} {value}")
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

fn format_celsius(value: f64) -> String {
    format!("{value:.1}°C")
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
    use super::{
        benchmark_qdrant_live_card, browser_base_url, human_elapsed_ms, monitoring_url,
        worst_status,
    };
    use serde_json::json;

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

    #[test]
    fn benchmark_qdrant_card_uses_last_success_snapshot_without_error_rows() {
        let snapshot = json!({
            "thresholds": {
                "qdrant": {
                    "optimize_queue": { "target": 10.0 },
                    "update_queue_length": { "target": 0.0 }
                }
            },
            "benchmark_qdrant": {
                "configured": true,
                "available": false,
                "active": false,
                "from_last_success": true,
                "http_url": "http://127.0.0.1:7633",
                "memory_resident_bytes": 422123456.0,
                "points_count": 70200.0,
                "segments_count": 8.0,
                "index_optimize_queue": 0.0,
                "update_queue_length": 0.0
            }
        });
        let card = benchmark_qdrant_live_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("alert"));
        assert_eq!(card["status_label"].as_str(), Some("тест не запущен"));
        assert_eq!(card["value"].as_str(), Some("402.57 MiB"));
        assert!(
            card["note"]
                .as_str()
                .unwrap_or_default()
                .contains("последние сохранённые результаты")
        );
        let empty_rows = Vec::new();
        let labels = card["rows"]
            .as_array()
            .unwrap_or(&empty_rows)
            .iter()
            .filter_map(|row| row["label"].as_str())
            .collect::<Vec<_>>();
        assert!(!labels.contains(&"Что это значит"));
        assert!(!labels.contains(&"Техническая причина"));
    }

    #[test]
    fn benchmark_qdrant_card_without_cache_shows_test_not_running_without_error_rows() {
        let snapshot = json!({
            "thresholds": {
                "qdrant": {
                    "optimize_queue": { "target": 10.0 },
                    "update_queue_length": { "target": 0.0 }
                }
            },
            "benchmark_qdrant": {
                "configured": true,
                "available": false,
                "active": false,
                "from_last_success": false,
                "http_url": "http://127.0.0.1:7633",
                "index_optimize_queue": null,
                "update_queue_length": null,
                "memory_resident_bytes": null,
                "points_count": null,
                "segments_count": null
            }
        });
        let card = benchmark_qdrant_live_card(&snapshot);
        assert_eq!(card["status_label"].as_str(), Some("тест не запущен"));
        assert_eq!(card["value"].as_str(), Some("ещё нет данных"));
        let empty_rows = Vec::new();
        let labels = card["rows"]
            .as_array()
            .unwrap_or(&empty_rows)
            .iter()
            .filter_map(|row| row["label"].as_str())
            .collect::<Vec<_>>();
        assert!(!labels.contains(&"Что это значит"));
        assert!(!labels.contains(&"Техническая причина"));
    }

    #[test]
    fn benchmark_qdrant_card_marks_stopped_test_even_if_metrics_are_still_available() {
        let snapshot = json!({
            "thresholds": {
                "qdrant": {
                    "optimize_queue": { "target": 10.0 },
                    "update_queue_length": { "target": 0.0 }
                }
            },
            "benchmark_qdrant": {
                "configured": true,
                "available": true,
                "active": false,
                "from_last_success": false,
                "http_url": "http://127.0.0.1:7633",
                "memory_resident_bytes": 219709440.0,
                "points_count": 218800.0,
                "segments_count": 8.0,
                "index_optimize_queue": 0.0,
                "update_queue_length": 0.0
            }
        });
        let card = benchmark_qdrant_live_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("alert"));
        assert_eq!(card["status_label"].as_str(), Some("тест не запущен"));
        assert_eq!(card["value"].as_str(), Some("209.53 MiB"));
        assert!(
            card["note"]
                .as_str()
                .unwrap_or_default()
                .contains("Тест сейчас не запущен")
        );
    }
}
