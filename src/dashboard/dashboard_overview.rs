use super::*;

pub(super) fn build_headline(snapshot: &Value, captured_at_epoch_ms: u64) -> Value {
    let pass = snapshot["sla"]["summary"]["pass"].as_u64().unwrap_or(0);
    let alert = snapshot["sla"]["summary"]["alert"].as_u64().unwrap_or(0);
    let critical = snapshot["sla"]["summary"]["critical"].as_u64().unwrap_or(0);
    let unknown = snapshot["sla"]["summary"]["unknown"].as_u64().unwrap_or(0);
    let token_headline = &snapshot["token_budget_report"]["token_budget_report"]["headline"];
    let active_agent_headline = &snapshot["active_agent_budget"]["headline"];
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
        "token_title": active_agent_headline["title"]
            .as_str()
            .or_else(|| token_headline["title"].as_str())
            .unwrap_or("ещё нет данных"),
        "token_value": active_agent_headline["value_text"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| format_percent(token_headline["value_percent"].as_f64())),
        "token_scope": if active_agent_headline.is_object() {
            ""
        } else {
            token_headline["scope_label"].as_str().unwrap_or("")
        },
    })
}

pub(super) fn build_top_cards(snapshot: &Value) -> Vec<Value> {
    vec![
        live_latency_compare_card(snapshot),
        working_state_live_card(snapshot),
    ]
}

fn headline_status_label(status: &str) -> &'static str {
    match status {
        "pass" => "система в норме",
        "alert" => "нужно внимание",
        "critical" => "есть критичные сигналы",
        "waiting" => "данных пока мало",
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
