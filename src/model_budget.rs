//! Session-wide dispatch guards. Estimates/reservations are not provider billing caps.
use crate::{
    store::{Session, Store},
    usage::{Call, Scope},
};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub session: Limits,
}
impl Config {
    pub fn is_empty(&self) -> bool {
        !self.session.enabled()
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub requests: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub estimated_cost_usd: Option<f64>,
}
impl Limits {
    pub fn enabled(&self) -> bool {
        self.requests.is_some()
            || self.input_tokens.is_some()
            || self.output_tokens.is_some()
            || self.estimated_cost_usd.is_some()
    }
    pub fn validate(&self) -> Result<()> {
        if self
            .estimated_cost_usd
            .is_some_and(|v| !v.is_finite() || v < 0.0)
        {
            bail!("budgets.session.estimated_cost_usd must be finite and nonnegative");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reservation {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub estimated_cost_usd: Option<f64>,
}
#[derive(Debug)]
pub struct Blocked(pub String);
impl std::fmt::Display for Blocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f,"Session model budget blocked dispatch: {}. No model request sent. Review budgets.session in project configuration and restart/resume explicitly. All branches and request purposes share this budget; retries consume it too.",self.0)
    }
}
impl std::error::Error for Blocked {}
#[derive(Debug)]
struct Totals {
    requests: u64,
    input: Option<u64>,
    output: Option<u64>,
    cost: Option<f64>,
}
fn totals(calls: &[Call]) -> Totals {
    let mut total = Totals {
        requests: 0,
        input: Some(0),
        output: Some(0),
        cost: Some(0.0),
    };
    for call in calls.iter().filter(|c| c.status != "not-sent") {
        total.requests = total.requests.saturating_add(1);
        let reserve = call.reservation.as_ref();
        let complete = call.status == "response-received";
        let input = if complete {
            call.counts.input.value
        } else {
            reserve
                .map(|r| r.input_tokens.max(call.counts.input.value.unwrap_or(0)))
                .or(call.counts.input.value)
        };
        let output = if complete {
            call.counts.output.value
        } else {
            reserve.map(|r| r.output_tokens.max(call.counts.output.value.unwrap_or(0)))
        };
        let compatible = call
            .metadata
            .pricing
            .as_ref()
            .is_some_and(|p| p.currency == "USD")
            && call
                .response_model
                .as_ref()
                .is_none_or(|m| m == &call.model)
            && !call
                .counts
                .notes
                .iter()
                .any(|n| n.starts_with("Requested service_tier:"));
        let cost = if !compatible {
            None
        } else if complete {
            call.estimated_cost
        } else {
            match (
                call.estimated_cost,
                reserve.and_then(|r| r.estimated_cost_usd),
            ) {
                (Some(actual), Some(reserved)) => Some(actual.max(reserved)),
                (_, reserved) => reserved,
            }
        };
        total.input = total.input.zip(input).and_then(|(a, b)| a.checked_add(b));
        total.output = total.output.zip(output).and_then(|(a, b)| a.checked_add(b));
        total.cost = total.cost.zip(cost).and_then(|(a, b)| {
            let sum = a + b;
            sum.is_finite().then_some(sum)
        });
    }
    total
}
fn reserve_cost(call: &Call, input: u64, output: u64, priority: bool) -> Option<f64> {
    if priority {
        return None;
    }
    let p = call.metadata.pricing.as_ref()?;
    let age = p.freshness(chrono::Utc::now());
    if p.currency != "USD"
        || age.starts_with("future")
        || age.starts_with("stale")
        || age.starts_with("unknown")
    {
        return None;
    }
    let mut input_rate = p.input_per_million?.max(p.cached_input_per_million?);
    let mut output_rate = p.output_per_million?;
    // Reserve the highest configured context/cache rates, without predicting cache hits.
    for tier in &p.tiers {
        input_rate = input_rate
            .max(tier.input_per_million?)
            .max(tier.cached_input_per_million?);
        output_rate = output_rate.max(tier.output_per_million?);
    }
    for creation in std::iter::once(p.cache_creation.as_ref())
        .chain(p.tiers.iter().map(|t| t.cache_creation.as_ref()))
        .flatten()
    {
        for rate in [
            creation.default_per_million,
            creation.five_minute_per_million,
            creation.one_hour_per_million,
        ]
        .into_iter()
        .flatten()
        {
            input_rate = input_rate.max(rate);
        }
    }
    let cost = (input as f64 * input_rate + output as f64 * output_rate) / 1_000_000.0;
    (cost.is_finite() && cost >= 0.0).then_some(cost)
}
impl Store {
    /// Called under the same immediate transaction that publishes the dispatch.
    pub(crate) fn reserve_model_budget(
        &self,
        s: &Session,
        config: &Config,
        call: &mut Call,
        max_output: u32,
        priority: bool,
    ) -> Result<()> {
        let limits = &config.session;
        let Some(input) = call.counts.input.value else {
            if limits.enabled() {
                return Err(Blocked("input token estimate unavailable".into()).into());
            }
            return Ok(());
        };
        let output = u64::from(max_output);
        let cost = reserve_cost(call, input, output, priority);
        call.reservation = Some(Reservation {
            input_tokens: input,
            output_tokens: output,
            estimated_cost_usd: cost,
        });
        if !limits.enabled() {
            return Ok(());
        }
        let usage = self
            .usage_report(s, Scope::Session)
            .map_err(|error| Blocked(format!("session accounting unavailable: {error:#}")))?;
        let total = totals(&usage.calls);
        if limits.requests.is_some_and(|limit| total.requests >= limit) {
            return Err(Blocked(format!(
                "request limit reached ({} reserved/sent)",
                total.requests
            ))
            .into());
        }
        for (label, used, next, limit) in [
            (
                "input tokens",
                total.input,
                Some(input),
                limits.input_tokens,
            ),
            (
                "output tokens",
                total.output,
                Some(output),
                limits.output_tokens,
            ),
        ] {
            if let Some(limit) = limit {
                let projected = used.zip(next).and_then(|(a, b)| a.checked_add(b));
                if projected.is_none_or(|v| v > limit) {
                    return Err(Blocked(format!("{label}: prior usage/reservations plus this request exceed {limit}, or historical usage is unknown. This request reserves {input} estimated input and {output} maximum output tokens; provider.max_tokens controls the output reservation")).into());
                }
            }
        }
        if let Some(limit) = limits.estimated_cost_usd {
            let projected = total.cost.zip(cost).map(|(a, b)| a + b);
            if projected.is_none_or(|v| !v.is_finite() || v > limit) {
                return Err(Blocked(format!("estimated USD cost exceeds {limit}, or complete recent USD pricing/historical cost is unavailable (priority-tier prices are unsupported)")).into());
            }
        }
        Ok(())
    }
    pub fn model_budget_status(&self, s: &Session, config: &Config) -> Result<String> {
        if !config.session.enabled() {
            return Ok("Session model budgets: disabled".into());
        }
        let usage = self.usage_report(s, Scope::Session)?;
        let total = totals(&usage.calls);
        Ok(format!("Session model budgets · all branches/purposes\nLimits: requests {} · input {} · output {} · estimated USD {}\nReserved/accounted: {} requests · input {} · output {} · estimated USD {}\nPending/failed requests retain reservations. Unknown history is not zero. Limits are a policy snapshot; restart active workers after changing configuration. Tokenization/pricing are estimates, not provider billing guarantees. Cached input is part of input, not added again.",config.session.requests.map(|v|v.to_string()).unwrap_or_else(||"disabled".into()),config.session.input_tokens.map(|v|v.to_string()).unwrap_or_else(||"disabled".into()),config.session.output_tokens.map(|v|v.to_string()).unwrap_or_else(||"disabled".into()),config.session.estimated_cost_usd.map(|v|format!("{v:.8}")).unwrap_or_else(||"disabled".into()),total.requests,total.input.map(|n|n.to_string()).unwrap_or_else(||"unknown".into()),total.output.map(|n|n.to_string()).unwrap_or_else(||"unknown".into()),total.cost.map(|n|format!("{n:.8}")).unwrap_or_else(||"unknown".into())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::ModelMetadata;
    use serde_json::json;
    fn metadata() -> ModelMetadata {
        serde_json::from_value(json!({"pricing":{
            "input_per_million":2.0,"cached_input_per_million":0.2,"output_per_million":3.0,
            "currency":"USD","source":"fixture","effective_date":chrono::Utc::now().format("%Y-%m-%d").to_string()
        }})).unwrap()
    }
    fn pending(s: &Session) -> Call {
        Call::new(
            s,
            "work",
            "coding",
            "fixture",
            "https://fixture.invalid",
            metadata(),
            &[json!({"role":"user","content":"hello"})],
        )
    }
    fn register(
        store: &Store,
        s: &Session,
        call: &mut Call,
        limits: Limits,
        max: u32,
    ) -> Result<()> {
        let tx = rusqlite::Transaction::new_unchecked(
            &store.db,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        store.reserve_model_budget(s, &Config { session: limits }, call, max, false)?;
        store.begin_model_call(s, call)?;
        tx.commit()?;
        Ok(())
    }
    #[test]
    fn pending_failed_and_cross_branch_calls_consume_budget_without_double_counting_cache() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut s = store.create("demo", "goal", "work").unwrap();
        let limits = Limits {
            requests: Some(1),
            ..Default::default()
        };
        let mut first = pending(&s);
        register(&store, &s, &mut first, limits.clone(), 100).unwrap();
        let original = s.clone();
        let cutoff = store.history(&s).unwrap().last().unwrap().0;
        store.fork(&mut s, cutoff, "work").unwrap();
        let mut next = pending(&s);
        assert!(register(&store, &s, &mut next, limits, 100).is_err());
        let used = totals(&store.usage_report(&s, Scope::Session).unwrap().calls);
        assert_eq!(used.requests, 1);
        assert_eq!(used.output, Some(100));
        first.status = "failed-response".into();
        store.finish_model_call(&original, &first).unwrap();
        assert!(register(
            &store,
            &s,
            &mut next,
            Limits {
                output_tokens: Some(199),
                ..Default::default()
            },
            100
        )
        .is_err());
        let response=crate::provider::Completion::parse(200,r#"{"choices":[{"message":{"content":"done"}}],"usage":{"prompt_tokens":10,"completion_tokens":2,"prompt_tokens_details":{"cached_tokens":8}}}"#).unwrap();
        first.finish(&response);
        let used = totals(&[first]);
        assert_eq!(used.input, Some(10));
        assert_eq!(used.output, Some(2));
        assert!((used.cost.unwrap() - 0.0000116).abs() < 1e-12);
    }
    #[test]
    fn reservations_use_maximum_rates_and_unknown_cost_currency_and_history_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "work").unwrap();
        let mut call = pending(&s);
        let limits = Limits {
            estimated_cost_usd: Some(0.0),
            ..Default::default()
        };
        assert!(register(&store, &s, &mut call, limits.clone(), 100).is_err());
        assert!(store
            .usage_report(&s, Scope::Session)
            .unwrap()
            .calls
            .is_empty());
        call.metadata.pricing.as_mut().unwrap().currency = "EUR".into();
        assert!(register(
            &store,
            &s,
            &mut call,
            Limits {
                estimated_cost_usd: Some(1.0),
                ..Default::default()
            },
            100
        )
        .is_err());
        call.metadata = metadata();
        call.metadata.pricing.as_mut().unwrap().tiers = vec![crate::usage::PricingTier {
            above_input_tokens: 1_000_000,
            input_per_million: Some(10.0),
            cached_input_per_million: Some(1.0),
            output_per_million: Some(20.0),
            cache_creation: None,
        }];
        store
            .reserve_model_budget(&s, &Config::default(), &mut call, 100, false)
            .unwrap();
        let reserve = call.reservation.as_ref().unwrap();
        let expected = (reserve.input_tokens as f64 * 10.0 + 100.0 * 20.0) / 1_000_000.0;
        assert!((reserve.estimated_cost_usd.unwrap() - expected).abs() < 1e-12);
        let mut legacy = pending(&s);
        legacy.reservation = None;
        store.begin_model_call(&s, &mut legacy).unwrap();
        assert!(register(
            &store,
            &s,
            &mut call,
            Limits {
                output_tokens: Some(10000),
                ..Default::default()
            },
            100
        )
        .is_err());
        assert!(register(
            &store,
            &s,
            &mut call,
            Limits {
                estimated_cost_usd: Some(1000.0),
                ..Default::default()
            },
            100
        )
        .is_err());
        assert!(register(
            &store,
            &s,
            &mut call,
            Limits {
                requests: Some(2),
                ..Default::default()
            },
            100
        )
        .is_ok());
    }
    #[test]
    fn historical_non_usd_or_priority_cost_is_not_treated_as_usd_and_old_json_still_loads() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "work").unwrap();
        let mut call = pending(&s);
        call.status = "response-received".into();
        call.estimated_cost = Some(0.5);
        call.metadata.pricing.as_mut().unwrap().currency = "EUR".into();
        assert!(totals(&[call.clone()]).cost.is_none());
        call.metadata = metadata();
        call.counts
            .notes
            .push("Requested service_tier: priority".into());
        assert!(totals(&[call.clone()]).cost.is_none());
        let mut raw = serde_json::to_value(&call).unwrap();
        raw.as_object_mut().unwrap().remove("reservation");
        assert!(serde_json::from_value::<Call>(raw)
            .unwrap()
            .reservation
            .is_none());
        assert!(reserve_cost(&call, 10, 100, true).is_none());
        call.status = "not-sent".into();
        assert_eq!(totals(&[call]).requests, 0);
    }
    #[test]
    fn two_connections_cannot_overbook_a_session_request_reservation() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "work").unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let mut threads = Vec::new();
        for _ in 0..2 {
            let root = root.path().to_path_buf();
            let id = s.id.clone();
            let barrier = barrier.clone();
            threads.push(std::thread::spawn(move || {
                let store = Store::open(&root).unwrap();
                let s = store.session(&id).unwrap();
                let mut call = pending(&s);
                barrier.wait();
                register(
                    &store,
                    &s,
                    &mut call,
                    Limits {
                        requests: Some(1),
                        ..Default::default()
                    },
                    100,
                )
                .is_ok()
            }));
        }
        assert_eq!(
            threads
                .into_iter()
                .map(|t| t.join().unwrap())
                .filter(|ok| *ok)
                .count(),
            1
        );
        assert_eq!(
            store.usage_report(&s, Scope::Session).unwrap().calls.len(),
            1
        );
    }
}
