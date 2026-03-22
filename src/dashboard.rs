use crate::config::{self, AppConfig};
use crate::hardware_telemetry::{AcceleratorSummary, MachineSummary, collect_machine_summary};
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;
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
      --panel-outer-shadow:
        0 0 2px rgba(17, 28, 33, 0.30),
        0 0 12px rgba(17, 28, 33, 0.16),
        0 0 28px rgba(17, 28, 33, 0.10),
        0 22px 44px -30px rgba(17, 28, 33, 0.18);
      --border: rgba(30, 42, 47, 0.10);
      --surface: rgba(255, 255, 255, 0.72);
      --surface-raised: rgba(255, 255, 255, 0.78);
      --surface-solid: rgba(255, 255, 255, 0.82);
      --surface-border: rgba(30, 42, 47, 0.08);
      --table-scroll-track: rgba(23, 34, 39, 0.86);
      --table-scroll-thumb: rgba(24, 108, 98, 0.82);
      --table-scroll-thumb-strong: rgba(39, 146, 132, 0.92);
      --hero-glow: rgba(13, 107, 111, 0.11);
      --error-border: rgba(182, 56, 43, 0.18);
      --card-outer-shadow:
        0 0 1px rgba(18, 28, 33, 0.24),
        0 0 8px rgba(18, 28, 33, 0.12),
        0 0 18px rgba(18, 28, 33, 0.08),
        0 16px 28px -24px rgba(18, 28, 33, 0.16);
      --card-inner-shadow:
        inset 0 0 0 1px rgba(8, 70, 61, 0.52),
        inset 0 1px 0 rgba(255, 255, 255, 0.028),
        inset 0 12px 16px -14px rgba(8, 63, 55, 0.20),
        inset 0 -12px 16px -14px rgba(7, 52, 46, 0.16),
        inset 12px 0 16px -14px rgba(8, 58, 51, 0.16),
        inset -12px 0 16px -14px rgba(8, 58, 51, 0.16);
    }

    * { box-sizing: border-box; }
    html {
      scrollbar-gutter: stable both-edges;
      scrollbar-width: thin;
      scrollbar-color: transparent transparent;
    }

    html::-webkit-scrollbar {
      width: 12px;
      height: 12px;
    }

    html::-webkit-scrollbar-track {
      background: transparent;
      border-radius: 999px;
    }

    html::-webkit-scrollbar-thumb {
      background: transparent;
      border-radius: 999px;
      border: 2px solid transparent;
      transition: background 0.14s ease, border-width 0.14s ease, box-shadow 0.14s ease;
    }

    html:hover,
    html:focus-within {
      scrollbar-color: var(--table-scroll-thumb) var(--table-scroll-track);
    }

    html:hover::-webkit-scrollbar-track,
    html:focus-within::-webkit-scrollbar-track {
      background: var(--table-scroll-track);
    }

    html:hover::-webkit-scrollbar-thumb,
    html:focus-within::-webkit-scrollbar-thumb {
      background: linear-gradient(180deg, var(--table-scroll-thumb), var(--table-scroll-thumb-strong));
      border: 2px solid var(--table-scroll-track);
    }

    html:hover::-webkit-scrollbar-thumb:hover,
    html:focus-within::-webkit-scrollbar-thumb:hover {
      border-width: 0;
      box-shadow: 0 0 0 1px rgba(78, 189, 171, 0.28);
    }

    body {
      margin: 0;
      min-height: 100vh;
      background: var(--bg);
      color: var(--ink);
      font-family: "IBM Plex Sans", "Segoe UI", "Helvetica Neue", sans-serif;
    }

    a { color: var(--accent); }

    .shell {
      max-width: 1660px;
      margin: 0 auto;
      padding: 18px 20px 40px;
    }

    .hero {
      display: grid;
      grid-template-columns: minmax(0, 1.74fr) minmax(300px, 0.46fr);
      gap: 14px;
      align-items: start;
      margin-bottom: 14px;
    }

    .panel {
      background: var(--paper);
      border: none;
      border-radius: 24px;
      box-shadow: var(--panel-outer-shadow);
      position: relative;
      backdrop-filter: blur(14px);
    }

    .hero-main {
      padding: 8px 16px 10px;
      position: relative;
      overflow: visible;
      display: grid;
      gap: 4px;
    }

    .brand-line {
      display: flex;
      align-items: flex-start;
      margin: -24px 0 -36px -8px;
    }

    .brand-lockup {
      width: min(100%, 440px);
      height: auto;
      display: block;
      filter: drop-shadow(0 14px 28px rgba(11, 16, 32, 0.10));
    }

    .hero-cards {
      display: grid;
      grid-template-columns: repeat(3, minmax(0, 1fr));
      gap: 12px;
      margin-top: -10px;
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
      border: none;
      box-shadow: none;
      position: relative;
      overflow: visible;
      isolation: isolate;
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

    .machine-grid {
      grid-template-columns: repeat(4, minmax(0, 1fr));
      align-items: start;
    }

    .machine-grid .machine-compact {
      padding: 14px 16px;
    }

    .machine-grid .machine-compact .card-value {
      font-size: clamp(20px, 2.8vw, 28px);
    }

    .machine-grid .machine-compact .card-note {
      font-size: 13px;
    }

    .machine-grid .machine-compact .metric-row {
      padding-top: 6px;
    }

    .metric-card,
    .service-card,
    .glossary-card,
    .link-card {
      padding: 18px;
      border-radius: 20px;
      border: none;
      background: var(--surface-raised);
      box-shadow: none;
      position: relative;
      overflow: visible;
      isolation: isolate;
    }

    .metric-card.pass,
    .service-card.pass { background: linear-gradient(180deg, rgba(29, 124, 91, 0.04), var(--surface-solid)); }
    .metric-card.alert,
    .service-card.alert { background: linear-gradient(180deg, rgba(185, 109, 16, 0.04), var(--surface-solid)); }
    .metric-card.critical,
    .service-card.critical { background: linear-gradient(180deg, rgba(182, 56, 43, 0.04), var(--surface-solid)); }
    .metric-card.unknown,
    .service-card.unknown { background: linear-gradient(180deg, rgba(97, 113, 122, 0.04), var(--surface-solid)); }

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
      z-index: 2;
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

    .has-tooltip:hover,
    .has-tooltip:focus-visible {
      z-index: 30;
    }

    .compare-card {
      padding: 20px;
      border-radius: 20px;
      border: none;
      background: var(--surface-raised);
      display: grid;
      gap: 16px;
      box-shadow: none;
      position: relative;
      overflow: visible;
      isolation: isolate;
    }

    .compare-card.pass { background: linear-gradient(180deg, rgba(29, 124, 91, 0.04), var(--surface-solid)); }
    .compare-card.alert { background: linear-gradient(180deg, rgba(185, 109, 16, 0.04), var(--surface-solid)); }
    .compare-card.critical { background: linear-gradient(180deg, rgba(182, 56, 43, 0.04), var(--surface-solid)); }
    .compare-card.unknown { background: linear-gradient(180deg, rgba(97, 113, 122, 0.04), var(--surface-solid)); }

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
      border: none;
      border-radius: 18px;
      background: var(--surface);
      padding: 16px;
      display: grid;
      gap: 6px;
      box-shadow: none;
      position: relative;
      overflow: visible;
      isolation: isolate;
    }

    .side-block:hover,
    .side-block:focus-within,
    .metric-card:hover,
    .metric-card:focus-within,
    .service-card:hover,
    .service-card:focus-within,
    .glossary-card:hover,
    .glossary-card:focus-within,
    .link-card:hover,
    .link-card:focus-within,
    .compare-card:hover,
    .compare-card:focus-within,
    .compare-metric:hover,
    .compare-metric:focus-within {
      z-index: 12;
    }

    .side-block::before,
    .metric-card::before,
    .service-card::before,
    .glossary-card::before,
    .link-card::before,
    .compare-card::before,
    .compare-metric::before {
      content: "";
      position: absolute;
      inset: 0;
      border-radius: inherit;
      pointer-events: none;
      box-shadow: var(--card-inner-shadow);
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
      scrollbar-gutter: stable both-edges;
      scrollbar-width: thin;
      scrollbar-color: transparent transparent;
    }

    .compare-table-wrap::-webkit-scrollbar {
      height: 12px;
    }

    .compare-table-wrap::-webkit-scrollbar-track {
      background: transparent;
      border-radius: 999px;
    }

    .compare-table-wrap::-webkit-scrollbar-thumb {
      background: transparent;
      border-radius: 999px;
      border: 2px solid transparent;
      transition: background 0.14s ease, border-width 0.14s ease, box-shadow 0.14s ease;
    }

    .compare-table-wrap::-webkit-scrollbar-corner {
      background: transparent;
    }

    .compare-card:hover .compare-table-wrap,
    .compare-table-wrap:hover,
    .compare-table-wrap:focus-within {
      scrollbar-color: var(--table-scroll-thumb) var(--table-scroll-track);
    }

    .compare-card:hover .compare-table-wrap::-webkit-scrollbar-track,
    .compare-table-wrap:hover::-webkit-scrollbar-track,
    .compare-table-wrap:focus-within::-webkit-scrollbar-track {
      background: var(--table-scroll-track);
    }

    .compare-card:hover .compare-table-wrap::-webkit-scrollbar-thumb,
    .compare-table-wrap:hover::-webkit-scrollbar-thumb,
    .compare-table-wrap:focus-within::-webkit-scrollbar-thumb {
      background: linear-gradient(180deg, var(--table-scroll-thumb), var(--table-scroll-thumb-strong));
      border: 2px solid var(--table-scroll-track);
    }

    .compare-card:hover .compare-table-wrap::-webkit-scrollbar-thumb:hover,
    .compare-table-wrap:hover::-webkit-scrollbar-thumb:hover,
    .compare-table-wrap:focus-within::-webkit-scrollbar-thumb:hover {
      border-width: 0;
      box-shadow: 0 0 0 1px rgba(78, 189, 171, 0.28);
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
        --panel-outer-shadow:
          0 0 2px rgba(0, 0, 0, 0.82),
          0 0 16px rgba(0, 0, 0, 0.44),
          0 0 40px rgba(0, 0, 0, 0.28),
          0 24px 48px -32px rgba(0, 0, 0, 0.34);
        --border: rgba(238, 244, 247, 0.08);
        --surface: rgba(17, 25, 30, 0.78);
        --surface-raised: rgba(17, 25, 30, 0.88);
        --surface-solid: rgba(20, 30, 36, 0.94);
        --surface-border: rgba(238, 244, 247, 0.08);
        --table-scroll-track: rgba(14, 22, 27, 0.96);
        --table-scroll-thumb: rgba(18, 104, 93, 0.88);
        --table-scroll-thumb-strong: rgba(37, 147, 131, 0.96);
        --hero-glow: rgba(121, 210, 197, 0.18);
        --error-border: rgba(255, 143, 130, 0.30);
        --card-outer-shadow:
          0 0 1px rgba(0, 0, 0, 0.60),
          0 0 10px rgba(0, 0, 0, 0.28),
          0 0 22px rgba(0, 0, 0, 0.16),
          0 18px 34px -26px rgba(0, 0, 0, 0.28);
        --card-inner-shadow:
          inset 0 0 0 1px rgba(10, 104, 88, 0.58),
          inset 0 1px 0 rgba(255, 255, 255, 0.030),
          inset 0 12px 18px -15px rgba(7, 82, 69, 0.26),
          inset 0 -12px 18px -15px rgba(6, 63, 54, 0.20),
          inset 12px 0 18px -15px rgba(7, 71, 61, 0.20),
          inset -12px 0 18px -15px rgba(7, 71, 61, 0.20);
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

      .machine-grid {
        grid-template-columns: repeat(2, minmax(0, 1fr));
      }
    }

    @media (max-width: 640px) {
      .machine-grid {
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
          <img class="brand-lockup" src="/brand/amai_lockup.svg" alt="Amai">
        </div>
        <div class="hero-cards" id="hero-cards"></div>
      </div>
      <aside class="panel hero-side">
        <div id="summary-status"></div>
        <div class="side-block">
          <div class="kv" id="headline-kv"></div>
        </div>
        <div class="side-block">
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
      <div class="cards machine-grid" id="machine-cards"></div>
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
    const INTERACTION_HOLD_SELECTOR = [
      ".has-tooltip:hover",
      ".side-block:hover",
      ".metric-card:hover",
      ".service-card:hover",
      ".glossary-card:hover",
      ".link-card:hover",
      ".compare-card:hover",
      ".compare-metric:hover",
      ".hero-main:hover",
      ".hero-side:hover",
    ].join(", ");
    let refreshInFlight = false;
    let interactionHoldUntil = 0;

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

    function extendInteractionHold(multiplier = 4) {
      interactionHoldUntil = Math.max(
        interactionHoldUntil,
        Date.now() + Math.max(REFRESH_MS * multiplier, 1500)
      );
    }

    function hasActiveSelection() {
      const selection = window.getSelection();
      return Boolean(
        selection &&
        !selection.isCollapsed &&
        selection.toString().trim().length > 0
      );
    }

    function hasInteractiveFocus() {
      const active = document.activeElement;
      return Boolean(active && active !== document.body && active.closest(".shell"));
    }

    function isRefreshPaused() {
      if (Date.now() < interactionHoldUntil) {
        return true;
      }
      if (hasActiveSelection()) {
        return true;
      }
      if (hasInteractiveFocus()) {
        return true;
      }
      return Boolean(document.querySelector(INTERACTION_HOLD_SELECTOR));
    }

    function renderCompareCard(container, card) {
      const element = document.createElement("article");
      element.className = `compare-card ${statusClass(card.status)}`;

      const head = document.createElement("div");
      head.className = "compare-head";
      head.appendChild(labelWithTooltip(card.title, card.title_tooltip, "card-title"));
      head.appendChild(textNode("div", `status-pill ${statusClass(card.status)}`, card.status_label));
      element.appendChild(head);

      if (card.headline_value) {
        element.appendChild(textNode("p", "card-value", card.headline_value));
      }

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
        ["Почему такой статус", headline.status_reason],
        ["Сейчас", `${headline.token_value} (${headline.token_scope})`],
        ["Обновление", headline.captured_at],
        ["Автообновление", `${meta.refresh_seconds} сек. / пауза при чтении`],
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
        if (card.extra_class) {
          element.classList.add(card.extra_class);
        }

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

    async function loadDashboard(force = false) {
      if (!force && isRefreshPaused()) {
        return;
      }
      if (refreshInFlight) {
        return;
      }
      refreshInFlight = true;
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
      } finally {
        refreshInFlight = false;
      }
    }

    document.addEventListener("pointerdown", () => extendInteractionHold(6), true);
    document.addEventListener("selectionchange", () => {
      if (hasActiveSelection()) {
        extendInteractionHold(8);
      }
    });
    document.addEventListener("focusin", (event) => {
      if (event.target && event.target.closest && event.target.closest(".shell")) {
        extendInteractionHold(8);
      }
    }, true);
    document.addEventListener("mouseover", (event) => {
      if (
        event.target &&
        event.target.closest &&
        event.target.closest(
          ".has-tooltip, .side-block, .metric-card, .service-card, .glossary-card, .link-card, .compare-card, .compare-metric, .hero-main, .hero-side"
        )
      ) {
        extendInteractionHold(5);
      }
    }, true);

    loadDashboard(true);
    setInterval(() => loadDashboard(false), REFRESH_MS);
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
        "status_label": headline_status_label(status),
        "status_reason": headline_status_reason(pass, alert, critical, unknown),
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
            Some(format_optional(hot_load["qps"].as_f64(), |v| format!("{v:.2} qps"))),
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
        compare_table_card(
            "Полный холодный прогон",
            "Это последний честный end-to-end cold benchmark по реальным репозиториям и query slices.".to_string(),
            cold_contour_status(snapshot),
            Some(source_label(
                "Источник: benchmark. Последний сохранённый end-to-end cold benchmark; live-данные страницы сюда не подмешиваются",
                cold_contour["captured_at_epoch_ms"].as_u64(),
            )),
            Some("Это проверка первого запроса без прогрева. Она меряет весь путь ответа целиком: от выбора нужного маршрута до сборки готового контекста для ответа.".to_string()),
            Some(format_ms(cold_contour["machine_readable_summary"]["p95"].as_f64())),
            vec![
                compare_table_row(
                    "Cold P95",
                    "Цель и факт по p95 в полном cold end-to-end пути.",
                    compare_pair(
                        format_ms(cold_contour["profile"]["target_p95_ms"].as_f64()),
                        format_ms(cold_contour["machine_readable_summary"]["p95"].as_f64()),
                    ),
                ),
                compare_table_row(
                    "Cold P99",
                    "Цель и факт по p99 в полном cold end-to-end пути.",
                    compare_pair(
                        format_ms(cold_contour["profile"]["target_p99_ms"].as_f64()),
                        format_ms(cold_contour["machine_readable_summary"]["p99"].as_f64()),
                    ),
                ),
                compare_table_row(
                    "Cold Max",
                    "Цель и факт по самому тяжёлому выбросу в cold benchmark.",
                    compare_pair(
                        format_ms(cold_contour["profile"]["target_max_ms"].as_f64()),
                        format_ms(cold_contour["machine_readable_summary"]["max"].as_f64()),
                    ),
                ),
                compare_table_row(
                    "Precision",
                    "Точность: насколько чисто найденный контекст оказался релевантным.",
                    compare_pair(
                        format_ratio_percent(cold_contour["profile"]["min_precision"].as_f64()),
                        format_ratio_percent(cold_contour["machine_readable_summary"]["precision"].as_f64()),
                    ),
                ),
                compare_table_row(
                    "Recall",
                    "Полнота: насколько полно система нашла нужные целевые данные.",
                    compare_pair(
                        format_ratio_percent(cold_contour["profile"]["min_recall"].as_f64()),
                        format_ratio_percent(cold_contour["machine_readable_summary"]["recall"].as_f64()),
                    ),
                ),
                compare_table_row(
                    "Hit rate",
                    "Доля запросов, где система действительно попала в нужную цель.",
                    compare_pair(
                        format_ratio_percent(cold_contour["profile"]["min_target_hit_rate"].as_f64()),
                        format_ratio_percent(cold_contour["machine_readable_summary"]["hit_rate"].as_f64()),
                    ),
                ),
                compare_table_row(
                    "Выборка",
                    "Сколько cold-запросов вошло в итоговый benchmark.",
                    compare_pair(
                        "эталон не задан".to_string(),
                        format_u64(cold_contour["machine_readable_summary"]["sample_count"].as_u64()),
                    ),
                ),
                compare_table_row(
                    "Repo count",
                    "Сколько разных репозиториев вошло в последний cold benchmark.",
                    compare_pair(
                        "эталон не задан".to_string(),
                        format_u64(cold_contour["machine_readable_summary"]["repo_count"].as_u64()),
                    ),
                ),
                compare_table_row(
                    "Query slices",
                    "Сколько разных типов запросов покрывает последний cold benchmark.",
                    compare_pair(
                        "эталон не задан".to_string(),
                        format_u64(cold_contour["machine_readable_summary"]["query_slice_count"].as_u64()),
                    ),
                ),
                compare_table_row(
                    "Duration",
                    "Сколько длился полный последний cold benchmark.",
                    compare_pair(
                        "эталон не задан".to_string(),
                        format_optional(
                            cold_contour["machine_readable_summary"]["duration"].as_f64(),
                            |value| format!("{value:.2} сек."),
                        ),
                    ),
                ),
            ],
        ),
        compare_table_card(
            "Точность и изоляция",
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
            Some(format_f64_count(accuracy["cross_project_leakage"].as_f64())),
            vec![
                compare_table_row(
                    "Leakage",
                    "Для строгой проектной изоляции утечки между проектами должны быть равны нулю.",
                    compare_pair(
                        "0".to_string(),
                        format_f64_count(accuracy["cross_project_leakage"].as_f64()),
                    ),
                ),
                compare_table_row(
                    "Symbol precision",
                    "Насколько точно retrieval попадает в нужные символы, функции и сущности.",
                    compare_pair(
                        format_ratio_percent(thresholds["accuracy"]["symbol_precision"]["target"].as_f64()),
                        format_ratio_percent(accuracy["symbol_precision"].as_f64()),
                    ),
                ),
                compare_table_row(
                    "Semantic precision",
                    "Насколько точно семантический слой попадает в правильный контекст.",
                    compare_pair(
                        format_ratio_percent(thresholds["accuracy"]["semantic_precision"]["target"].as_f64()),
                        format_ratio_percent(accuracy["semantic_precision"].as_f64()),
                    ),
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
            match machine.physical_cpus {
                Some(physical) => format!(
                    "{}. Физических ядер: {}. Логических потоков: {}.",
                    machine.cpu_model, physical, machine.logical_cpus
                ),
                None => machine.cpu_model.clone(),
            },
            "pass",
            Some(machine.cpu_source_label.clone()),
            Some("Автоматически собранный профиль CPU. Набор live-полей зависит от ОС и доступных сенсоров, но источник всегда определяется без хардкода под текущую машину.".to_string()),
            vec![
                metric_row(
                    "Нагрузка",
                    format_optional(machine.cpu_usage_percent, |value| format!("{value:.1}%")),
                    Some("Живая текущая загрузка CPU по всей системе."),
                ),
                metric_row(
                    "Температура",
                    format_optional(machine.cpu_temperature_celsius, format_celsius),
                    Some("Текущая температура CPU по доступному сенсору этой ОС."),
                ),
                metric_row(
                    "Максимум частоты",
                    format_optional(machine.cpu_max_mhz, |value| format!("{value:.0} MHz")),
                    Some("Максимальная частота процессора, которую система смогла определить автоматически."),
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
            Some(machine.memory_source_label.clone()),
            Some(
                "Автоматически собранный профиль RAM. Тип и скорость берутся через цепочку OS-specific providers, а live usage идёт из системного runtime.".to_string(),
            ),
            vec![
                metric_row(
                    "Тип",
                    machine.memory_type.clone(),
                    Some("Автоматически определённый тип оперативной памяти."),
                ),
                metric_row(
                    "Скорость",
                    machine.memory_speed_label.clone(),
                    Some("Автоматически определённая скорость оперативной памяти."),
                ),
                metric_row(
                    "Занято",
                    format!("{:.2} GiB", machine.used_memory_gib),
                    Some("Сколько оперативной памяти занято прямо сейчас."),
                ),
                metric_row(
                    "Свободно",
                    format!("{:.2} GiB", machine.available_memory_gib),
                    Some("Сколько оперативной памяти система считает доступной прямо сейчас."),
                ),
                metric_row(
                    "Использование",
                    format_optional(machine.memory_used_percent, |value| format!("{value:.1}%")),
                    Some("Текущая доля занятой оперативной памяти."),
                ),
                metric_row(
                    "Swap",
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
            Some(machine.disk_source_label.clone()),
            Some("Автоматически собранный профиль основного диска. Где ОС даёт live I/O и термоданные, они показываются здесь; где не даёт, панель честно оставляет поле пустым.".to_string()),
            vec![
                metric_row(
                    "Объём",
                    format!("{:.2} GiB", machine.disk_total_gib),
                    Some("Полный размер основного диска."),
                ),
                metric_row(
                    "Свободно",
                    format!("{:.2} GiB", machine.disk_available_gib),
                    Some("Сколько свободного места осталось на основном диске."),
                ),
                metric_row(
                    "Использование",
                    format_optional(machine.disk_used_percent, |value| format!("{value:.1}%")),
                    Some("Текущая доля занятого пространства на основном диске."),
                ),
                metric_row(
                    "Нагрузка",
                    format_optional(machine.disk_busy_percent, |value| format!("{value:.1}%")),
                    Some("Насколько диск был занят операциями ввода-вывода между двумя последними refresh панели."),
                ),
                metric_row(
                    "Чтение",
                    format_optional(machine.disk_read_mib_per_sec, |value| format!("{value:.2} MiB/s")),
                    Some("Текущая скорость чтения между двумя последними refresh панели."),
                ),
                metric_row(
                    "Запись",
                    format_optional(machine.disk_write_mib_per_sec, |value| format!("{value:.2} MiB/s")),
                    Some("Текущая скорость записи между двумя последними refresh панели."),
                ),
                metric_row(
                    "Температура",
                    format_optional(machine.disk_temperature_celsius, format_celsius),
                    Some("Температура NVMe/SSD по живому датчику."),
                ),
                metric_row(
                    "Firmware",
                    machine.disk_firmware.clone(),
                    Some("Версия прошивки основного диска."),
                ),
            ],
        ));
        cards.extend(build_accelerator_cards(&machine.accelerators));
    } else {
        cards.push(card(
            "Машина",
            "ещё нет данных".to_string(),
            "Сводку по железу пока не удалось собрать автоматически.".to_string(),
            "unknown",
        ));
    }

    if let Some(install_state) = install_state {
        cards.push(with_extra_class(
            card(
                "Установленный клиент",
                client_display_name(&install_state.client_key).to_string(),
                format!(
                    "Профиль: {}. Config: {}.",
                    install_state.stack_profile, install_state.client_config
                ),
                "pass",
            ),
            "machine-compact",
        ));
        cards.push(with_extra_class(
            card(
                "Сборка",
                install_state.package_version.clone(),
                format!(
                    "Ревизия: {}. Установлено: {}.",
                    install_state.repo_revision,
                    human_epoch_seconds(install_state.installed_at_epoch_seconds)
                ),
                "pass",
            ),
            "machine-compact",
        ));
    } else {
        cards.push(with_extra_class(
            card(
                "Установка",
                "ещё не найдена".to_string(),
                "state/install_state.json пока не найден, поэтому панель не видит последнюю user-facing установку.".to_string(),
                "unknown",
            ),
            "machine-compact",
        ));
    }
    cards
}

fn build_accelerator_cards(accelerators: &[AcceleratorSummary]) -> Vec<Value> {
    let mut cards = Vec::new();
    let Some(primary) = accelerators.first() else {
        cards.push(card_with_rows(
            "Графика и ускорители",
            "не обнаружено".to_string(),
            "Автоопределение не нашло доступный GPU, iGPU, eGPU или другой ускоритель в этой среде.".to_string(),
            "unknown",
            Some("Источник: accelerator auto-detect provider chain".to_string()),
            Some("Этот блок показывает все найденные графические и AI-ускорители: встроенную графику, дискретные GPU, внешние GPU и другие accelerator-устройства.".to_string()),
            vec![
                metric_row(
                    "Устройств",
                    "0".to_string(),
                    Some("Сколько графических и accelerator-устройств удалось обнаружить автоматически."),
                ),
                metric_row(
                    "Основное устройство",
                    "не обнаружено".to_string(),
                    Some("Какое устройство система выбрала бы основным для показа, если бы оно было найдено."),
                ),
            ],
        ));
        return cards;
    };

    let additional_count = accelerators.len().saturating_sub(1);
    let primary_note = match &primary.driver_version {
        Some(driver) => format!(
            "{}. Стек: {}. Драйвер: {}.",
            primary.kind_label, primary.backend, driver
        ),
        None => format!("{}. Стек: {}.", primary.kind_label, primary.backend),
    };
    let mut primary_rows = vec![
        metric_row(
            "Устройств",
            accelerators.len().to_string(),
            Some("Сколько графических и accelerator-устройств система обнаружила автоматически."),
        ),
        metric_row(
            "Тип",
            primary.kind_label.clone(),
            Some("Какой тип ускорителя система определила для основного устройства."),
        ),
        metric_row(
            "Стек",
            primary.backend.clone(),
            Some("Какой vendor stack или runtime система смогла определить автоматически."),
        ),
        metric_row(
            "Драйвер",
            primary
                .driver_version
                .clone()
                .unwrap_or_else(|| "данные недоступны".to_string()),
            Some("Версия драйвера или runtime, если provider смог её определить."),
        ),
        metric_row(
            "Память",
            format_optional(primary.total_vram_gib, |value| format!("{value:.2} GiB")),
            Some("Полный объём видеопамяти или локальной памяти ускорителя, если provider дал это поле."),
        ),
        metric_row(
            "Использовано памяти",
            format_optional(primary.used_vram_gib, |value| format!("{value:.2} GiB")),
            Some("Сколько памяти ускорителя занято прямо сейчас."),
        ),
        metric_row(
            "Нагрузка",
            format_optional(primary.utilization_percent, |value| format!("{value:.1}%")),
            Some("Текущая загрузка основного ускорителя, если live provider умеет её отдавать."),
        ),
        metric_row(
            "Температура",
            format_optional(primary.temperature_celsius, format_celsius),
            Some("Текущая температура основного ускорителя по доступному live provider."),
        ),
        metric_row(
            "Мощность",
            format_optional(primary.power_watts, |value| format!("{value:.2} W")),
            Some("Текущее энергопотребление основного ускорителя, если provider умеет его отдавать."),
        ),
    ];
    if additional_count > 0 {
        primary_rows.push(metric_row(
            "Другие устройства",
            accelerators[1..]
                .iter()
                .map(|item| format!("{}: {}", item.kind_label, item.model))
                .collect::<Vec<_>>()
                .join("; "),
            Some("Остальные найденные ускорители в этой машине."),
        ));
    }
    cards.push(card_with_rows(
        "Графика и ускорители",
        primary.model.clone(),
        primary_note,
        if primary.detected { "pass" } else { "unknown" },
        Some(primary.source_label.clone()),
        Some("Основным показывается ускоритель с самым богатым live-профилем. Остальные устройства перечислены ниже или отдельными карточками.".to_string()),
        primary_rows,
    ));

    for accelerator in accelerators.iter().skip(1) {
        cards.push(with_extra_class(
            card_with_rows(
                "Доп. ускоритель",
                accelerator.model.clone(),
                match &accelerator.driver_version {
                    Some(driver) => format!(
                        "{}. Стек: {}. Драйвер: {}.",
                        accelerator.kind_label, accelerator.backend, driver
                    ),
                    None => format!("{}. Стек: {}.", accelerator.kind_label, accelerator.backend),
                },
                if accelerator.detected { "pass" } else { "unknown" },
                Some(accelerator.source_label.clone()),
                Some("Дополнительное графическое или accelerator-устройство, найденное в этой машине.".to_string()),
                vec![
                    metric_row("Тип", accelerator.kind_label.clone(), Some("Определённый тип дополнительного ускорителя.")),
                    metric_row(
                        "Память",
                        format_optional(accelerator.total_vram_gib, |value| format!("{value:.2} GiB")),
                        Some("Полный объём памяти дополнительного ускорителя, если provider смог его дать."),
                    ),
                    metric_row(
                        "Нагрузка",
                        format_optional(accelerator.utilization_percent, |value| format!("{value:.1}%")),
                        Some("Текущая загрузка дополнительного ускорителя, если live provider умеет её отдавать."),
                    ),
                    metric_row(
                        "Температура",
                        format_optional(accelerator.temperature_celsius, format_celsius),
                        Some("Текущая температура дополнительного ускорителя, если live provider умеет её отдавать."),
                    ),
                ],
            ),
            "machine-compact",
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
                    "Измерено probe P95",
                    format_ms(snapshot["postgres"]["query_probe_p95_ms"].as_f64()),
                    Some("Фактический p95 живого PostgreSQL probe на этом refresh."),
                ),
                metric_row(
                    "Эталон usage",
                    format_ratio_percent(snapshot["thresholds"]["postgres"]["connection_usage_ratio"]["target"].as_f64()),
                    Some("Желаемая доля занятых соединений PostgreSQL."),
                ),
                metric_row(
                    "Измерено usage",
                    format_ratio_percent(snapshot["postgres"]["connection_usage_ratio"].as_f64()),
                    Some("Фактическая доля занятых соединений прямо сейчас."),
                ),
                metric_row(
                    "Измерено TPS",
                    format_optional(snapshot["postgres"]["transactions_per_sec"].as_f64(), |v| format!("{v:.2}")),
                    Some("Сколько транзакций в секунду база делает между snapshot-ами."),
                ),
                metric_row(
                    "Измерено WAL throughput",
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
                    "Optimize queue",
                    format_f64_count(snapshot["qdrant"]["index_optimize_queue"].as_f64()),
                    Some("Текущая очередь оптимизации индекса Qdrant."),
                ),
                metric_row(
                    "Эталон update queue",
                    format_f64_count(snapshot["thresholds"]["qdrant"]["update_queue_length"]["target"].as_f64()),
                    Some("Желаемая длина очереди обновлений Qdrant."),
                ),
                metric_row(
                    "Update queue",
                    format_f64_count(snapshot["qdrant"]["update_queue_length"].as_f64()),
                    Some("Текущая длина очереди обновлений Qdrant."),
                ),
                metric_row(
                    "Resident memory",
                    format_optional(snapshot["qdrant"]["memory_resident_bytes"].as_f64(), human_bytes),
                    Some("Объём памяти, который Qdrant держит в resident state прямо сейчас."),
                ),
                metric_row(
                    "Points",
                    format_f64_count(snapshot["qdrant"]["points_count"].as_f64()),
                    Some("Сколько точек сейчас лежит в активной кодовой коллекции Qdrant."),
                ),
                metric_row(
                    "Segments",
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
                    "Измерено publish P95",
                    format_ms(snapshot["nats"]["publish_probe_p95_ms"].as_f64()),
                    Some("Фактический p95 для живого publish probe на этом refresh."),
                ),
                metric_row(
                    "Эталон lag",
                    format_f64_count(snapshot["thresholds"]["nats"]["consumer_lag_msgs"]["target"].as_f64()),
                    Some("Желаемый максимум непрочитанных сообщений."),
                ),
                metric_row(
                    "Измерено lag",
                    format_f64_count(snapshot["nats"]["consumer_lag_msgs"].as_f64()),
                    Some("Текущая consumer lag в JetStream."),
                ),
                metric_row(
                    "Эталон disk usage",
                    format_ratio_percent(snapshot["thresholds"]["nats"]["jetstream_disk_usage_ratio"]["target"].as_f64()),
                    Some("Желаемая доля занятого диска JetStream."),
                ),
                metric_row(
                    "Измерено disk usage",
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
    let snapshot_mode = !active;
    let live_or_snapshot_label = if snapshot_mode { "Последний срез" } else { "" };
    let note = if active && available {
        "Живые системные показатели отдельного Qdrant, который сейчас занят внешним benchmark-прогоном. Эти числа не смешиваются с Amai live.".to_string()
    } else if !active && available {
        "Тест сейчас не запущен. Показан последний измеренный срез отдельного benchmark-Qdrant, чтобы вы не теряли картину после остановки прогона.".to_string()
    } else if from_last_success {
        "Показан последний сохранённый срез внешнего benchmark-Qdrant. Новый тест сейчас не запущен, но последние успешные числа сохранены для сравнения.".to_string()
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
            "Источник: последний измеренный срез Qdrant /metrics внешнего бенча ({}). Тест сейчас не запущен.",
            benchmark["http_url"].as_str().unwrap_or("unknown")
        ))
    } else if from_last_success {
        Some(format!(
            "Источник: последний сохранённый срез Qdrant /metrics внешнего бенча ({}). Тест сейчас не запущен.",
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
            &prefixed_metric_label(live_or_snapshot_label, "optimize queue"),
            format_f64_count(benchmark["index_optimize_queue"].as_f64()),
            Some(if snapshot_mode {
                "Последняя измеренная очередь оптимизации индекса у внешнего benchmark-Qdrant перед остановкой теста."
            } else {
                "Текущая очередь оптимизации индекса у внешнего benchmark-Qdrant."
            }),
        ),
        metric_row(
            "Эталон update queue",
            format_f64_count(
                snapshot["thresholds"]["qdrant"]["update_queue_length"]["target"].as_f64(),
            ),
            Some("Желаемая длина очереди обновлений у внешнего benchmark-Qdrant."),
        ),
        metric_row(
            &prefixed_metric_label(live_or_snapshot_label, "update queue"),
            format_f64_count(benchmark["update_queue_length"].as_f64()),
            Some(if snapshot_mode {
                "Последняя измеренная длина очереди обновлений у внешнего benchmark-Qdrant перед остановкой теста."
            } else {
                "Текущая длина очереди обновлений у внешнего benchmark-Qdrant."
            }),
        ),
        metric_row(
            &prefixed_metric_label(live_or_snapshot_label, "resident memory"),
            format_optional(benchmark["memory_resident_bytes"].as_f64(), human_bytes),
            Some(if snapshot_mode {
                "Объём памяти в последнем измеренном срезе внешнего benchmark-Qdrant."
            } else {
                "Объём памяти, который отдельный benchmark-Qdrant держит прямо сейчас."
            }),
        ),
        metric_row(
            &prefixed_metric_label(live_or_snapshot_label, "points"),
            format_f64_count(benchmark["points_count"].as_f64()),
            Some(if snapshot_mode {
                "Сколько точек было загружено во внешний benchmark-Qdrant в последнем измеренном срезе."
            } else {
                "Сколько точек сейчас загружено во внешний benchmark-Qdrant."
            }),
        ),
        metric_row(
            &prefixed_metric_label(live_or_snapshot_label, "segments"),
            format_f64_count(benchmark["segments_count"].as_f64()),
            Some(if snapshot_mode {
                "Сколько сегментов держал внешний benchmark-Qdrant в последнем измеренном срезе."
            } else {
                "Сколько сегментов сейчас держит внешний benchmark-Qdrant."
            }),
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
            "meaning": "Это проверка первого запроса без прогрева. Она показывает, сколько занимает весь путь ответа целиком, пока у системы ещё нет готового быстрого кэша."
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

fn card(title: &str, value: String, note: String, status: &str) -> Value {
    card_with_rows(title, value, note, status, None, None, Vec::new())
}

fn with_extra_class(mut card: Value, extra_class: &str) -> Value {
    if let Some(object) = card.as_object_mut() {
        object.insert("extra_class".to_string(), Value::from(extra_class));
    }
    card
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
    headline_value: Option<String>,
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
        "headline_value": headline_value,
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

fn prefixed_metric_label(prefix: &str, metric: &str) -> String {
    if prefix.trim().is_empty() {
        metric.to_string()
    } else {
        format!("{prefix} {metric}")
    }
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

fn headline_status_label(status: &str) -> &'static str {
    match status {
        "pass" => "система в норме",
        "alert" => "нужно внимание",
        "critical" => "есть критичные сигналы",
        _ => "данных пока мало",
    }
}

fn headline_status_reason(pass: u64, alert: u64, critical: u64, unknown: u64) -> String {
    if critical > 0 {
        format!("Критичных проверок: {critical}. Предупреждений: {alert}.")
    } else if alert > 0 {
        format!("Предупреждений: {alert}. Критичных проверок нет.")
    } else if unknown > 0 {
        format!("Неопределённых проверок: {unknown}. Остальные зелёные: {pass}.")
    } else {
        format!("Все проверки зелёные: {pass}.")
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
                .contains("последний сохранённый срез")
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
