use super::*;

pub(super) fn model_token_savings_metric_row(scope_summary: &Value, alignment: &Value) -> Value {
    let observed_with_amai = scope_summary["verified_observed_whole_cycle_with_amai_tokens"]
        .as_u64()
        .unwrap_or(0);
    let tooltip = model_token_savings_tooltip(scope_summary, alignment);
    let preliminary = scope_summary["preliminary"].as_bool().unwrap_or(false);
    let strict_components =
        human_client_limit_components(&alignment["strict_client_meter_slice"]["components"]);

    let value = if let Some((verified_without, verified_with, verified_saved, _verified_pct)) =
        exact_model_token_pair(scope_summary, alignment)
    {
        let prefix = if preliminary {
            "Предварительный учтённый same-meter срез"
        } else {
            "Учтённый same-meter срез"
        };
        let scope_suffix = strict_components
            .as_deref()
            .map(|components| format!(" ({components})"))
            .unwrap_or_default();
        format!(
            "{prefix}{scope_suffix}: без Amai {}, с Amai {}, экономия {}",
            format_u64(Some(verified_without)),
            format_u64(Some(verified_with)),
            format_signed_count(Some(verified_saved))
        )
    } else if observed_with_amai > 0 {
        format!(
            "Точного процента пока нет; с Amai уже видно {}",
            format_u64(Some(observed_with_amai))
        )
    } else {
        "Точного процента пока нет".to_string()
    };

    metric_row("Экономия токенов модели", value, Some(tooltip.as_str()))
}

pub(super) fn model_token_savings_note_sentence(
    scope_summary: &Value,
    alignment: &Value,
) -> Option<String> {
    let observed_with_amai = scope_summary["verified_observed_whole_cycle_with_amai_tokens"]
        .as_u64()
        .unwrap_or(0);
    let preliminary = scope_summary["preliminary"].as_bool().unwrap_or(false);
    let counted_events = scope_summary["counted_events"].as_u64().unwrap_or(0);
    let events_total = scope_summary["events_total"].as_u64().unwrap_or(0);

    if let Some((verified_without, verified_with, verified_saved, _verified_pct)) =
        exact_model_token_pair(scope_summary, alignment)
    {
        let mut note = format!(
            "Здесь уже есть полное совпадение с реальной шкалой лимита модели для учтённого среза: без Amai было {}, с Amai стало {}, экономия {}.",
            format_u64(Some(verified_without)),
            format_u64(Some(verified_with)),
            format_signed_count(Some(verified_saved))
        );
        if let Some(components) =
            human_client_limit_components(&alignment["strict_client_meter_slice"]["components"])
        {
            note.push(' ');
            note.push_str(&format!(
                "В exact-пару здесь вошёл только strict same-meter срез: {components}."
            ));
        }
        if preliminary {
            note.push(' ');
            note.push_str(&format!(
                "Это пока предварительная выборка: учтено {} из {} событий, поэтому этот процент нельзя читать как экономию всей сессии целиком.",
                format_u64(Some(counted_events)),
                format_u64(Some(events_total))
            ));
        }
        return Some(note);
    }

    if observed_with_amai > 0 {
        let mut note = format!(
            "Точный процент экономии токенов модели здесь пока не показан: с Amai уже честно видно {}, но полного совпадения с реальной шкалой лимита модели для этого scope ещё нет.",
            format_u64(Some(observed_with_amai))
        );
        if let Some(blocker_sentence) = exact_pair_primary_blocker_note_sentence(alignment) {
            note.push(' ');
            note.push_str(&blocker_sentence);
        }
        return Some(note);
    }

    let mut note = String::from(
        "Точный процент экономии токенов модели здесь пока не показан: полное совпадение с реальной шкалой лимита модели для этого scope ещё не собрано.",
    );
    if let Some(blocker_sentence) = exact_pair_primary_blocker_note_sentence(alignment) {
        note.push(' ');
        note.push_str(&blocker_sentence);
    }
    Some(note)
}

fn model_token_savings_tooltip(statement_preview: &Value, alignment: &Value) -> String {
    let observed_with_amai = statement_preview["verified_observed_whole_cycle_with_amai_tokens"]
        .as_u64()
        .unwrap_or(0);
    let preliminary = statement_preview["preliminary"].as_bool().unwrap_or(false);
    let counted_events = statement_preview["counted_events"].as_u64().unwrap_or(0);
    let events_total = statement_preview["events_total"].as_u64().unwrap_or(0);

    if let Some((verified_without, verified_with, verified_saved, _verified_pct)) =
        exact_model_token_pair(statement_preview, alignment)
    {
        let mut tooltip = format!(
            "Этот ряд показывает exact same-meter pair для учтённого среза, а не для всей сессии.\n- Без Amai: {}\n- С Amai: {}\n- Экономия: {}",
            format_u64(Some(verified_without)),
            format_u64(Some(verified_with)),
            format_signed_count(Some(verified_saved))
        );
        if let Some(components) =
            human_client_limit_components(&alignment["strict_client_meter_slice"]["components"])
        {
            tooltip.push_str(&format!("\n- Что вошло в срез: {components}"));
        }
        if preliminary {
            tooltip.push_str(&format!(
                "\n- Статус выборки: preliminary, учтено {} из {} событий",
                format_u64(Some(counted_events)),
                format_u64(Some(events_total))
            ));
        }
        tooltip.push_str(
            "\n- Slice math здесь exact и идёт по тому же meter, которым клиент считает лимит.\n- Сам процент этого slice intentionally не показывается как user-facing claim: процент на карточке разрешён только для строки «Экономия на реальной шкале».",
        );
        return tooltip;
    }

    if observed_with_amai > 0 {
        return format!(
            "Этот ряд показывает точную корреляцию между токенами модели без Amai и с Amai только после materialized same-meter pair. С Amai уже видно {} observed токенов, но exact pair для этого scope ещё не materialized, поэтому процент честно не показывается.",
            format_u64(Some(observed_with_amai))
        );
    }

    "Этот ряд показывает точную корреляцию между токенами модели без Amai и с Amai только после materialized same-meter pair. Пока exact pair для этого scope ещё не materialized, поэтому процент честно не показывается.".to_string()
}

pub(super) fn client_limit_alignment_metric_row(alignment: &Value) -> Option<Value> {
    let state = alignment["alignment_state"].as_str()?;
    let live_events = alignment["live_events_count"].as_u64().unwrap_or(0);
    let non_live_events = alignment["non_live_events_count"].as_u64().unwrap_or(0);
    let value = if alignment["same_meter_as_client_limit"].as_bool() == Some(true) {
        "да".to_string()
    } else {
        match state {
            "no_usage_observed" => "ещё нет usage".to_string(),
            "only_non_live_scope_activity" => format!(
                "нет: только non-live (live {} / non-live {})",
                format_u64(Some(live_events)),
                format_u64(Some(non_live_events))
            ),
            "live_usage_unconfirmed_not_meter_equivalent" => format!(
                "нет: live ещё не подтверждено (live {} / non-live {})",
                format_u64(Some(live_events)),
                format_u64(Some(non_live_events))
            ),
            "partial_lower_bound_not_meter_equivalent" => format!(
                "нет: lower bound части цикла (live {} / non-live {})",
                format_u64(Some(live_events)),
                format_u64(Some(non_live_events))
            ),
            "whole_cycle_partially_observed_not_meter_equivalent" => format!(
                "нет: cycle observed частично (live {} / non-live {})",
                format_u64(Some(live_events)),
                format_u64(Some(non_live_events))
            ),
            "whole_cycle_observed_baseline_partial" => format!(
                "нет: cycle observed, baseline ещё partial (live {} / non-live {})",
                format_u64(Some(live_events)),
                format_u64(Some(non_live_events))
            ),
            "whole_cycle_observed_explicit_boundary_not_meter_equivalent" => format!(
                "нет: strict slice есть, continuity boundary explicit (live {} / non-live {})",
                format_u64(Some(live_events)),
                format_u64(Some(non_live_events))
            ),
            other => format!("нет: {other}"),
        }
    };
    Some(metric_row(
        "Связь с лимитом клиента",
        value,
        client_limit_alignment_tooltip(alignment).as_deref(),
    ))
}

pub(super) fn client_limit_strict_slice_metric_row(alignment: &Value) -> Option<Value> {
    if alignment["strict_client_meter_slice"]["same_meter_equivalent_for_slice"].as_bool()
        != Some(true)
    {
        return None;
    }
    let lower_bound = alignment["strict_client_meter_slice"]["lower_bound_tokens"]
        .as_u64()
        .unwrap_or(0);
    if lower_bound == 0 {
        return None;
    }
    let value = if let Some(components) =
        human_client_limit_components(&alignment["strict_client_meter_slice"]["components"])
    {
        format!("{lower_bound} токенов: {components}")
    } else {
        format!("{lower_bound} токенов")
    };
    Some(metric_row(
        "Строгий same-meter срез",
        value,
        Some(
            "Этот ряд показывает уже materialized strict same-meter lower bound: часть клиентского лимитного метра, где baseline-equivalent semantics уже честно доказаны и не зависят от guessed continuity baseline.",
        ),
    ))
}

pub(super) fn client_limit_explicit_boundary_metric_row(alignment: &Value) -> Option<Value> {
    if alignment["explicit_boundary_surface"]["blocks_full_same_meter_equivalence"].as_bool()
        != Some(true)
    {
        return None;
    }
    let components =
        human_client_limit_components(&alignment["explicit_boundary_surface"]["components"])?;
    let label = if alignment["explicit_boundary_surface"]["state"].as_str()
        == Some("amai_continuity_boundary")
    {
        "Граница continuity"
    } else {
        "Явная baseline-граница"
    };
    Some(metric_row(
        label,
        components,
        alignment["explicit_boundary_surface"]["note"].as_str(),
    ))
}

pub(super) fn client_limit_boundary_tokens_metric_row(alignment: &Value) -> Option<Value> {
    let observed_tokens = if alignment["continuity_boundary_rollup"]["state"].as_str()
        == Some("amai_continuity_boundary_observed")
    {
        alignment["continuity_boundary_rollup"]["observed_tokens"]
            .as_u64()
            .unwrap_or(0)
    } else {
        if alignment["explicit_boundary_surface"]["state"].as_str()
            != Some("amai_continuity_boundary")
        {
            return None;
        }
        let boundary_components =
            alignment["explicit_boundary_surface"]["components"].as_array()?;
        alignment["baseline_equivalence"]["component_semantics"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| {
                item["code"].as_str().is_some_and(|code| {
                    boundary_components
                        .iter()
                        .any(|component| component.as_str() == Some(code))
                })
            })
            .filter(|item| item["whole_cycle_observed_complete"].as_bool() == Some(true))
            .filter_map(|item| item["observed_tokens"].as_u64())
            .sum::<u64>()
    };
    if observed_tokens == 0 {
        return None;
    }
    Some(metric_row(
        "Токены continuity boundary",
        format!(
            "{} токенов вне strict client-meter slice",
            format_u64(Some(observed_tokens))
        ),
        Some(
            "Этот ряд показывает observed token weight для Amai-specific continuity boundary. Эти токены уже честно видны в agent cycle, но не входят в strict same-meter client slice, пока для них нет truthful pre-Amai baseline-equivalent модели.",
        ),
    ))
}

pub(super) fn human_client_limit_component(code: &str) -> Option<&'static str> {
    match code {
        "client_prompt" => Some("исходный запрос клиента"),
        "assistant_generation" => Some("генерация ответа моделью"),
        "tool_overhead_outside_retrieval" => Some("tool/orchestration overhead вне retrieval"),
        "continuity_restore_outside_retrieval" => Some("continuity-restore overhead вне retrieval"),
        _ => None,
    }
}

pub(super) fn human_client_limit_components(node: &Value) -> Option<String> {
    let components = node
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str())
        .filter_map(human_client_limit_component)
        .collect::<Vec<_>>();
    if components.is_empty() {
        None
    } else {
        Some(components.join(", "))
    }
}

pub(super) fn client_limit_alignment_note_sentence(alignment: &Value) -> Option<String> {
    let state = alignment["alignment_state"].as_str()?;
    if alignment["same_meter_as_client_limit"].as_bool() == Some(true) {
        return Some(
            "Этот срез уже materialized в том же meter, которым клиент считает лимит: цифры карточки и точный процент model-token savings теперь коррелируют напрямую."
                .to_string(),
        );
    }
    Some(match state {
        "no_usage_observed" => {
            "Этот срез ещё не видел usage-событий, поэтому сравнивать его со шкалой лимита клиента пока рано.".to_string()
        }
        "only_non_live_scope_activity" => {
            "Сейчас в этом срезе есть только non-live активность, поэтому его цифра не обязана двигаться вместе со шкалой лимита клиента.".to_string()
        }
        "live_usage_unconfirmed_not_meter_equivalent" => {
            "Здесь уже были live-события, но confirmed lower bound ещё не набрался, поэтому эта цифра пока не эквивалентна шкале лимита клиента.".to_string()
        }
        "partial_lower_bound_not_meter_equivalent" => {
            "Даже здесь это пока lower bound части агентного цикла, а не тот же полный метр, которым клиент считает лимит сессии.".to_string()
        }
        "whole_cycle_partially_observed_not_meter_equivalent" => {
            "Здесь уже начали появляться observed whole-cycle компоненты, но покрытие ещё неполное, поэтому эта цифра всё ещё не эквивалентна шкале лимита клиента.".to_string()
        }
        "whole_cycle_observed_explicit_boundary_not_meter_equivalent" => {
            let measured = human_client_limit_components(
                &alignment["baseline_equivalence"]["measured_baseline_components"],
            );
            let boundary = human_client_limit_components(
                &alignment["baseline_equivalence"]["explicitly_unmodeled_baseline_components"],
            );
            match (measured, boundary) {
                (Some(measured), Some(boundary)) => format!(
                    "Здесь whole-cycle компоненты уже fully observed; strict same-meter lower bound уже materialized для {measured}, а для {boundary} boundary сознательно поднят как explicit truth-boundary. Это уже не просто partial baseline, но и не полный client-limit meter."
                ),
                _ => "Здесь whole-cycle observed компоненты уже видны по live событиям, а remaining gap оставлен как explicit truth-boundary, поэтому метрика остаётся честно non-equivalent.".to_string(),
            }
        }
        "whole_cycle_observed_baseline_partial" => {
            if alignment["baseline_equivalence"]["state"].as_str()
                == Some("baseline_semantics_unmaterialized")
            {
                if let Some(fully_observed) = human_client_limit_components(
                    &alignment["baseline_equivalence"]["fully_observed_components"],
                ) {
                    format!(
                        "Здесь applicable whole-cycle компоненты уже полностью observed ({fully_observed}), но baseline всё ещё не эквивалентен полному клиентскому лимиту, поэтому метрика остаётся честно non-equivalent."
                    )
                } else {
                    "Здесь whole-cycle observed компоненты уже видны по live событиям, но baseline всё ещё не эквивалентен полному клиентскому лимиту, поэтому метрика остаётся честно non-equivalent.".to_string()
                }
            } else if alignment["baseline_equivalence"]["state"].as_str()
                == Some("baseline_component_semantics_explicit_boundary")
            {
                let measured = human_client_limit_components(
                    &alignment["baseline_equivalence"]["measured_baseline_components"],
                );
                let boundary = human_client_limit_components(
                    &alignment["baseline_equivalence"]["explicitly_unmodeled_baseline_components"],
                );
                match (measured, boundary) {
                    (Some(measured), Some(boundary)) => format!(
                        "Здесь whole-cycle компоненты уже fully observed; baseline-equivalent semantics уже materialized для {measured}, а для {boundary} gap оставлен как explicit truth-boundary без guessed baseline, поэтому метрика остаётся честно non-equivalent."
                    ),
                    _ => "Здесь whole-cycle observed компоненты уже видны по live событиям, но baseline всё ещё не эквивалентен полному клиентскому лимиту, поэтому метрика остаётся честно non-equivalent.".to_string(),
                }
            } else if alignment["baseline_equivalence"]["state"].as_str()
                == Some("baseline_component_semantics_partial")
            {
                let measured = human_client_limit_components(
                    &alignment["baseline_equivalence"]["measured_baseline_components"],
                );
                let missing = human_client_limit_components(
                    &alignment["baseline_equivalence"]["missing_baseline_components"],
                );
                match (measured, missing) {
                    (Some(measured), Some(missing)) => format!(
                        "Здесь whole-cycle компоненты уже fully observed; baseline-equivalent semantics уже materialized для {measured}, но ещё не materialized для {missing}, поэтому метрика остаётся честно non-equivalent."
                    ),
                    _ => "Здесь whole-cycle observed компоненты уже видны по live событиям, но baseline всё ещё не эквивалентен полному клиентскому лимиту, поэтому метрика остаётся честно non-equivalent.".to_string(),
                }
            } else {
                "Здесь whole-cycle observed компоненты уже видны по live событиям, но baseline всё ещё не эквивалентен полному клиентскому лимиту, поэтому метрика остаётся честно non-equivalent.".to_string()
            }
        }
        other => format!(
            "Этот срез пока не эквивалентен клиентскому лимиту сессии: state={other}."
        ),
    })
}

pub(super) fn client_limit_alignment_tooltip(alignment: &Value) -> Option<String> {
    let state = alignment["alignment_state"].as_str()?;
    if alignment["same_meter_as_client_limit"].as_bool() == Some(true) {
        return Some(
            "Эта строка показывает, обязана ли карточка двигаться в том же метре, которым клиент считает внешний лимит сессии. Сейчас ответ: да.\n- same-meter alignment уже materialized.\n- Exact model-token pair можно читать как тот же meter, которым клиент считает лимит.\n- Remaining explicit boundary для этого scope нет."
                .to_string(),
        );
    }
    let mut reasons = alignment["blocking_reasons"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|reason| reason.as_str())
        .filter_map(human_client_limit_alignment_reason)
        .collect::<Vec<_>>();
    if reasons.is_empty() {
        reasons
            .push("текущий savings-layer всё ещё не совпадает с полным метром клиентского лимита");
    }
    let state_note = match state {
        "no_usage_observed" => "В этом scope ещё нет usage-событий.",
        "only_non_live_scope_activity" => {
            "В этом scope пока есть только non-live события, поэтому карточка не обязана совпадать с внешней шкалой лимита."
        }
        "live_usage_unconfirmed_not_meter_equivalent" => {
            "Live usage уже был, но подтверждённый lower bound ещё не накопился."
        }
        "partial_lower_bound_not_meter_equivalent" => {
            "Даже подтверждённая цифра здесь пока описывает только lower bound части агентного цикла."
        }
        "whole_cycle_partially_observed_not_meter_equivalent" => {
            "Whole-cycle observed компоненты уже начали materialize-иться, но покрытие по live событиям ещё неполное."
        }
        "whole_cycle_observed_explicit_boundary_not_meter_equivalent" => {
            "Whole-cycle observed компоненты уже fully covered, strict same-meter lower bound уже materialized, но remaining gap оставлен как explicit continuity truth-boundary."
        }
        "whole_cycle_observed_baseline_partial" => {
            "Whole-cycle observed компоненты уже видны по live событиям, но baseline-equivalent semantics для клиентского лимита ещё не materialized."
        }
        _ => "Этот scope пока не эквивалентен лимиту клиента.",
    };
    let mut tooltip = String::from(
        "Эта строка показывает, обязана ли карточка двигаться в том же метре, которым клиент считает внешний лимит сессии. Сейчас ответ: нет.",
    );
    tooltip.push('\n');
    tooltip.push_str("- ");
    tooltip.push_str(state_note);
    if alignment["strict_client_meter_slice"]["same_meter_equivalent_for_slice"].as_bool()
        == Some(true)
    {
        let lower_bound = alignment["strict_client_meter_slice"]["lower_bound_tokens"]
            .as_u64()
            .unwrap_or(0);
        let components =
            human_client_limit_components(&alignment["strict_client_meter_slice"]["components"]);
        if lower_bound > 0 {
            tooltip.push('\n');
            tooltip.push_str("- ");
            tooltip.push_str("strict same-meter lower bound уже materialized: ");
            tooltip.push_str(&lower_bound.to_string());
            tooltip.push_str(" токенов");
            if let Some(components) = components {
                tooltip.push_str(" по компонентам ");
                tooltip.push_str(&components);
            }
        }
    }
    if alignment["baseline_equivalence"]["state"].as_str()
        == Some("baseline_semantics_unmaterialized")
    {
        if let Some(fully_observed) = human_client_limit_components(
            &alignment["baseline_equivalence"]["fully_observed_components"],
        ) {
            tooltip.push('\n');
            tooltip.push_str("- ");
            tooltip.push_str("applicable whole-cycle компоненты уже fully observed: ");
            tooltip.push_str(&fully_observed);
        }
    } else if alignment["baseline_equivalence"]["state"].as_str()
        == Some("baseline_component_semantics_explicit_boundary")
    {
        if let Some(measured) = human_client_limit_components(
            &alignment["baseline_equivalence"]["measured_baseline_components"],
        ) {
            tooltip.push('\n');
            tooltip.push_str("- ");
            tooltip.push_str("baseline-equivalent semantics уже materialized для: ");
            tooltip.push_str(&measured);
        }
        if let Some(boundary) = human_client_limit_components(
            &alignment["baseline_equivalence"]["explicitly_unmodeled_baseline_components"],
        ) {
            tooltip.push('\n');
            tooltip.push_str("- ");
            tooltip.push_str("explicit truth-boundary без guessed baseline оставлен для: ");
            tooltip.push_str(&boundary);
        }
    } else if alignment["baseline_equivalence"]["state"].as_str()
        == Some("baseline_component_semantics_partial")
    {
        if let Some(measured) = human_client_limit_components(
            &alignment["baseline_equivalence"]["measured_baseline_components"],
        ) {
            tooltip.push('\n');
            tooltip.push_str("- ");
            tooltip.push_str("baseline-equivalent semantics уже materialized для: ");
            tooltip.push_str(&measured);
        }
        if let Some(missing) = human_client_limit_components(
            &alignment["baseline_equivalence"]["missing_baseline_components"],
        ) {
            tooltip.push('\n');
            tooltip.push_str("- ");
            tooltip.push_str("baseline-equivalent semantics ещё missing для: ");
            tooltip.push_str(&missing);
        }
    } else if alignment["baseline_equivalence"]["state"].as_str()
        == Some("whole_cycle_components_incomplete")
    {
        if let Some(incomplete) = human_client_limit_components(
            &alignment["baseline_equivalence"]["incomplete_components"],
        ) {
            tooltip.push('\n');
            tooltip.push_str("- ");
            tooltip.push_str("whole-cycle coverage ещё incomplete по: ");
            tooltip.push_str(&incomplete);
        }
    }
    for reason in reasons {
        tooltip.push('\n');
        tooltip.push_str("- ");
        tooltip.push_str(reason);
    }
    Some(tooltip)
}

fn human_client_limit_alignment_reason(reason: &str) -> Option<&'static str> {
    match reason {
        "client_prompt_unmeasured" => {
            Some("в этот слой пока не входят токены исходного запроса клиента")
        }
        "assistant_generation_unmeasured" => {
            Some("в этот слой пока не входят токены генерации ответа моделью")
        }
        "tool_overhead_outside_retrieval_unmeasured" => {
            Some("в этот слой пока не входит tool/orchestration overhead вне retrieval")
        }
        "continuity_restore_outside_retrieval_unmeasured" => {
            Some("в этот слой пока не входит continuity-restore overhead вне retrieval")
        }
        "client_prompt_partially_measured" => {
            Some("токены исходного запроса клиента уже видны только на части live-событий")
        }
        "assistant_generation_partially_measured" => {
            Some("токены генерации ответа уже видны только на части live-событий")
        }
        "tool_overhead_outside_retrieval_partially_measured" => {
            Some("tool/orchestration overhead вне retrieval уже виден только на части live-событий")
        }
        "continuity_restore_outside_retrieval_partially_measured" => {
            Some("continuity-restore overhead вне retrieval уже виден только на части live-событий")
        }
        "same_meter_baseline_unmeasured" => Some(
            "whole-cycle observed слой уже виден, но baseline ещё не эквивалентен клиентскому spend meter",
        ),
        "same_meter_baseline_explicit_boundary" => Some(
            "часть same-meter baseline contour оставлена как явная truth-boundary без guessed pre-Amai baseline",
        ),
        "same_meter_baseline_partially_measured" => Some(
            "часть applicable whole-cycle компонентов уже имеет baseline-equivalent semantics, но не весь contour ещё materialized",
        ),
        "no_usage_observed_in_scope" => Some("в этом scope ещё не было usage-событий"),
        "no_live_usage_in_scope" => Some("в этом scope пока нет live usage"),
        "non_live_events_present_in_scope" => Some(
            "в этом scope уже есть non-live события, которые не совпадают с клиентским spend meter",
        ),
        "no_confirmed_live_usage_in_scope" => {
            Some("live usage уже был, но ещё не дошёл до confirmed lane")
        }
        _ => None,
    }
}

pub(super) fn token_lane_summary(
    baseline_tokens: Option<u64>,
    delivered_tokens: Option<u64>,
    recovery_tokens: Option<u64>,
    delta_tokens: Option<i64>,
) -> String {
    match (baseline_tokens, delivered_tokens, recovery_tokens) {
        (Some(baseline), Some(delivered), Some(recovery)) => format!(
            "без Amai {}, от Amai {}, уточнения {}, итог {}",
            format_u64(Some(baseline)),
            format_u64(Some(delivered)),
            format_u64(Some(recovery)),
            format_signed_count(delta_tokens)
        ),
        _ => "ещё нет данных".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn model_token_savings_row_surfaces_exact_meter_equivalent_percent() {
        let statement_preview = json!({
            "verified_without_amai_measured_tokens": 320,
            "verified_with_amai_measured_tokens": 240,
            "verified_observed_whole_cycle_with_amai_tokens": 240
        });
        let alignment = json!({
            "same_meter_as_client_limit": true,
            "continuity_boundary_rollup": {
                "observed_tokens": 0
            }
        });

        let row = model_token_savings_metric_row(&statement_preview, &alignment);
        assert_eq!(row["label"].as_str(), Some("Экономия токенов модели"));
        assert_eq!(
            row["value"].as_str(),
            Some("Учтённый same-meter срез: без Amai 320, с Amai 240, экономия 80")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("Сам процент этого slice intentionally не показывается")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("идёт по тому же meter, которым клиент считает лимит")
        );

        let note = model_token_savings_note_sentence(&statement_preview, &alignment).expect("note");
        assert!(note.contains("учтённого среза"));
        assert!(!note.contains("25.00%"));
    }

    #[test]
    fn model_token_savings_row_prefers_strict_same_meter_lower_bound() {
        let statement_preview = json!({
            "verified_without_amai_measured_tokens": 605,
            "verified_with_amai_measured_tokens": 0,
            "observed_whole_cycle_with_amai_tokens": 605,
            "verified_observed_whole_cycle_with_amai_tokens": 589
        });
        let alignment = json!({
            "same_meter_as_client_limit": true,
            "strict_client_meter_slice": {
                "lower_bound_tokens": 609
            },
            "baseline_equivalence": {
                "measured_baseline_tokens_lower_bound": 609
            }
        });

        let row = model_token_savings_metric_row(&statement_preview, &alignment);
        assert_eq!(
            row["value"].as_str(),
            Some("Учтённый same-meter срез: без Amai 609, с Amai 605, экономия 4")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("Сам процент этого slice intentionally не показывается")
        );
    }

    #[test]
    fn model_token_savings_row_marks_preliminary_same_meter_slice_and_components() {
        let statement_preview = json!({
            "preliminary": true,
            "counted_events": 1,
            "events_total": 1,
            "observed_whole_cycle_with_amai_tokens": 99,
            "verified_observed_whole_cycle_with_amai_tokens": 99
        });
        let alignment = json!({
            "same_meter_as_client_limit": true,
            "strict_client_meter_slice": {
                "lower_bound_tokens": 640,
                "components": [
                    "client_prompt",
                    "continuity_restore_outside_retrieval"
                ]
            },
            "baseline_equivalence": {
                "measured_baseline_tokens_lower_bound": 640
            }
        });

        let row = model_token_savings_metric_row(&statement_preview, &alignment);
        assert!(
            row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("Предварительный учтённый same-meter срез")
        );
        assert!(
            row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("исходный запрос клиента")
        );
        assert!(
            row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("экономия 541")
        );
        assert!(
            row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("continuity-restore overhead вне retrieval")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("Сам процент этого slice intentionally не показывается")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("preliminary, учтено 1 из 1 событий")
        );

        let note = model_token_savings_note_sentence(&statement_preview, &alignment).expect("note");
        assert!(note.contains("strict same-meter срез"));
        assert!(note.contains("нельзя читать как экономию всей сессии целиком"));
        assert!(!note.contains("84.53%"));
    }

    #[test]
    fn model_token_savings_row_hides_percent_until_exact_pair_materializes() {
        let statement_preview = json!({
            "verified_baseline_tokens": 320,
            "verified_delivered_tokens": 248,
            "verified_recovery_tokens": 8,
            "verified_effective_saved_tokens": 64,
            "verified_effective_savings_pct": 20.0,
            "verified_observed_whole_cycle_with_amai_tokens": 609
        });
        let alignment = json!({
            "same_meter_as_client_limit": false,
            "explicit_boundary_surface": {
                "state": "amai_continuity_boundary"
            },
            "continuity_boundary_rollup": {
                "observed_tokens": 609
            }
        });

        let row = model_token_savings_metric_row(&statement_preview, &alignment);
        assert_eq!(
            row["value"].as_str(),
            Some("Точного процента пока нет; с Amai уже видно 609")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("exact pair для этого scope ещё не materialized")
        );

        let note = model_token_savings_note_sentence(&statement_preview, &alignment).expect("note");
        assert!(note.contains("Точный процент"));
        assert!(note.contains("реальной шкалой лимита модели"));
    }

    #[test]
    fn model_token_note_surfaces_primary_exact_pair_blocker() {
        let statement_preview = json!({
            "verified_observed_whole_cycle_with_amai_tokens": 3524046
        });
        let alignment = json!({
            "same_meter_as_client_limit": false,
            "exact_pair_status": {
                "blockers": [{
                    "code": "tool_overhead_outside_retrieval",
                    "missing_live_events": 36,
                    "irrecoverable_missing_live_events": 13
                }]
            }
        });

        let note = model_token_savings_note_sentence(&statement_preview, &alignment).expect("note");
        assert!(note.contains("missing 36 live events"));
        assert!(note.contains("13 irrecoverable"));
        assert!(note.contains("23 ещё recoverable"));
    }

    #[test]
    fn client_limit_alignment_tooltip_surfaces_explicit_baseline_boundary_components() {
        let alignment = json!({
            "alignment_state": "whole_cycle_observed_explicit_boundary_not_meter_equivalent",
            "same_meter_as_client_limit": false,
            "live_events_count": 79,
            "non_live_events_count": 0,
            "strict_client_meter_slice": {
                "same_meter_equivalent_for_slice": true,
                "lower_bound_tokens": 316,
                "components": ["client_prompt"]
            },
            "blocking_reasons": [
                "same_meter_baseline_explicit_boundary"
            ],
            "baseline_equivalence": {
                "state": "baseline_component_semantics_explicit_boundary",
                "measured_baseline_components": [
                    "client_prompt"
                ],
                "explicitly_unmodeled_baseline_components": [
                    "continuity_restore_outside_retrieval"
                ],
                "remaining_gap_reason": "same_meter_baseline_explicit_boundary"
            }
        });

        let tooltip =
            client_limit_alignment_tooltip(&alignment).expect("baseline equivalence tooltip");
        assert!(tooltip.contains("исходный запрос клиента"));
        assert!(tooltip.contains("continuity-restore overhead вне retrieval"));
        assert!(tooltip.contains("explicit truth-boundary"));
        assert!(tooltip.contains("strict same-meter lower bound уже materialized"));
        assert!(tooltip.contains("316 токенов"));

        let note =
            client_limit_alignment_note_sentence(&alignment).expect("baseline equivalence note");
        assert!(note.contains("explicit truth-boundary"));
        assert!(note.contains("не просто partial baseline"));
    }

    #[test]
    fn client_limit_extra_rows_surface_strict_slice_and_continuity_boundary() {
        let alignment = json!({
            "strict_client_meter_slice": {
                "same_meter_equivalent_for_slice": true,
                "lower_bound_tokens": 320,
                "components": ["client_prompt"]
            },
            "explicit_boundary_surface": {
                "state": "amai_continuity_boundary",
                "blocks_full_same_meter_equivalence": true,
                "components": ["continuity_restore_outside_retrieval"],
                "note": "Continuity boundary."
            }
        });

        let strict_row = client_limit_strict_slice_metric_row(&alignment).expect("strict row");
        assert_eq!(
            strict_row["label"].as_str(),
            Some("Строгий same-meter срез")
        );
        assert!(
            strict_row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("320")
        );
        assert!(
            strict_row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("исходный запрос клиента")
        );

        let boundary_row =
            client_limit_explicit_boundary_metric_row(&alignment).expect("boundary row");
        assert_eq!(boundary_row["label"].as_str(), Some("Граница continuity"));
        assert!(
            boundary_row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("continuity-restore overhead вне retrieval")
        );

        let boundary_tokens_row = client_limit_boundary_tokens_metric_row(&json!({
            "explicit_boundary_surface": {
                "state": "amai_continuity_boundary",
                "components": ["continuity_restore_outside_retrieval"]
            },
            "baseline_equivalence": {
                "component_semantics": [
                    {
                        "code": "continuity_restore_outside_retrieval",
                        "whole_cycle_observed_complete": true,
                        "observed_tokens": 50329
                    }
                ]
            }
        }))
        .expect("boundary tokens row");
        assert_eq!(
            boundary_tokens_row["label"].as_str(),
            Some("Токены continuity boundary")
        );
        assert!(
            boundary_tokens_row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("50329")
        );
    }
}
