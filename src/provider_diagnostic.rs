//! Bounded, inspectable provider evidence, separate from the concise user advice.
use serde::{Deserialize, Serialize};
use serde_json::Value;
const LIMIT: usize = 16_384;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    pub body: String,
    pub truncated: bool,
    pub request_ids: Vec<(String, String)>,
}
fn scrub(value: &mut Value) -> bool {
    let mut changed = false;
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if matches!(
                    key.to_ascii_lowercase().replace('-', "_").as_str(),
                    "api_key"
                        | "apikey"
                        | "authorization"
                        | "password"
                        | "access_token"
                        | "refresh_token"
                        | "client_secret"
                        | "secret"
                        | "cookie"
                        | "set_cookie"
                        | "token"
                        | "credentials"
                ) {
                    *value = Value::String("[REDACTED]".into());
                    changed = true;
                } else {
                    changed |= scrub(value);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                changed |= scrub(value);
            }
        }
        _ => {}
    }
    changed
}

fn safe_text(text: &str, secret: &str) -> String {
    let text = if secret.is_empty() {
        text.to_owned()
    } else {
        text.replace(secret, "[REDACTED]")
    };
    let mut value = String::new();
    for ch in text.chars() {
        if ch.is_control() && ch != '\n' && ch != '\t' {
            value.extend(ch.escape_default());
        } else {
            value.push(ch);
        }
    }
    value
}
impl Diagnostic {
    pub fn capture(body: &str, headers: &reqwest::header::HeaderMap, secret: &str) -> Self {
        let mut body = body.to_owned();
        if let Ok(mut json) = serde_json::from_str::<Value>(&body) {
            if scrub(&mut json) {
                body = serde_json::to_string_pretty(&json).unwrap();
            }
        }
        body = safe_text(&body, secret);
        let truncated = body.len() > LIMIT;
        if truncated {
            let mut end = LIMIT;
            while !body.is_char_boundary(end) {
                end -= 1;
            }
            body.truncate(end);
        }
        let request_ids = ["x-request-id", "x-amzn-requestid", "cf-ray"]
            .into_iter()
            .filter_map(|name| {
                headers
                    .get(name)
                    .and_then(|value| value.to_str().ok())
                    .map(|value| {
                        (
                            name.into(),
                            safe_text(value, secret).chars().take(256).collect(),
                        )
                    })
            })
            .collect();
        Self {
            body,
            truncated,
            request_ids,
        }
    }
    pub fn redact_key(&mut self, secret: &str) {
        // Capture normally redacts before truncation; transport causes are also scrubbed here.
        if !secret.is_empty() {
            self.body = self.body.replace(secret, "[REDACTED]");
            if self.truncated {
                for end in (1..secret.len())
                    .rev()
                    .filter(|end| secret.is_char_boundary(*end))
                {
                    if self.body.ends_with(&secret[..end]) {
                        self.body.truncate(self.body.len() - end);
                        self.body.push_str("[REDACTED]");
                        break;
                    }
                }
            }
            for (_, value) in &mut self.request_ids {
                *value = value.replace(secret, "[REDACTED]");
            }
        }
    }
    pub fn text(&self) -> String {
        format!(
            "Provider response/error · credentials redacted{}\n{}\n{}",
            if self.truncated {
                " · truncated at 16 KiB"
            } else {
                ""
            },
            self.request_ids
                .iter()
                .map(|(key, value)| format!("{key}: {value}"))
                .collect::<Vec<_>>()
                .join("\n"),
            self.body
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_preserve_messages_but_scrub_keys_structured_secrets_and_controls() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("x-request-id", "support-123".parse().unwrap());
        headers.insert("set-cookie", "private-cookie".parse().unwrap());
        let diagnostic = Diagnostic::capture(
            r#"{"detail":"Model busy","api_key":"other-private-key","nested":{"Authorization":"Bearer private-token"},"message":"used known-api-key"}"#,
            &headers,
            "known-api-key",
        );
        let text = diagnostic.text();
        assert!(text.contains("Model busy"));
        assert!(text.contains("support-123"));
        for secret in [
            "other-private-key",
            "private-token",
            "known-api-key",
            "private-cookie",
        ] {
            assert!(!text.contains(secret), "leaked {secret}");
        }
        let diagnostic = Diagnostic::capture("gateway \u{1b}[31m error", &headers, "");
        assert!(!diagnostic.body.contains('\u{1b}'));
        let diagnostic = Diagnostic::capture(&"é".repeat(20_000), &headers, "");
        assert!(diagnostic.truncated);
        assert!(diagnostic.body.len() <= LIMIT);
    }
}
