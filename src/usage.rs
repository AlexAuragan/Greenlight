//! Per-dispatch usage ledger. Reported counts, estimates and unknowns stay distinct.
use crate::{
    provider::Completion,
    store::{Session, Store},
};
use anyhow::{bail, Context, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};
use uuid::Uuid;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelMetadata {
    pub tokenizer: Option<String>,
    pub pricing: Option<Pricing>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pricing {
    pub input_per_million: Option<f64>,
    pub cached_input_per_million: Option<f64>,
    pub output_per_million: Option<f64>,
    pub currency: String,
    pub source: String,
    pub effective_date: String,
}

impl ModelMetadata {
    pub fn validate(&self) -> Result<()> {
        if self
            .tokenizer
            .as_deref()
            .is_some_and(|v| !matches!(v, "cl100k_base" | "o200k_base"))
        {
            bail!("tokenizer must be cl100k_base or o200k_base; counts remain estimates");
        }
        if let Some(p) = &self.pricing {
            for rate in [
                p.input_per_million,
                p.cached_input_per_million,
                p.output_per_million,
            ]
            .into_iter()
            .flatten()
            {
                if !rate.is_finite() || rate < 0.0 {
                    bail!("model prices must be finite and nonnegative");
                }
            }
            if p.currency.len() != 3
                || !p.currency.bytes().all(|v| v.is_ascii_uppercase())
                || p.source.trim().is_empty()
            {
                bail!("pricing requires a three-letter uppercase currency and a source");
            }
            chrono::NaiveDate::parse_from_str(&p.effective_date, "%Y-%m-%d")
                .context("pricing effective_date must be YYYY-MM-DD")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TokenCount {
    pub value: Option<u64>,
    pub source: String,
}
impl TokenCount {
    fn unknown() -> Self {
        Self {
            value: None,
            source: "unknown".into(),
        }
    }
    fn reported(value: u64) -> Self {
        Self {
            value: Some(value),
            source: "provider".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Counts {
    pub input: TokenCount,
    pub cached_input: TokenCount,
    pub output: TokenCount,
    pub reasoning: Option<u64>,
    pub raw: Option<Value>,
    pub notes: Vec<String>,
}

fn estimate(model: &str, metadata: &ModelMetadata, text: &str) -> TokenCount {
    static CL: OnceLock<Option<tiktoken_rs::CoreBPE>> = OnceLock::new();
    static O: OnceLock<Option<tiktoken_rs::CoreBPE>> = OnceLock::new();
    let name = model.rsplit('/').next().unwrap_or(model);
    let encoding = metadata.tokenizer.as_deref().unwrap_or_else(|| {
        if ["gpt-4o", "gpt-4.1", "gpt-5", "o1", "o3", "o4"]
            .iter()
            .any(|p| name.starts_with(p))
        {
            "o200k_base"
        } else {
            "cl100k_base"
        }
    });
    let bpe = if encoding == "o200k_base" {
        O.get_or_init(|| tiktoken_rs::o200k_base().ok())
    } else {
        CL.get_or_init(|| tiktoken_rs::cl100k_base().ok())
    };
    TokenCount {
        value: bpe
            .as_ref()
            .map(|bpe| bpe.encode_ordinary(text).len() as u64),
        source: format!("estimated:tiktoken:{encoding}"),
    }
}

impl Counts {
    pub fn estimated(model: &str, metadata: &ModelMetadata, messages: &[Value]) -> Self {
        Self {input:estimate(model,metadata,&serde_json::to_string(messages).unwrap_or_default()),cached_input:TokenCount::unknown(),output:TokenCount::unknown(),reasoning:None,raw:None,notes:vec!["Fallback counts tokenize serialized messages; model chat templates and billing may differ. Cache usage cannot be inferred.".into()]}
    }
    pub fn finish(&mut self, model: &str, metadata: &ModelMetadata, response: &Completion) {
        if let Ok(content) = &response.content {
            self.output = estimate(model, metadata, content);
        }
        self.raw = response.raw_usage.clone();
        let Some(raw) = &self.raw else { return };
        let input = raw
            .get("prompt_tokens")
            .and_then(Value::as_u64)
            .or_else(|| raw.get("input_tokens").and_then(Value::as_u64));
        let output = raw
            .get("completion_tokens")
            .and_then(Value::as_u64)
            .or_else(|| raw.get("output_tokens").and_then(Value::as_u64));
        let cached = raw
            .pointer("/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_u64)
            .or_else(|| {
                raw.pointer("/input_tokens_details/cached_tokens")
                    .and_then(Value::as_u64)
            })
            .or_else(|| raw.get("prompt_cache_hit_tokens").and_then(Value::as_u64))
            .or_else(|| raw.get("cached_tokens").and_then(Value::as_u64))
            .or_else(|| raw.get("cache_read_input_tokens").and_then(Value::as_u64));
        if let Some(input) = input {
            // Native Anthropic input_tokens excludes cached read/creation;
            // OpenAI-compatible prompt_tokens already includes cached input.
            let total = if raw.get("prompt_tokens").is_none()
                && (raw.get("cache_read_input_tokens").is_some()
                    || raw.get("cache_creation_input_tokens").is_some())
            {
                let component = |name: &str| raw.get(name).map_or(Some(0), Value::as_u64);
                component("cache_read_input_tokens")
                    .and_then(|read| input.checked_add(read))
                    .and_then(|v| {
                        component("cache_creation_input_tokens")
                            .and_then(|created| v.checked_add(created))
                    })
            } else {
                Some(input)
            };
            if let Some(total) = total {
                self.input = TokenCount::reported(total);
            } else {
                self.notes.push(
                    "Invalid or overflowing provider input total; fallback estimate retained."
                        .into(),
                );
            }
        }
        if let Some(output) = output {
            self.output = TokenCount::reported(output);
        }
        if let Some(cached) = cached {
            if self.input.source != "provider"
                || self.input.value.is_some_and(|input| cached <= input)
            {
                self.cached_input = TokenCount::reported(cached);
                if self.input.source != "provider"
                    && self.input.value.is_some_and(|input| cached > input)
                {
                    self.notes.push("Reported cached input exceeds the fallback input estimate; cost remains unavailable.".into());
                }
            } else {
                self.notes.push("Cached-input report cannot be reconciled with reported total input; cache usage remains unknown.".into());
            }
        }
        self.reasoning = raw
            .pointer("/completion_tokens_details/reasoning_tokens")
            .and_then(Value::as_u64);
        if raw["cache_creation_input_tokens"]
            .as_u64()
            .is_some_and(|v| v > 0)
        {
            self.notes.push(
                "Cache-creation pricing is unsupported; no flat-rate cost estimate is supplied."
                    .into(),
            );
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Call {
    pub id: String,
    pub started_at_utc: String,
    pub created_event: Option<i64>,
    pub session: String,
    pub branch: String,
    pub attempt: Option<String>,
    pub step: String,
    pub purpose: String,
    pub provider: String,
    pub model: String,
    pub response_id: Option<String>,
    pub response_model: Option<String>,
    pub status: String,
    pub counts: Counts,
    pub metadata: ModelMetadata,
    pub estimated_cost: Option<f64>,
}

impl Call {
    pub fn new(
        s: &Session,
        step: &str,
        purpose: &str,
        model: &str,
        provider: &str,
        metadata: ModelMetadata,
        messages: &[Value],
    ) -> Self {
        let counts = Counts::estimated(model, &metadata, messages);
        Self {
            id: Uuid::new_v4().to_string(),
            started_at_utc: chrono::Utc::now().to_rfc3339(),
            created_event: None,
            session: s.id.clone(),
            branch: s.active_branch.clone(),
            attempt: None,
            step: step.into(),
            purpose: purpose.into(),
            provider: provider.into(),
            model: model.into(),
            response_id: None,
            response_model: None,
            status: "started-or-interrupted".into(),
            counts,
            metadata,
            estimated_cost: None,
        }
    }
    pub fn finish(&mut self, response: &Completion) {
        self.status = if response.content.is_ok() {
            "response-received"
        } else {
            "failed-response"
        }
        .into();
        self.response_id = response.response_id.clone();
        self.response_model = response.response_model.clone();
        self.counts.finish(&self.model, &self.metadata, response);
        if self
            .response_model
            .as_ref()
            .is_some_and(|model| model != &self.model)
        {
            self.counts.notes.push(
                "Returned model differs from requested model; configured price was not applied."
                    .into(),
            );
        }
        self.estimated_cost = self.cost();
    }
    fn cost(&self) -> Option<f64> {
        let p = self.metadata.pricing.as_ref()?;
        if self
            .response_model
            .as_ref()
            .is_some_and(|m| m != &self.model)
        {
            return None;
        }
        if self
            .counts
            .raw
            .as_ref()
            .and_then(|v| v["cache_creation_input_tokens"].as_u64())
            .is_some_and(|v| v > 0)
        {
            return None;
        }
        let input = self.counts.input.value?;
        let output = self.counts.output.value?;
        let input_rate = p.input_per_million?;
        let output_rate = p.output_per_million?;
        let cost = if let Some(cached) = self.counts.cached_input.value {
            (input.checked_sub(cached)? as f64) * input_rate
                + (cached as f64)
                    * if cached == 0 {
                        0.0
                    } else {
                        p.cached_input_per_million?
                    }
        } else if p.cached_input_per_million == Some(input_rate) {
            (input as f64) * input_rate
        } else {
            return None;
        };
        let cost = (cost + (output as f64) * output_rate) / 1_000_000.0;
        cost.is_finite().then_some(cost)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TokenTotals {
    pub reported: u64,
    pub estimated: u64,
    pub estimated_requests: u64,
    pub unknown_requests: u64,
}
impl TokenTotals {
    fn add(&mut self, count: &TokenCount) {
        match count.value {
            Some(v) if count.source == "provider" => {
                self.reported = self.reported.saturating_add(v)
            }
            Some(v) => {
                self.estimated = self.estimated.saturating_add(v);
                self.estimated_requests += 1;
            }
            None => self.unknown_requests += 1,
        }
    }
    fn label(&self) -> String {
        format!(
            "{} reported + {} estimated; {} unknown",
            self.reported, self.estimated, self.unknown_requests
        )
    }
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Totals {
    pub requests: u64,
    pub input: TokenTotals,
    pub cached_input: TokenTotals,
    pub output: TokenTotals,
    pub estimated_cost_by_currency: BTreeMap<String, f64>,
    pub incomplete_cost_requests: u64,
}
impl Totals {
    fn add(&mut self, call: &Call) {
        self.requests += 1;
        self.input.add(&call.counts.input);
        self.cached_input.add(&call.counts.cached_input);
        self.output.add(&call.counts.output);
        if let (Some(cost), Some(price)) = (call.estimated_cost, &call.metadata.pricing) {
            *self
                .estimated_cost_by_currency
                .entry(price.currency.clone())
                .or_default() += cost;
        } else {
            self.incomplete_cost_requests += 1;
        }
    }
    pub fn compact(&self) -> String {
        if self.requests == 0 {
            return "Usage: no recorded requests; historical calls are not backfilled".into();
        }
        let count = |t: &TokenTotals| {
            format!(
                "{}{}{}",
                t.reported.saturating_add(t.estimated),
                if t.estimated_requests > 0 { "~" } else { "" },
                if t.unknown_requests > 0 { "+?" } else { "" }
            )
        };
        format!(
            "Usage: in {} cache {} out {} | ~ estimate, ? unknown | usage command",
            count(&self.input),
            count(&self.cached_input),
            count(&self.output)
        )
    }
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum Scope {
    Session,
    Branch,
    Visible,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub scope: String,
    pub session: String,
    pub branch: String,
    pub totals: Totals,
    pub by_model: BTreeMap<String, Totals>,
    pub by_attempt: BTreeMap<String, Totals>,
    pub calls: Vec<Call>,
}
impl Report {
    pub fn display(&self) -> String {
        let costs = |total: &Totals| {
            let mut text = total
                .estimated_cost_by_currency
                .iter()
                .map(|(currency, cost)| format!("{currency} {cost:.8} estimated"))
                .collect::<Vec<_>>()
                .join(", ");
            if text.is_empty() {
                text = "unavailable".into();
            }
            format!(
                "{text}; {} incomplete-cost requests",
                total.incomplete_cost_requests
            )
        };
        let mut text=format!("Usage scope: {}\nSession: {}\nBranch: {}\nRequests: {}\nInput: {}\nCached input (subset of input): {}\nOutput: {}\n",self.scope,self.session,self.branch,self.totals.requests,self.totals.input.label(),self.totals.cached_input.label(),self.totals.output.label());
        for (currency, cost) in &self.totals.estimated_cost_by_currency {
            text.push_str(&format!("Estimated cost subtotal: {currency} {cost:.8}\n"));
        }
        if self.totals.requests > 0 && self.totals.estimated_cost_by_currency.is_empty() {
            text.push_str(
                "Estimated cost: unavailable (missing usage/rates or unsupported billing rules).\n",
            );
        }
        text.push_str(&format!("Requests with incomplete/unknown cost: {}\nConfigured rates are snapshots, not live-verified prices or provider invoices.\nHistorical calls predating the ledger are not backfilled.\n",self.totals.incomplete_cost_requests));
        for (model, total) in &self.by_model {
            text.push_str(&format!(
                "Model {model}: {} requests; input [{}], cache [{}], output [{}]\n",
                total.requests,
                total.input.label(),
                total.cached_input.label(),
                total.output.label()
            ));
            text.push_str(&format!("  Cost subtotal: {}\n", costs(total)));
        }
        for (attempt, total) in &self.by_attempt {
            text.push_str(&format!("Attempt {attempt}: {} requests; input [{}], cache [{}], output [{}]\n  Cost subtotal: {}\n",total.requests,total.input.label(),total.cached_input.label(),total.output.label(),costs(total)));
        }
        text
    }
}

impl Store {
    pub fn begin_model_call(&self, s: &Session, call: &mut Call) -> Result<()> {
        call.attempt=self.db.query_row("SELECT id FROM step_attempts WHERE session=?1 AND branch=?2 AND step=?3 ORDER BY start_event DESC LIMIT 1",params![s.id,s.active_branch,call.step],|r|r.get(0)).optional()?;
        let tx = self.db.unchecked_transaction()?;
        let event = self.event(s, Some(&call.step), "model.call-started", &call.id)?;
        call.created_event = Some(event);
        self.db.execute("INSERT INTO model_calls(id,session,branch,created_event,payload) VALUES (?1,?2,?3,?4,?5)",params![call.id,s.id,s.active_branch,event,serde_json::to_string(call)?])?;
        tx.commit()?;
        Ok(())
    }
    pub fn finish_model_call(&self, s: &Session, call: &Call) -> Result<()> {
        let tx = self.db.unchecked_transaction()?;
        // One internal ID per actual dispatch; re-reporting never adds a charge.
        let changed = self.db.execute(
            "UPDATE model_calls SET payload=?2 WHERE id=?1 AND session=?3 AND branch=?4",
            params![call.id, serde_json::to_string(call)?, s.id, s.active_branch],
        )?;
        if changed != 1 {
            bail!("model usage request record is missing");
        }
        self.event(s, Some(&call.step), "model.usage", &call.id)?;
        tx.commit()?;
        Ok(())
    }
    pub fn usage_report(&self, s: &Session, scope: Scope) -> Result<Report> {
        let visible = if matches!(scope, Scope::Visible) {
            Some(
                self.history(s)?
                    .into_iter()
                    .map(|e| e.0)
                    .collect::<BTreeSet<_>>(),
            )
        } else {
            None
        };
        let mut query=self.db.prepare("SELECT branch,created_event,payload FROM model_calls WHERE session=?1 ORDER BY created_event")?;
        let rows = query.query_map([&s.id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut report = Report {
            scope: match scope {
                Scope::Session => "session spending, all branches",
                Scope::Branch => "current branch spending only",
                Scope::Visible => {
                    "visible ancestry, includes inherited calls without adding charges"
                }
            }
            .into(),
            session: s.id.clone(),
            branch: s.active_branch.clone(),
            totals: Totals::default(),
            by_model: BTreeMap::new(),
            by_attempt: BTreeMap::new(),
            calls: Vec::new(),
        };
        for row in rows {
            let (branch, event, payload) = row?;
            if matches!(scope, Scope::Branch) && branch != s.active_branch {
                continue;
            }
            if visible.as_ref().is_some_and(|v| !v.contains(&event)) {
                continue;
            }
            let call: Call =
                serde_json::from_str(&payload).context("invalid persisted usage record")?;
            report.totals.add(&call);
            report
                .by_model
                .entry(format!("{} / {}", call.provider, call.model))
                .or_default()
                .add(&call);
            report
                .by_attempt
                .entry(call.attempt.clone().unwrap_or_else(|| "unassigned".into()))
                .or_default()
                .add(&call);
            report.calls.push(call);
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{engine::Engine, provider::Script};
    use serde_json::json;
    use std::{
        fs,
        sync::{Arc, Mutex},
    };

    fn metadata() -> ModelMetadata {
        ModelMetadata {
            tokenizer: Some("cl100k_base".into()),
            pricing: Some(Pricing {
                input_per_million: Some(2.0),
                cached_input_per_million: Some(0.5),
                output_per_million: Some(10.0),
                currency: "USD".into(),
                source: "test fixture, not actual provider prices".into(),
                effective_date: "2026-11-14".into(),
            }),
        }
    }
    fn call(s: &Session) -> Call {
        Call::new(
            s,
            "work",
            "coding",
            "fixture",
            "https://provider.invalid",
            metadata(),
            &[json!({"role":"user","content":"hello"})],
        )
    }
    fn response(raw: Value) -> Completion {
        Completion {
            content: Ok("hello world".into()),
            raw_usage: Some(raw),
            response_id: Some("response-fixture".into()),
            response_model: None,
        }
    }
    fn usage() -> Value {
        json!({"prompt_tokens":100,"prompt_tokens_details":{"cached_tokens":40},"completion_tokens":20,"completion_tokens_details":{"reasoning_tokens":5}})
    }

    #[test]
    fn reported_cached_tokens_are_a_subset_and_reasoning_is_not_added_twice() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "work").unwrap();
        let mut record = call(&s);
        record.finish(&response(usage()));
        assert_eq!(record.counts.input.value, Some(100));
        assert_eq!(record.counts.cached_input.value, Some(40));
        assert_eq!(record.counts.output.value, Some(20));
        assert_eq!(record.counts.reasoning, Some(5));
        assert_eq!(record.counts.input.source, "provider");
        assert!((record.estimated_cost.unwrap() - 0.00034).abs() < 1e-12);
    }

    #[test]
    fn fallback_tokenization_never_fabricates_cache_hits_or_free_usage() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "work").unwrap();
        let mut record = call(&s);
        record.finish(&Completion {
            content: Ok("Bonjour 世界".into()),
            raw_usage: None,
            response_id: None,
            response_model: None,
        });
        assert!(record
            .counts
            .input
            .source
            .starts_with("estimated:tiktoken:"));
        assert!(record.counts.input.value.unwrap() > 0 && record.counts.output.value.unwrap() > 0);
        assert!(record.counts.cached_input.value.is_none());
        assert!(record.estimated_cost.is_none());
        record
            .metadata
            .pricing
            .as_mut()
            .unwrap()
            .cached_input_per_million = Some(2.0);
        assert!(record.cost().is_some()); // Cache uncertainty cannot affect identical rates.
    }

    #[test]
    fn provider_variations_and_invalid_cache_usage_are_explicit() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "work").unwrap();
        let mut record = call(&s);
        record.finish(&response(json!({"input_tokens":60,"cache_read_input_tokens":40,"cache_creation_input_tokens":10,"output_tokens":20})));
        assert_eq!(record.counts.input.value, Some(110));
        assert_eq!(record.counts.cached_input.value, Some(40));
        assert!(record.estimated_cost.is_none());
        let mut invalid = call(&s);
        invalid.finish(&response(
            json!({"prompt_tokens":10,"cached_tokens":100,"completion_tokens":20}),
        ));
        assert!(invalid.counts.cached_input.value.is_none());
        assert!(invalid.estimated_cost.is_none());
        assert!(invalid
            .counts
            .notes
            .iter()
            .any(|note| note.contains("cannot be reconciled")));
        let mut deepseek = call(&s);
        deepseek.finish(&response(
            json!({"prompt_tokens":100,"prompt_cache_hit_tokens":40,"completion_tokens":20}),
        ));
        assert_eq!(deepseek.counts.cached_input.value, Some(40));
        let mut responses_api = call(&s);
        responses_api.finish(&response(json!({"input_tokens":100,"input_tokens_details":{"cached_tokens":40},"output_tokens":20})));
        assert_eq!(responses_api.counts.input.value, Some(100));
        assert_eq!(responses_api.counts.cached_input.value, Some(40));
        let mut partial = call(&s);
        partial.finish(&response(
            json!({"cached_tokens":40,"completion_tokens":20}),
        ));
        assert_eq!(partial.counts.cached_input.value, Some(40));
        assert_eq!(partial.counts.cached_input.source, "provider");
        assert!(partial.counts.input.source.starts_with("estimated"));
    }

    #[test]
    fn bad_input_totals_do_not_discard_valid_output_or_apply_an_alias_price() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "goal", "work").unwrap();
        let mut record = call(&session);
        record.finish(&response(
            json!({"input_tokens":u64::MAX,"cache_read_input_tokens":40,"output_tokens":20}),
        ));
        assert!(record.counts.input.source.starts_with("estimated"));
        assert_eq!(record.counts.output.value, Some(20));
        assert_eq!(record.counts.output.source, "provider");
        let mut alias = call(&session);
        let mut reply = response(usage());
        reply.response_model = Some("different-model".into());
        alias.finish(&reply);
        assert!(alias.estimated_cost.is_none());
        let mut missing_rate = call(&session);
        missing_rate
            .metadata
            .pricing
            .as_mut()
            .unwrap()
            .output_per_million = None;
        missing_rate.finish(&response(usage()));
        assert!(missing_rate.estimated_cost.is_none());
    }

    #[test]
    fn configured_metadata_loads_and_missing_metadata_remains_unknown() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("greenlight.yaml");
        fs::write(&path,"provider:\n  model: fixture\n  model_metadata:\n    fixture:\n      tokenizer: cl100k_base\n      pricing:\n        input_per_million: 2\n        cached_input_per_million: 0.5\n        output_per_million: 10\n        currency: USD\n        source: test-fixture\n        effective_date: '2026-11-14'\n").unwrap();
        let cfg = crate::config::load_config(root.path()).unwrap();
        assert_eq!(
            cfg.provider.model_metadata["fixture"]
                .pricing
                .as_ref()
                .unwrap()
                .input_per_million,
            Some(2.0)
        );
        let text = fs::read_to_string(&path).unwrap();
        fs::write(
            &path,
            text.replace("input_per_million: 2", "input_per_million: -2"),
        )
        .unwrap();
        assert!(crate::config::load_config(root.path()).is_err());
        ModelMetadata::default().validate().unwrap();
        let estimated = estimate("gpt-4o", &ModelMetadata::default(), "hello");
        assert!(estimated.source.contains("o200k_base"));
        assert!(estimated.value.unwrap() > 0);
    }

    #[test]
    fn usage_survives_api_errors_and_missing_response_content() {
        for status in [200, 429] {
            let raw = json!({"id":"fixture","usage":usage(),"error":{"message":"fixture error"}})
                .to_string();
            let parsed = Completion::parse(status, &raw).unwrap();
            assert!(parsed.content.is_err());
            let root = tempfile::tempdir().unwrap();
            let store = Store::open(root.path()).unwrap();
            let s = store.create("demo", "goal", "work").unwrap();
            let mut record = call(&s);
            record.finish(&parsed);
            assert_eq!(record.counts.output.value, Some(20));
            assert_eq!(record.status, "failed-response");
        }
        assert!(Completion::parse(502, "not-json").is_err());
    }

    #[test]
    fn pricing_requires_valid_rates_and_provenance() {
        metadata().validate().unwrap();
        for rate in [-1.0, f64::INFINITY, f64::NAN] {
            let mut m = metadata();
            m.pricing.as_mut().unwrap().input_per_million = Some(rate);
            assert!(m.validate().is_err());
        }
        let mut m = metadata();
        m.pricing.as_mut().unwrap().effective_date = "yesterday".into();
        assert!(m.validate().is_err());
        let mut m = metadata();
        m.tokenizer = Some("invented".into());
        assert!(m.validate().is_err());
        let mut m = metadata();
        m.pricing.as_mut().unwrap().source = String::new();
        assert!(m.validate().is_err());
    }

    #[test]
    fn ledger_deduplicates_reporting_and_separates_fork_spending() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut s = store.create("demo", "goal", "work").unwrap();
        let parent = s.active_branch.clone();
        store
            .begin_attempt(&s, "work", "definition", "fixture")
            .unwrap();
        let mut first = call(&s);
        store.begin_model_call(&s, &mut first).unwrap();
        first.finish(&response(usage()));
        store.finish_model_call(&s, &first).unwrap();
        store.finish_model_call(&s, &first).unwrap();
        assert_eq!(
            store
                .usage_report(&s, Scope::Session)
                .unwrap()
                .totals
                .requests,
            1
        );
        let cutoff = store
            .event(&s, Some("work"), "step.completed", "done")
            .unwrap();
        store.fork(&mut s, cutoff, "work").unwrap();
        assert_eq!(
            store
                .usage_report(&s, Scope::Branch)
                .unwrap()
                .totals
                .requests,
            0
        );
        assert_eq!(
            store
                .usage_report(&s, Scope::Visible)
                .unwrap()
                .totals
                .requests,
            1
        );
        store
            .begin_attempt(&s, "work", "definition", "fixture")
            .unwrap();
        let mut second = call(&s);
        second.metadata.pricing.as_mut().unwrap().input_per_million = Some(4.0);
        store.begin_model_call(&s, &mut second).unwrap();
        second.finish(&response(usage()));
        store.finish_model_call(&s, &second).unwrap();
        assert_eq!(
            store
                .usage_report(&s, Scope::Branch)
                .unwrap()
                .totals
                .requests,
            1
        );
        assert_eq!(
            store
                .usage_report(&s, Scope::Session)
                .unwrap()
                .totals
                .requests,
            2
        );
        store.switch_branch(&mut s, &parent, "work").unwrap();
        assert_eq!(
            store
                .usage_report(&s, Scope::Visible)
                .unwrap()
                .totals
                .requests,
            1
        );
        drop(store);
        let store = Store::open(root.path()).unwrap();
        let report = store.usage_report(&s, Scope::Session).unwrap();
        assert_eq!(
            report.calls[0]
                .metadata
                .pricing
                .as_ref()
                .unwrap()
                .input_per_million,
            Some(2.0)
        );
        assert_eq!(
            report.calls[1]
                .metadata
                .pricing
                .as_ref()
                .unwrap()
                .input_per_million,
            Some(4.0)
        );
        assert_eq!(report.by_attempt.len(), 2);
        assert!((report.totals.estimated_cost_by_currency["USD"] - 0.00080).abs() < 1e-12);
    }

    #[test]
    fn interrupted_requests_remain_unknown_after_reopening() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let s = store.create("demo", "goal", "work").unwrap();
        store.begin_model_call(&s, &mut call(&s)).unwrap();
        drop(store);
        let store = Store::open(root.path()).unwrap();
        let report = store.usage_report(&s, Scope::Session).unwrap();
        assert_eq!(report.totals.requests, 1);
        assert_eq!(report.totals.output.unknown_requests, 1);
        assert_eq!(report.totals.incomplete_cost_requests, 1);
        assert_eq!(report.calls[0].status, "started-or-interrupted");
    }

    #[tokio::test]
    async fn all_engine_call_purposes_and_protocol_errors_are_accounted_for() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("workflows/demo");
        fs::create_dir_all(&folder).unwrap();
        fs::write(root.path().join("greenlight.yaml"),"provider:\n  model: fixture\n  base_url: https://user:fixture-password@example.invalid/v1?key=fixture-secret\n").unwrap();
        fs::write(
            folder.join("workflow.yaml"),
            "id: demo\ntitle: Demo\nstart: work\nsteps: [work]\n",
        )
        .unwrap();
        fs::write(folder.join("work.md"),"---\nid: work\ntools: []\nverification:\n  type: agent\n  prompt: review.md\ndeviations:\n  - id: blocker\n    action: pause\n    prompt: review.md\n---\nFixture\n").unwrap();
        fs::write(folder.join("review.md"), "Review fixture").unwrap();
        let mut engine = Engine::open(root.path()).unwrap();
        engine
            .cfg
            .provider
            .model_metadata
            .insert("fixture".into(), metadata());
        let responses = [
            "invalid protocol",
            "action: deviation\ndeviation_id: blocker\nsummary: fixture",
            "detected: false\nreason: unsupported",
            "action: artifact\nname: report.md\ncontent: evidence",
            "action: finish",
            "passed: true\nreason: supported",
        ];
        engine.provider.script = Some(Arc::new(Mutex::new(Script {
            responses: responses.into_iter().map(|s| Ok(s.into())).collect(),
            usages: (0..6).map(|_| Some(usage())).collect(),
            requests: Vec::new(),
        })));
        let session = engine.store.create("demo", "goal", "work").unwrap();
        engine.run(&session.id, None, None).await.unwrap();
        let report = engine
            .store
            .usage_report(&engine.store.session(&session.id).unwrap(), Scope::Session)
            .unwrap();
        assert_eq!(report.totals.requests, 6);
        assert_eq!(report.totals.input.reported, 600);
        assert_eq!(report.totals.cached_input.reported, 240);
        assert_eq!(report.totals.output.reported, 120);
        assert_eq!(
            report
                .calls
                .iter()
                .filter(|c| c.purpose == "coding")
                .count(),
            4
        );
        assert_eq!(
            report
                .calls
                .iter()
                .filter(|c| c.purpose == "verification")
                .count(),
            1
        );
        assert_eq!(
            report
                .calls
                .iter()
                .filter(|c| c.purpose == "deviation-detector")
                .count(),
            1
        );
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(!serialized.contains("fixture-password"));
        assert!(!serialized.contains("fixture-secret"));
        assert_eq!(
            engine.store.session(&session.id).unwrap().status,
            "completed"
        );
    }

    #[tokio::test]
    async fn failed_model_requests_are_persisted_before_workflow_error_handling() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("workflows/demo");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            root.path().join("greenlight.yaml"),
            "provider:\n  model: fixture\n",
        )
        .unwrap();
        fs::write(
            folder.join("workflow.yaml"),
            "id: demo\ntitle: Demo\nstart: work\nsteps: [work]\n",
        )
        .unwrap();
        fs::write(
            folder.join("work.md"),
            "---\nid: work\nverification:\n  type: none\n---\nFixture\n",
        )
        .unwrap();
        let mut engine = Engine::open(root.path()).unwrap();
        engine.provider.script = Some(Arc::new(Mutex::new(Script {
            responses: vec![Err("injected request failure".into())].into(),
            ..Default::default()
        })));
        let session = engine.store.create("demo", "goal", "work").unwrap();
        engine.run(&session.id, None, None).await.unwrap();
        let report = engine.store.usage_report(&session, Scope::Session).unwrap();
        assert_eq!(report.totals.requests, 1);
        assert_eq!(report.calls[0].status, "failed-or-unknown-billing");
        assert_eq!(report.totals.output.unknown_requests, 1);
        assert_eq!(engine.store.session(&session.id).unwrap().status, "blocked");
    }
}
