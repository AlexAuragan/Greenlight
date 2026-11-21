//! Safe provider diagnostics. Never retain response messages, URLs or credentials.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

/// Any unsuccessful central model call is an execution failure, not a verifier's
/// negative verdict. This also covers ledger/audit errors after paid dispatch.
#[derive(Debug)]
pub struct ModelCallFailed;
impl fmt::Display for ModelCallFailed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Model request or usage recording failed")
    }
}
impl std::error::Error for ModelCallFailed {}
pub fn is_fatal_model_error(error: &anyhow::Error) -> bool {
    error.downcast_ref::<ModelCallFailed>().is_some() || error.downcast_ref::<Failure>().is_some()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Credentials,
    Billing,
    RateLimit,
    Permission,
    Request,
    Unavailable,
    Timeout,
    Connection,
    Protocol,
    Cancelled,
    Unknown,
}
/// Fixed vocabulary only: arbitrary provider prose never enters saved diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitReason {
    Concurrency,
    TokenThroughput,
    RequestFrequency,
    Capacity,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Failure {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<crate::provider_diagnostic::Diagnostic>,
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_reason: Option<LimitReason>,
    pub status: Option<u16>,
    pub retry_after_seconds: Option<u64>,
    pub retryable: bool,
    pub billing_uncertain: bool,
}
impl std::error::Error for Failure {}
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let advice=match self.kind {
            Kind::Credentials=>"API credentials are missing, invalid or expired. Check the configured API-key environment variable and replace/reload the key if needed.",
            Kind::Billing=>"Provider credit or billing quota is exhausted. Check the account balance, billing limits and plan before resuming; automatic retries are disabled.",
            Kind::RateLimit => match self.limit_reason {
                Some(LimitReason::Concurrency) => "Provider reports a concurrent-request limit. Check other clients using the same account/key and wait for active requests to finish.",
                Some(LimitReason::TokenThroughput) => "Provider reports a token-throughput limit. Wait for the limit window to reset; review request context and output limits.",
                Some(LimitReason::RequestFrequency) => "Provider reports a request-frequency limit. Wait before retrying or reduce request frequency.",
                Some(LimitReason::Capacity) => "Provider reports model/server capacity unavailable. Wait or try another available model; this does not establish that your account concurrency limit was reached.",
                None => "Provider rate-limit/capacity rejection. The precise reason is unavailable; this alone does not establish that your concurrent-request limit was reached. Check provider status and account limits before retrying.",
            },
            Kind::Permission=>"Provider denied access. Check key permissions, organization/project access and model availability.",
            Kind::Request=>"Provider rejected the request. Check the model, context length and request configuration before resuming.",
            Kind::Unavailable=>"Provider service is temporarily unavailable. Check its service status and retry later.",
            Kind::Timeout=>"Provider request timed out. Check connectivity and provider status before resuming.",
            Kind::Connection=>"Could not connect to the provider. Check network access and the configured endpoint.",
            Kind::Protocol=>"Provider returned an invalid or unsupported response. Check endpoint/model compatibility before resuming.",
            Kind::Cancelled=>"Execution stopped by user. No further request will start; resume explicitly when ready.",
            Kind::Unknown=>"Provider request failed. Check provider status and account settings before resuming.",
        };
        write!(f, "{advice}")?;
        if let Some(status) = self.status {
            write!(f, " (HTTP {status})")?;
        }
        if let Some(seconds) = self.retry_after_seconds {
            write!(f, " Retry-After: {seconds}s.")?;
        }
        if self.billing_uncertain {
            write!(f," The request may have been processed; billing remains uncertain. No automatic retry.")?;
        }
        Ok(())
    }
}
impl Failure {
    pub fn retry_prompt(&self, model: &str) -> String {
        let failure = self;
        let mut text = format!("{failure}\n\nRetry sends the same request to {model}. Earlier tools will not run again.\nWait or fix the provider issue, then retry. Any remaining Retry-After cooldown is respected.\nLocal credential/config changes may require restarting Greenlight; this request retains its original settings.");
        if let Some(diagnostic) = &self.diagnostic {
            text.push_str(&format!("\n\n{}\nEsc hides this popup; Provider details and recorded activity retain this diagnostic for copying.", diagnostic.text()));
        }
        text.push_str(
            "\n[y] Retry request / [n] Stop for now. Esc hides this popup and keeps waiting.",
        );
        text
    }
    pub fn local(kind: Kind) -> Self {
        Self {
            kind,
            diagnostic: None,
            limit_reason: None,
            status: None,
            retry_after_seconds: None,
            retryable: false,
            billing_uncertain: false,
        }
    }
    pub fn http(status: u16, body: &Value, retry_after_seconds: Option<u64>) -> Self {
        let codes = [
            "/error/code",
            "/error/type",
            "/code",
            "/type",
            "/detail/code",
            "/detail/type",
            "/detail/error/code",
            "/detail/error/type",
        ]
        .iter()
        .filter_map(|path| body.pointer(path).and_then(Value::as_str))
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
        let messages = [
            body.pointer("/error/message"),
            body.get("error"),
            body.get("message"),
            body.get("detail"),
            body.pointer("/detail/message"),
            body.pointer("/detail/error"),
            body.pointer("/detail/error/message"),
        ]
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
        let billing = codes.iter().any(|code| {
            matches!(
                code.as_str(),
                "insufficient_quota"
                    | "insufficient_funds"
                    | "insufficient_credits"
                    | "quota_exceeded"
                    | "billing_hard_limit_reached"
                    | "billing_not_active"
                    | "credit_balance_exhausted"
                    | "payment_required"
            )
        }) || messages.iter().any(|message| {
            [
                "insufficient balance",
                "insufficient credits",
                "credit balance is too low",
                "exceeded your current quota",
            ]
            .iter()
            .any(|phrase| message.contains(phrase))
        });
        let explicit_rate = codes.iter().any(|code| {
            matches!(
                code.as_str(),
                "rate_limit_exceeded"
                    | "rate_limit_error"
                    | "too_many_requests"
                    | "requests_limit_exceeded"
                    | "tokens_per_minute_exceeded"
            )
        });
        let auth = codes.iter().any(|code| {
            matches!(
                code.as_str(),
                "invalid_api_key" | "authentication_error" | "invalid_token" | "token_expired"
            )
        });
        let kind = if billing || status == 402 {
            Kind::Billing
        } else if status == 401 || auth {
            Kind::Credentials
        } else if status == 429 {
            Kind::RateLimit
        } else if status == 403 {
            Kind::Permission
        } else if status == 408 || status == 504 {
            Kind::Timeout
        } else if (500..600).contains(&status) {
            Kind::Unavailable
        } else if (400..500).contains(&status) {
            Kind::Request
        } else {
            Kind::Unknown
        };
        let limit_reason = if kind != Kind::RateLimit {
            None
        } else if codes.iter().any(|code| {
            matches!(
                code.as_str(),
                "model_busy" | "model_overloaded" | "model_capacity_exceeded"
            )
        }) || messages.iter().any(|message| {
            [
                "model busy",
                "model is busy",
                "model is overloaded",
                "model overloaded",
                "model capacity",
                "no available replicas",
            ]
            .iter()
            .any(|phrase| message.contains(phrase))
        }) {
            Some(LimitReason::Capacity)
        } else if codes.iter().any(|code| {
            matches!(
                code.as_str(),
                "concurrency_limit_exceeded" | "max_concurrent_requests_exceeded"
            )
        }) || messages.iter().any(|message| {
            (message.contains("concurrency") || message.contains("concurrent requests"))
                && (message.contains("limit")
                    || message.contains("too many")
                    || message.contains("maximum"))
        }) {
            Some(LimitReason::Concurrency)
        } else if codes.iter().any(|code| {
            matches!(
                code.as_str(),
                "tokens_per_minute_exceeded" | "token_rate_limit_exceeded"
            )
        }) || messages.iter().any(|message| {
            message.contains("tokens per minute") || message.contains("tokens-per-minute")
        }) {
            Some(LimitReason::TokenThroughput)
        } else if codes.iter().any(|code| {
            matches!(
                code.as_str(),
                "requests_per_minute_exceeded" | "request_rate_limit_exceeded"
            )
        }) || messages.iter().any(|message| {
            message.contains("requests per minute")
                || message.contains("requests-per-minute")
                || message.contains("requests per second")
        }) {
            Some(LimitReason::RequestFrequency)
        } else {
            None
        };
        Self {
            kind,
            diagnostic: None,
            limit_reason,
            status: Some(status),
            retry_after_seconds,
            retryable: status == 429
                && kind == Kind::RateLimit
                && (explicit_rate || limit_reason == Some(LimitReason::Capacity)),
            billing_uncertain: status == 408 || status >= 500,
        }
    }
    pub fn transport(error: &reqwest::Error) -> Self {
        let kind = if error.is_connect() {
            Kind::Connection
        } else if error.is_timeout() {
            Kind::Timeout
        } else if error.is_builder() {
            Kind::Request
        } else {
            Kind::Unknown
        };
        let mut detail = error.to_string();
        let mut source = std::error::Error::source(error);
        for _ in 0..6 {
            let Some(cause) = source else { break };
            detail.push_str(&format!("\nCaused by: {cause}"));
            source = cause.source();
        }
        if let Some(url) = error.url() {
            detail = detail.replace(url.as_str(), "[request URL]");
        }
        Self {
            kind,
            diagnostic: Some(crate::provider_diagnostic::Diagnostic::capture(
                &detail,
                &Default::default(),
                "",
            )),
            limit_reason: None,
            status: None,
            retry_after_seconds: None,
            retryable: kind == Kind::Connection,
            billing_uncertain: !matches!(kind, Kind::Connection | Kind::Request),
        }
    }
    pub fn automatic_attempts(&self) -> usize {
        if self.limit_reason == Some(LimitReason::Capacity) {
            5
        } else {
            3
        }
    }
    /// Capacity rejections allow four waits, others two; each wait <=15s.
    pub fn delay(&self, attempt: usize) -> Option<Duration> {
        if !self.retryable || attempt >= self.automatic_attempts() {
            return None;
        }
        if let Some(seconds) = self.retry_after_seconds {
            return (seconds <= 15).then(|| Duration::from_secs(seconds));
        }
        let jitter = (uuid::Uuid::new_v4().as_u128() % 251) as u64;
        Some(Duration::from_millis(
            (1u64 << attempt.saturating_sub(1).min(3)) * 1000 + jitter,
        ))
    }
}
pub fn retry_after(value: Option<&str>) -> Option<u64> {
    let value = value?.trim();
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Some(value.parse().unwrap_or(u64::MAX));
    }
    let date = chrono::DateTime::parse_from_rfc2822(value).ok()?;
    let milliseconds = (date.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_milliseconds();
    Some(if milliseconds <= 0 {
        0
    } else {
        (milliseconds as u64).div_ceil(1000)
    })
}
pub async fn wait(delay: Duration, cancel: &Arc<AtomicBool>) -> Result<(), Failure> {
    let deadline = tokio::time::Instant::now() + delay;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(Failure::local(Kind::Cancelled));
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Ok(());
        }
        tokio::time::sleep((deadline - now).min(Duration::from_millis(50))).await;
    }
}

/// Shared stop signal for in-flight work and waits. Does not interrupt synchronous writes.
pub async fn cancelled(cancel: &Arc<AtomicBool>) {
    while !cancel.load(std::sync::atomic::Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn classifications_are_actionable_and_never_echo_response_secrets() {
        for (status, code, kind) in [
            (401, "invalid_api_key", Kind::Credentials),
            (402, "", Kind::Billing),
            (429, "insufficient_quota", Kind::Billing),
            (403, "", Kind::Permission),
            (429, "rate_limit_exceeded", Kind::RateLimit),
            (400, "context_length_exceeded", Kind::Request),
            (503, "", Kind::Unavailable),
            (504, "", Kind::Timeout),
        ] {
            let failure = Failure::http(
                status,
                &json!({"error":{"code":code,"message":"fixture-secret https://user:password@example.test?token=secret"}}),
                None,
            );
            assert_eq!(failure.kind, kind);
            let output = format!("{} {}", failure, serde_json::to_string(&failure).unwrap());
            assert!(!output.contains("fixture-secret"));
            assert!(!output.contains("password"));
            assert_eq!(
                failure.retryable,
                status == 429 && code == "rate_limit_exceeded"
            );
        }
        let failure = Failure::http(
            429,
            &json!({"error":{"code":"rate_limit_exceeded","type":"insufficient_quota"}}),
            Some(0),
        );
        assert_eq!(failure.kind, Kind::Billing);
        assert!(!failure.retryable);
        assert_eq!(
            Failure::http(400, &json!({"error":"Insufficient balance"}), None).kind,
            Kind::Billing
        );
        assert!(!Failure::http(429, &Value::Null, None).retryable);
    }
    #[test]
    fn retry_after_and_budgets_are_bounded_without_shortening_server_waits() {
        assert_eq!(retry_after(Some("12")), Some(12));
        assert_eq!(retry_after(Some("Wed, 21 Oct 2015 07:28:00 GMT")), Some(0));
        let future = (chrono::Utc::now() + chrono::Duration::seconds(60)).to_rfc2822();
        assert!(retry_after(Some(&future)).is_some_and(|n| (59..=61).contains(&n)));
        assert_eq!(retry_after(Some("invalid")), None);
        assert_eq!(
            retry_after(Some("999999999999999999999999999999")),
            Some(u64::MAX)
        );
        let mut failure = Failure::http(
            429,
            &json!({"error":{"code":"rate_limit_exceeded"}}),
            Some(15),
        );
        assert_eq!(failure.delay(1), Some(Duration::from_secs(15)));
        assert!(failure.delay(3).is_none());
        failure.retry_after_seconds = Some(16);
        assert!(failure.delay(1).is_none());
        failure.retry_after_seconds = None;
        assert!((1000..=1250).contains(&failure.delay(1).unwrap().as_millis()));
        assert!((2000..=2250).contains(&failure.delay(2).unwrap().as_millis()));
    }
    #[tokio::test]
    async fn cancellation_interrupts_backoff_without_waiting_for_deadline() {
        let flag = Arc::new(AtomicBool::new(false));
        let trigger = flag.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            trigger.store(true, Ordering::SeqCst);
        });
        let result =
            tokio::time::timeout(Duration::from_secs(1), wait(Duration::from_secs(15), &flag))
                .await
                .unwrap();
        assert_eq!(result.unwrap_err().kind, Kind::Cancelled);
    }
    #[test]
    fn rate_diagnostics_distinguish_reported_reasons_without_retaining_prose() {
        for (body, reason) in [
            (
                json!({"error":{"code":"concurrency_limit_exceeded","message":"fixture-secret"}}),
                Some(LimitReason::Concurrency),
            ),
            (
                json!({"detail":"Maximum concurrent requests limit exceeded: fixture-secret"}),
                Some(LimitReason::Concurrency),
            ),
            (
                json!({"detail":{"error":{"message":"model is overloaded fixture-secret"}}}),
                Some(LimitReason::Capacity),
            ),
            (
                json!({"error":{"code":"tokens_per_minute_exceeded"}}),
                Some(LimitReason::TokenThroughput),
            ),
            (
                json!({"detail":{"message":"requests per second exceeded fixture-secret"}}),
                Some(LimitReason::RequestFrequency),
            ),
            (json!({"error":{"message":"fixture-secret"}}), None),
            (json!({"error":{"code":"capacity_exceeded"}}), None),
            (json!({"error":{"code":"requests_limit_exceeded"}}), None),
        ] {
            let failure = Failure::http(429, &body, None);
            assert_eq!(failure.kind, Kind::RateLimit);
            assert_eq!(failure.limit_reason, reason);
            let saved = serde_json::to_string(&failure).unwrap();
            assert!(!saved.contains("fixture-secret"));
            assert!(!failure.to_string().contains("fixture-secret"));
            if reason == Some(LimitReason::Capacity) {
                assert!(failure.retryable);
            }
            if reason.is_none() {
                assert!(failure.to_string().contains("does not establish"));
            }
        }
        let billing = Failure::http(
            429,
            &json!({"detail":{"error":{"code":"insufficient_quota","message":"model is overloaded fixture-secret"}}}),
            None,
        );
        assert_eq!(billing.kind, Kind::Billing);
        assert_eq!(billing.limit_reason, None);
        assert!(!billing.retryable);
    }

    #[test]
    fn old_saved_failure_events_remain_readable() {
        let failure: Failure = serde_json::from_value(json!({
            "kind":"rate_limit", "status":429, "retry_after_seconds":null,
            "retryable":false,"billing_uncertain":false
        }))
        .unwrap();
        assert_eq!(failure.limit_reason, None);
    }
}
