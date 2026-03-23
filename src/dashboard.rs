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
      --waiting: #3f6f93;
      --waiting-soft: rgba(63, 111, 147, 0.12);
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
      max-width: 1900px;
      margin: 0 auto;
      padding: 18px 20px 40px;
    }

    .hero {
      display: grid;
      grid-template-columns: minmax(0, 2.34fr) minmax(320px, 0.28fr);
      gap: 8px;
      align-items: stretch;
      margin-bottom: 8px;
    }

    .panel {
      background: var(--paper);
      border: none;
      border-radius: 12px;
      box-shadow: var(--panel-outer-shadow);
      position: relative;
      backdrop-filter: blur(14px);
    }

    .hero-main {
      padding: 14px 16px 14px;
      position: relative;
      overflow: visible;
      display: flex;
      flex-direction: column;
      justify-content: flex-start;
      gap: 10px;
      min-height: 0;
    }

    .hero-top {
      display: grid;
      grid-template-columns: minmax(0, 1.54fr) minmax(360px, 430px);
      gap: 8px;
      align-items: start;
    }

    .brand-line {
      display: flex;
      align-items: flex-start;
      width: 100%;
      margin: -8px 0 0 -8px;
    }

    .brand-lockup {
      width: min(100%, 1120px);
      height: auto;
      display: block;
      filter: drop-shadow(0 14px 28px rgba(11, 16, 32, 0.10));
    }

    .hero-cards {
      display: grid;
      grid-template-columns: repeat(3, minmax(0, 1fr));
      gap: 8px;
      margin-top: 2px;
      align-items: stretch;
    }

    .hero-metric-card {
      padding: 14px 14px;
      border-radius: 10px;
    }

    .hero-metric-card .card-top {
      margin-bottom: 4px;
      align-items: center;
    }

    .hero-metric-card .card-title {
      font-size: 14px;
    }

    .hero-metric-card .card-value {
      margin: 6px 0 4px;
    }

    .hero-metric-card .card-note {
      font-size: 12px;
      line-height: 1.34;
    }

    .hero-side {
      padding: 10px;
      display: flex;
      flex-direction: column;
      gap: 8px;
      min-height: 0;
      align-self: stretch;
      height: 100%;
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
      white-space: nowrap;
      flex-shrink: 0;
    }

    .status-pill.pass { background: var(--pass-soft); color: var(--pass); }
    .status-pill.alert { background: var(--alert-soft); color: var(--alert); }
    .status-pill.critical { background: var(--critical-soft); color: var(--critical); }
    .status-pill.waiting { background: var(--waiting-soft); color: var(--waiting); }
    .status-pill.unknown { background: var(--unknown-soft); color: var(--unknown); }

    .side-block {
      padding: 10px 12px;
      border-radius: 10px;
      background: var(--surface);
      border: none;
      box-shadow: none;
      position: relative;
      overflow: visible;
      isolation: isolate;
    }

    .hero-summary-block #summary-status {
      margin-bottom: 8px;
    }

    .summary-head-row {
      display: grid;
      grid-template-columns: auto auto 1fr auto;
      align-items: center;
      column-gap: 16px;
    }

    .summary-version-label,
    .summary-version-inline {
      color: var(--ink);
      font-size: 14px;
      font-weight: 800;
      line-height: 1;
      white-space: nowrap;
    }

    .summary-version-label {
      color: var(--muted);
    }

    .summary-version-inline {
      justify-self: end;
      text-align: right;
      padding-left: 20px;
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
      padding: 14px;
      margin-bottom: 8px;
    }

    .cards {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
      gap: 8px;
    }

    .benchmark-cards-grid {
      grid-template-columns: repeat(3, minmax(0, 1fr));
      align-items: stretch;
    }

    .benchmark-cards-grid > .compare-card {
      height: 100%;
    }

    .benchmark-cards-grid > .compare-card:not(.benchmark-span-full) .compare-table-wrap {
      margin-top: auto;
    }

    .machine-grid {
      grid-template-columns: repeat(4, minmax(0, 1fr));
      grid-auto-flow: dense;
      grid-auto-rows: 6px;
      align-items: start;
    }

    .machine-grid .machine-compact {
      padding: 12px 14px;
    }

    .machine-grid .machine-compact .card-value {
      margin: 6px 0 4px;
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
      padding: 14px;
      border-radius: 10px;
      border: none;
      background: var(--surface-raised);
      box-shadow: none;
      position: relative;
      overflow: visible;
      isolation: isolate;
      display: flex;
      flex-direction: column;
      justify-content: flex-start;
      gap: 0;
    }

    .metric-card.pass,
    .service-card.pass { background: linear-gradient(180deg, rgba(29, 124, 91, 0.04), var(--surface-solid)); }
    .metric-card.alert,
    .service-card.alert { background: linear-gradient(180deg, rgba(185, 109, 16, 0.04), var(--surface-solid)); }
    .metric-card.critical,
    .service-card.critical { background: linear-gradient(180deg, rgba(182, 56, 43, 0.04), var(--surface-solid)); }
    .metric-card.waiting,
    .service-card.waiting { background: linear-gradient(180deg, rgba(63, 111, 147, 0.04), var(--surface-solid)); }
    .metric-card.unknown,
    .service-card.unknown { background: linear-gradient(180deg, rgba(97, 113, 122, 0.04), var(--surface-solid)); }

    .card-top {
      display: flex;
      justify-content: space-between;
      align-items: start;
      gap: 8px;
      margin-bottom: 6px;
    }

    .card-title {
      margin: 0;
      font-size: 15px;
      color: var(--muted);
      font-weight: 700;
    }

    .card-value {
      margin: 6px 0 6px;
    }

    .card-note {
      margin: 0;
      color: var(--muted);
      font-size: 14px;
      line-height: 1.5;
    }

    .card-source {
      margin-top: 6px;
      color: var(--accent);
      font-size: 12px;
      font-weight: 700;
      letter-spacing: 0.02em;
    }

    .metric-rows {
      margin: 10px 0 0;
      padding: 0;
      list-style: none;
      display: grid;
      gap: 6px;
    }

    .metric-row {
      display: grid;
      grid-template-columns: minmax(0, 1fr) auto;
      gap: 8px;
      align-items: start;
      padding-top: 6px;
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
      display: inline-flex;
      align-items: center;
      cursor: help;
      text-decoration: underline dotted rgba(13, 107, 111, 0.45);
      text-underline-offset: 3px;
      z-index: 2;
    }

    .has-tooltip:hover,
    .has-tooltip:focus-visible {
      z-index: 30;
    }

    .tooltip-layer {
      position: fixed;
      top: 0;
      left: 0;
      min-width: 220px;
      max-width: min(360px, calc(100vw - 24px));
      padding: 10px 12px;
      border-radius: 12px;
      background: rgba(8, 13, 17, 0.96);
      color: #f7fafc;
      font-size: 12px;
      line-height: 1.45;
      text-transform: none;
      letter-spacing: normal;
      white-space: normal;
      box-shadow: 0 18px 42px rgba(0, 0, 0, 0.28);
      pointer-events: none;
      opacity: 0;
      transform: translateY(4px);
      transition: opacity 0.14s ease, transform 0.14s ease;
      z-index: 10000;
    }

    .tooltip-layer.visible {
      opacity: 1;
      transform: translateY(0);
    }

    .compare-card {
      padding: 14px;
      border-radius: 10px;
      border: none;
      background: var(--surface-raised);
      display: flex;
      flex-direction: column;
      justify-content: flex-start;
      gap: 0;
      box-shadow: none;
      position: relative;
      overflow: visible;
      isolation: isolate;
    }

    .compare-card.pass { background: linear-gradient(180deg, rgba(29, 124, 91, 0.04), var(--surface-solid)); }
    .compare-card.alert { background: linear-gradient(180deg, rgba(185, 109, 16, 0.04), var(--surface-solid)); }
    .compare-card.critical { background: linear-gradient(180deg, rgba(182, 56, 43, 0.04), var(--surface-solid)); }
    .compare-card.waiting { background: linear-gradient(180deg, rgba(63, 111, 147, 0.04), var(--surface-solid)); }
    .compare-card.unknown { background: linear-gradient(180deg, rgba(97, 113, 122, 0.04), var(--surface-solid)); }

    .benchmark-span-full {
      grid-column: 1 / -1;
    }

    .compare-head {
      display: flex;
      justify-content: space-between;
      align-items: start;
      gap: 8px;
      margin-bottom: 6px;
    }

    .compare-headline {
      margin: 0 0 8px;
    }

    .compare-grid {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 8px;
      margin-top: 4px;
    }

    .compare-metric {
      border: none;
      border-radius: 10px;
      background: var(--surface);
      padding: 12px;
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
    }

    .compare-metric-note {
      margin: 0;
      color: var(--muted);
      font-size: 13px;
      line-height: 1.45;
    }

    .compare-table-wrap {
      margin-top: 8px;
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

    .compare-value-stack {
      display: inline-grid;
      justify-items: end;
      gap: 2px;
      line-height: 1.1;
    }

    .compare-value-stack-primary {
      color: var(--ink);
      font-weight: 700;
    }

    .compare-value-stack-secondary {
      color: var(--muted);
      font-size: 11px;
      font-weight: 700;
      letter-spacing: 0.02em;
      text-transform: none;
    }

    .compare-card.table-transposed .compare-table {
      table-layout: fixed;
    }

    .compare-card.table-transposed .compare-table th,
    .compare-card.table-transposed .compare-table td {
      white-space: normal;
      text-align: center;
    }

    .compare-card.table-transposed .compare-table th:first-child,
    .compare-card.table-transposed .compare-table td:first-child {
      text-align: left;
      min-width: 160px;
      width: 20%;
    }

    .service-headline {
      margin: 0 0 6px;
    }

    .card-value,
    .service-headline,
    .compare-headline,
    .compare-metric-value,
    .machine-grid .machine-compact .card-value {
      font-size: clamp(20px, 2.2vw, 22px);
      line-height: 1.04;
      letter-spacing: -0.03em;
      font-weight: 800;
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
      border-radius: 10px;
      background: var(--critical-soft);
      color: var(--critical);
      font-weight: 700;
      margin-bottom: 10px;
      border: 1px solid var(--error-border);
    }

    .link-disabled {
      color: var(--muted);
      font-weight: 700;
      text-decoration: none;
      cursor: default;
    }

    .hero-links-block {
      display: flex;
      flex-direction: column;
      height: 100%;
      min-height: 100%;
    }

    .hero-links-block .link-list {
      flex: 1;
      align-content: start;
      padding-left: 0;
      list-style: none;
      gap: 10px;
    }

    .hero-links-block .link-list li {
      display: block;
      padding: 10px 12px;
      border-radius: 10px;
      background: rgba(255, 255, 255, 0.02);
    }

    .link-item-main {
      min-width: 0;
      display: grid;
      gap: 6px;
    }

    .link-item-main a,
    .link-item-main .link-disabled {
      font-weight: 700;
      font-size: 14px;
      line-height: 1.3;
      text-decoration-thickness: 1px;
    }

    .link-item-note {
      color: var(--muted);
      font-size: 13px;
      line-height: 1.45;
    }

    .link-inline {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      max-width: 100%;
      position: relative;
    }

    .copy-link-btn {
      appearance: none;
      border: none;
      border-radius: 999px;
      width: 24px;
      height: 24px;
      padding: 0;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      background: rgba(121, 210, 197, 0.10);
      color: var(--accent);
      font: inherit;
      font-size: 13px;
      font-weight: 700;
      line-height: 1;
      cursor: pointer;
      white-space: nowrap;
      opacity: 0;
      transform: translateY(1px);
      transition: opacity 0.14s ease, background 0.14s ease, transform 0.14s ease;
    }

    .hero-links-block .link-list li:hover .copy-link-btn,
    .hero-links-block .link-list li:focus-within .copy-link-btn,
    .link-inline:hover .copy-link-btn,
    .link-inline:focus-within .copy-link-btn {
      opacity: 1;
      transform: translateY(0);
    }

    .copy-link-btn:hover,
    .copy-link-btn:focus-visible {
      background: rgba(121, 210, 197, 0.20);
      outline: none;
    }

    .inline-path,
    .inline-copyable {
      color: var(--accent);
      font-weight: 700;
      text-decoration: underline;
      text-decoration-color: rgba(121, 210, 197, 0.42);
      text-decoration-thickness: 1px;
      text-underline-offset: 3px;
    }

    .inline-path {
      white-space: nowrap;
    }

    code {
      font-family: "IBM Plex Mono", "JetBrains Mono", "SFMono-Regular", monospace;
      font-size: 0.92em;
    }

    @media (prefers-color-scheme: dark) {
      :root {
        color-scheme: dark;
        --bg: radial-gradient(circle at top, #1b454b 0%, #14262d 38%, #0e181d 100%);
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
        --waiting: #8fb9e0;
        --waiting-soft: rgba(76, 127, 173, 0.24);
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

      .hero-top {
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

    @media (max-width: 1200px) {
      .benchmark-cards-grid {
        grid-template-columns: repeat(2, minmax(0, 1fr));
      }
    }

    @media (max-width: 640px) {
      .benchmark-cards-grid {
        grid-template-columns: 1fr;
      }

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
        <div class="hero-top">
          <div class="brand-line">
            <img class="brand-lockup" src="/brand/amai_lockup.svg" alt="Amai">
          </div>
          <div class="side-block hero-summary-block">
            <div id="summary-status"></div>
            <div class="kv" id="headline-kv"></div>
          </div>
        </div>
        <div class="hero-cards" id="hero-cards"></div>
      </div>
      <aside class="panel hero-side">
        <div class="side-block hero-links-block">
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
      <div class="cards benchmark-cards-grid" id="benchmark-cards"></div>
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
  <div id="tooltip-layer" class="tooltip-layer" hidden></div>

  <script>
    const REFRESH_MS = __REFRESH_MS__;
    const errorBanner = document.getElementById("error-banner");
    const tooltipLayer = document.getElementById("tooltip-layer");
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
    let activeTooltipTarget = null;

    function statusClass(status) {
      return ["pass", "alert", "critical", "waiting", "unknown"].includes(status) ? status : "unknown";
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

    function createCopyButton(valueToCopy) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "copy-link-btn";
      button.textContent = "⧉";
      button.setAttribute("aria-label", "Скопировать");
      button.title = "Скопировать";
      button.addEventListener("click", async (event) => {
        event.preventDefault();
        event.stopPropagation();
        try {
          await navigator.clipboard.writeText(valueToCopy);
          button.textContent = "✓";
          button.title = "Скопировано";
          setTimeout(() => {
            button.textContent = "⧉";
            button.title = "Скопировать";
          }, 1200);
        } catch (_) {
          button.textContent = "!";
          button.title = "Не удалось скопировать";
          setTimeout(() => {
            button.textContent = "⧉";
            button.title = "Скопировать";
          }, 1200);
        }
      });
      return button;
    }

    function createInlineCopyableText(label, copyValue, href = null) {
      const wrap = document.createElement("span");
      wrap.className = "link-inline";
      if (href) {
        const link = document.createElement("a");
        link.href = href;
        link.textContent = label;
        link.target = "_blank";
        link.rel = "noreferrer";
        wrap.appendChild(link);
      } else {
        wrap.appendChild(textNode("span", "inline-copyable", label));
      }
      wrap.appendChild(createCopyButton(copyValue));
      return wrap;
    }

    function appendInlineNoteFragment(container, fragment) {
      const urlMatch = fragment.match(/https?:\/\/[^\s]+/);
      if (urlMatch) {
        const [matched] = urlMatch;
        const index = fragment.indexOf(matched);
        if (index > 0) {
          container.appendChild(document.createTextNode(fragment.slice(0, index)));
        }
        container.appendChild(createInlineCopyableText(matched, matched, matched));
        const tail = fragment.slice(index + matched.length);
        if (tail) {
          appendInlineNoteFragment(container, tail);
        }
        return;
      }

      const pathMatch = fragment.match(/(?:\.\.?\/[A-Za-z0-9_./-]+|\/[A-Za-z0-9_./-]+)/);
      if (pathMatch) {
        const [matched] = pathMatch;
        const index = fragment.indexOf(matched);
        if (index > 0) {
          container.appendChild(document.createTextNode(fragment.slice(0, index)));
        }
        const pathWrap = createInlineCopyableText(matched, matched);
        const inlinePath = pathWrap.querySelector(".inline-copyable");
        if (inlinePath) {
          inlinePath.classList.add("inline-path");
        }
        container.appendChild(pathWrap);
        const tail = fragment.slice(index + matched.length);
        if (tail) {
          appendInlineNoteFragment(container, tail);
        }
        return;
      }

      container.appendChild(document.createTextNode(fragment));
    }

    function labelWithTooltip(text, tooltip, className = "metric-label") {
      const wrap = document.createElement("span");
      wrap.className = tooltip ? `${className} has-tooltip` : className;
      if (tooltip) {
        wrap.tabIndex = 0;
        wrap.setAttribute("data-tip", tooltip);
      }
      if (text.includes("\n")) {
        wrap.style.whiteSpace = "pre-line";
        wrap.style.lineHeight = "1.08";
      }
      wrap.textContent = text;
      return wrap;
    }

    function showTooltip(target) {
      if (!tooltipLayer || !target) {
        return;
      }
      const tip = target.getAttribute("data-tip");
      if (!tip) {
        hideTooltip();
        return;
      }
      activeTooltipTarget = target;
      tooltipLayer.textContent = tip;
      tooltipLayer.hidden = false;
      tooltipLayer.classList.add("visible");
      target.setAttribute("aria-describedby", "tooltip-layer");
      positionTooltip(target);
    }

    function hideTooltip(target = null) {
      if (!tooltipLayer) {
        return;
      }
      if (target && activeTooltipTarget && target !== activeTooltipTarget) {
        return;
      }
      if (activeTooltipTarget) {
        activeTooltipTarget.removeAttribute("aria-describedby");
      }
      activeTooltipTarget = null;
      tooltipLayer.classList.remove("visible");
      tooltipLayer.hidden = true;
      tooltipLayer.textContent = "";
    }

    function positionTooltip(target = activeTooltipTarget) {
      if (!tooltipLayer || !target || tooltipLayer.hidden) {
        return;
      }

      const margin = 12;
      const targetRect = target.getBoundingClientRect();
      tooltipLayer.style.left = "0px";
      tooltipLayer.style.top = "0px";
      tooltipLayer.style.maxWidth = `${Math.max(220, Math.min(360, window.innerWidth - margin * 2))}px`;
      const tooltipRect = tooltipLayer.getBoundingClientRect();

      let left = targetRect.left + targetRect.width / 2 - tooltipRect.width / 2;
      left = Math.max(margin, Math.min(left, window.innerWidth - tooltipRect.width - margin));

      let top = targetRect.top - tooltipRect.height - 12;
      if (top < margin) {
        top = Math.min(
          window.innerHeight - tooltipRect.height - margin,
          targetRect.bottom + 12
        );
      }

      tooltipLayer.style.left = `${left}px`;
      tooltipLayer.style.top = `${Math.max(margin, top)}px`;
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
      addExtraClasses(element, card.extra_class);
      if (card.table_orientation === "transposed") {
        element.classList.add("table-transposed");
      }

      const head = document.createElement("div");
      head.className = "compare-head";
      head.appendChild(labelWithTooltip(card.title, card.title_tooltip, "card-title"));
      head.appendChild(textNode("div", `status-pill ${statusClass(card.status)}`, card.status_label));
      element.appendChild(head);

      if (card.headline_value) {
        const headline = document.createElement("p");
        headline.className = "service-headline compare-headline";
        appendCompareCellValue(headline, card.headline_value);
        element.appendChild(headline);
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
      renderCompareTable(table, card.table, card.table_orientation);
      tableWrap.appendChild(table);
      element.appendChild(tableWrap);

      container.appendChild(element);
    }

    function renderCompareTable(table, tableData, orientation) {
      if (orientation === "transposed") {
        renderTransposedCompareTable(table, tableData);
        return;
      }

      renderStandardCompareTable(table, tableData);
    }

    function renderStandardCompareTable(table, tableData) {
      const thead = document.createElement("thead");
      const headRow = document.createElement("tr");
      tableData.columns.forEach((column) => {
        const th = document.createElement("th");
        th.appendChild(labelWithTooltip(column.label, column.tooltip, ""));
        headRow.appendChild(th);
      });
      thead.appendChild(headRow);
      table.appendChild(thead);

      const tbody = document.createElement("tbody");
      tableData.rows.forEach((row) => {
        const tr = document.createElement("tr");
        const labelCell = document.createElement("td");
        labelCell.appendChild(labelWithTooltip(row.label, row.tooltip, ""));
        tr.appendChild(labelCell);
        row.values.forEach((value) => {
          const valueCell = document.createElement("td");
          appendCompareCellValue(valueCell, value);
          tr.appendChild(valueCell);
        });
        tbody.appendChild(tr);
      });
      table.appendChild(tbody);
    }

    function renderTransposedCompareTable(table, tableData) {
      const valueColumns = tableData.columns.slice(1);
      const metrics = tableData.rows;

      const thead = document.createElement("thead");
      const headRow = document.createElement("tr");
      const scopeHeader = document.createElement("th");
      scopeHeader.appendChild(labelWithTooltip("Срез", "Какой слой сравнения показан в строке: эталон или тестовые данные.", ""));
      headRow.appendChild(scopeHeader);
      metrics.forEach((metric) => {
        const th = document.createElement("th");
        th.appendChild(labelWithTooltip(metric.label, metric.tooltip, ""));
        headRow.appendChild(th);
      });
      thead.appendChild(headRow);
      table.appendChild(thead);

      const tbody = document.createElement("tbody");
      valueColumns.forEach((column, columnIndex) => {
        const tr = document.createElement("tr");
        const labelCell = document.createElement("td");
        labelCell.appendChild(labelWithTooltip(column.label, column.tooltip, ""));
        tr.appendChild(labelCell);

        metrics.forEach((metric) => {
          const valueCell = document.createElement("td");
          appendCompareCellValue(valueCell, metric.values[columnIndex] || "ещё нет данных");
          tr.appendChild(valueCell);
        });
        tbody.appendChild(tr);
      });
      table.appendChild(tbody);
    }

    function appendCompareCellValue(cell, value) {
      if (typeof value === "string" && value.includes("\n")) {
        const [primary, ...secondaryParts] = value.split("\n");
        const stack = document.createElement("span");
        stack.className = "compare-value-stack";
        stack.appendChild(textNode("span", "compare-value-stack-primary", primary));
        stack.appendChild(
          textNode(
            "span",
            "compare-value-stack-secondary",
            secondaryParts.join(" ").trim() || ""
          )
        );
        cell.appendChild(stack);
        return;
      }
      cell.textContent = value;
    }

    function addExtraClasses(element, extraClass) {
      if (!extraClass) {
        return;
      }
      extraClass
        .split(/\s+/)
        .filter(Boolean)
        .forEach((className) => element.classList.add(className));
    }

    function renderSummary(meta, headline) {
      const summary = document.getElementById("summary-status");
      clearNode(summary);
      const pill = textNode("div", `status-pill ${statusClass(headline.status)}`, headline.status_label);
      const headRow = document.createElement("div");
      headRow.className = "summary-head-row";
      headRow.appendChild(pill);
      headRow.appendChild(textNode("div", "summary-version-label", "Версия"));
      headRow.appendChild(textNode("div", "summary-version-inline", meta.package_version || "ещё нет данных"));
      summary.appendChild(headRow);

      const kv = document.getElementById("headline-kv");
      clearNode(kv);
      const rows = [
        ["Почему такой статус", headline.status_reason],
        ["Сейчас", `${headline.token_value} (${headline.token_scope})`],
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
        const main = document.createElement("div");
        main.className = "link-item-main";
        if (entry.url) {
          main.appendChild(createInlineCopyableText(entry.label, entry.url, entry.url));
        } else {
          main.appendChild(textNode("span", "link-disabled", entry.label));
        }
        if (entry.note) {
          const note = document.createElement("span");
          note.className = "link-item-note";
          appendInlineNoteFragment(note, entry.note);
          main.appendChild(note);
        }
        li.appendChild(main);
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
        addExtraClasses(element, card.extra_class);

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

      if (containerId === "machine-cards") {
        applyMasonryGrid(container);
      }
    }

    function applyMasonryGrid(container) {
      if (!container || !container.classList.contains("machine-grid")) {
        return;
      }

      const styles = window.getComputedStyle(container);
      const rowGap = Number.parseFloat(styles.rowGap || "0");
      const rowHeight = Number.parseFloat(styles.gridAutoRows || "0");
      if (!rowHeight || Number.isNaN(rowHeight)) {
        return;
      }

      const children = Array.from(container.children);
      children.forEach((child) => {
        child.style.gridRowEnd = "span 1";
      });

      requestAnimationFrame(() => {
        children.forEach((child) => {
          const height = child.getBoundingClientRect().height;
          const span = Math.max(1, Math.ceil((height + rowGap) / (rowHeight + rowGap)));
          child.style.gridRowEnd = `span ${span}`;
        });
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
        hideTooltip();
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
      const tooltipTarget =
        event.target && event.target.closest ? event.target.closest(".has-tooltip") : null;
      if (tooltipTarget) {
        showTooltip(tooltipTarget);
      }
      if (event.target && event.target.closest && event.target.closest(".shell")) {
        extendInteractionHold(8);
      }
    }, true);
    document.addEventListener("focusout", (event) => {
      const tooltipTarget =
        event.target && event.target.closest ? event.target.closest(".has-tooltip") : null;
      if (tooltipTarget) {
        hideTooltip(tooltipTarget);
      }
    }, true);
    document.addEventListener("mouseover", (event) => {
      const tooltipTarget =
        event.target && event.target.closest ? event.target.closest(".has-tooltip") : null;
      if (tooltipTarget) {
        showTooltip(tooltipTarget);
      }
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
    document.addEventListener("mouseout", (event) => {
      const tooltipTarget =
        event.target && event.target.closest ? event.target.closest(".has-tooltip") : null;
      const relatedTooltip =
        event.relatedTarget && event.relatedTarget.closest
          ? event.relatedTarget.closest(".has-tooltip")
          : null;
      if (tooltipTarget && relatedTooltip !== tooltipTarget) {
        hideTooltip(tooltipTarget);
      }
    }, true);
    document.addEventListener("scroll", () => positionTooltip(), true);

    window.addEventListener("resize", () => {
      positionTooltip();
      const machineCards = document.getElementById("machine-cards");
      if (machineCards && machineCards.children.length > 0) {
        applyMasonryGrid(machineCards);
      }
    });

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
        "machine_cards": build_machine_cards(snapshot, machine.as_ref(), install_state.as_ref()),
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
    let sla_status = if critical > 0 {
        "critical"
    } else if alert > 0 {
        "alert"
    } else if unknown > 0 {
        "unknown"
    } else {
        "pass"
    };
    let live_status = live_latency_compare_status(snapshot);
    let status = combine_headline_statuses(sla_status, live_status);
    json!({
        "status": status,
        "status_label": headline_status_label(status),
        "status_reason": headline_status_reason(pass, alert, critical, unknown, live_status),
        "captured_at": human_timestamp(captured_at_epoch_ms),
        "summary": format!("SLA сейчас: pass={pass}, alert={alert}, critical={critical}, unknown={unknown}"),
        "token_title": token_headline["title"].as_str().unwrap_or("ещё нет данных"),
        "token_value": format_percent(token_headline["value_percent"].as_f64()),
        "token_scope": token_headline["scope_label"].as_str().unwrap_or("ещё нет данных"),
    })
}

fn build_top_cards(snapshot: &Value) -> Vec<Value> {
    vec![
        live_latency_compare_card(snapshot),
        working_state_live_card(snapshot),
    ]
}

fn build_benchmark_cards(snapshot: &Value) -> Vec<Value> {
    let hot_load = &snapshot["latest_retrieval_load_hot"]["load_verification"];
    let hot_retrieval = &snapshot["latest_retrieval_hot"]["benchmark"];
    let cold_contour = &snapshot["latest_cold_path_benchmark"]["cold_benchmark"];
    let accuracy = &snapshot["latest_retrieval_accuracy"]["accuracy_verification"];
    let thresholds = &snapshot["thresholds"];
    let hot_load_sample_count = hot_load["success_count"]
        .as_u64()
        .zip(hot_load["error_count"].as_u64())
        .map(|(success, errors)| success + errors);
    let hot_load_scope = format!(
        "project={} / namespace={} / query={} / execution_mode={}",
        hot_load["project"].as_str().unwrap_or("ещё нет данных"),
        hot_load["namespace"].as_str().unwrap_or("ещё нет данных"),
        hot_load["query"].as_str().unwrap_or("ещё нет данных"),
        hot_load["execution_mode"]
            .as_str()
            .unwrap_or("ещё нет данных"),
    );
    let hot_retrieval_scope = format!(
        "project={} / namespace={} / query={} / disable_cache={}",
        hot_retrieval["project"]
            .as_str()
            .unwrap_or("ещё нет данных"),
        hot_retrieval["namespace"]
            .as_str()
            .unwrap_or("ещё нет данных"),
        hot_retrieval["query"].as_str().unwrap_or("ещё нет данных"),
        hot_retrieval["disable_cache"]
            .as_bool()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "ещё нет данных".to_string()),
    );

    vec![
        compare_table_card(
            "Hot Load Benchmark / latest_retrieval_load_hot",
            format!(
                "Контур данных: latest_retrieval_load_hot.load_verification. Scope snapshot: {hot_load_scope}. Это отдельный hot-load прогон по прогретому быстрому пути. Он не равен retrieval.hot_p95_ms и не является живой телеметрией текущей сессии. Burst QPS здесь считается как success_count / wall_clock, а не как целый счётчик за полную секунду. В последнем прогоне это {} запросов за {}.",
                format_u64(hot_load_sample_count),
                format_ms(snapshot, hot_load["wall_clock_ms"].as_f64()),
            ),
            hot_load_benchmark_status(hot_load, thresholds),
            Some(source_label(
                &format!(
                    "Источник: benchmark snapshot latest_retrieval_load_hot.load_verification. Scope: {hot_load_scope}. Live-данные страницы сюда не подмешиваются"
                ),
                hot_load["captured_at_epoch_ms"].as_u64(),
            )),
            Some("Это отдельный параллельный load-contour. Он нужен для Burst QPS, worker-ов и error-rate под нагрузкой. Его нельзя один к одному сравнивать с retrieval hot benchmark, который питает SLA `retrieval.hot_p95_ms`.".to_string()),
            Some(format_burst_qps_table(hot_load["qps"].as_f64())),
            vec![
                compare_table_row(
                    "Burst QPS",
                    "Средняя скорость внутри короткого benchmark-окна hot-load прогона. Это burst-rate, а не обещание стабильной обычной пропускной способности.",
                    compare_pair(
                        format_burst_qps_threshold(
                            thresholds["load"]["hot_qps"].get("target").and_then(Value::as_f64),
                            ">",
                        ),
                        format_burst_qps_table(hot_load["qps"].as_f64()),
                    ),
                ),
                compare_table_row(
                    "P50",
                    "Медиана hot benchmark. Обычный уровень задержки в отдельном нагрузочном прогоне.",
                    format_time_compare_pair(
                        snapshot,
                        thresholds["load"]["hot_benchmark_table"]["target_p50_ms"].as_f64(),
                        hot_load["p50_ms"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "P95",
                    "Тяжёлый хвост hot benchmark. Почти все прогретые ответы должны укладываться в эту границу.",
                    format_time_compare_pair(
                        snapshot,
                        thresholds["load"]["hot_benchmark_table"]["target_p95_ms"].as_f64(),
                        hot_load["p95_ms"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "P99",
                    "Редкие тяжёлые выбросы в отдельном hot-load benchmark.",
                    format_time_compare_pair(
                        snapshot,
                        thresholds["load"]["hot_benchmark_table"]["target_p99_ms"].as_f64(),
                        hot_load["p99_ms"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "Max",
                    "Самый тяжёлый одиночный запрос в последнем hot-load benchmark.",
                    format_time_compare_pair(
                        snapshot,
                        thresholds["load"]["hot_benchmark_table"]["target_max_ms"].as_f64(),
                        hot_load["max_ms"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "Error rate",
                    "Доля ошибок в отдельном hot-load benchmark. Здесь целевой уровень должен быть нулевым.",
                    compare_pair(
                        format_zero_or_at_most_percent(
                            thresholds["load"]["hot_error_rate"].get("target").and_then(Value::as_f64),
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
            "Hot Retrieval Benchmark / latest_retrieval_hot",
            format!(
                "Контур данных: latest_retrieval_hot.benchmark. Scope snapshot: {hot_retrieval_scope}. Это именно источник SLA-метрики retrieval.hot_p95_ms. Это не hot-load benchmark и не живая телеметрия текущей сессии."
            ),
            hot_retrieval_benchmark_status(hot_retrieval, thresholds),
            Some(source_label(
                &format!(
                    "Источник: benchmark snapshot latest_retrieval_hot.benchmark. Этот snapshot напрямую кормит SLA retrieval.hot_p95_ms. Scope: {hot_retrieval_scope}"
                ),
                hot_retrieval["captured_at_epoch_ms"].as_u64(),
            )),
            Some("Это короткий retrieval-бенчмарк одиночного повторного запроса. Он показывает latency самого retrieval-контура и именно его значения идут в SLA `retrieval.hot_p95_ms`.".to_string()),
            Some(format_ms(snapshot, hot_retrieval["p95_ms"].as_f64())),
            vec![
                compare_table_row(
                    "Burst QPS",
                    "Средняя скорость внутри короткого retrieval benchmark-окна. Это burst-rate этого контура, а не нагрузочный QPS из hot-load и не SLA-порог.",
                    compare_pair(
                        "нет SLA-порога".to_string(),
                        format_burst_qps_table(hot_retrieval["qps"].as_f64()),
                    ),
                ),
                compare_table_row(
                    "P50",
                    "Медиана одиночного повторного retrieval-запроса в benchmark-контуре, который кормит SLA retrieval.hot_p95_ms.",
                    format_time_compare_pair(
                        snapshot,
                        thresholds["retrieval"]["hot_live_table"]["target_p50_ms"].as_f64(),
                        hot_retrieval["p50_ms"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "P95",
                    "Тяжёлый хвост retrieval hot benchmark. Именно этот показатель используется в SLA retrieval.hot_p95_ms.",
                    format_time_compare_pair(
                        snapshot,
                        thresholds["retrieval"]["hot_live_table"]["target_p95_ms"].as_f64(),
                        hot_retrieval["p95_ms"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "P99",
                    "Редкие тяжёлые выбросы в retrieval hot benchmark.",
                    format_time_compare_pair(
                        snapshot,
                        thresholds["retrieval"]["hot_live_table"]["target_p99_ms"].as_f64(),
                        hot_retrieval["p99_ms"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "Max",
                    "Самый тяжёлый одиночный запрос в retrieval hot benchmark.",
                    format_time_compare_pair(
                        snapshot,
                        thresholds["retrieval"]["hot_live_table"]["target_max_ms"].as_f64(),
                        hot_retrieval["max_ms"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "Итерации",
                    "Сколько измерений вошло в последний retrieval hot benchmark snapshot.",
                    compare_pair(
                        format_threshold_at_least_or_equal(
                            thresholds["retrieval"]["hot_benchmark_table"]["target_iterations"]
                                .as_f64(),
                            "",
                            0,
                        ),
                        format_u64(hot_retrieval["iterations"].as_u64()),
                    ),
                ),
                compare_table_row(
                    "Warmup",
                    "Сколько прогревочных запросов было выполнено перед измерением retrieval hot benchmark.",
                    compare_pair(
                        format_threshold_at_least_or_equal(
                            thresholds["retrieval"]["hot_benchmark_table"]["target_warmup"]
                                .as_f64(),
                            "",
                            0,
                        ),
                        format_u64(hot_retrieval["warmup"].as_u64()),
                    ),
                ),
            ],
        ),
        compare_table_card(
            "Cold End-to-End Benchmark / latest_cold_path_benchmark",
            "Контур данных: latest_cold_path_benchmark.cold_benchmark. Это последний честный end-to-end cold benchmark по реальным репозиториям и query slices.".to_string(),
            cold_contour_status(snapshot),
            Some(source_label(
                "Источник: benchmark snapshot latest_cold_path_benchmark.cold_benchmark. Live-данные страницы сюда не подмешиваются",
                cold_contour["captured_at_epoch_ms"].as_u64(),
            )),
            Some("Это проверка первого запроса без прогрева. Она меряет весь путь ответа целиком: от выбора нужного маршрута до сборки готового контекста для ответа.".to_string()),
            Some(format_ms(
                snapshot,
                cold_contour["machine_readable_summary"]["p95"].as_f64(),
            )),
            vec![
                compare_table_row(
                    "Cold P50",
                    "Цель и факт по обычному уровню задержки в полном cold end-to-end пути.",
                    format_time_compare_pair(
                        snapshot,
                        cold_contour["profile"]["target_p50_ms"].as_f64(),
                        cold_contour["machine_readable_summary"]["p50"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "Cold P95",
                    "Цель и факт по p95 в полном cold end-to-end пути.",
                    format_time_compare_pair(
                        snapshot,
                        cold_contour["profile"]["target_p95_ms"].as_f64(),
                        cold_contour["machine_readable_summary"]["p95"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "Cold P99",
                    "Цель и факт по p99 в полном cold end-to-end пути.",
                    format_time_compare_pair(
                        snapshot,
                        cold_contour["profile"]["target_p99_ms"].as_f64(),
                        cold_contour["machine_readable_summary"]["p99"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "Cold Max",
                    "Цель и факт по самому тяжёлому выбросу в cold benchmark.",
                    format_time_compare_pair(
                        snapshot,
                        cold_contour["profile"]["target_max_ms"].as_f64(),
                        cold_contour["machine_readable_summary"]["max"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "Precision",
                    "Точность: насколько чисто найденный контекст оказался релевантным.",
                    compare_pair(
                        format_threshold_value(
                            cold_contour["profile"]["min_precision"]
                                .as_f64()
                                .map(|value| value * 100.0),
                            ">=",
                            "%",
                            2,
                        ),
                        format_ratio_percent(cold_contour["machine_readable_summary"]["precision"].as_f64()),
                    ),
                ),
                compare_table_row(
                    "Recall",
                    "Полнота: насколько полно система нашла нужные целевые данные.",
                    compare_pair(
                        format_threshold_value(
                            cold_contour["profile"]["min_recall"]
                                .as_f64()
                                .map(|value| value * 100.0),
                            ">=",
                            "%",
                            2,
                        ),
                        format_ratio_percent(cold_contour["machine_readable_summary"]["recall"].as_f64()),
                    ),
                ),
                compare_table_row(
                    "Hit rate",
                    "Доля запросов, где система действительно попала в нужную цель.",
                    compare_pair(
                        format_threshold_value(
                            cold_contour["profile"]["min_target_hit_rate"]
                                .as_f64()
                                .map(|value| value * 100.0),
                            ">=",
                            "%",
                            2,
                        ),
                        format_ratio_percent(cold_contour["machine_readable_summary"]["hit_rate"].as_f64()),
                    ),
                ),
                compare_table_row(
                    "Выборка",
                    "Сколько cold-запросов вошло в итоговый benchmark.",
                    compare_pair(
                        format_threshold_at_least_or_equal(
                            cold_contour["profile"]["min_sample_count"].as_f64(),
                            "",
                            0,
                        ),
                        format_u64(cold_contour["machine_readable_summary"]["sample_count"].as_u64()),
                    ),
                ),
                compare_table_row(
                    "Repo count",
                    "Сколько разных репозиториев вошло в последний cold benchmark.",
                    compare_pair(
                        format_threshold_at_least_or_equal(
                            cold_contour["profile"]["min_repo_count"].as_f64(),
                            "",
                            0,
                        ),
                        format_u64(cold_contour["machine_readable_summary"]["repo_count"].as_u64()),
                    ),
                ),
                compare_table_row(
                    "Query slices",
                    "Сколько разных типов запросов покрывает последний cold benchmark.",
                    compare_pair(
                        format_threshold_at_least_or_equal(
                            cold_contour["profile"]["min_query_slice_count"].as_f64(),
                            "",
                            0,
                        ),
                        format_u64(cold_contour["machine_readable_summary"]["query_slice_count"].as_u64()),
                    ),
                ),
                compare_table_row(
                    "Duration",
                    "Сколько длился полный последний cold benchmark.",
                    format_seconds_compare_pair(
                        snapshot,
                        cold_contour["profile"]["max_duration_seconds"].as_f64(),
                        cold_contour["machine_readable_summary"]["duration"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "Leakage",
                    "Сколько cross-project утечек поймал cold benchmark. Для строгой изоляции здесь должно оставаться ровно 0.",
                    compare_pair(
                        format_threshold_value(
                            cold_contour["profile"]["max_leakage"].as_f64(),
                            "=",
                            "",
                            0,
                        ),
                        format_u64(cold_contour["machine_readable_summary"]["leakage"].as_u64()),
                    ),
                ),
                compare_table_row(
                    "Error rate",
                    "Доля ошибок в последнем полном cold benchmark.",
                    compare_pair(
                        format_zero_or_at_most_percent(
                            cold_contour["profile"]["max_error_rate"]
                                .as_f64()
                                .map(|value| value * 100.0),
                        ),
                        format_percent(cold_contour["machine_readable_summary"]["error_rate"].as_f64()),
                    ),
                ),
            ],
        ),
        with_table_orientation(
            with_extra_class(
                compare_table_card(
                    "Accuracy / Isolation Verification / latest_retrieval_accuracy",
                    "Контур данных: latest_retrieval_accuracy.accuracy_verification. Этот блок не потоковый: он показывает последний сохранённый accuracy/isolation verification contour. Карточка развернута по ширине, чтобы accuracy и isolation читались рядом и не сжимали остальные benchmark-блоки."
                        .to_string(),
                    worst_status(
                        status_for_metric_prefix(snapshot, "accuracy.cross_project_leakage"),
                        worst_status(
                            status_for_metric_prefix(snapshot, "accuracy.symbol_precision"),
                            status_for_metric_prefix(snapshot, "accuracy.semantic_precision"),
                        ),
                    ),
                    Some(source_label(
                        "Источник: benchmark snapshot latest_retrieval_accuracy.accuracy_verification. Live-данные страницы сюда не подмешиваются",
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
                                format_ratio_percent(
                                    thresholds["accuracy"]["symbol_precision"]["target"].as_f64(),
                                ),
                                format_ratio_percent(accuracy["symbol_precision"].as_f64()),
                            ),
                        ),
                        compare_table_row(
                            "Semantic precision",
                            "Насколько точно семантический слой попадает в правильный контекст.",
                            compare_pair(
                                format_ratio_percent(
                                    thresholds["accuracy"]["semantic_precision"]["target"].as_f64(),
                                ),
                                format_ratio_percent(accuracy["semantic_precision"].as_f64()),
                            ),
                        ),
                    ],
                ),
                "benchmark-span-full",
            ),
            "transposed",
        ),
    ]
}

fn hot_retrieval_benchmark_status(hot_retrieval: &Value, thresholds: &Value) -> &'static str {
    combine_statuses(&[
        status_strict_less_than(
            hot_retrieval["p50_ms"].as_f64(),
            thresholds["retrieval"]["hot_live_table"]["target_p50_ms"].as_f64(),
        ),
        status_strict_less_than(
            hot_retrieval["p95_ms"].as_f64(),
            thresholds["retrieval"]["hot_live_table"]["target_p95_ms"].as_f64(),
        ),
        status_strict_less_than(
            hot_retrieval["p99_ms"].as_f64(),
            thresholds["retrieval"]["hot_live_table"]["target_p99_ms"].as_f64(),
        ),
        status_strict_less_than(
            hot_retrieval["max_ms"].as_f64(),
            thresholds["retrieval"]["hot_live_table"]["target_max_ms"].as_f64(),
        ),
        status_at_least_or_equal(
            hot_retrieval["iterations"].as_f64(),
            thresholds["retrieval"]["hot_benchmark_table"]["target_iterations"].as_f64(),
        ),
        status_at_least_or_equal(
            hot_retrieval["warmup"].as_f64(),
            thresholds["retrieval"]["hot_benchmark_table"]["target_warmup"].as_f64(),
        ),
    ])
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
    let session_raw_baseline = current_session["total_naive_tokens"]
        .as_u64()
        .or_else(|| current_session["baseline_tokens"].as_u64());
    let session_raw_delivered = current_session["total_context_tokens"]
        .as_u64()
        .or_else(|| current_session["delivered_tokens"].as_u64());
    let session_raw_percent = current_session["effective_savings_pct"].as_f64();
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
        card_with_rows(
            "Экономия токенов за текущую сессию",
            format_signed_count(session_saved),
            if session_events > 0 {
                format!(
                    "Сессия здесь = непрерывная работа без паузы дольше 30 минут. Новая сессия начинается после паузы дольше 30 минут. Длительность: {}. Строго verified live для этого KPI: {} из {} событий текущей сессии. Проверенная реальная экономия по ним: {}. {}",
                    elapsed_since_epoch_label(session_started, session_ended),
                    format_u64(Some(session_events)),
                    format_u64(Some(session_events_total)),
                    format_percent(session_percent),
                    recovery_sentence(session_recovery)
                ) + &format!(
                    " До более строгого полезного ответа без лишнего доуточнения уже дотянулись: {} событий, {} от всей выборки, экономия по ним: {}.",
                    format_u64(Some(session_answer_count)),
                    format_percent(session_answer_rate),
                    format_percent(session_answer_percent)
                ) + &format!(" {}", verified_vs_excluded_sentence(current_session))
                    + &format!(
                        " {} {}",
                        raw_savings_sentence(
                            session_raw_baseline,
                            session_raw_delivered,
                            session_raw_percent
                        ),
                        client_budget_disclaimer()
                    )
            } else if session_events_total > 0 {
                format!(
                    "В этой сессии уже есть Amai-запросы: {}. Но строго verified live для главного KPI пока: 0 из {}. Поэтому главный KPI по сессии ещё не накоплен.",
                    format_u64(Some(session_events_total)),
                    format_u64(Some(session_events_total)),
                ) + &format!(
                    " {} {}",
                    raw_savings_sentence(
                        session_raw_baseline,
                        session_raw_delivered,
                        session_raw_percent
                    ),
                    client_budget_disclaimer()
                )
            } else {
                "В текущей непрерывной сессии Amai ещё не накопил ни одного учтённого запроса, поэтому реальную экономию пока рано показывать.".to_string()
            },
            savings_status(session_saved, session_events, session_events_total),
            None,
            Some("Эта карточка отвечает на вопрос, сколько токенов Amai сэкономил в текущей непрерывной сессии работы. Сессия здесь начинается заново после паузы дольше 30 минут и считает только strict verified live retrieval-события без потери качества. Raw contour ниже показан отдельно только для объяснения и не является тем же KPI.".to_string()),
            current_session_lane_rows(current_session),
        ),
        card_with_tooltip(
            "Экономия токенов за рабочее окно",
            format_signed_count(rolling_saved),
            if rolling_events > 0 {
                format!(
                    "Это текущее скользящее окно профиля {}. Период: {}. Строго verified live для этого KPI: {} из {} событий окна. Проверенная реальная экономия: {}. {}",
                    rolling_window_label,
                    elapsed_since_epoch_label(rolling_started, rolling_ended),
                    format_u64(Some(rolling_events)),
                    format_u64(Some(rolling_events_total)),
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
                    "В текущем рабочем окне уже есть Amai-запросы: {}. Но строго verified live для главного KPI пока: 0 из {}. Поэтому реальную экономию за окно пока рано считать устойчивой.",
                    format_u64(Some(rolling_events_total)),
                    format_u64(Some(rolling_events_total))
                )
            } else {
                "В текущем рабочем окне Amai ещё не накопил учтённых запросов, поэтому здесь пока нет живой verified статистики.".to_string()
            },
            savings_status(rolling_saved, rolling_events, rolling_events_total),
            Some(format!("Эта карточка показывает не одну сессию, а текущее скользящее рабочее окно профиля {}. Окно может захватывать несколько сессий подряд и нужно для недавнего тренда, а не только для последнего непрерывного захода. В главный KPI здесь тоже попадают только strict verified live retrieval-события без потери качества.", rolling_window_label)),
        ),
        card_with_tooltip(
            "Экономия токенов за всё время записи",
            format_signed_count(lifetime_saved),
            if lifetime_events > 0 {
                format!(
                    "Это итог с первого записанного Amai retrieval-события в этой установке. Период: {}. Строго verified live для этого KPI: {} из {} событий всей истории записи. Проверенная реальная экономия: {}. {}",
                    elapsed_since_epoch_label(lifetime_started, lifetime_ended),
                    format_u64(Some(lifetime_events)),
                    format_u64(Some(lifetime_events_total)),
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
                    "После установки уже накоплены Amai-запросы: {}. Но строго verified live для главного KPI пока: 0 из {}. Поэтому главный KPI пока не считается надёжным.",
                    format_u64(Some(lifetime_events_total)),
                    format_u64(Some(lifetime_events_total))
                )
            } else {
                "После установки Amai ещё не накопил учтённых запросов, поэтому здесь пока нет итоговой живой статистики.".to_string()
            },
            savings_status(lifetime_saved, lifetime_events, lifetime_events_total),
            Some("Эта карточка показывает накопительный итог с первого записанного retrieval-события в текущей установке Amai. Это не процент от лимита чата и не вся история всех внешних клиентов навсегда. В главный lifetime KPI попадают только strict verified live retrieval-события без потери качества; benchmark, proof и другой инженерный трафик в него не смешиваются.".to_string()),
        ),
    ]
}

fn build_machine_cards(
    snapshot: &Value,
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
    cards.push(with_extra_class(
        artifact_cleanup_card(snapshot),
        "machine-compact",
    ));
    cards
}

fn artifact_cleanup_card(snapshot: &Value) -> Value {
    let cleanup = &snapshot["artifact_cleanup"];
    if !cleanup.is_object() || cleanup["status"].as_str().is_some() {
        return card_with_rows(
            "Локальный мусор и retention",
            "ещё нет данных".to_string(),
            "Policy-driven cleanup для rebuildable хвоста ещё не успел записать последний summary.".to_string(),
            "unknown",
            Some("Источник: state/tooling/artifact_cleanup/latest.json".to_string()),
            Some("Этот блок показывает только rebuildable локальный хвост Amai. Live state и исторические данные сервисов сюда не входят.".to_string()),
            vec![],
        );
    }

    let safe_reclaimable_bytes = cleanup["selected_reclaimable_bytes"].as_u64().unwrap_or(0);
    let safe_selected = cleanup["selected"].as_u64().unwrap_or(0);
    let safe_expired = cleanup["expired"].as_u64().unwrap_or(0);
    let aggressive_reclaimable_bytes = cleanup["aggressive_preview_reclaimable_bytes"]
        .as_u64()
        .unwrap_or(safe_reclaimable_bytes);
    let aggressive_selected = cleanup["aggressive_preview_selected"]
        .as_u64()
        .unwrap_or(safe_selected);
    let captured_at_epoch_ms = cleanup["captured_at_epoch_ms"].as_u64();
    let kept_latest = cleanup["kept_latest"].as_u64().unwrap_or(0);
    let protected = cleanup["protected"].as_u64().unwrap_or(0);
    let targets_scanned = cleanup["targets_scanned"].as_u64().unwrap_or(0);
    let last_apply = &cleanup["last_apply"];
    let last_reclaim_bytes = last_apply["reclaimed_bytes"].as_u64().unwrap_or(0);
    let last_deleted = last_apply["deleted"].as_u64().unwrap_or(0);
    let last_apply_mode = last_apply["mode"].as_str().unwrap_or("conservative");
    let last_apply_at = last_apply["captured_at_epoch_ms"].as_u64();

    let value = if aggressive_reclaimable_bytes > 0 {
        format!(
            "{} preview",
            human_bytes(aggressive_reclaimable_bytes as f64)
        )
    } else if safe_reclaimable_bytes > 0 {
        format!("{} safe", human_bytes(safe_reclaimable_bytes as f64))
    } else {
        "по policy чисто".to_string()
    };
    let mut note = format!(
        "Safe policy чистит только то, что уже aged past TTL и не попадает под keep-latest. Aggressive preview показывает, сколько rebuildable хвоста можно убрать сразу, не трогая live state. Последний sweep: {}.",
        captured_at_epoch_ms
            .map(human_timestamp)
            .unwrap_or_else(|| "ещё нет данных".to_string())
    );
    if last_reclaim_bytes > 0 {
        let last_apply_label = last_apply_at
            .map(human_timestamp)
            .unwrap_or_else(|| "неизвестно когда".to_string());
        note.push_str(&format!(
            " Последний apply-run уже вернул {} ({last_deleted} entries, mode={last_apply_mode}) в {last_apply_label}.",
            human_bytes(last_reclaim_bytes as f64)
        ));
    }

    card_with_rows(
        "Локальный мусор и retention",
        value,
        note,
        artifact_cleanup_status(snapshot),
        Some("Источник: state/tooling/artifact_cleanup/latest.json".to_string()),
        Some("Это локальный hygiene contour для build/cache хвостов Amai. Он не удаляет state PostgreSQL, Qdrant, MinIO или NATS.".to_string()),
        vec![
            metric_row(
                "Safe reclaim now",
                human_bytes(safe_reclaimable_bytes as f64),
                Some("Сколько места можно вернуть прямо сейчас, не нарушая TTL и keep-latest policy."),
            ),
            metric_row(
                "Aggressive preview",
                human_bytes(aggressive_reclaimable_bytes as f64),
                Some("Сколько rebuildable хвоста можно убрать сразу explicit aggressive path-ом, не трогая live state."),
            ),
            metric_row(
                "Last reclaim",
                if last_reclaim_bytes > 0 {
                    format!(
                        "{} ({last_deleted}, {last_apply_mode})",
                        human_bytes(last_reclaim_bytes as f64)
                    )
                } else {
                    "ещё не было".to_string()
                },
                Some("Сколько места вернул последний apply-run cleanup policy и в каком режиме он был выполнен."),
            ),
            metric_row(
                "Safe кандидаты",
                safe_selected.to_string(),
                Some("Сколько отдельных entries уже попали под текущую conservative policy."),
            ),
            metric_row(
                "Aggressive кандидаты",
                aggressive_selected.to_string(),
                Some("Сколько отдельных entries можно было бы убрать explicit aggressive path-ом прямо сейчас."),
            ),
            metric_row(
                "TTL already expired",
                safe_expired.to_string(),
                Some("Сколько entries уже aged past TTL, даже если limit сейчас не даёт выбрать их все."),
            ),
            metric_row(
                "Keep latest / protected",
                format!("{kept_latest} / {protected}"),
                Some("Что policy сейчас удерживает: недавние entries по keep-latest и активные защищённые paths."),
            ),
            metric_row(
                "Targets scanned",
                targets_scanned.to_string(),
                Some("Сколько cleanup-target directories сейчас участвует в policy-driven контуре."),
            ),
        ],
    )
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
            Some(
                "Полный объём видеопамяти или локальной памяти ускорителя, если provider дал это поле.",
            ),
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
            Some(
                "Текущее энергопотребление основного ускорителя, если provider умеет его отдавать.",
            ),
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
            format_ms(snapshot, snapshot["postgres"]["query_probe_p95_ms"].as_f64()),
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
                    format_ms(
                        snapshot,
                        snapshot["thresholds"]["postgres"]["query_probe_p95_ms"]["target"]
                            .as_f64(),
                    ),
                    Some("Целевой p95 для короткого живого PostgreSQL probe."),
                ),
                metric_row(
                    "Измерено probe P95",
                    format_ms(snapshot, snapshot["postgres"]["query_probe_p95_ms"].as_f64()),
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
            format_ms(snapshot, snapshot["nats"]["publish_probe_p95_ms"].as_f64()),
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
                    format_ms(
                        snapshot,
                        snapshot["thresholds"]["nats"]["publish_probe_p95_ms"]["target"].as_f64(),
                    ),
                    Some("Целевой p95 для живого publish probe."),
                ),
                metric_row(
                    "Измерено publish P95",
                    format_ms(snapshot, snapshot["nats"]["publish_probe_p95_ms"].as_f64()),
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
    let live_or_snapshot_label = if snapshot_mode {
        "Последний срез"
    } else {
        ""
    };
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
        warnings.push(humanize_check(snapshot, check));
    }
    if let Some(warning) = artifact_cleanup_warning(snapshot) {
        warnings.push(warning);
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
            "term": "Burst QPS",
            "meaning": "Средняя скорость внутри конкретного benchmark-окна. Это не live поток страницы и не обещание стабильной обычной пропускной способности."
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

fn card_with_tooltip(
    title: &str,
    value: String,
    note: String,
    status: &str,
    title_tooltip: Option<String>,
) -> Value {
    card_with_rows(title, value, note, status, None, title_tooltip, Vec::new())
}

fn with_extra_class(mut card: Value, extra_class: &str) -> Value {
    if let Some(object) = card.as_object_mut() {
        object.insert("extra_class".to_string(), Value::from(extra_class));
    }
    card
}

fn with_table_orientation(mut card: Value, table_orientation: &str) -> Value {
    if let Some(object) = card.as_object_mut() {
        object.insert(
            "table_orientation".to_string(),
            Value::from(table_orientation),
        );
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
    let hot_targets = live_latency_table_targets(snapshot, "hot");
    let cold_targets = live_latency_table_targets(snapshot, "cold");
    let hot_assessment = assess_live_latency_slice(hot, &hot_targets);
    let cold_assessment = assess_live_latency_slice(cold, &cold_targets);
    let overall_status =
        combine_live_compare_status(&[hot_assessment.status, cold_assessment.status]);

    json!({
        "kind": "live_compare",
        "title": "Как Amai отвечает на live retrieval сейчас",
        "title_tooltip": "Это живое сравнение только двух retrieval-режимов: повторный запрос по уже прогретому кэшу и первый запрос без прогрева. Continuity, handoff, working-state, observability proof и benchmark-снимки сюда не входят.",
        "status": overall_status,
        "status_label": status_label(overall_status),
        "source_label": "Источник: живая retrieval-выборка текущей сессии из token_budget live lane, обновляется при новых context-pack запросах. Benchmark-данные сюда не подмешиваются.",
        "note": "Сверху показана медиана, то есть обычный уровень ответа в каждом retrieval-режиме. Это не карточка про всю работу проекта: continuity, handoff, observe/proof и другие линии Amai живут отдельно.",
        "metrics": [
            {
                "label": "Повторный запрос",
                "tooltip": "Это уже прогретый путь: пользователь повторяет похожий запрос, а Amai не стартует с пустого места.",
                "value": if hot_has_data {
                    format_ms(snapshot, hot.and_then(|slice| slice["p50_latency_ms"].as_f64()))
                } else {
                    "ещё нет данных".to_string()
                },
                "note": hot_assessment.note
            },
            {
                "label": "Первый запрос",
                "tooltip": "Это первый запрос без fast-cache и без прогрева. Он всегда тяжелее и лучше показывает реальную цену холодного старта.",
                "value": if cold_has_data {
                    format_ms(snapshot, cold.and_then(|slice| slice["p50_latency_ms"].as_f64()))
                } else {
                    "ещё нет данных".to_string()
                },
                "note": cold_assessment.note
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
                    "values": target_values(snapshot, &hot_targets)
                },
                {
                    "label": "Повторный запрос — сейчас",
                    "tooltip": "Текущая живая hot-выборка этой сессии.",
                    "values": compare_values(snapshot, hot, hot_sample_count)
                },
                {
                    "label": "Первый запрос — эталон",
                    "tooltip": "Это фиксированные цели для первого запроса без прогрева. Они не зависят от текущей сессии и всегда должны быть заполнены.",
                    "values": target_values(snapshot, &cold_targets)
                },
                {
                    "label": "Первый запрос — сейчас",
                    "tooltip": "Текущая живая cold-выборка этой сессии.",
                    "values": compare_values(snapshot, cold, cold_sample_count)
                }
            ]
        }
    })
}

fn working_state_live_card(snapshot: &Value) -> Value {
    let restore = &snapshot["latest_working_state_restore"]["working_state_restore"];
    if !restore.is_object() {
        return card_with_rows(
            "На чём Amai реально работает сейчас",
            "ещё нет данных".to_string(),
            "Working-state / continuity lane пока не дал последнего restore snapshot, поэтому панель ещё не может показать текущую рабочую линию.".to_string(),
            "unknown",
            Some("Источник: latest_working_state_restore.working_state_restore".to_string()),
            Some("Этот блок показывает не latency retrieval, а текущую рабочую линию Amai: цель, следующий шаг, последнюю команду и активные файлы.".to_string()),
            vec![],
        );
    }

    let current_goal =
        compact_dashboard_text(restore["current_goal"].as_str(), 72, "ещё нет данных");
    let next_step = compact_dashboard_text(restore["next_step"].as_str(), 108, "ещё нет данных");
    let scope = format!(
        "{} / {} / {}",
        restore["project"]["code"]
            .as_str()
            .unwrap_or("ещё нет данных"),
        restore["namespace"]["code"]
            .as_str()
            .unwrap_or("ещё нет данных"),
        restore["agent_scope"].as_str().unwrap_or("shared"),
    );
    let session_age_ms = restore["session_age_ms"].as_u64().map(|value| value as f64);
    let events_count = restore["events_count"].as_u64();
    let last_command =
        compact_dashboard_text(restore["last_command"].as_str(), 72, "ещё нет данных");
    let last_results = compact_dashboard_text(
        restore["last_results_summary"].as_str(),
        108,
        "ещё нет данных",
    );
    let recent_queries = restore["recent_queries"]
        .as_array()
        .map(|items| items.len() as u64)
        .unwrap_or(0);
    let active_files = restore["active_files"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let active_files_count = active_files.len() as u64;
    let active_files_preview = active_files
        .iter()
        .filter_map(Value::as_str)
        .map(|path| {
            Path::new(path)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(path)
                .to_string()
        })
        .take(3)
        .collect::<Vec<_>>()
        .join(", ");
    let restore_confidence = restore["restore_confidence"]
        .as_str()
        .unwrap_or("preliminary");
    let status = match restore_confidence {
        "high" | "medium" => "pass",
        _ if events_count.unwrap_or(0) > 0 => "alert",
        _ => "unknown",
    };

    card_with_rows(
        "На чём Amai реально работает сейчас",
        current_goal,
        format!(
            "Это working-state / continuity lane Amai, а не latency retrieval. Следующий обязательный шаг: {}.",
            next_step
        ),
        status,
        Some(source_label(
            "Источник: latest_working_state_restore.working_state_restore. Этот блок питается continuity / working-state, а не token_budget live retrieval",
            restore["captured_at_epoch_ms"].as_u64(),
        )),
        Some("Показывает текущую рабочую линию Amai: цель, следующий шаг, последнюю команду и активные файлы. Именно здесь видно реальную проектную работу, которая не обязана проходить через live retrieval.".to_string()),
        vec![
            metric_row(
                "Scope",
                scope,
                Some("Project / namespace / agent scope, внутри которого Amai сейчас держит рабочую линию."),
            ),
            metric_row(
                "Сессия",
                format!(
                    "{} • {} событий",
                    format_ms(snapshot, session_age_ms),
                    format_u64(events_count)
                ),
                Some("Возраст текущей непрерывной рабочей сессии и сколько событий working-state уже вошло в restore snapshot."),
            ),
            metric_row(
                "Последняя команда",
                last_command,
                Some("Какая последняя команда или действие оставили этот working-state след."),
            ),
            metric_row(
                "Последний результат",
                last_results,
                Some("Короткое human-readable summary того, что Amai считает последним materialized результатом."),
            ),
            metric_row(
                "Активные файлы",
                if active_files_preview.is_empty() {
                    format_u64(Some(active_files_count))
                } else {
                    format!("{} • {}", format_u64(Some(active_files_count)), active_files_preview)
                },
                Some("Сколько файлов Amai считает активными сейчас и какие первые несколько он видит в рабочей линии."),
            ),
            metric_row(
                "Recent queries",
                format_u64(Some(recent_queries)),
                Some("Сколько недавних retrieval-запросов вошло в working-state restore. Здесь может быть 0, если работа шла через continuity/proof, а не через live retrieval."),
            ),
        ],
    )
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
                { "label": "Тестовые\nданные", "tooltip": "Фактический результат последнего сохранённого benchmark-прогона." }
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

fn compact_dashboard_text(value: Option<&str>, max_chars: usize, fallback: &str) -> String {
    let text = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let truncated = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    format!("{truncated}…")
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

fn status_at_least_or_equal(current: Option<f64>, target: Option<f64>) -> &'static str {
    match (current, target) {
        (Some(current), Some(target)) if current >= target => "pass",
        (Some(_), Some(_)) => "critical",
        _ => "unknown",
    }
}

fn compare_values(snapshot: &Value, slice: Option<&Value>, sample_count: u64) -> Vec<String> {
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
        format_ms(
            snapshot,
            slice.and_then(|value| value["p50_latency_ms"].as_f64()),
        ),
        format_ms(
            snapshot,
            slice.and_then(|value| value["p95_latency_ms"].as_f64()),
        ),
        format_ms(
            snapshot,
            slice.and_then(|value| value["p99_latency_ms"].as_f64()),
        ),
        format_ms(
            snapshot,
            slice.and_then(|value| value["max_latency_ms"].as_f64()),
        ),
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

struct LiveLatencySliceAssessment {
    status: &'static str,
    note: String,
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

fn target_values(snapshot: &Value, targets: &LiveLatencyTableTargets) -> Vec<String> {
    vec![
        format_time_threshold(snapshot, Some(targets.p50_ms), "<"),
        format_time_threshold(snapshot, Some(targets.p95_ms), "<"),
        format_time_threshold(snapshot, Some(targets.p99_ms), "<"),
        format_time_threshold(snapshot, Some(targets.max_ms), "<"),
        format_target_u64(">", targets.sample_count),
    ]
}

fn status_label(status: &str) -> &'static str {
    match status {
        "pass" => "в норме",
        "alert" => "внимание",
        "critical" => "критично",
        "waiting" => "ждём verified-выборку",
        _ => "нет данных",
    }
}

fn headline_status_label(status: &str) -> &'static str {
    match status {
        "pass" => "система в норме",
        "alert" => "нужно внимание",
        "critical" => "есть критичные сигналы",
        "waiting" => "идёт накопление выборки",
        _ => "данных пока мало",
    }
}

fn headline_status_reason(
    pass: u64,
    alert: u64,
    critical: u64,
    unknown: u64,
    live_status: &str,
) -> String {
    let mut base = if critical > 0 {
        format!("Критичных SLA-проверок: {critical}. Предупреждений: {alert}.")
    } else if alert > 0 {
        format!("SLA-предупреждений: {alert}. Критичных SLA-проверок нет.")
    } else if unknown > 0 {
        format!("Неопределённых SLA-проверок: {unknown}. Остальные зелёные: {pass}.")
    } else {
        format!("Все SLA-проверки зелёные: {pass}.")
    };

    match live_status {
        "critical" => {
            base.push_str(" Живой пользовательский поток сейчас в критичном состоянии.");
        }
        "alert" => {
            base.push_str(" Живой пользовательский поток сейчас требует внимания.");
        }
        "unknown" => {
            base.push_str(" По живому пользовательскому потоку пока недостаточно данных.");
        }
        _ => {}
    }

    base
}

fn assess_live_latency_slice(
    slice: Option<&Value>,
    targets: &LiveLatencyTableTargets,
) -> LiveLatencySliceAssessment {
    let Some(slice) = slice else {
        return LiveLatencySliceAssessment {
            status: "unknown",
            note: "В этой сессии ещё не накопилась живая выборка для этого режима.".to_string(),
        };
    };

    let sample_count = slice["sample_count"].as_u64().unwrap_or_default();
    if sample_count == 0 {
        return LiveLatencySliceAssessment {
            status: "unknown",
            note: "В этой сессии ещё не накопилась живая выборка для этого режима.".to_string(),
        };
    }

    let metrics = [
        ("P50", slice["p50_latency_ms"].as_f64(), targets.p50_ms),
        ("P95", slice["p95_latency_ms"].as_f64(), targets.p95_ms),
        ("P99", slice["p99_latency_ms"].as_f64(), targets.p99_ms),
        ("Max", slice["max_latency_ms"].as_f64(), targets.max_ms),
    ];

    let missing_metrics = metrics
        .iter()
        .filter_map(|(label, value, _)| value.is_none().then_some(*label))
        .collect::<Vec<_>>();
    if !missing_metrics.is_empty() {
        return LiveLatencySliceAssessment {
            status: "unknown",
            note: format!(
                "Часть живых значений ещё не собрана: {}.",
                missing_metrics.join(", ")
            ),
        };
    }

    let failed_metrics = metrics
        .iter()
        .filter_map(|(label, value, target)| {
            (!value.is_some_and(|value| value < *target)).then_some(*label)
        })
        .collect::<Vec<_>>();
    let sample_ok = sample_count > targets.sample_count;

    if !sample_ok {
        return LiveLatencySliceAssessment {
            status: "alert",
            note: if failed_metrics.is_empty() {
                format!(
                    "По задержке всё хорошо, но выборка ещё мала: {} из > {}.",
                    format_u64(Some(sample_count)),
                    format_u64(Some(targets.sample_count))
                )
            } else {
                format!(
                    "Пока рано считать режим зелёным: выборка ещё мала ({} из > {}), а текущие значения ещё не лучше эталона по {}.",
                    format_u64(Some(sample_count)),
                    format_u64(Some(targets.sample_count)),
                    failed_metrics.join(", ")
                )
            },
        };
    }

    if !failed_metrics.is_empty() {
        return LiveLatencySliceAssessment {
            status: "critical",
            note: format!(
                "Эталон уже честно не выполняется по {}. Живая выборка: {}.",
                failed_metrics.join(", "),
                format_u64(Some(sample_count))
            ),
        };
    }

    LiveLatencySliceAssessment {
        status: "pass",
        note: format!(
            "Эталон выдержан. Живая выборка: {}.",
            format_u64(Some(sample_count))
        ),
    }
}

fn live_latency_compare_status(snapshot: &Value) -> &'static str {
    let hot_targets = live_latency_table_targets(snapshot, "hot");
    let cold_targets = live_latency_table_targets(snapshot, "cold");
    let hot_status = assess_live_latency_slice(latency_slice(snapshot, "hot"), &hot_targets).status;
    let cold_status =
        assess_live_latency_slice(latency_slice(snapshot, "cold"), &cold_targets).status;
    combine_live_compare_status(&[hot_status, cold_status])
}

fn combine_live_compare_status(statuses: &[&str]) -> &'static str {
    if statuses.contains(&"critical") {
        return "critical";
    }
    if statuses.contains(&"alert") {
        return "alert";
    }
    if statuses.iter().all(|status| *status == "pass") {
        return "pass";
    }
    "unknown"
}

fn combine_headline_statuses(sla_status: &str, live_status: &str) -> &'static str {
    match live_status {
        "critical" => "critical",
        "alert" => {
            if sla_status == "critical" {
                "critical"
            } else {
                "alert"
            }
        }
        _ => match sla_status {
            "pass" => "pass",
            "alert" => "alert",
            "critical" => "critical",
            _ => "unknown",
        },
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
            "waiting"
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

fn current_session_lane_rows(summary: &Value) -> Vec<Value> {
    vec![
        metric_row(
            "Verified lane",
            token_lane_summary(
                summary["verified_baseline_tokens"].as_u64(),
                summary["verified_delivered_tokens"].as_u64(),
                summary["verified_recovery_tokens"].as_u64(),
                summary["verified_effective_saved_tokens"].as_i64(),
            ),
            Some("Главный KPI этой карточки: только verified live-события без потери качества."),
        ),
        metric_row(
            "Raw live lane",
            token_lane_summary(
                summary["total_naive_tokens"].as_u64(),
                summary["total_context_tokens"].as_u64(),
                summary["total_recovery_tokens"].as_u64(),
                summary["total_effective_saved_tokens"].as_i64(),
            ),
            Some(
                "Весь live contour без quality gate. Нужен как explain-lane, а не как главный KPI.",
            ),
        ),
        metric_row(
            "Outside verified",
            format!(
                "{} событий, delta {}",
                summary["excluded_events_count"].as_u64().unwrap_or(0),
                format_signed_count(summary["excluded_effective_saved_tokens"].as_i64())
            ),
            Some(
                "События, которые пока не вошли в verified KPI: им ещё не хватило quality gate или они synthetic/debug.",
            ),
        ),
    ]
}

fn raw_savings_sentence(
    baseline_tokens: Option<u64>,
    delivered_tokens: Option<u64>,
    savings_percent: Option<f64>,
) -> String {
    match (baseline_tokens, delivered_tokens) {
        (Some(baseline), Some(delivered)) => format!(
            "Сырой live contour пока показывает baseline {} токенов против реально доставленных {}{}.",
            format_u64(Some(baseline)),
            format_u64(Some(delivered)),
            savings_percent
                .map(|value| format!(", preliminary delta {}", format_percent(Some(value))))
                .unwrap_or_default()
        ),
        _ => {
            "Сырой live contour для этой сессии ещё не накопил понятную пару baseline vs delivered."
                .to_string()
        }
    }
}

fn verified_vs_excluded_sentence(summary: &Value) -> String {
    let verified_events = summary["counted_events"].as_u64().unwrap_or(0);
    let verified_baseline = summary["verified_baseline_tokens"].as_u64();
    let verified_delivered = summary["verified_delivered_tokens"].as_u64();
    let verified_recovery = summary["verified_recovery_tokens"].as_u64();
    let verified_saved = summary["verified_effective_saved_tokens"].as_i64();
    let excluded_events = summary["excluded_events_count"].as_u64().unwrap_or(0);
    let excluded_baseline = summary["excluded_baseline_tokens"].as_u64();
    let excluded_delivered = summary["excluded_delivered_tokens"].as_u64();
    let excluded_recovery = summary["excluded_recovery_tokens"].as_u64();
    let excluded_saved = summary["excluded_effective_saved_tokens"].as_i64();

    let verified_lane = match (verified_baseline, verified_delivered, verified_recovery) {
        (Some(baseline), Some(delivered), Some(recovery)) => format!(
            "Главный verified KPI сейчас считает только {} quality-gated событий: baseline {}, delivered {}, recovery {}, честный delta {}.",
            format_u64(Some(verified_events)),
            format_u64(Some(baseline)),
            format_u64(Some(delivered)),
            format_u64(Some(recovery)),
            format_signed_count(verified_saved)
        ),
        _ => format!(
            "Главный verified KPI сейчас считает только {} quality-gated событий.",
            format_u64(Some(verified_events))
        ),
    };
    if excluded_events == 0 {
        return verified_lane;
    }
    let excluded_lane = match (excluded_baseline, excluded_delivered, excluded_recovery) {
        (Some(baseline), Some(delivered), Some(recovery)) => format!(
            "Вне verified KPI пока остаются {} событий: baseline {}, delivered {}, recovery {}, delta {}.",
            format_u64(Some(excluded_events)),
            format_u64(Some(baseline)),
            format_u64(Some(delivered)),
            format_u64(Some(recovery)),
            format_signed_count(excluded_saved)
        ),
        _ => format!(
            "Вне verified KPI пока остаются {} событий.",
            format_u64(Some(excluded_events))
        ),
    };
    format!("{verified_lane} {excluded_lane}")
}

fn client_budget_disclaimer() -> &'static str {
    "Это не процент от лимита клиента/чата: карточка считает только retrieval payload Amai против baseline, а не все токены thread-а."
}

fn token_lane_summary(
    baseline_tokens: Option<u64>,
    delivered_tokens: Option<u64>,
    recovery_tokens: Option<u64>,
    delta_tokens: Option<i64>,
) -> String {
    match (baseline_tokens, delivered_tokens, recovery_tokens) {
        (Some(baseline), Some(delivered), Some(recovery)) => format!(
            "{} -> {} -> {} => {}",
            format_u64(Some(baseline)),
            format_u64(Some(delivered)),
            format_u64(Some(recovery)),
            format_signed_count(delta_tokens)
        ),
        _ => "ещё нет данных".to_string(),
    }
}

fn artifact_cleanup_status(snapshot: &Value) -> &'static str {
    let cleanup = &snapshot["artifact_cleanup"];
    if !cleanup.is_object() || cleanup["status"].as_str().is_some() {
        return "unknown";
    }
    if cleanup["selected"].as_u64().unwrap_or(0) > 0 {
        "alert"
    } else if cleanup["aggressive_preview_selected"].as_u64().unwrap_or(0) > 0 {
        "alert"
    } else {
        "pass"
    }
}

fn artifact_cleanup_warning(snapshot: &Value) -> Option<String> {
    let cleanup = &snapshot["artifact_cleanup"];
    if !cleanup.is_object() || cleanup["status"].as_str().is_some() {
        return None;
    }
    let safe_bytes = cleanup["selected_reclaimable_bytes"].as_u64().unwrap_or(0);
    let aggressive_bytes = cleanup["aggressive_preview_reclaimable_bytes"]
        .as_u64()
        .unwrap_or(0);
    if safe_bytes > 0 {
        return Some(format!(
            "Локальный rebuildable хвост уже aged past TTL: safe reclaim сейчас {}. Это не live state и его можно убрать policy-cleanup path-ом.",
            human_bytes(safe_bytes as f64)
        ));
    }
    if aggressive_bytes > 0 {
        return Some(format!(
            "Локальный rebuildable хвост ещё не дожил до TTL, но aggressive reclaim path уже мог бы вернуть {} без удаления live state. Safe policy сейчас специально ждёт возрастной запас.",
            human_bytes(aggressive_bytes as f64)
        ));
    }
    None
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

fn humanize_check(snapshot: &Value, check: &Value) -> String {
    let metric = check["metric"].as_str().unwrap_or("unknown.metric");
    let status = status_label(check["status"].as_str().unwrap_or("unknown"));
    let value = match check["value"].as_f64() {
        Some(number) if metric.ends_with("_ratio") => format!("{:.2}%", number * 100.0),
        Some(number) if metric.ends_with("_ms") => format_ms(snapshot, Some(number)),
        Some(number) if metric.ends_with("_seconds") => format_seconds(snapshot, Some(number)),
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
        "load.hot_qps" => "Горячий быстрый путь держит меньше Burst QPS, чем обещано.",
        "load.hot_p50_ms" => "Обычная hot-задержка в benchmark-прогоне стала выше целевой планки.",
        "load.hot_p95_ms" => "Тяжёлый хвост hot benchmark стал выше обещанной границы.",
        "load.hot_p99_ms" => "Редкие тяжёлые выбросы в hot benchmark стали слишком большими.",
        "load.hot_max_ms" => "Самый тяжёлый запрос в hot benchmark вышел за безопасную границу.",
        "load.hot_error_rate" => "Под нагрузкой появились ошибки на быстром пути.",
        "observability.benchmark_contamination" => {
            "В benchmark-витрину подмешался live-context или другой неподходящий source."
        }
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

#[derive(Debug, Clone, Copy)]
struct DashboardTimingFormat<'a> {
    switch_to_nanoseconds_below_ms: f64,
    switch_to_microseconds_below_ms: f64,
    switch_to_seconds_at_or_above_ms: f64,
    non_positive_floor_label: &'a str,
    seconds_suffix: &'a str,
    milliseconds_suffix: &'a str,
    microseconds_suffix: &'a str,
    nanoseconds_suffix: &'a str,
    seconds_decimals: usize,
    milliseconds_decimals: usize,
    microseconds_decimals: usize,
    nanoseconds_decimals: usize,
}

#[derive(Debug, Clone, Copy)]
enum DurationDisplayUnit {
    Seconds,
    Milliseconds,
    Microseconds,
    Nanoseconds,
}

fn format_ms(snapshot: &Value, value: Option<f64>) -> String {
    format_duration_ms(dashboard_timing_format(snapshot), value)
}

fn format_seconds(snapshot: &Value, value: Option<f64>) -> String {
    format_duration_ms(
        dashboard_timing_format(snapshot),
        value.map(|number| number * 1000.0),
    )
}

fn format_duration_ms(policy: DashboardTimingFormat<'_>, value: Option<f64>) -> String {
    render_duration_ms_with_unit(policy, value, None)
}

fn render_duration_ms_with_unit(
    policy: DashboardTimingFormat<'_>,
    value: Option<f64>,
    unit: Option<DurationDisplayUnit>,
) -> String {
    match value {
        Some(number) if number <= 0.0 => policy.non_positive_floor_label.to_string(),
        Some(number) => {
            let display_unit = unit.unwrap_or_else(|| auto_duration_display_unit(policy, number));
            let (scaled, decimals, suffix) = match display_unit {
                DurationDisplayUnit::Seconds => (
                    number / 1000.0,
                    policy.seconds_decimals,
                    policy.seconds_suffix,
                ),
                DurationDisplayUnit::Milliseconds => (
                    number,
                    policy.milliseconds_decimals,
                    policy.milliseconds_suffix,
                ),
                DurationDisplayUnit::Microseconds => (
                    number * 1000.0,
                    policy.microseconds_decimals,
                    policy.microseconds_suffix,
                ),
                DurationDisplayUnit::Nanoseconds => (
                    number * 1_000_000.0,
                    policy.nanoseconds_decimals,
                    policy.nanoseconds_suffix,
                ),
            };
            format!("{} {}", format_decimal_trimmed(scaled, decimals), suffix)
        }
        None => "ещё нет данных".to_string(),
    }
}

fn auto_duration_display_unit(
    policy: DashboardTimingFormat<'_>,
    value_ms: f64,
) -> DurationDisplayUnit {
    if value_ms >= policy.switch_to_seconds_at_or_above_ms {
        DurationDisplayUnit::Seconds
    } else if value_ms < policy.switch_to_nanoseconds_below_ms {
        DurationDisplayUnit::Nanoseconds
    } else if value_ms < policy.switch_to_microseconds_below_ms {
        DurationDisplayUnit::Microseconds
    } else {
        DurationDisplayUnit::Milliseconds
    }
}

fn compare_duration_display_unit(
    policy: DashboardTimingFormat<'_>,
    left_ms: Option<f64>,
    right_ms: Option<f64>,
) -> Option<DurationDisplayUnit> {
    [left_ms, right_ms]
        .into_iter()
        .flatten()
        .filter(|value| *value > 0.0)
        .reduce(f64::max)
        .map(|value| auto_duration_display_unit(policy, value))
}

fn dashboard_timing_format(snapshot: &Value) -> DashboardTimingFormat<'_> {
    let timing = &snapshot["thresholds"]["dashboard"]["timing_format"];
    DashboardTimingFormat {
        switch_to_nanoseconds_below_ms: timing["switch_to_nanoseconds_below_ms"]
            .as_f64()
            .expect("dashboard timing policy missing switch_to_nanoseconds_below_ms"),
        switch_to_microseconds_below_ms: timing["switch_to_microseconds_below_ms"]
            .as_f64()
            .expect("dashboard timing policy missing switch_to_microseconds_below_ms"),
        switch_to_seconds_at_or_above_ms: timing["switch_to_seconds_at_or_above_ms"]
            .as_f64()
            .expect("dashboard timing policy missing switch_to_seconds_at_or_above_ms"),
        non_positive_floor_label: timing["non_positive_floor_label"]
            .as_str()
            .expect("dashboard timing policy missing non_positive_floor_label"),
        seconds_suffix: timing["seconds_suffix"]
            .as_str()
            .expect("dashboard timing policy missing seconds_suffix"),
        milliseconds_suffix: timing["milliseconds_suffix"]
            .as_str()
            .expect("dashboard timing policy missing milliseconds_suffix"),
        microseconds_suffix: timing["microseconds_suffix"]
            .as_str()
            .expect("dashboard timing policy missing microseconds_suffix"),
        nanoseconds_suffix: timing["nanoseconds_suffix"]
            .as_str()
            .expect("dashboard timing policy missing nanoseconds_suffix"),
        seconds_decimals: timing["seconds_decimals"]
            .as_u64()
            .expect("dashboard timing policy missing seconds_decimals")
            as usize,
        milliseconds_decimals: timing["milliseconds_decimals"]
            .as_u64()
            .expect("dashboard timing policy missing milliseconds_decimals")
            as usize,
        microseconds_decimals: timing["microseconds_decimals"]
            .as_u64()
            .expect("dashboard timing policy missing microseconds_decimals")
            as usize,
        nanoseconds_decimals: timing["nanoseconds_decimals"]
            .as_u64()
            .expect("dashboard timing policy missing nanoseconds_decimals")
            as usize,
    }
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

fn format_threshold_at_least(value: Option<f64>, unit: &str, decimals: usize) -> String {
    format_threshold_value(value, ">", unit, decimals)
}

fn format_threshold_at_least_or_equal(value: Option<f64>, unit: &str, decimals: usize) -> String {
    format_threshold_value(value, ">=", unit, decimals)
}

fn format_zero_or_at_most_percent(value: Option<f64>) -> String {
    match value {
        Some(number) if number.abs() < f64::EPSILON => {
            format_threshold_value(Some(number), "=", "%", 2)
        }
        Some(number) => format_threshold_value(Some(number), "<=", "%", 2),
        None => "ещё нет данных".to_string(),
    }
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

fn format_time_threshold(snapshot: &Value, value: Option<f64>, operator: &str) -> String {
    format_threshold_rendered(operator, format_ms(snapshot, value))
}

fn format_threshold_rendered(operator: &str, rendered: String) -> String {
    if rendered == "ещё нет данных" {
        rendered
    } else {
        format!("{operator} {rendered}")
    }
}

fn format_decimal(value: f64, decimals: usize) -> String {
    format!("{value:.prec$}", prec = decimals)
}

fn format_decimal_trimmed(value: f64, decimals: usize) -> String {
    let mut rendered = format_decimal(value, decimals);
    while rendered.contains('.') && rendered.ends_with('0') {
        rendered.pop();
    }
    if rendered.ends_with('.') {
        rendered.pop();
    }
    rendered
}

fn format_time_compare_pair(
    snapshot: &Value,
    target_ms: Option<f64>,
    current_ms: Option<f64>,
    operator: &str,
) -> Vec<String> {
    let policy = dashboard_timing_format(snapshot);
    let unit = compare_duration_display_unit(policy, target_ms, current_ms);
    compare_pair(
        format_threshold_rendered(
            operator,
            render_duration_ms_with_unit(policy, target_ms, unit),
        ),
        render_duration_ms_with_unit(policy, current_ms, unit),
    )
}

fn format_seconds_compare_pair(
    snapshot: &Value,
    target_seconds: Option<f64>,
    current_seconds: Option<f64>,
    operator: &str,
) -> Vec<String> {
    format_time_compare_pair(
        snapshot,
        target_seconds.map(|value| value * 1000.0),
        current_seconds.map(|value| value * 1000.0),
        operator,
    )
}

fn format_burst_qps_table(value: Option<f64>) -> String {
    match value {
        Some(number) => format!("{}\nBurst QPS", format_burst_qps_number(number)),
        None => "ещё нет данных".to_string(),
    }
}

fn format_burst_qps_threshold(value: Option<f64>, operator: &str) -> String {
    match value {
        Some(number) => format!("{operator} {}\nBurst QPS", format_burst_qps_number(number)),
        None => "ещё нет данных".to_string(),
    }
}

fn format_burst_qps_number(value: f64) -> String {
    let mut rendered = format!("{value:.2}");
    while rendered.contains('.') && rendered.ends_with('0') {
        rendered.pop();
    }
    if rendered.ends_with('.') {
        rendered.pop();
    }
    rendered
}

fn format_u64(value: Option<u64>) -> String {
    value
        .map(|number| number.to_string())
        .unwrap_or_else(|| "ещё нет данных".to_string())
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
        benchmark_qdrant_live_card, browser_base_url, build_benchmark_cards, build_hero_cards,
        build_machine_cards, build_top_cards, format_ms, format_time_compare_pair,
        human_elapsed_ms, live_latency_compare_card, monitoring_url, worst_status,
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
    fn format_ms_uses_dashboard_timing_policy_from_snapshot() {
        let snapshot = json!({
            "thresholds": {
                "dashboard": {
                    "timing_format": {
                        "switch_to_nanoseconds_below_ms": 0.0005,
                        "switch_to_microseconds_below_ms": 2.0,
                        "switch_to_seconds_at_or_above_ms": 1000.0,
                        "non_positive_floor_label": "below timer floor",
                        "seconds_suffix": "secs",
                        "milliseconds_suffix": "millis",
                        "microseconds_suffix": "micros",
                        "nanoseconds_suffix": "nanos",
                        "seconds_decimals": 2,
                        "milliseconds_decimals": 2,
                        "microseconds_decimals": 1,
                        "nanoseconds_decimals": 0
                    }
                }
            }
        });

        assert_eq!(format_ms(&snapshot, Some(0.0)), "below timer floor");
        assert_eq!(format_ms(&snapshot, Some(0.0004)), "400 nanos");
        assert_eq!(format_ms(&snapshot, Some(0.0015)), "1.5 micros");
        assert_eq!(format_ms(&snapshot, Some(2.3456)), "2.35 millis");
        assert_eq!(format_ms(&snapshot, Some(2345.6)), "2.35 secs");
    }

    #[test]
    fn compare_time_pair_uses_one_row_unit_for_target_and_current() {
        let snapshot = json!({
            "thresholds": {
                "dashboard": {
                    "timing_format": {
                        "switch_to_nanoseconds_below_ms": 0.001,
                        "switch_to_microseconds_below_ms": 1.0,
                        "switch_to_seconds_at_or_above_ms": 1000.0,
                        "non_positive_floor_label": "0 ns",
                        "seconds_suffix": "s",
                        "milliseconds_suffix": "ms",
                        "microseconds_suffix": "µs",
                        "nanoseconds_suffix": "ns",
                        "seconds_decimals": 3,
                        "milliseconds_decimals": 3,
                        "microseconds_decimals": 3,
                        "nanoseconds_decimals": 0
                    }
                }
            }
        });

        assert_eq!(
            format_time_compare_pair(&snapshot, Some(1.0), Some(0.674), "<"),
            vec!["< 1 ms".to_string(), "0.674 ms".to_string()]
        );
        assert_eq!(
            format_time_compare_pair(&snapshot, Some(0.015), Some(0.003226), "<"),
            vec!["< 15 µs".to_string(), "3.226 µs".to_string()]
        );
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

    #[test]
    fn live_compare_card_is_not_green_when_samples_are_missing_or_under_target() {
        let snapshot = json!({
            "thresholds": {
                "dashboard": {
                    "timing_format": {
                        "switch_to_nanoseconds_below_ms": 0.001,
                        "switch_to_microseconds_below_ms": 1.0,
                        "switch_to_seconds_at_or_above_ms": 1000.0,
                        "non_positive_floor_label": "0 ns",
                        "seconds_suffix": "s",
                        "milliseconds_suffix": "ms",
                        "microseconds_suffix": "µs",
                        "nanoseconds_suffix": "ns",
                        "seconds_decimals": 3,
                        "milliseconds_decimals": 3,
                        "microseconds_decimals": 3,
                        "nanoseconds_decimals": 0
                    }
                },
                "retrieval": {
                    "hot_live_table": {
                        "target_p50_ms": 1.0,
                        "target_p95_ms": 1.0,
                        "target_p99_ms": 2.0,
                        "target_max_ms": 5.0,
                        "target_sample_count": 100000
                    },
                    "cold_live_table": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 4.0,
                        "target_p99_ms": 6.0,
                        "target_max_ms": 10.0,
                        "target_sample_count": 10000
                    }
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "latency_slices": [
                            {
                                "state": "cold",
                                "sample_count": 14,
                                "p50_latency_ms": 2.0,
                                "p95_latency_ms": 4.0,
                                "p99_latency_ms": 4.0,
                                "max_latency_ms": 4.0
                            }
                        ]
                    }
                }
            }
        });

        let card = live_latency_compare_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("alert"));
        assert_eq!(card["status_label"].as_str(), Some("внимание"));
        assert!(
            card["metrics"][0]["note"]
                .as_str()
                .unwrap_or_default()
                .contains("ещё не накопилась живая выборка")
        );
        assert!(
            card["metrics"][1]["note"]
                .as_str()
                .unwrap_or_default()
                .contains("Пока рано считать режим зелёным")
        );
    }

    #[test]
    fn live_compare_card_is_green_only_when_both_modes_strictly_pass() {
        let snapshot = json!({
            "thresholds": {
                "dashboard": {
                    "timing_format": {
                        "switch_to_nanoseconds_below_ms": 0.001,
                        "switch_to_microseconds_below_ms": 1.0,
                        "switch_to_seconds_at_or_above_ms": 1000.0,
                        "non_positive_floor_label": "0 ns",
                        "seconds_suffix": "s",
                        "milliseconds_suffix": "ms",
                        "microseconds_suffix": "µs",
                        "nanoseconds_suffix": "ns",
                        "seconds_decimals": 3,
                        "milliseconds_decimals": 3,
                        "microseconds_decimals": 3,
                        "nanoseconds_decimals": 0
                    }
                },
                "retrieval": {
                    "hot_live_table": {
                        "target_p50_ms": 1.0,
                        "target_p95_ms": 1.0,
                        "target_p99_ms": 2.0,
                        "target_max_ms": 5.0,
                        "target_sample_count": 100000
                    },
                    "cold_live_table": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 4.0,
                        "target_p99_ms": 6.0,
                        "target_max_ms": 10.0,
                        "target_sample_count": 10000
                    }
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "latency_slices": [
                            {
                                "state": "hot",
                                "sample_count": 100001,
                                "p50_latency_ms": 0.4,
                                "p95_latency_ms": 0.7,
                                "p99_latency_ms": 1.2,
                                "max_latency_ms": 2.5
                            },
                            {
                                "state": "cold",
                                "sample_count": 10001,
                                "p50_latency_ms": 1.2,
                                "p95_latency_ms": 2.1,
                                "p99_latency_ms": 3.4,
                                "max_latency_ms": 5.2
                            }
                        ]
                    }
                }
            }
        });

        let card = live_latency_compare_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("pass"));
        assert_eq!(card["status_label"].as_str(), Some("в норме"));
    }

    #[test]
    fn top_cards_split_live_retrieval_from_real_workline() {
        let snapshot = json!({
            "thresholds": {
                "dashboard": {
                    "timing_format": {
                        "switch_to_nanoseconds_below_ms": 0.001,
                        "switch_to_microseconds_below_ms": 1.0,
                        "switch_to_seconds_at_or_above_ms": 1000.0,
                        "non_positive_floor_label": "0 ns",
                        "seconds_suffix": "s",
                        "milliseconds_suffix": "ms",
                        "microseconds_suffix": "µs",
                        "nanoseconds_suffix": "ns",
                        "seconds_decimals": 3,
                        "milliseconds_decimals": 3,
                        "microseconds_decimals": 3,
                        "nanoseconds_decimals": 0
                    }
                },
                "retrieval": {
                    "hot_live_table": {
                        "target_p50_ms": 1.0,
                        "target_p95_ms": 1.0,
                        "target_p99_ms": 2.0,
                        "target_max_ms": 5.0,
                        "target_sample_count": 100000
                    },
                    "cold_live_table": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 4.0,
                        "target_p99_ms": 6.0,
                        "target_max_ms": 10.0,
                        "target_sample_count": 10000
                    }
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "latency_slices": [
                            {
                                "state": "hot",
                                "sample_count": 100001,
                                "p50_latency_ms": 0.4,
                                "p95_latency_ms": 0.7,
                                "p99_latency_ms": 1.2,
                                "max_latency_ms": 2.5
                            },
                            {
                                "state": "cold",
                                "sample_count": 10001,
                                "p50_latency_ms": 1.2,
                                "p95_latency_ms": 2.1,
                                "p99_latency_ms": 3.4,
                                "max_latency_ms": 5.2
                            }
                        ]
                    }
                }
            },
            "latest_working_state_restore": {
                "working_state_restore": {
                    "captured_at_epoch_ms": 1774239281880u64,
                    "project": { "code": "art" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "art::continuity::default",
                    "session_age_ms": 15u64,
                    "events_count": 3u64,
                    "current_goal": "Amai observability guardrail proof materialized",
                    "next_step": "Вывести guardrail verdict в dashboard/service layer.",
                    "last_command": "continuity handoff",
                    "last_results_summary": "Зафиксирован handoff для art :: continuity",
                    "active_files": [
                        "/home/art/agent-memory-index/src/observe.rs",
                        "/home/art/agent-memory-index/src/dashboard.rs"
                    ],
                    "recent_queries": [],
                    "restore_confidence": "preliminary"
                }
            }
        });

        let cards = build_top_cards(&snapshot);
        assert_eq!(cards.len(), 2);
        assert_eq!(
            cards[0]["title"].as_str(),
            Some("Как Amai отвечает на live retrieval сейчас")
        );
        assert_eq!(
            cards[1]["title"].as_str(),
            Some("На чём Amai реально работает сейчас")
        );
        assert!(
            cards[1]["note"]
                .as_str()
                .unwrap_or_default()
                .contains("working-state / continuity lane")
        );
    }

    #[test]
    fn current_session_card_explains_raw_savings_vs_client_budget() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "events_total": 2,
                        "counted_events": 0,
                        "verified_effective_saved_tokens": 0,
                        "verified_effective_savings_pct": 0.0,
                        "total_naive_tokens": 920432,
                        "total_context_tokens": 94,
                        "effective_savings_pct": 99.98978740417543
                    },
                    "rolling_window": {},
                    "lifetime": {},
                    "profile": {
                        "display_name": "Обычная рабочая машина"
                    }
                }
            }
        });

        let cards = build_hero_cards(&snapshot);
        let note = cards[0]["note"].as_str().unwrap_or_default();
        assert!(note.contains("baseline"));
        assert!(note.contains("реально доставленных"));
        assert!(note.contains("Это не процент от лимита клиента/чата"));
        let rows = cards[0]["rows"].as_array().expect("rows");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["label"].as_str(), Some("Verified lane"));
        assert_eq!(rows[1]["label"].as_str(), Some("Raw live lane"));
    }

    #[test]
    fn hero_cards_explain_scope_and_strict_verified_fraction() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "events_total": 4,
                        "counted_events": 1,
                        "verified_effective_saved_tokens": 120,
                        "verified_effective_savings_pct": 25.0,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 25.0,
                        "answer_like_counted_events": 1,
                        "verified_answer_like_savings_pct": 25.0,
                        "verified_baseline_tokens": 200,
                        "verified_delivered_tokens": 80,
                        "verified_recovery_tokens": 0,
                        "excluded_events_count": 3,
                        "excluded_effective_saved_tokens": 50,
                        "excluded_baseline_tokens": 400,
                        "excluded_delivered_tokens": 350,
                        "excluded_recovery_tokens": 0,
                        "total_naive_tokens": 600,
                        "total_context_tokens": 430,
                        "effective_savings_pct": 28.33,
                        "total_effective_saved_tokens": 170,
                        "total_recovery_tokens": 0
                    },
                    "rolling_window": {
                        "events_total": 12,
                        "counted_events": 6,
                        "verified_effective_saved_tokens": 38622,
                        "verified_effective_savings_pct": 83.29,
                        "started_at_epoch_ms": 10,
                        "ended_at_epoch_ms": 20,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 33.33,
                        "answer_like_counted_events": 6,
                        "verified_answer_like_savings_pct": 83.29
                    },
                    "lifetime": {
                        "events_total": 56,
                        "counted_events": 22,
                        "verified_effective_saved_tokens": 4824306,
                        "verified_effective_savings_pct": 99.14,
                        "started_at_epoch_ms": 100,
                        "ended_at_epoch_ms": 200,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 39.29,
                        "answer_like_counted_events": 22,
                        "verified_answer_like_savings_pct": 99.14
                    },
                    "profile": {
                        "display_name": "Обычная рабочая машина"
                    }
                }
            }
        });

        let cards = build_hero_cards(&snapshot);
        assert_eq!(cards[0]["status"].as_str(), Some("pass"));
        assert_eq!(
            cards[0]["title_tooltip"].as_str(),
            Some(
                "Эта карточка отвечает на вопрос, сколько токенов Amai сэкономил в текущей непрерывной сессии работы. Сессия здесь начинается заново после паузы дольше 30 минут и считает только strict verified live retrieval-события без потери качества. Raw contour ниже показан отдельно только для объяснения и не является тем же KPI."
            )
        );
        assert!(cards[1]["title_tooltip"].as_str().is_some_and(|value| {
            value.contains("не одну сессию, а текущее скользящее рабочее окно")
        }));
        assert!(cards[2]["title_tooltip"].as_str().is_some_and(|value| {
            value.contains("накопительный итог с первого записанного retrieval-события")
        }));
        assert!(
            cards[1]["note"]
                .as_str()
                .unwrap_or_default()
                .contains("6 из 12")
        );
        assert!(
            cards[2]["note"]
                .as_str()
                .unwrap_or_default()
                .contains("22 из 56")
        );
    }

    #[test]
    fn hero_session_card_uses_waiting_status_before_verified_sample_exists() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "events_total": 1,
                        "counted_events": 0,
                        "verified_effective_saved_tokens": 0,
                        "verified_effective_savings_pct": 0.0,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 0.0,
                        "answer_like_counted_events": 0,
                        "verified_answer_like_savings_pct": 0.0,
                        "excluded_events_count": 1,
                        "excluded_effective_saved_tokens": 243216,
                        "excluded_baseline_tokens": 243300,
                        "excluded_delivered_tokens": 84,
                        "excluded_recovery_tokens": 0,
                        "total_naive_tokens": 243300,
                        "total_context_tokens": 84,
                        "effective_savings_pct": 99.97,
                        "total_effective_saved_tokens": 243216,
                        "total_recovery_tokens": 0
                    },
                    "rolling_window": {
                        "events_total": 0,
                        "counted_events": 0
                    },
                    "lifetime": {
                        "events_total": 0,
                        "counted_events": 0
                    },
                    "profile": {
                        "display_name": "Обычная рабочая машина"
                    }
                }
            }
        });

        let cards = build_hero_cards(&snapshot);
        assert_eq!(cards[0]["status"].as_str(), Some("waiting"));
        assert_eq!(
            cards[0]["status_label"].as_str(),
            Some("ждём verified-выборку")
        );
        assert!(
            cards[0]["note"]
                .as_str()
                .unwrap_or_default()
                .contains("строго verified live для главного KPI пока: 0 из 1")
        );
    }

    #[test]
    fn machine_cards_include_artifact_cleanup_visibility() {
        let snapshot = json!({
            "artifact_cleanup": {
                "captured_at_epoch_ms": 42,
                "selected": 0,
                "selected_reclaimable_bytes": 0,
                "expired": 0,
                "kept_latest": 3,
                "protected": 1,
                "targets_scanned": 7,
                "aggressive_preview_selected": 4,
                "aggressive_preview_reclaimable_bytes": 35_604_527_338u64,
                "last_apply": {
                    "captured_at_epoch_ms": 41,
                    "mode": "aggressive",
                    "deleted": 30,
                    "reclaimed_bytes": 50_424_092_586u64
                }
            }
        });
        let cards = build_machine_cards(&snapshot, None, None);
        let cleanup_card = cards
            .iter()
            .find(|card| card["title"].as_str() == Some("Локальный мусор и retention"))
            .expect("cleanup card");
        assert_eq!(cleanup_card["status"].as_str(), Some("alert"));
        assert_eq!(cleanup_card["rows"][1]["value"].as_str(), Some("33.16 GiB"));
        assert_eq!(
            cleanup_card["rows"][2]["value"].as_str(),
            Some("46.96 GiB (30, aggressive)")
        );
    }

    #[test]
    fn benchmark_cards_name_lanes_explicitly() {
        let snapshot = json!({
            "latest_retrieval_load_hot": {
                "load_verification": {
                    "captured_at_epoch_ms": 1,
                    "project": "project_alpha",
                    "namespace": "review",
                    "query": "alpha_only_token",
                    "execution_mode": "hot_cache_only",
                    "qps": 1224682.0,
                    "p50_ms": 0.007,
                    "p95_ms": 0.010,
                    "p99_ms": 0.015,
                    "max_ms": 0.439,
                    "error_rate": 0.0,
                    "workers": 17,
                    "success_count": 10013,
                    "error_count": 0
                }
            },
            "latest_retrieval_hot": {
                "benchmark": {
                    "captured_at_epoch_ms": 2,
                    "project": "project_alpha",
                    "namespace": "default",
                    "query": "alpha_runtime_summary",
                    "disable_cache": false,
                    "qps": 1661.13,
                    "p50_ms": 0.568,
                    "p95_ms": 0.681,
                    "p99_ms": 1.182,
                    "max_ms": 1.182,
                    "iterations": 20,
                    "warmup": 3
                }
            },
            "latest_cold_path_benchmark": {
                "cold_benchmark": {
                    "captured_at_epoch_ms": 3,
                    "executive_summary": { "verdict": "TARGET MET" },
                    "profile": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 4.0,
                        "target_p99_ms": 6.0,
                        "target_max_ms": 10.0,
                        "min_precision": 0.9,
                        "min_recall": 0.9,
                        "min_target_hit_rate": 0.9,
                        "min_sample_count": 100.0,
                        "min_repo_count": 75.0,
                        "min_query_slice_count": 200.0,
                        "max_duration_seconds": 120.0,
                        "max_leakage": 0.0,
                        "max_error_rate": 0.0
                    },
                    "machine_readable_summary": {
                        "p50": 1.0,
                        "p95": 2.0,
                        "p99": 3.0,
                        "max": 4.0,
                        "precision": 1.0,
                        "recall": 1.0,
                        "hit_rate": 1.0,
                        "sample_count": 1000,
                        "repo_count": 75,
                        "query_slice_count": 200,
                        "duration": 10.0,
                        "leakage": 0,
                        "error_rate": 0.0
                    }
                }
            },
            "latest_retrieval_accuracy": {
                "accuracy_verification": {
                    "captured_at_epoch_ms": 4,
                    "cross_project_leakage": 0.0,
                    "symbol_precision": 1.0,
                    "semantic_precision": 1.0
                }
            },
            "thresholds": {
                "dashboard": {
                    "timing_format": {
                        "switch_to_nanoseconds_below_ms": 0.001,
                        "switch_to_microseconds_below_ms": 1.0,
                        "switch_to_seconds_at_or_above_ms": 1000.0,
                        "non_positive_floor_label": "0 ns",
                        "seconds_suffix": "s",
                        "milliseconds_suffix": "ms",
                        "microseconds_suffix": "µs",
                        "nanoseconds_suffix": "ns",
                        "seconds_decimals": 3,
                        "milliseconds_decimals": 3,
                        "microseconds_decimals": 3,
                        "nanoseconds_decimals": 0
                    }
                },
                "load": {
                    "hot_qps": { "target": 1200000.0 },
                    "hot_error_rate": { "target": 0.0 },
                    "hot_benchmark_table": {
                        "target_p50_ms": 0.012,
                        "target_p95_ms": 0.015,
                        "target_p99_ms": 0.020,
                        "target_max_ms": 0.500,
                        "target_workers": 16.0,
                        "target_sample_count": 10000.0
                    }
                },
                "retrieval": {
                    "hot_live_table": {
                        "target_p50_ms": 1.0,
                        "target_p95_ms": 1.0,
                        "target_p99_ms": 2.0,
                        "target_max_ms": 5.0
                    },
                    "hot_benchmark_table": {
                        "target_iterations": 20.0,
                        "target_warmup": 3.0
                    }
                },
                "accuracy": {
                    "symbol_precision": { "target": 0.99 },
                    "semantic_precision": { "target": 0.98 }
                }
            },
            "sla": {
                "checks": [
                    { "metric": "accuracy.cross_project_leakage", "status": "pass" },
                    { "metric": "accuracy.symbol_precision", "status": "pass" },
                    { "metric": "accuracy.semantic_precision", "status": "pass" }
                ]
            }
        });

        let cards = build_benchmark_cards(&snapshot);
        assert_eq!(
            cards[0]["title"].as_str(),
            Some("Hot Load Benchmark / latest_retrieval_load_hot")
        );
        assert_eq!(
            cards[1]["title"].as_str(),
            Some("Hot Retrieval Benchmark / latest_retrieval_hot")
        );
        assert!(
            cards[0]["note"]
                .as_str()
                .unwrap_or_default()
                .contains("Он не равен retrieval.hot_p95_ms")
        );
        assert!(
            cards[1]["note"]
                .as_str()
                .unwrap_or_default()
                .contains("источник SLA-метрики retrieval.hot_p95_ms")
        );
        assert_eq!(
            cards[0]["table"]["rows"][0]["values"][0].as_str(),
            Some("> 1200000\nBurst QPS")
        );
        assert_eq!(
            cards[0]["table"]["rows"][0]["values"][1].as_str(),
            Some("1224682\nBurst QPS")
        );
        assert_eq!(
            cards[0]["table"]["rows"][5]["values"][0].as_str(),
            Some("= 0.00%")
        );
        assert_eq!(
            cards[1]["table"]["rows"][0]["values"][0].as_str(),
            Some("нет SLA-порога")
        );
        assert_eq!(
            cards[1]["table"]["rows"][5]["values"][0].as_str(),
            Some(">= 20")
        );
        assert_eq!(
            cards[1]["table"]["rows"][6]["values"][0].as_str(),
            Some(">= 3")
        );
        assert_eq!(
            cards[2]["table"]["rows"][8]["values"][0].as_str(),
            Some(">= 75")
        );
        assert_eq!(
            cards[3]["table"]["rows"][1]["values"][0].as_str(),
            Some("99.00%")
        );
        assert_eq!(
            cards[3]["table"]["rows"][2]["values"][0].as_str(),
            Some("98.00%")
        );
        assert_eq!(
            cards[3]["extra_class"].as_str(),
            Some("benchmark-span-full")
        );
        assert_eq!(cards[3]["table_orientation"].as_str(), Some("transposed"));
    }
}
