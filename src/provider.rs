use crate::config::ProviderConfig;
use crate::provider_failure::{Failure, Kind};
use anyhow::Result;
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::{atomic::AtomicBool, Arc};

#[derive(Clone)]
pub struct Provider {
    client: Client,
    pub cfg: ProviderConfig,
    pub retry_cancel: Arc<AtomicBool>,
    #[cfg(test)]
    pub script: Option<std::sync::Arc<std::sync::Mutex<Script>>>,
}
#[cfg(test)]
#[derive(Default)]
pub struct Script {
    pub responder: Option<fn(&[Value])->std::result::Result<String,String>>,
    pub responses: std::collections::VecDeque<Result<String, String>>,
    pub requests: Vec<Vec<Value>>,
    pub usages: std::collections::VecDeque<Option<Value>>,
    pub completions: std::collections::VecDeque<Completion>,
}
pub struct Completion {
    pub failure: Option<Failure>,
    pub content: std::result::Result<String, String>,
    pub raw_usage: Option<Value>,
    pub response_id: Option<String>,
    pub response_model: Option<String>,
}
impl Completion {
    pub(crate) fn parse(status: u16, body: &str) -> Result<Self> {
        let v: Value = if (200..300).contains(&status) {
            serde_json::from_str(body).map_err(|_| Failure {
                billing_uncertain: true,
                ..Failure::local(Kind::Protocol)
            })?
        } else {
            serde_json::from_str(body).unwrap_or(Value::Null)
        };
        let mut failure = if !(200..300).contains(&status)
            || v.get("error").is_some_and(|error| !error.is_null())
        {
            Some(Failure::http(status, &v, None))
        } else if v["choices"][0]["message"]["content"].as_str().is_none() {
            Some(Failure {
                billing_uncertain: true,
                ..Failure::local(Kind::Protocol)
            })
        } else {
            None
        };
        if let Some(failure) = &mut failure {
            if failure.retryable && v.get("usage").is_some_and(|usage| !usage.is_null()) {
                failure.retryable = false;
                failure.billing_uncertain = true;
            }
        }
        let content = if let Some(failure) = &failure {
            Err(failure.to_string())
        } else {
            Ok(v["choices"][0]["message"]["content"]
                .as_str()
                .unwrap()
                .to_owned())
        };
        Ok(Self {
            failure,
            content,
            raw_usage: v.get("usage").cloned(),
            response_id: v["id"].as_str().map(str::to_owned),
            response_model: v["model"].as_str().map(str::to_owned),
        })
    }
}
async fn read_body(mut response: reqwest::Response, inference: bool) -> Result<String> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| Failure {
        billing_uncertain: inference,
        ..Failure::local(Kind::Protocol)
    })? {
        if bytes.len() + chunk.len() > 8 * 1024 * 1024 {
            return Err(Failure {
                billing_uncertain: inference,
                ..Failure::local(Kind::Protocol)
            }
            .into());
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| {
        Failure {
            billing_uncertain: inference,
            ..Failure::local(Kind::Protocol)
        }
        .into()
    })
}
impl Provider {
    pub fn new(cfg: ProviderConfig) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .connect_timeout(std::time::Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("HTTP client configuration is valid"),
            cfg,
            retry_cancel: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            script: None,
        }
    }
    fn key(&self) -> Result<String> {
        std::env::var(&self.cfg.api_key_env)
            .ok()
            .filter(|key| !key.is_empty() && key.bytes().all(|b| b.is_ascii_graphic()))
            .ok_or_else(|| Failure::local(Kind::Credentials).into())
    }
    pub async fn complete(&self, model: &str, messages: &[Value]) -> Result<Completion> {
        #[cfg(test)]
        if let Some(script) = &self.script {
            let mut script = script.lock().unwrap();
            script.requests.push(messages.to_vec());
            if let Some(completion) = script.completions.pop_front() {
                return Ok(completion);
            }
            let usage = script.usages.pop_front().flatten();
            let content = match script.responder {
                Some(responder)=>responder(messages),
                None=>script.responses.pop_front().unwrap_or_else(||Err("unexpected provider call".into())),
            }.map_err(anyhow::Error::msg)?;
            return Ok(Completion {
                failure: None,
                content: Ok(content),
                raw_usage: usage,
                response_id: None,
                response_model: None,
            });
        }
        let data = json!({"model":model,"messages":messages,"max_tokens":self.cfg.max_tokens,"stream":false});
        let r = self
            .client
            .post(format!(
                "{}/chat/completions",
                self.cfg.base_url.trim_end_matches('/')
            ))
            .bearer_auth(self.key()?)
            .json(&data)
            .send()
            .await
            .map_err(|error| Failure::transport(&error))?;
        let status = r.status();
        let retry = crate::provider_failure::retry_after(
            r.headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok()),
        );
        let body = read_body(r, true).await?;
        let mut completion = Completion::parse(status.as_u16(), &body)?;
        if let Some(failure) = &mut completion.failure {
            failure.retry_after_seconds = retry;
            completion.content = Err(failure.to_string());
        }
        Ok(completion)
    }
    pub async fn models(&self) -> Result<Vec<String>> {
        let r = self
            .client
            .get(format!(
                "{}/models",
                self.cfg.base_url.trim_end_matches('/')
            ))
            .bearer_auth(self.key()?)
            .send()
            .await
            .map_err(|error| {
                let mut failure = Failure::transport(&error);
                failure.billing_uncertain = false;
                failure
            })?;
        let status = r.status();
        let retry = crate::provider_failure::retry_after(
            r.headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok()),
        );
        let body = read_body(r, false).await?;
        let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
        if !status.is_success() {
            let mut failure = Failure::http(status.as_u16(), &v, retry);
            failure.billing_uncertain = false;
            return Err(failure.into());
        }
        let ids = v["data"]
            .as_array()
            .ok_or_else(|| Failure::local(Kind::Protocol))?
            .iter()
            .filter_map(|m| m["id"].as_str().map(str::to_owned))
            .collect();
        Ok(ids)
    }
}

#[cfg(test)]
mod failure_tests {
    use super::*;
    #[test]
    fn protocol_errors_and_non_json_http_errors_are_safe() {
        let error = Completion::parse(200, "fixture-secret not JSON")
            .err()
            .unwrap();
        assert!(!error.to_string().contains("fixture-secret"));
        assert!(error.downcast_ref::<Failure>().unwrap().billing_uncertain);
        let completion = Completion::parse(502, "<html>fixture-secret</html>").unwrap();
        assert_eq!(completion.failure.unwrap().kind, Kind::Unavailable);
        assert!(!completion.content.unwrap_err().contains("fixture-secret"));
        let completion = Completion::parse(
            429,
            r#"{"error":{"code":"rate_limit_exceeded"},"usage":{"completion_tokens":2}}"#,
        )
        .unwrap();
        let failure = completion.failure.unwrap();
        assert!(!failure.retryable);
        assert!(failure.billing_uncertain);
    }
    #[test]
    fn missing_credentials_produce_safe_local_guidance() {
        let cfg: ProviderConfig = serde_yaml::from_str(&format!(
            "model: fixture\napi_key_env: GREENLIGHT_MISSING_{}\n",
            uuid::Uuid::new_v4().simple()
        ))
        .unwrap();
        let provider = Provider::new(cfg);
        let error = provider.key().unwrap_err();
        assert_eq!(
            error.downcast_ref::<Failure>().unwrap().kind,
            Kind::Credentials
        );
        assert!(error.to_string().contains("environment variable"));
    }
}

#[cfg(test)]
mod http_tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    fn mock(
        status: u16,
        body: &str,
        header: &str,
    ) -> (Provider, String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let body = body.to_string();
        let header = header.to_string();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 1024];
            loop {
                let n = socket.read(&mut buffer).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..n]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                    let length = head
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let response=format!("HTTP/1.1 {status} Mock\r\nContent-Length: {}\r\nConnection: close\r\n{header}\r\n{body}",body.len());
            socket.write_all(response.as_bytes()).unwrap();
        });
        let name = format!("GREENLIGHT_TEST_KEY_{}", uuid::Uuid::new_v4().simple());
        std::env::set_var(&name, "fixture-key");
        let cfg: ProviderConfig = serde_yaml::from_str(&format!(
            "model: fixture\nbase_url: http://{address}/v1\napi_key_env: {name}\n"
        ))
        .unwrap();
        let mut provider = Provider::new(cfg);
        provider.client = Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .unwrap();
        (provider, name, server)
    }
    #[tokio::test]
    async fn actual_http_response_preserves_retry_after_and_hides_error_body() {
        let (provider, key, server) = mock(
            429,
            r#"{"error":{"code":"rate_limit_exceeded","message":"fixture-key fixture-secret"}}"#,
            "Retry-After: 7\r\n",
        );
        let completion = provider.complete("fixture", &[]).await.unwrap();
        let failure = completion.failure.unwrap();
        assert_eq!(failure.retry_after_seconds, Some(7));
        assert!(failure.retryable);
        assert!(!completion.content.unwrap_err().contains("fixture-key"));
        server.join().unwrap();
        std::env::remove_var(key);
    }
    #[tokio::test]
    async fn models_endpoint_classifies_non_json_auth_errors_without_echoing_them() {
        let (provider, key, server) = mock(401, "fixture-key fixture-secret", "");
        let error = provider.models().await.unwrap_err();
        assert_eq!(
            error.downcast_ref::<Failure>().unwrap().kind,
            Kind::Credentials
        );
        assert!(!error.to_string().contains("fixture-key"));
        server.join().unwrap();
        std::env::remove_var(key);
    }
}
