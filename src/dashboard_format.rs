use serde_json::Value;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

pub(crate) fn format_signed_token_delta(value: i64) -> String {
    if value > 0 {
        format!("+{}", format_u64(Some(value.unsigned_abs())))
    } else if value < 0 {
        format!("-{}", format_u64(Some(value.unsigned_abs())))
    } else {
        "0".to_string()
    }
}

pub(crate) fn format_signed_percent_points(value: f64) -> String {
    if value >= 0.0 {
        format!("+{value:.2} п.п.")
    } else {
        format!("{value:.2} п.п.")
    }
}

pub(crate) fn format_ru_count_noun<'a>(
    count: u64,
    singular: &'a str,
    paucal: &'a str,
    plural: &'a str,
) -> &'a str {
    let rem100 = count % 100;
    let rem10 = count % 10;
    if (11..=14).contains(&rem100) {
        plural
    } else {
        match rem10 {
            1 => singular,
            2..=4 => paucal,
            _ => plural,
        }
    }
}

pub(crate) fn compare_pair(target: String, current: String) -> Vec<String> {
    vec![target, current]
}

pub(crate) fn human_timestamp(epoch_ms: u64) -> String {
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

pub(crate) fn human_timestamp_clock(epoch_ms: u64) -> String {
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
    let format = format_description!("[hour]:[minute]:[second] MSK");
    datetime
        .to_offset(offset)
        .format(&format)
        .unwrap_or_else(|_| "ещё нет данных".to_string())
}

pub(crate) fn human_epoch_seconds(epoch_seconds: u64) -> String {
    if epoch_seconds == 0 {
        return "ещё нет данных".to_string();
    }
    human_timestamp(epoch_seconds.saturating_mul(1000))
}

pub(crate) fn source_label(prefix: &str, epoch_ms: Option<u64>) -> String {
    match epoch_ms.filter(|value| *value > 0) {
        Some(epoch_ms) => format!("{prefix}. Измерено: {}.", human_timestamp(epoch_ms)),
        None => prefix.to_string(),
    }
}

pub(crate) fn client_display_name(key: &str) -> &str {
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
pub(crate) struct DashboardTimingFormat<'a> {
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

fn default_dashboard_timing_format() -> DashboardTimingFormat<'static> {
    DashboardTimingFormat {
        switch_to_nanoseconds_below_ms: 0.001,
        switch_to_microseconds_below_ms: 1.0,
        switch_to_seconds_at_or_above_ms: 1000.0,
        non_positive_floor_label: "0 ns",
        seconds_suffix: "s",
        milliseconds_suffix: "ms",
        microseconds_suffix: "µs",
        nanoseconds_suffix: "ns",
        seconds_decimals: 3,
        milliseconds_decimals: 3,
        microseconds_decimals: 3,
        nanoseconds_decimals: 0,
    }
}

pub(crate) fn format_ms(snapshot: &Value, value: Option<f64>) -> String {
    format_duration_ms(dashboard_timing_format(snapshot), value)
}

pub(crate) fn format_seconds(snapshot: &Value, value: Option<f64>) -> String {
    format_duration_ms(
        dashboard_timing_format(snapshot),
        value.map(|number| number * 1000.0),
    )
}

pub(crate) fn format_duration_ms(policy: DashboardTimingFormat<'_>, value: Option<f64>) -> String {
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

fn dashboard_timing_format(snapshot: &Value) -> DashboardTimingFormat<'_> {
    let timing = &snapshot["thresholds"]["dashboard"]["timing_format"];
    let default = default_dashboard_timing_format();
    DashboardTimingFormat {
        switch_to_nanoseconds_below_ms: timing["switch_to_nanoseconds_below_ms"]
            .as_f64()
            .unwrap_or(default.switch_to_nanoseconds_below_ms),
        switch_to_microseconds_below_ms: timing["switch_to_microseconds_below_ms"]
            .as_f64()
            .unwrap_or(default.switch_to_microseconds_below_ms),
        switch_to_seconds_at_or_above_ms: timing["switch_to_seconds_at_or_above_ms"]
            .as_f64()
            .unwrap_or(default.switch_to_seconds_at_or_above_ms),
        non_positive_floor_label: timing["non_positive_floor_label"]
            .as_str()
            .unwrap_or(default.non_positive_floor_label),
        seconds_suffix: timing["seconds_suffix"]
            .as_str()
            .unwrap_or(default.seconds_suffix),
        milliseconds_suffix: timing["milliseconds_suffix"]
            .as_str()
            .unwrap_or(default.milliseconds_suffix),
        microseconds_suffix: timing["microseconds_suffix"]
            .as_str()
            .unwrap_or(default.microseconds_suffix),
        nanoseconds_suffix: timing["nanoseconds_suffix"]
            .as_str()
            .unwrap_or(default.nanoseconds_suffix),
        seconds_decimals: timing["seconds_decimals"]
            .as_u64()
            .unwrap_or(default.seconds_decimals as u64) as usize,
        milliseconds_decimals: timing["milliseconds_decimals"]
            .as_u64()
            .unwrap_or(default.milliseconds_decimals as u64)
            as usize,
        microseconds_decimals: timing["microseconds_decimals"]
            .as_u64()
            .unwrap_or(default.microseconds_decimals as u64)
            as usize,
        nanoseconds_decimals: timing["nanoseconds_decimals"]
            .as_u64()
            .unwrap_or(default.nanoseconds_decimals as u64) as usize,
    }
}

pub(crate) fn format_ratio_percent(value: Option<f64>) -> String {
    value
        .map(|number| format!("{:.2}%", number * 100.0))
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

pub(crate) fn format_percent(value: Option<f64>) -> String {
    value
        .map(|number| format!("{number:.2}%"))
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

pub(crate) fn format_threshold_at_least(value: Option<f64>, unit: &str, decimals: usize) -> String {
    format_threshold_value(value, ">", unit, decimals)
}

pub(crate) fn format_threshold_at_least_or_equal(
    value: Option<f64>,
    unit: &str,
    decimals: usize,
) -> String {
    format_threshold_value(value, ">=", unit, decimals)
}

pub(crate) fn format_zero_or_at_most_percent(value: Option<f64>) -> String {
    match value {
        Some(number) if number.abs() < f64::EPSILON => {
            format_threshold_value(Some(number), "=", "%", 2)
        }
        Some(number) => format_threshold_value(Some(number), "<=", "%", 2),
        None => "ещё нет данных".to_string(),
    }
}

pub(crate) fn format_threshold_value(
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

pub(crate) fn format_time_threshold(
    snapshot: &Value,
    value: Option<f64>,
    operator: &str,
) -> String {
    format_threshold_rendered(operator, format_ms(snapshot, value))
}

pub(crate) fn format_threshold_rendered(operator: &str, rendered: String) -> String {
    if rendered == "ещё нет данных" {
        rendered
    } else {
        format!("{operator} {rendered}")
    }
}

pub(crate) fn format_decimal(value: f64, decimals: usize) -> String {
    format!("{value:.prec$}", prec = decimals)
}

pub(crate) fn format_decimal_trimmed(value: f64, decimals: usize) -> String {
    let mut rendered = format_decimal(value, decimals);
    while rendered.contains('.') && rendered.ends_with('0') {
        rendered.pop();
    }
    if rendered.ends_with('.') {
        rendered.pop();
    }
    rendered
}

pub(crate) fn format_time_compare_pair(
    snapshot: &Value,
    target_ms: Option<f64>,
    current_ms: Option<f64>,
    operator: &str,
) -> Vec<String> {
    let policy = dashboard_timing_format(snapshot);
    compare_pair(
        format_threshold_rendered(
            operator,
            render_duration_ms_with_unit(policy, target_ms, None),
        ),
        render_duration_ms_with_unit(policy, current_ms, None),
    )
}

pub(crate) fn format_seconds_compare_pair(
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

pub(crate) fn format_burst_qps_table(value: Option<f64>) -> String {
    match value {
        Some(number) => format!("{}\nBurst QPS", format_burst_qps_number(number)),
        None => "ещё нет данных".to_string(),
    }
}

pub(crate) fn format_burst_qps_threshold(value: Option<f64>, operator: &str) -> String {
    match value {
        Some(number) => format!("{operator} {}\nBurst QPS", format_burst_qps_number(number)),
        None => "ещё нет данных".to_string(),
    }
}

pub(crate) fn format_burst_qps_number(value: f64) -> String {
    let mut rendered = format!("{value:.2}");
    while rendered.contains('.') && rendered.ends_with('0') {
        rendered.pop();
    }
    if rendered.ends_with('.') {
        rendered.pop();
    }
    rendered
}

pub(crate) fn format_u64(value: Option<u64>) -> String {
    value
        .map(|number| number.to_string())
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

pub(crate) fn format_target_u64(operator: &str, value: u64) -> String {
    format!("{operator} {value}")
}

pub(crate) fn format_signed_count(value: Option<i64>) -> String {
    value
        .map(|number| number.to_string())
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

pub(crate) fn format_count_with_word(value: u64, one: &str, few: &str, many: &str) -> String {
    let last_two = value % 100;
    let last_one = value % 10;
    let word = if (11..=14).contains(&last_two) {
        many
    } else {
        match last_one {
            1 => one,
            2..=4 => few,
            _ => many,
        }
    };
    format!("{value} {word}")
}

pub(crate) fn format_f64_count(value: Option<f64>) -> String {
    value
        .map(|number| format!("{number:.0}"))
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

pub(crate) fn format_optional<F>(value: Option<f64>, formatter: F) -> String
where
    F: FnOnce(f64) -> String,
{
    value
        .map(formatter)
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

pub(crate) fn human_bytes(value: f64) -> String {
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

pub(crate) fn human_bytes_per_sec(value: f64) -> String {
    format!("{}/s", human_bytes(value))
}

pub(crate) fn format_celsius(value: f64) -> String {
    format!("{value:.1}°C")
}

pub(crate) fn elapsed_since_epoch_label(
    start_epoch_ms: Option<u64>,
    end_epoch_ms: Option<u64>,
) -> String {
    let Some(start_epoch_ms) = start_epoch_ms.filter(|value| *value > 0) else {
        return "ещё нет данных".to_string();
    };
    let Some(end_epoch_ms) = end_epoch_ms.filter(|value| *value >= start_epoch_ms) else {
        return "ещё нет данных".to_string();
    };
    human_elapsed_ms(end_epoch_ms.saturating_sub(start_epoch_ms))
}

pub(crate) fn human_elapsed_ms(value_ms: u64) -> String {
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
