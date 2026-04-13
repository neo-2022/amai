use super::*;
use std::cmp::Reverse;
use std::env;

pub(super) fn build_machine_cards(
    snapshot: &Value,
    machine: Option<&MachineSummary>,
    install_state: Option<&dashboard_context::InstallState>,
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
                    format_optional(machine.disk_read_mib_per_sec, |value| {
                        format!("{value:.2} MiB/s")
                    }),
                    Some("Текущая скорость чтения между двумя последними refresh панели."),
                ),
                metric_row(
                    "Запись",
                    format_optional(machine.disk_write_mib_per_sec, |value| {
                        format!("{value:.2} MiB/s")
                    }),
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
        cards.push(with_status_tooltip(
            card(
                "Машина",
                "ещё нет данных".to_string(),
                "Сводку по железу пока не удалось собрать автоматически.".to_string(),
                "unknown",
            ),
            "Статус пока не может считаться нормальным по следующим причинам:\n- Автоматический сбор machine summary пока не дал результат.\n- Поэтому панель не может показать текущий профиль железа.",
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
            with_status_tooltip(
                card(
                    "Установка",
                    "ещё не найдена".to_string(),
                    "state/install_state.json пока не найден, поэтому панель не видит последнюю user-facing установку.".to_string(),
                    "unknown",
                ),
                "Статус пока не может считаться нормальным по следующим причинам:\n- Файл state/install_state.json пока не найден.\n- Без него панель не видит последнюю пользовательскую установку этого клиента.",
            ),
            "machine-compact",
        ));
    }
    cards.push(with_extra_class(
        artifact_cleanup_card(snapshot, machine),
        "machine-compact",
    ));
    cards
}

fn artifact_cleanup_card(snapshot: &Value, machine: Option<&MachineSummary>) -> Value {
    let cleanup = &snapshot["artifact_cleanup"];
    if !cleanup.is_object() || cleanup["status"].as_str().is_some() {
        return card_with_rows(
            "Локальный мусор и retention",
            "ещё нет данных".to_string(),
            "Policy-driven cleanup для rebuildable хвоста ещё не успел записать последний summary."
                .to_string(),
            "unknown",
            Some("Источник: state/tooling/artifact_cleanup/latest.json".to_string()),
            Some(
                "Этот блок показывает только rebuildable локальный хвост Amai. Live state и исторические данные сервисов сюда не входят.".to_string(),
            ),
            vec![],
        );
    }

    let safe_reclaimable_bytes = cleanup["selected_reclaimable_bytes"].as_u64().unwrap_or(0);
    let policy_retained_reclaimable_bytes = cleanup["policy_retained_reclaimable_bytes"]
        .as_u64()
        .unwrap_or(0);
    let manual_only_reclaimable_bytes = cleanup["manual_only_reclaimable_bytes"]
        .as_u64()
        .unwrap_or(0);
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
    let repo_inventory = &cleanup["repo_inventory"];
    let repo_total_bytes = repo_inventory["repo_total_bytes"].as_u64().unwrap_or(0);
    let cleanup_scope_bytes = repo_inventory["cleanup_scope_bytes"].as_u64().unwrap_or(0);
    let out_of_policy_bytes = repo_inventory["out_of_policy_bytes"].as_u64().unwrap_or(0);
    let unreadable_paths_count = repo_inventory["unreadable_paths_count"]
        .as_u64()
        .unwrap_or(0);
    let unreadable_paths_sample = repo_inventory["unreadable_paths_sample"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let large_unmanaged_roots = repo_inventory["large_unmanaged_roots"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let manual_only_targets = repo_inventory["manual_only_targets"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let policy_retained_targets = cleanup["policy_retained_targets"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let manual_only_reclaimable_targets = cleanup["manual_only_reclaimable_targets"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let operator_reclaim_hints = artifact_cleanup_operator_reclaim_hints(cleanup);
    let last_apply = &cleanup["last_apply"];
    let last_reclaim_bytes = last_apply["reclaimed_bytes"].as_u64().unwrap_or(0);
    let last_deleted = last_apply["deleted"].as_u64().unwrap_or(0);
    let last_apply_mode = last_apply["mode"].as_str().unwrap_or("conservative");
    let last_apply_at = last_apply["captured_at_epoch_ms"].as_u64();

    let value = if !large_unmanaged_roots.is_empty() && out_of_policy_bytes > 0 {
        format!("{} вне policy", human_bytes(out_of_policy_bytes as f64))
    } else if safe_reclaimable_bytes > 0 {
        format!("{} safe", human_bytes(safe_reclaimable_bytes as f64))
    } else if manual_only_reclaimable_bytes > 0 {
        format!(
            "{} manual",
            human_bytes(manual_only_reclaimable_bytes as f64)
        )
    } else if policy_retained_reclaimable_bytes > 0 {
        format!(
            "{} ждёт TTL",
            human_bytes(policy_retained_reclaimable_bytes as f64)
        )
    } else if aggressive_reclaimable_bytes > 0 {
        format!(
            "{} preview",
            human_bytes(aggressive_reclaimable_bytes as f64)
        )
    } else {
        "по policy чисто".to_string()
    };
    let mut note = format!(
        "Safe policy чистит только то, что уже aged past TTL и не попадает под keep-latest. Aggressive preview показывает, сколько rebuildable хвоста можно убрать сразу, не трогая live state. Последний sweep: {}.",
        captured_at_epoch_ms
            .map(human_timestamp)
            .unwrap_or_else(|| "ещё нет данных".to_string())
    );
    if let Some(root) = large_unmanaged_roots.first() {
        let root_path = root["path"].as_str().unwrap_or("неизвестный root");
        let root_unmanaged_bytes = root["unmanaged_bytes"].as_u64().unwrap_or(0);
        note.push_str(&format!(
            " Основной локальный вес сейчас лежит вне cleanup policy: {root_path} = {} unmanaged bytes.",
            human_bytes(root_unmanaged_bytes as f64)
        ));
    } else if let Some(sample) = unreadable_paths_sample.first() {
        let sample_path = sample.as_str().unwrap_or("неизвестный path");
        note.push_str(&format!(
            " Inventory читает repo как best-effort lower bound: один из unreadable live-state путей сейчас {sample_path}. Поэтому часть вне-policy веса может жить там и не является broken cleanup contour.",
        ));
    }
    if let Some(target) = manual_only_targets.first() {
        let target_path = target["path"].as_str().unwrap_or("неизвестный target");
        note.push_str(&format!(
            " Для {target_path} уже есть explicit manual-only cleanup contour: используйте `observe cleanup-artifacts --target {target_path} --apply` или `--target {target_path} --aggressive --apply`, auto-retention этот путь не трогает."
        ));
    }
    if let Some(target) = policy_retained_targets.first() {
        let target_path = target["path"].as_str().unwrap_or("неизвестный target");
        let target_bytes = target["aggressive_preview_reclaimable_bytes"]
            .as_u64()
            .unwrap_or(0);
        note.push_str(&format!(
            " Сейчас основной policy-covered hot storage удерживается возрастным запасом и keep-latest: {target_path} = {}. Это не unmanaged drift и не сломанный cleanup, а осознанный retention hold.",
            human_bytes(target_bytes as f64)
        ));
    }
    if let Some(hint) = operator_reclaim_hints.first() {
        let target_path = hint["path"].as_str().unwrap_or("неизвестный target");
        let reclaimable_bytes = hint["reclaimable_bytes"].as_u64().unwrap_or(0);
        let command = hint["recommended_command"]
            .as_str()
            .unwrap_or("observe cleanup-artifacts --help");
        note.push_str(&format!(
            " Если место нужно вернуть раньше, ближайший operator reclaim path уже materialized: {target_path} = {} через `{command}`.",
            human_bytes(reclaimable_bytes as f64)
        ));
    }
    if last_reclaim_bytes > 0 {
        let last_apply_label = last_apply_at
            .map(human_timestamp)
            .unwrap_or_else(|| "неизвестно когда".to_string());
        note.push_str(&format!(
            " Последний apply-run уже вернул {} ({last_deleted} entries, mode={last_apply_mode}) в {last_apply_label}.",
            human_bytes(last_reclaim_bytes as f64)
        ));
    }

    let mut card = card_with_rows(
        "Локальный мусор и retention",
        value,
        note,
        artifact_cleanup_status(snapshot, machine),
        Some("Источник: state/tooling/artifact_cleanup/latest.json".to_string()),
        Some(
            "Это локальный hygiene contour для build/cache хвостов Amai. Он не удаляет state PostgreSQL, Qdrant, MinIO или NATS.".to_string(),
        ),
        vec![
            metric_row(
                "Repo footprint",
                human_bytes(repo_total_bytes as f64),
                Some("Сколько места сейчас занимает весь repo-root, включая то, что не входит в cleanup policy."),
            ),
            metric_row(
                "Cleanup scope",
                human_bytes(cleanup_scope_bytes as f64),
                Some("Сколько места сейчас лежит внутри управляемых cleanup-target roots."),
            ),
            metric_row(
                "Вне policy",
                human_bytes(out_of_policy_bytes as f64),
                Some("Сколько места сейчас лежит вне cleanup-target roots и поэтому не удаляется auto-retention path-ом."),
            ),
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
                "Policy-retained hot storage",
                human_bytes(policy_retained_reclaimable_bytes as f64),
                Some("Сколько rebuildable веса уже входит в cleanup policy, но пока удерживается TTL/keep-latest и therefore ещё не попадает под safe reclaim."),
            ),
            metric_row(
                "Manual reclaim now",
                human_bytes(manual_only_reclaimable_bytes as f64),
                Some("Сколько веса сейчас доступно только через explicit/manual cleanup contours, а не через auto-retention."),
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
                "Heavy unmanaged roots",
                if large_unmanaged_roots.is_empty() {
                    "нет".to_string()
                } else {
                    large_unmanaged_roots
                        .iter()
                        .map(|root| {
                            let path = root["path"].as_str().unwrap_or("неизвестный root");
                            let unmanaged_bytes = root["unmanaged_bytes"].as_u64().unwrap_or(0);
                            format!("{path} ({})", human_bytes(unmanaged_bytes as f64))
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                },
                Some("Крупные директории вне cleanup policy. Они не попадают под TTL/keep-latest auto-path."),
            ),
            metric_row(
                "Manual-only contours",
                if manual_only_targets.is_empty() {
                    "нет".to_string()
                } else {
                    manual_only_targets
                        .iter()
                        .map(|target| {
                            let path = target["path"].as_str().unwrap_or("неизвестный target");
                            let ttl_hours = target["ttl_hours"].as_u64().unwrap_or(0);
                            let keep_latest = target["keep_latest"].as_u64().unwrap_or(0);
                            let total_bytes = target["total_bytes"].as_u64().unwrap_or(0);
                            format!(
                                "{path} ({}, ttl {ttl_hours}h, keep_latest {keep_latest})",
                                human_bytes(total_bytes as f64)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                },
                Some("Пути, которые уже заведены в cleanup policy, но остаются только на explicit/manual path и не удаляются auto-retention-ом."),
            ),
            metric_row(
                "Policy waiting targets",
                if policy_retained_targets.is_empty() {
                    "нет".to_string()
                } else {
                    policy_retained_targets
                        .iter()
                        .map(|target| {
                            let path = target["path"].as_str().unwrap_or("неизвестный target");
                            let ttl_hours = target["ttl_hours"].as_u64().unwrap_or(0);
                            let keep_latest = target["keep_latest"].as_u64().unwrap_or(0);
                            let reclaimable = target["aggressive_preview_reclaimable_bytes"]
                                .as_u64()
                                .unwrap_or(0);
                            format!(
                                "{path} ({}, ttl {ttl_hours}h, keep_latest {keep_latest})",
                                human_bytes(reclaimable as f64)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                },
                Some("Cleanup-targets, которые уже policy-covered, но всё ещё intentionally удерживаются возрастным запасом или keep-latest."),
            ),
            metric_row(
                "Manual reclaim targets",
                if manual_only_reclaimable_targets.is_empty() {
                    "нет".to_string()
                } else {
                    manual_only_reclaimable_targets
                        .iter()
                        .map(|target| {
                            let path = target["path"].as_str().unwrap_or("неизвестный target");
                            let reclaimable = target["aggressive_preview_reclaimable_bytes"]
                                .as_u64()
                                .unwrap_or(0);
                            format!("{path} ({})", human_bytes(reclaimable as f64))
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                },
                Some("Manual-only cleanup contours, где reclaim уже доступен, но auto-retention этот path не трогает."),
            ),
            metric_row(
                "Operator reclaim next",
                if operator_reclaim_hints.is_empty() {
                    "нет".to_string()
                } else {
                    operator_reclaim_hints
                        .iter()
                        .map(artifact_cleanup_reclaim_hint_summary)
                        .collect::<Vec<_>>()
                        .join("; ")
                },
                Some("Точные команды для самых тяжёлых reclaim-кандидатов, если место нужно вернуть раньше TTL/keep-latest."),
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
            metric_row(
                "Unreadable contents",
                unreadable_paths_count.to_string(),
                Some("Сколько путей inventory не смог прочитать. Repo footprint тогда считается как best-effort lower bound."),
            ),
            metric_row(
                "Unreadable sample",
                if unreadable_paths_sample.is_empty() {
                    "нет".to_string()
                } else {
                    unreadable_paths_sample
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                },
                Some("Примеры путей, которые inventory не смог прочитать и поэтому считает repo footprint только как best-effort lower bound."),
            ),
        ],
    );
    if let Some(tooltip) = status_reason_tooltip(
        artifact_cleanup_status(snapshot, machine),
        artifact_cleanup_warning(snapshot, machine)
            .into_iter()
            .collect(),
        "Cleanup contour видит локальный rebuildable хвост, который уже требует внимания.",
    ) {
        card = with_status_tooltip(card, &tooltip);
    }
    card
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
                Some(
                    "Дополнительное графическое или accelerator-устройство, найденное в этой машине."
                        .to_string(),
                ),
                vec![
                    metric_row(
                        "Тип",
                        accelerator.kind_label.clone(),
                        Some("Определённый тип дополнительного ускорителя."),
                    ),
                    metric_row(
                        "Память",
                        format_optional(accelerator.total_vram_gib, |value| {
                            format!("{value:.2} GiB")
                        }),
                        Some("Полный объём памяти дополнительного ускорителя, если provider смог его дать."),
                    ),
                    metric_row(
                        "Нагрузка",
                        format_optional(accelerator.utilization_percent, |value| {
                            format!("{value:.1}%")
                        }),
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

pub(super) fn build_governance_card(snapshot: &Value) -> Value {
    let governance = &snapshot["governance_surface"];
    if !governance.is_object() {
        return card_with_rows(
            "Жизненный цикл памяти",
            "ещё нет данных".to_string(),
            "Пока панель не собрала machine-readable surface по forgetting, quarantine и memory governance."
                .to_string(),
            "unknown",
            Some(
                "Источник: governance_surface из live snapshot. Пока этот слой не surfaced."
                    .to_string(),
            ),
            Some(
                "Показывает, как Amai чистит, архивирует и пересматривает память, не теряя protected truth и explainability."
                    .to_string(),
            ),
            vec![],
        );
    }

    let open_conflicts = governance["wrong_link_rate"]["open_conflict_count"]
        .as_u64()
        .unwrap_or(0);
    let active_quarantine = governance["poisoning_alert_count"]["active_quarantine_items"]
        .as_u64()
        .unwrap_or(0);
    let disputed_items = governance["trust_state_distribution"]["disputed_memory_items"]
        .as_u64()
        .unwrap_or(0);
    let forgetting_total = governance["human_override_audit"]["forgetting_audit_log_entries_total"]
        .as_u64()
        .unwrap_or(0);
    let status = if open_conflicts > 0 || active_quarantine > 0 || disputed_items > 0 {
        "alert"
    } else if forgetting_total > 0 {
        "pass"
    } else {
        "unknown"
    };
    let pruning_job_total = governance["forgetting_job_breakdown"]["pruning_job"]
        .as_u64()
        .unwrap_or(0);
    let cold_archive_job_total = governance["forgetting_job_breakdown"]["cold_archive_job"]
        .as_u64()
        .unwrap_or(0);
    let revalidation_job_total = governance["forgetting_job_breakdown"]["revalidation_job"]
        .as_u64()
        .unwrap_or(0);
    let dedup_job_total = governance["forgetting_job_breakdown"]["de_duplication_job"]
        .as_u64()
        .unwrap_or(0);
    let summarize_job_total = governance["forgetting_job_breakdown"]["summarization_job"]
        .as_u64()
        .unwrap_or(0);
    let stale_rate = governance["stale_memory_error_rate"]["rate"].as_f64();
    let top_quarantine = governance["poisoning_alert_count"]["active_quarantine_breakdown"]
        .as_array()
        .and_then(|items| items.first());
    let top_conflict = governance["open_conflict_breakdown"]
        .as_array()
        .and_then(|items| items.first());
    let headline_value = if status == "alert" {
        let mut parts = Vec::new();
        if active_quarantine > 0 {
            parts.push(format!(
                "{} в quarantine",
                format_u64(Some(active_quarantine))
            ));
        }
        if open_conflicts > 0 {
            parts.push(format!(
                "{} {}",
                format_u64(Some(open_conflicts)),
                format_ru_count_noun(open_conflicts, "конфликт", "конфликта", "конфликтов")
            ));
        }
        if disputed_items > 0 {
            parts.push(format!(
                "{} {}",
                format_u64(Some(disputed_items)),
                format_ru_count_noun(disputed_items, "спорный", "спорных", "спорных")
            ));
        }
        if parts.is_empty() {
            "требует внимания".to_string()
        } else {
            parts.join(" • ")
        }
    } else if forgetting_total > 0 {
        format!(
            "{} forgetting-действий зафиксировано",
            format_u64(Some(forgetting_total))
        )
    } else {
        "ещё нет действий".to_string()
    };
    let alert_note = format_governance_alert_note(top_quarantine, top_conflict);

    card_with_rows(
        "Жизненный цикл памяти",
        headline_value,
        if status == "alert" {
            alert_note.unwrap_or_else(|| {
                "Карточка требует внимания, потому что в live memory governance сейчас есть quarantine или открытые truth-конфликты."
                    .to_string()
            })
        } else {
            "Здесь видно, как Amai реально чистит и пересматривает память: pruning, archive, revalidation и dedup surfaced отдельно, а protected truth не должен исчезать тихо."
                .to_string()
        },
        status,
        Some(
            "Источник: live governance_surface. Карточка показывает не policy-обещание, а фактический audit contour forgetting и trust."
                .to_string(),
        ),
        Some(
            "Stage 9 surface: explainable forgetting, quarantine/trust pressure и реальный объём lifecycle-действий."
                .to_string(),
        ),
        vec![
            metric_row(
                "Pruning",
                format_u64(Some(pruning_job_total)),
                Some("Сколько pruning-действий уже записано через TTL или low-utility cleanup."),
            ),
            metric_row(
                "Archive",
                format_u64(Some(cold_archive_job_total)),
                Some("Сколько stale derivative items уже переведено в cold archive."),
            ),
            metric_row(
                "Revalidation",
                format_u64(Some(revalidation_job_total)),
                Some("Сколько stale current items уже отправлено в pending_review."),
            ),
            metric_row(
                "Dedup / compaction",
                format_u64(Some(dedup_job_total)),
                Some("Сколько duplicate branches уже схлопнуто через de-duplication / compaction contour."),
            ),
            metric_row(
                "Summarization",
                format_u64(Some(summarize_job_total)),
                Some("Пока это explicit no-op contract. Здесь не должно быть тихой псевдо-активности."),
            ),
            metric_row(
                "Stale rate",
                format_ratio_percent(stale_rate),
                Some("Доля archived/pruned items от всей памяти. Это не KPI успеха, а честный pressure indicator cleanup-контура."),
            ),
            metric_row(
                "Quarantine",
                format_u64(Some(active_quarantine)),
                Some("Сколько memory items сейчас ещё удерживаются в quarantine и требуют ручного разбора."),
            ),
            metric_row(
                "Спорные",
                format_u64(Some(disputed_items)),
                Some("Сколько memory items сейчас имеют disputed trust-state."),
            ),
            metric_row(
                "Открытые конфликты",
                format_u64(Some(open_conflicts)),
                Some("Сколько wrong-link / truth конфликтов сейчас ещё не закрыто."),
            ),
        ],
    )
}

fn format_governance_alert_note(
    top_quarantine: Option<&Value>,
    top_conflict: Option<&Value>,
) -> Option<String> {
    let mut reasons = Vec::new();
    if let Some(item) = top_quarantine {
        let count = item["item_count"].as_u64().unwrap_or(0);
        let reason = item["quarantine_reason"].as_str().unwrap_or("unknown");
        let entity_kind = item["entity_kind"].as_str().unwrap_or("unknown");
        let source_kind = item["source_kind"].as_str().unwrap_or("unknown");
        reasons.push(format!(
            "главный quarantine-класс: {} ({}, {}, {})",
            format_u64(Some(count)),
            humanize_identifier(reason),
            humanize_identifier(entity_kind),
            humanize_identifier(source_kind)
        ));
    }
    if let Some(item) = top_conflict {
        let count = item["item_count"].as_u64().unwrap_or(0);
        let summary = compact_dashboard_text(item["summary"].as_str(), 56, "unknown");
        let source_kind = item["source_kind"].as_str().unwrap_or("unknown");
        reasons.push(format!(
            "главный конфликт: {} ({}, {})",
            format_u64(Some(count)),
            summary,
            humanize_identifier(source_kind)
        ));
    }
    if reasons.is_empty() {
        None
    } else {
        Some(format!(
            "Карточка требует внимания: {}.",
            reasons.join("; ")
        ))
    }
}

pub(super) fn build_warnings(snapshot: &Value, machine: Option<&MachineSummary>) -> Vec<String> {
    let mut warnings = Vec::new();
    for check in snapshot["sla"]["checks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|check| check["status"].as_str().unwrap_or("unknown") != "pass")
    {
        warnings.push(humanize_check(snapshot, check));
    }
    if let Some(warning) = artifact_cleanup_warning(snapshot, machine) {
        warnings.push(warning);
    }
    warnings
}

pub(super) fn build_glossary() -> Vec<Value> {
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

pub(super) fn build_links(base_url: &str) -> Vec<Value> {
    let mut links = vec![json!({
        "label": "",
        "note": "",
        "items": [
            {
                "label": "Raw dashboard JSON",
                "url": format!("{base_url}/api/dashboard"),
                "note": "Если хотите отдать эти же данные другой программе."
            },
            {
                "label": "Raw snapshot JSON",
                "url": format!("{base_url}/api/snapshot"),
                "note": "Полный live snapshot без human-упаковки."
            },
            {
                "label": "Prometheus metrics",
                "url": format!("{base_url}/metrics"),
                "note": "Инженерный слой для scrape и алертов."
            },
            {
                "label": "Health JSON",
                "url": format!("{base_url}/healthz"),
                "note": "Быстрый health-check с тем же SLA-контуром."
            }
        ]
    })];

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
        "label": "",
        "note": "",
        "items": [
            {
                "label": "Prometheus",
                "url": if prometheus_available { Value::from(monitoring_url(base_url, &prometheus_port)) } else { Value::Null },
                "note": if prometheus_available {
                    "Глубокие live-метрики уже доступны."
                } else {
                    "Мониторинг сейчас не поднят. Сначала запустите ./scripts/monitoring_up.sh."
                }
            },
            {
                "label": "Grafana",
                "url": if grafana_available { Value::from(monitoring_url(base_url, &grafana_port)) } else { Value::Null },
                "note": if grafana_available {
                    if grafana_default_password {
                        format!("Готовая инженерная панель уже доступна. Логин: {}. Пароль пока стандартный из .env: admin_change_me. Лучше сменить его в AMI_GRAFANA_ADMIN_PASSWORD.", grafana_admin_user)
                    } else {
                        format!("Готовая инженерная панель уже доступна. Логин: {}. Текущий пароль задан в .env через AMI_GRAFANA_ADMIN_PASSWORD.", grafana_admin_user)
                    }
                } else {
                    "Grafana поднимается вместе с мониторингом. Сначала запустите ./scripts/monitoring_up.sh.".to_string()
                }
            }
        ]
    }));
    links
}

fn artifact_cleanup_pressure_state(
    cleanup: &Value,
    machine: Option<&MachineSummary>,
) -> Option<&'static str> {
    if cleanup["policy_retained_reclaimable_bytes"]
        .as_u64()
        .unwrap_or(0)
        == 0
    {
        return None;
    }
    let Some(machine) = machine else {
        return Some("waiting");
    };
    let thresholds = &cleanup["disk_pressure_thresholds"];
    let used_percent = machine.disk_used_percent.unwrap_or(0.0);
    let available_gib = machine.disk_available_gib;
    let alert_used_percent = thresholds["alert_used_percent"].as_f64().unwrap_or(85.0);
    let critical_used_percent = thresholds["critical_used_percent"].as_f64().unwrap_or(92.0);
    let alert_available_gib = thresholds["alert_available_gib"].as_f64().unwrap_or(150.0);
    let critical_available_gib = thresholds["critical_available_gib"]
        .as_f64()
        .unwrap_or(60.0);

    if used_percent >= critical_used_percent || available_gib <= critical_available_gib {
        Some("critical")
    } else if used_percent >= alert_used_percent || available_gib <= alert_available_gib {
        Some("alert")
    } else {
        Some("waiting")
    }
}

fn artifact_cleanup_operator_reclaim_hints(cleanup: &Value) -> Vec<Value> {
    if let Some(hints) = cleanup["operator_reclaim_hints"].as_array() {
        if !hints.is_empty() {
            return hints.clone();
        }
    }

    let mut hints = Vec::new();
    if let Some(targets) = cleanup["manual_only_reclaimable_targets"].as_array() {
        for target in targets {
            if let Some(hint) =
                artifact_cleanup_operator_reclaim_hint_from_target(target, "manual_only_cleanup")
            {
                hints.push(hint);
            }
        }
    }
    if let Some(targets) = cleanup["policy_retained_targets"].as_array() {
        for target in targets {
            if let Some(hint) = artifact_cleanup_operator_reclaim_hint_from_target(
                target,
                "policy_retained_hot_storage",
            ) {
                hints.push(hint);
            }
        }
    }
    hints.sort_by_key(|hint| Reverse(hint["reclaimable_bytes"].as_u64().unwrap_or(0)));
    hints.truncate(3);
    hints
}

fn artifact_cleanup_operator_reclaim_hint_from_target(
    target: &Value,
    reason: &str,
) -> Option<Value> {
    let path = target["path"].as_str()?;
    let selected_reclaimable_bytes = target["selected_reclaimable_bytes"].as_u64().unwrap_or(0);
    let aggressive_preview_reclaimable_bytes = target["aggressive_preview_reclaimable_bytes"]
        .as_u64()
        .unwrap_or(0);
    let use_aggressive = selected_reclaimable_bytes == 0;
    let reclaimable_bytes = if use_aggressive {
        aggressive_preview_reclaimable_bytes
    } else {
        selected_reclaimable_bytes
    };
    Some(json!({
        "path": path,
        "reason": reason,
        "reclaimable_bytes": reclaimable_bytes,
        "recommended_command": if use_aggressive {
            format!("observe cleanup-artifacts --target {path} --aggressive --apply")
        } else {
            format!("observe cleanup-artifacts --target {path} --apply")
        }
    }))
}

fn artifact_cleanup_reclaim_hint_summary(hint: &Value) -> String {
    let path = hint["path"].as_str().unwrap_or("неизвестный target");
    let reclaimable_bytes = hint["reclaimable_bytes"].as_u64().unwrap_or(0);
    let command = hint["recommended_command"]
        .as_str()
        .unwrap_or("observe cleanup-artifacts --help");
    format!(
        "{path} -> {command} ({})",
        human_bytes(reclaimable_bytes as f64)
    )
}

fn artifact_cleanup_status(snapshot: &Value, machine: Option<&MachineSummary>) -> &'static str {
    let cleanup = &snapshot["artifact_cleanup"];
    if !cleanup.is_object() || cleanup["status"].as_str().is_some() {
        return "unknown";
    }
    if cleanup["selected"].as_u64().unwrap_or(0) > 0 {
        "alert"
    } else if cleanup["repo_inventory"]["unmanaged_alert_triggered"].as_bool() == Some(true) {
        "alert"
    } else if cleanup["manual_only_reclaimable_bytes"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "alert"
    } else if let Some(status) = artifact_cleanup_pressure_state(cleanup, machine) {
        status
    } else if cleanup["aggressive_preview_selected"].as_u64().unwrap_or(0) > 0 {
        "alert"
    } else {
        "pass"
    }
}

pub(super) fn artifact_cleanup_warning(
    snapshot: &Value,
    machine: Option<&MachineSummary>,
) -> Option<String> {
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
    let repo_inventory = &cleanup["repo_inventory"];
    if repo_inventory["unmanaged_alert_triggered"].as_bool() == Some(true) {
        let out_of_policy_bytes = repo_inventory["out_of_policy_bytes"].as_u64().unwrap_or(0);
        let first_root = repo_inventory["large_unmanaged_roots"]
            .as_array()
            .and_then(|roots| roots.first())
            .cloned()
            .unwrap_or_default();
        let root_path = first_root["path"].as_str().unwrap_or("неизвестный root");
        let root_unmanaged_bytes = first_root["unmanaged_bytes"].as_u64().unwrap_or(0);
        let manual_only_target = repo_inventory["manual_only_targets"]
            .as_array()
            .and_then(|targets| targets.first())
            .cloned()
            .unwrap_or_default();
        let manual_only_path = manual_only_target["path"].as_str();
        let manual_hint = manual_only_path.map(|path| {
            format!(
                " Для {path} уже есть explicit manual cleanup contour: `observe cleanup-artifacts --target {path} --apply` или `--target {path} --aggressive --apply`."
            )
        }).unwrap_or_default();
        return Some(format!(
            "Основной локальный вес сейчас вне cleanup policy: всего {} вне managed targets, крупнейший root {} = {}. Auto-retention это не трогает, пока путь не включён в policy отдельным contour-ом.{}",
            human_bytes(out_of_policy_bytes as f64),
            root_path,
            human_bytes(root_unmanaged_bytes as f64),
            manual_hint
        ));
    }
    let manual_only_bytes = cleanup["manual_only_reclaimable_bytes"]
        .as_u64()
        .unwrap_or(0);
    if manual_only_bytes > 0 {
        let operator_hint = artifact_cleanup_operator_reclaim_hints(cleanup)
            .into_iter()
            .next()
            .unwrap_or_default();
        let command = operator_hint["recommended_command"]
            .as_str()
            .unwrap_or("observe cleanup-artifacts --apply");
        return Some(format!(
            "Сейчас уже есть {} reclaimable веса на manual-only cleanup contour. Auto-retention этот путь специально не трогает, поэтому нужен explicit operator run: `{command}`.",
            human_bytes(manual_only_bytes as f64),
        ));
    }
    let policy_retained_bytes = cleanup["policy_retained_reclaimable_bytes"]
        .as_u64()
        .unwrap_or(0);
    if policy_retained_bytes > 0 {
        let pressure_state = artifact_cleanup_pressure_state(cleanup, machine).unwrap_or("waiting");
        let first_hint = artifact_cleanup_operator_reclaim_hints(cleanup)
            .into_iter()
            .next()
            .unwrap_or_default();
        let target_path = first_hint["path"].as_str().unwrap_or("policy target");
        let target_bytes = first_hint["reclaimable_bytes"].as_u64().unwrap_or(0);
        let command = first_hint["recommended_command"]
            .as_str()
            .unwrap_or("observe cleanup-artifacts --aggressive --apply");
        return Some(match pressure_state {
            "critical" | "alert" => {
                let used = machine
                    .and_then(|summary| summary.disk_used_percent)
                    .map(|value| format!("{value:.1}%"))
                    .unwrap_or_else(|| "неизвестно".to_string());
                let available = machine
                    .map(|summary| format!("{:.2} GiB", summary.disk_available_gib))
                    .unwrap_or_else(|| "неизвестно".to_string());
                format!(
                    "На диске уже есть давление: used {used}, свободно {available}. При этом {} policy-covered hot storage всё ещё удерживается TTL/keep-latest. Следующий manual reclaim кандидат: {target_path} = {} через `{command}`.",
                    human_bytes(policy_retained_bytes as f64),
                    human_bytes(target_bytes as f64)
                )
            }
            _ => format!(
                "Сейчас {} rebuildable веса уже policy-covered, но intentionally удерживается TTL/keep-latest. Cleanup не сломан: это hot storage, которое auto-path уберёт позже. Если место нужно раньше, ближайший reclaim path уже готов: `{command}` для {target_path} = {}.",
                human_bytes(policy_retained_bytes as f64),
                human_bytes(target_bytes as f64)
            ),
        });
    }
    if aggressive_bytes > 0 {
        return Some(format!(
            "Локальный rebuildable хвост ещё не дожил до TTL, но aggressive reclaim path уже мог бы вернуть {} без удаления live state. Safe policy сейчас специально ждёт возрастной запас.",
            human_bytes(aggressive_bytes as f64)
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_machine_summary(
        disk_available_gib: f64,
        disk_used_percent: Option<f64>,
    ) -> MachineSummary {
        MachineSummary {
            cpu_model: "Synthetic CPU".to_string(),
            logical_cpus: 8,
            physical_cpus: Some(4),
            cpu_usage_percent: Some(12.0),
            cpu_temperature_celsius: None,
            cpu_max_mhz: Some(4200.0),
            cpu_source_label: "synthetic".to_string(),
            total_memory_gib: 64.0,
            available_memory_gib: 48.0,
            used_memory_gib: 16.0,
            memory_used_percent: Some(25.0),
            memory_type: "DDR5".to_string(),
            memory_speed_label: "5600 MT/s".to_string(),
            memory_source_label: "synthetic".to_string(),
            swap_total_gib: 16.0,
            swap_used_gib: 0.0,
            disk_device: Some("/dev/nvme0n1".to_string()),
            disk_model: "Synthetic NVMe".to_string(),
            disk_kind: "NVMe SSD".to_string(),
            disk_source_label: "synthetic".to_string(),
            disk_total_gib: 1900.0,
            disk_available_gib,
            disk_used_percent,
            disk_busy_percent: None,
            disk_read_mib_per_sec: None,
            disk_write_mib_per_sec: None,
            disk_temperature_celsius: None,
            disk_firmware: "test".to_string(),
            accelerators: Vec::<AcceleratorSummary>::new(),
        }
    }

    #[test]
    fn artifact_cleanup_warning_surfaces_large_unmanaged_root() {
        let snapshot = json!({
            "artifact_cleanup": {
                "selected_reclaimable_bytes": 0,
                "aggressive_preview_reclaimable_bytes": 0,
                "repo_inventory": {
                    "out_of_policy_bytes": 200_239_479_576u64,
                    "unmanaged_alert_triggered": true,
                    "large_unmanaged_roots": [
                        {
                            "path": "output/windows-vm-lab",
                            "unmanaged_bytes": 199_715_979_264u64
                        }
                    ],
                    "manual_only_targets": [
                        {
                            "path": "output/windows-vm-lab"
                        }
                    ]
                }
            }
        });
        let warning = artifact_cleanup_warning(&snapshot, None).expect("warning");
        assert!(warning.contains("вне cleanup policy"));
        assert!(warning.contains("output/windows-vm-lab"));
        assert!(
            warning.contains("observe cleanup-artifacts --target output/windows-vm-lab --apply")
        );
    }

    #[test]
    fn artifact_cleanup_card_surfaces_policy_retained_hot_storage_as_waiting() {
        let snapshot = json!({
            "artifact_cleanup": {
                "captured_at_epoch_ms": 42,
                "selected": 0,
                "selected_reclaimable_bytes": 0,
                "policy_retained_reclaimable_bytes": 18_460_613_632u64,
                "policy_retained_targets": [
                    {
                        "path": "target/debug",
                        "ttl_hours": 168,
                        "keep_latest": 3,
                        "aggressive_preview_reclaimable_bytes": 16_254_702_590u64
                    }
                ],
                "manual_only_reclaimable_bytes": 0,
                "manual_only_reclaimable_targets": [],
                "expired": 0,
                "kept_latest": 13,
                "protected": 0,
                "targets_scanned": 8,
                "aggressive_preview_selected": 19,
                "aggressive_preview_reclaimable_bytes": 32_577_450_367u64,
                "last_apply": {
                    "captured_at_epoch_ms": 41,
                    "mode": "aggressive",
                    "deleted": 1,
                    "reclaimed_bytes": 28_888_311_035u64
                },
                "repo_inventory": {
                    "repo_total_bytes": 35_728_482_155u64,
                    "cleanup_scope_bytes": 32_698_373_188u64,
                    "out_of_policy_bytes": 3_030_108_967u64,
                    "unmanaged_alert_triggered": false,
                    "large_unmanaged_roots": [],
                    "manual_only_targets": [
                        {
                            "path": "output/windows-vm-lab",
                            "ttl_hours": 24,
                            "keep_latest": 2,
                            "total_bytes": 15_079_381u64
                        }
                    ],
                    "unreadable_paths_count": 1
                }
            }
        });
        let cards = build_machine_cards(&snapshot, None, None);
        let cleanup_card = cards
            .iter()
            .find(|card| card["title"].as_str() == Some("Локальный мусор и retention"))
            .expect("cleanup card");
        assert_eq!(cleanup_card["status"].as_str(), Some("waiting"));
        assert_eq!(cleanup_card["value"].as_str(), Some("17.19 GiB ждёт TTL"));
        let operator_row = cleanup_card["rows"]
            .as_array()
            .expect("cleanup rows")
            .iter()
            .find(|row| row["label"].as_str() == Some("Operator reclaim next"))
            .expect("operator reclaim row");
        assert!(
            operator_row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("observe cleanup-artifacts --target target/debug --aggressive --apply")
        );
        let warning = artifact_cleanup_warning(&snapshot, None).expect("warning");
        assert!(warning.contains("policy-covered"));
        assert!(warning.contains("TTL/keep-latest"));
        assert!(warning.contains("target/debug"));
        assert!(warning.contains("--aggressive --apply"));
    }

    #[test]
    fn artifact_cleanup_card_escalates_policy_retained_hot_storage_under_disk_pressure() {
        let snapshot = json!({
            "artifact_cleanup": {
                "captured_at_epoch_ms": 42,
                "selected": 0,
                "selected_reclaimable_bytes": 0,
                "policy_retained_reclaimable_bytes": 18_460_613_632u64,
                "policy_retained_targets": [
                    {
                        "path": "target/debug",
                        "ttl_hours": 168,
                        "keep_latest": 3,
                        "aggressive_preview_reclaimable_bytes": 16_254_702_590u64
                    }
                ],
                "manual_only_reclaimable_bytes": 0,
                "manual_only_reclaimable_targets": [],
                "disk_pressure_thresholds": {
                    "alert_used_percent": 85.0,
                    "critical_used_percent": 92.0,
                    "alert_available_gib": 150.0,
                    "critical_available_gib": 60.0
                },
                "expired": 0,
                "kept_latest": 13,
                "protected": 0,
                "targets_scanned": 8,
                "aggressive_preview_selected": 19,
                "aggressive_preview_reclaimable_bytes": 32_577_450_367u64,
                "last_apply": {
                    "captured_at_epoch_ms": 41,
                    "mode": "aggressive",
                    "deleted": 1,
                    "reclaimed_bytes": 28_888_311_035u64
                },
                "repo_inventory": {
                    "repo_total_bytes": 35_728_482_155u64,
                    "cleanup_scope_bytes": 32_698_373_188u64,
                    "out_of_policy_bytes": 3_030_108_967u64,
                    "unmanaged_alert_triggered": false,
                    "large_unmanaged_roots": [],
                    "manual_only_targets": [],
                    "unreadable_paths_count": 1
                }
            }
        });
        let machine = synthetic_machine_summary(48.0, Some(94.0));
        let cards = build_machine_cards(&snapshot, Some(&machine), None);
        let cleanup_card = cards
            .iter()
            .find(|card| card["title"].as_str() == Some("Локальный мусор и retention"))
            .expect("cleanup card");
        assert_eq!(cleanup_card["status"].as_str(), Some("critical"));
        let warning = artifact_cleanup_warning(&snapshot, Some(&machine)).expect("warning");
        assert!(warning.contains("давление"));
        assert!(warning.contains("target/debug"));
        assert!(warning.contains("--aggressive --apply"));
    }

    #[test]
    fn artifact_cleanup_card_surfaces_unreadable_samples_as_best_effort_note() {
        let snapshot = json!({
            "artifact_cleanup": {
                "captured_at_epoch_ms": 42,
                "selected": 0,
                "selected_reclaimable_bytes": 0,
                "policy_retained_reclaimable_bytes": 18_460_613_632u64,
                "policy_retained_targets": [
                    {
                        "path": "target/debug",
                        "ttl_hours": 168,
                        "keep_latest": 3,
                        "aggressive_preview_reclaimable_bytes": 16_254_702_590u64
                    }
                ],
                "manual_only_reclaimable_bytes": 0,
                "manual_only_reclaimable_targets": [],
                "expired": 0,
                "kept_latest": 13,
                "protected": 0,
                "targets_scanned": 8,
                "aggressive_preview_selected": 19,
                "aggressive_preview_reclaimable_bytes": 32_577_450_367u64,
                "last_apply": {
                    "captured_at_epoch_ms": 41,
                    "mode": "aggressive",
                    "deleted": 1,
                    "reclaimed_bytes": 28_888_311_035u64
                },
                "repo_inventory": {
                    "repo_total_bytes": 35_728_482_155u64,
                    "cleanup_scope_bytes": 32_698_373_188u64,
                    "out_of_policy_bytes": 3_030_108_967u64,
                    "unmanaged_alert_triggered": false,
                    "large_unmanaged_roots": [],
                    "manual_only_targets": [],
                    "unreadable_paths_count": 1,
                    "unreadable_paths_sample": [
                        "/home/art/agent-memory-index/state/postgres/pgdata"
                    ]
                }
            }
        });
        let cards = build_machine_cards(&snapshot, None, None);
        let cleanup_card = cards
            .iter()
            .find(|card| card["title"].as_str() == Some("Локальный мусор и retention"))
            .expect("cleanup card");
        assert!(
            cleanup_card["note"]
                .as_str()
                .unwrap_or_default()
                .contains("best-effort lower bound")
        );
        let unreadable_row = cleanup_card["rows"]
            .as_array()
            .expect("cleanup rows")
            .iter()
            .find(|row| row["label"].as_str() == Some("Unreadable sample"))
            .expect("unreadable sample row");
        assert_eq!(
            unreadable_row["value"].as_str(),
            Some("/home/art/agent-memory-index/state/postgres/pgdata")
        );
    }
}
