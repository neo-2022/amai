use crate::retrieval_science;
use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvalPattern {
    RetrievalTarget,
    RecoveryTarget,
    IsolationBoundary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalVerdict {
    pub class_key: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct EvalSignals {
    pub expected_present: Option<bool>,
    pub unexpected_present: bool,
    pub boundary_clean: Option<bool>,
    pub fail_closed_ok: Option<bool>,
    pub has_expected_target: bool,
}

impl EvalSignals {
    pub fn from_details(details: &Value, has_expected_target: bool) -> Self {
        Self {
            expected_present: details["expected_present"].as_bool(),
            unexpected_present: details["unexpected_present"].as_bool().unwrap_or(false),
            boundary_clean: details["boundary_clean"].as_bool(),
            fail_closed_ok: details["fail_closed_ok"].as_bool(),
            has_expected_target,
        }
    }
}

pub fn derive_eval_verdict(pattern: EvalPattern, signals: &EvalSignals) -> Result<EvalVerdict> {
    let (class_key, reason) = match pattern {
        EvalPattern::RetrievalTarget => {
            if signals.expected_present == Some(true) && signals.unexpected_present {
                (
                    "over_included",
                    "Нужная цель нашлась, но вместе с ней приехал лишний или конфликтующий контекст.".to_string(),
                )
            } else if signals.expected_present == Some(true) {
                (
                    "hit_correct_target",
                    "Задача получила именно нужную retrieval-цель без конфликтующего шума."
                        .to_string(),
                )
            } else if signals.unexpected_present {
                (
                    "hit_wrong_target",
                    "Вместо нужной retrieval-цели пришёл чужой или запрещённый target.".to_string(),
                )
            } else if signals.has_expected_target {
                (
                    "under_retrieved",
                    "Нужная retrieval-цель не подтянулась в ответе.".to_string(),
                )
            } else {
                (
                    "not_useful",
                    "Контур не дал полезного retrieval-результата.".to_string(),
                )
            }
        }
        EvalPattern::RecoveryTarget => {
            if signals.expected_present == Some(true) && signals.unexpected_present {
                (
                    "over_included",
                    "Актуальная цель восстановилась, но вместе с ней осталась старая или лишняя версия.".to_string(),
                )
            } else if signals.expected_present == Some(true) {
                (
                    "recovered_useful",
                    "Amai восстановил актуальное рабочее состояние или свежий факт в полезном виде.".to_string(),
                )
            } else if signals.unexpected_present {
                (
                    "stale_target",
                    "Восстановление привело к устаревшей цели вместо текущей authoritative версии."
                        .to_string(),
                )
            } else if signals.has_expected_target {
                (
                    "under_retrieved",
                    "Нужное восстановление не дало целевой факт или рабочий статус.".to_string(),
                )
            } else {
                (
                    "not_useful",
                    "Контур восстановления не дал полезного результата.".to_string(),
                )
            }
        }
        EvalPattern::IsolationBoundary => {
            let boundary_clean = signals.boundary_clean.unwrap_or(false);
            let fail_closed_ok = signals.fail_closed_ok.unwrap_or(boundary_clean);
            if !boundary_clean || signals.unexpected_present {
                (
                    "hit_wrong_target",
                    "Изоляционная граница нарушилась и контур выдал чужой или запрещённый результат.".to_string(),
                )
            } else if signals.has_expected_target && signals.expected_present != Some(true) {
                (
                    "under_retrieved",
                    "Граница сохранилась, но нужная цель внутри своего контура не восстановилась."
                        .to_string(),
                )
            } else if fail_closed_ok {
                (
                    "hit_correct_target",
                    "Нужный контур сохранился изолированным и не протащил чужой результат."
                        .to_string(),
                )
            } else {
                (
                    "not_useful",
                    "Изоляционный контур не дал достаточно определённого полезного результата."
                        .to_string(),
                )
            }
        }
    };
    retrieval_science::validate_eval_verdict_class(class_key)?;
    Ok(EvalVerdict {
        class_key: class_key.to_string(),
        reason,
    })
}

pub fn summarize_eval_layer<I, S>(verdict_classes: I) -> Result<Value>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let catalog = retrieval_science::eval_verdict_catalog_json()?;
    let verdict_order = catalog["classes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["class_key"].as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    let mut verdict_counts = BTreeMap::<String, usize>::new();
    for verdict_class in verdict_classes {
        *verdict_counts
            .entry(verdict_class.as_ref().to_string())
            .or_default() += 1;
    }
    Ok(json!({
        "eval_verdict_model_version": catalog["eval_verdict_model_version"].clone(),
        "verdict_order": verdict_order,
        "verdict_counts": verdict_counts,
        "verdict_catalog": catalog["classes"].clone(),
    }))
}

#[cfg(test)]
mod tests {
    use super::{EvalPattern, EvalSignals, derive_eval_verdict, summarize_eval_layer};

    #[test]
    fn canonical_eval_layer_reaches_all_requested_verdict_classes() {
        let cases = [
            (
                EvalPattern::RetrievalTarget,
                EvalSignals {
                    expected_present: Some(true),
                    has_expected_target: true,
                    ..EvalSignals::default()
                },
                "hit_correct_target",
            ),
            (
                EvalPattern::RetrievalTarget,
                EvalSignals {
                    unexpected_present: true,
                    has_expected_target: true,
                    ..EvalSignals::default()
                },
                "hit_wrong_target",
            ),
            (
                EvalPattern::RecoveryTarget,
                EvalSignals {
                    unexpected_present: true,
                    has_expected_target: true,
                    ..EvalSignals::default()
                },
                "stale_target",
            ),
            (
                EvalPattern::RetrievalTarget,
                EvalSignals {
                    has_expected_target: true,
                    ..EvalSignals::default()
                },
                "under_retrieved",
            ),
            (
                EvalPattern::RetrievalTarget,
                EvalSignals {
                    expected_present: Some(true),
                    unexpected_present: true,
                    has_expected_target: true,
                    ..EvalSignals::default()
                },
                "over_included",
            ),
            (
                EvalPattern::RecoveryTarget,
                EvalSignals {
                    expected_present: Some(true),
                    has_expected_target: true,
                    ..EvalSignals::default()
                },
                "recovered_useful",
            ),
            (
                EvalPattern::RetrievalTarget,
                EvalSignals::default(),
                "not_useful",
            ),
        ];

        for (pattern, signals, expected_class) in cases {
            let verdict = derive_eval_verdict(pattern, &signals).expect("verdict");
            assert_eq!(verdict.class_key, expected_class);
        }
    }

    #[test]
    fn summarize_eval_layer_reports_counts_and_catalog() {
        let summary =
            summarize_eval_layer(["hit_correct_target", "over_included", "hit_correct_target"])
                .expect("summary");
        assert_eq!(
            summary["eval_verdict_model_version"].as_str(),
            Some("memory-eval-verdict-v1")
        );
        assert_eq!(
            summary["verdict_counts"]["hit_correct_target"].as_u64(),
            Some(2)
        );
        assert_eq!(summary["verdict_counts"]["over_included"].as_u64(), Some(1));
        assert!(summary["verdict_catalog"].as_array().is_some());
    }
}
