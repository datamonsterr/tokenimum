use super::{Baseline, BenchError, ModelPrice, Transcript, types::invalid};
use std::collections::{BTreeMap, BTreeSet};

/// Sum intact transcripts, attributing nested agents to their top-level spawning session.
///
/// # Errors
/// Refuses duplicate IDs, missing parents, cycles, unparented `so/*` agents and missing prices.
pub fn account(
    transcripts: &[Transcript],
    prices: &BTreeMap<String, ModelPrice>,
) -> Result<Baseline, BenchError> {
    let mut index = BTreeMap::new();
    for transcript in transcripts {
        if index
            .insert(transcript.spec.id.as_str(), transcript)
            .is_some()
        {
            return Err(invalid("duplicate session identifier"));
        }
    }
    let mut baseline = Baseline::default();
    let mut bounds: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    for transcript in transcripts {
        let mut root = transcript;
        let mut seen = BTreeSet::new();
        let mut chain_intact = true;
        loop {
            chain_intact &= root.intact;
            if !seen.insert(root.spec.id.as_str()) {
                return Err(invalid("agent parent cycle"));
            }
            if root.spec.id.starts_with("so/") && root.spec.parent.is_none() {
                return Err(invalid("agent session requires explicit parent"));
            }
            let Some(parent) = root.spec.parent.as_deref() else {
                break;
            };
            root = index
                .get(parent)
                .copied()
                .ok_or_else(|| invalid("missing spawning session"))?;
        }
        if !chain_intact {
            baseline.skipped.insert(
                transcript.spec.id.clone(),
                "incomplete session or spawning session".into(),
            );
            continue;
        }
        let metrics = &transcript.metrics;
        let target = baseline.sessions.entry(root.spec.id.clone()).or_default();
        target.tokens.add(metrics.tokens);
        target.turns += metrics.turns;
        target.tool_calls += metrics.tool_calls;
        target.tool_errors += metrics.tool_errors;
        let range = bounds
            .entry(root.spec.id.clone())
            .or_insert((root.started_ms, root.ended_ms));
        range.0 = range.0.min(transcript.started_ms);
        range.1 = range.1.max(transcript.ended_ms);
        target.wall_ms = range.1.abs_diff(range.0);
        for (model, tokens) in &metrics.by_model {
            let cost = prices
                .get(model)
                .ok_or_else(|| invalid("missing recorded model price"))?
                .cost(*tokens)?;
            target.cost_usd += cost;
            target
                .by_model
                .entry(model.clone())
                .or_default()
                .add(*tokens);
            baseline
                .by_model
                .entry(model.clone())
                .or_default()
                .add(*tokens);
            baseline.cost_usd += cost;
        }
        baseline.total.add(metrics.tokens);
    }
    Ok(baseline)
}
