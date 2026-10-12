//! Fixed-block decisions over retained external invocations.
use super::interval::*;
use super::{Check, Packet, case_complete};
use serde_json::{Value, json};
pub(super) fn summarize(packet: &Packet) -> Vec<Value> {
    let mut summaries = Vec::new();
    let family =
        packet.plan.cases.len() * (packet.plan.candidates.len() + usize::from(packet.plan.aa));
    for (ci, case) in packet.plan.cases.iter().enumerate() {
        let case_start = summaries.len();
        for arm in 1..packet.plan.candidates.len() + 1 + usize::from(packet.plan.aa) {
            let mut base = Vec::new();
            let mut values = Vec::new();
            for block in 0..packet.plan.blocks {
                let rows: Vec<_> = packet
                    .invocations
                    .iter()
                    .filter(|i| i.case == ci && i.block == block)
                    .collect();
                if rows.len() != packet.plan.candidates.len() + 1 + usize::from(packet.plan.aa)
                    || rows.iter().any(|i| i.status != "completed")
                {
                    continue;
                }
                base.push(mean(&rows.iter().find(|i| i.arm == 0).unwrap().values_ns));
                values.push(mean(&rows.iter().find(|i| i.arm == arm).unwrap().values_ns));
            }
            let ratios: Vec<_> = base.iter().zip(&values).map(|(b, a)| a / b).collect();
            let logs: Vec<_> = base
                .iter()
                .zip(&values)
                .map(|(b, a)| a.ln() - b.ln())
                .collect();
            let avg = (!logs.is_empty()).then(|| mean(&logs));
            let sd = (logs.len() > 1).then(|| {
                (logs.iter().map(|r| (r - avg.unwrap()).powi(2)).sum::<f64>()
                    / (logs.len() - 1) as f64)
                    .sqrt()
            });
            let mut issues = Vec::<String>::new();
            let mut interval = Value::Null;
            let mut verdict = "exploratory";
            if packet.mode == "confirm" {
                if !case_complete(packet, ci) {
                    issues.push("incomplete planned blocks or failed correctness".into());
                }
                if !matches!(
                    packet.placement["status"].as_str(),
                    Some("uniform_hardware" | "enforced_uniform")
                ) {
                    issues.push("unverified CPU placement".into());
                }
                if packet.invocations.iter().filter(|i| i.case == ci).any(|i| {
                    i.observations.len() < 3
                        || !i.observations.iter().any(|o| o["phase"] == "during")
                        || i.observations.iter().any(|o| {
                            if o["phase"] == "exit_transition" || o["phase"] == "start_transition" {
                                return o["used_for_admission"] != false;
                            }
                            o["clean"] != true
                                || o["policy"] != "whole-host-strict-v1"
                                || o["used_for_admission"] != true
                        })
                }) {
                    issues.push("missing, stale or unclean resource observations".into());
                }
                if let Some(config) = &packet.plan.confirmation {
                    interval = self::interval(
                        &base,
                        &values,
                        packet.plan.metric.resolution.unwrap_or(0.0)
                            * packet.plan.metric.scale().unwrap_or(0.0),
                        config.family_confidence,
                        family,
                    )
                    .unwrap_or(Value::Null);
                    if interval.is_null() {
                        issues.push("invalid interval or unresolved metric resolution".into());
                    }
                    verdict = if issues.is_empty() {
                        classify(&interval, config.margin_pct)
                    } else {
                        "invalid"
                    };
                } else {
                    issues.push("missing confirmation contract".into());
                    verdict = "invalid";
                }
            }
            summaries.push(json!({"case":case.id,"arm":packet.plan.candidates.get(arm-1).map(|a|a.id.as_str()).unwrap_or("__aa"),
                "complete_blocks":values.len(),"planned_blocks":packet.plan.blocks,"mean_log_ratio":avg,
                "latency_change_pct":avg.map(|r|r.exp_m1()*100.0).filter(|v|v.is_finite()),"log_ratio_sd":sd,
                "baseline_ns":describe(&base),"arm_ns":describe(&values),"paired_ratio":describe(&ratios),
                "interval":interval,"verdict":verdict,"validity_issues":issues,"inference":if packet.mode=="confirm" {"fixed_block"} else {"exploratory"},
                "control_present":packet.plan.aa,"correctness_checked":!matches!(packet.plan.check,Check::Unchecked{..})}));
        }
        if packet.plan.aa {
            let aa = summaries
                .last()
                .and_then(|s| s["latency_change_pct"].as_f64());
            for row in &mut summaries[case_start..] {
                row["effect_exceeds_control_diagnostic"] = json!(
                    aa.zip(row["latency_change_pct"].as_f64())
                        .map(|(aa, ab)| ab.abs() > aa.abs())
                );
            }
        }
        if packet.mode == "confirm" {
            let control = &summaries[summaries.len() - 1];
            let margin = packet
                .plan
                .confirmation
                .as_ref()
                .map(|c| c.control_margin_pct)
                .unwrap_or(0.0);
            let valid = control["verdict"] != "invalid";
            let equivalent = valid
                && control["interval"]["lower_pct"]
                    .as_f64()
                    .is_some_and(|v| v >= -margin)
                && control["interval"]["upper_pct"]
                    .as_f64()
                    .is_some_and(|v| v <= margin);
            for row in &mut summaries[case_start..] {
                row["control_equivalent"] = json!(equivalent);
                if row["verdict"] != "invalid" && !equivalent {
                    row["verdict"] = json!("inconclusive");
                    row["validity_issues"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!("A/A equivalence not established"));
                }
            }
        }
    }
    summaries
}
pub(super) fn exit_code(packet: &Packet) -> i32 {
    if !packet.complete {
        return 2;
    }
    if packet.mode == "iterate" {
        return 0;
    }
    let rows: Vec<_> = packet
        .summaries
        .iter()
        .filter(|s| s["arm"] != "__aa")
        .collect();
    if rows.is_empty()
        || rows.iter().any(|s| {
            matches!(
                s["verdict"].as_str(),
                None | Some("invalid" | "inconclusive" | "exploratory")
            )
        })
    {
        return 2;
    }
    if rows.iter().any(|s| s["verdict"] == "regression") {
        return 1;
    }
    let accept = packet
        .plan
        .confirmation
        .as_ref()
        .map(|c| c.accept.as_str())
        .unwrap_or("");
    if rows.iter().all(|s| match accept {
        "improvement" => s["verdict"] == "improvement",
        "equivalence" => s["verdict"] == "equivalent",
        "non_inferiority" => matches!(
            s["verdict"].as_str(),
            Some("improvement" | "equivalent" | "non_inferior")
        ),
        _ => false,
    }) {
        0
    } else {
        2
    }
}
