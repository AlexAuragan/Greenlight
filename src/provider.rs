#[path = "provider_stream.rs"]
pub mod stream;
use crate::config::ProviderConfig;
use crate::provider_failure::{Failure, Kind};
use anyhow::Result;
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::{atomic::AtomicBool, Arc};

#[derive(Clone)]
pub struct Provider {
    client: Client,
    credential: crate::credentials::Credential,
    pub cfg: ProviderConfig,
    pub retry_cancel: Arc<AtomicBool>,
    pub live: Option<stream::Preview>,
    #[cfg(test)]
    pub script: Option<std::sync::Arc<std::sync::Mutex<Script>>>,
}
#[cfg(test)]
type ScriptResponder = fn(&[Value]) -> std::result::Result<String, String>;

#[cfg(test)]
#[derive(Default)]
pub struct Script {
    pub responder: Option<ScriptResponder>,
    pub responses: std::collections::VecDeque<Result<String, String>>,
    pub requests: Vec<Vec<Value>>,
    pub usages: std::collections::VecDeque<Option<Value>>,
    pub completions: std::collections::VecDeque<Completion>,
}
/// Safe, ephemeral UI metadata; never includes the full key or URL credentials.
#[derive(Clone, Debug)]
pub(crate) struct ConnectionInfo {
    pub endpoint: String,
    pub model: String,
    pub credential: crate::credentials::Summary,
}
impl ConnectionInfo {
    pub fn current(cfg: &ProviderConfig) -> Self {
        Self::new(
            cfg,
            crate::credentials::Credential::capture(&cfg.api_key_env).summary,
        )
    }
    fn new(cfg: &ProviderConfig, credential: crate::credentials::Summary) -> Self {
        Self {
            endpoint: reqwest::Url::parse(&cfg.base_url)
                .ok()
                .filter(|url| matches!(url.scheme(), "https" | "http") && url.host_str().is_some())
                .map(|url| url.origin().ascii_serialization())
                .unwrap_or_else(|| "[invalid endpoint]".into()),
            model: cfg.model.clone(),
            credential,
        }
    }
    pub fn text(&self) -> String {
        format!("Endpoint origin: {}\nConfigured default model: {}\nAPI-key variable: {}\nAPI key: {}\nCredential source: {}\n\nPrecedence: inherited environment > .greenlight/.env > project .env.\nAn existing empty variable also overrides files.\nCredentials are loaded at process startup; editing .env does not reload them.\nRestart Greenlight after changing keys. A paused retry retains its captured key.\nOnly the first/last five characters are shown; short keys stay fully hidden.",
            self.endpoint, self.model, self.credential.name, self.credential.masked, self.credential.source)
    }
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
            if v.get("usage").is_some_and(|usage| !usage.is_null()) {
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
                // The engine owns retries and records each dispatch in the usage ledger.
                .retry(reqwest::retry::never())
                .build()
                .expect("HTTP client configuration is valid"),
            credential: crate::credentials::Credential::capture(&cfg.api_key_env),
            cfg,
            retry_cancel: Arc::new(AtomicBool::new(false)),
            live: None,
            #[cfg(test)]
            script: None,
        }
    }
    fn key(&self) -> Result<&str> {
        self.credential
            .value()
            .ok_or_else(|| Failure::local(Kind::Credentials).into())
    }
    pub(crate) fn connection_info(&self) -> ConnectionInfo {
        ConnectionInfo::new(&self.cfg, self.credential.summary.clone())
    }
    pub async fn complete(&self, model: &str, messages: &[Value]) -> Result<Completion> {
        if let Some(live) = &self.live {
            live.lock().unwrap().start(model);
        }
        let result = tokio::select! {
            biased;
            _ = crate::provider_failure::cancelled(&self.retry_cancel) => {
                let mut failure = Failure::local(Kind::Cancelled);
                // Dropping the HTTP future stops local work, not remote billing.
                failure.billing_uncertain = true;
                Err(failure.into())
            }
            result = self.complete_inner(model, messages) => result,
        };
        if let Some(live) = &self.live {
            live.lock().unwrap().state =
                if self.retry_cancel.load(std::sync::atomic::Ordering::SeqCst) {
                    "Stopped · remote processing/billing may be uncertain"
                } else if result.as_ref().is_ok_and(|c| c.failure.is_none()) {
                    "Response complete · validating action"
                } else {
                    "Request failed · inspect diagnostic/history"
                }
                .into();
        }
        result
    }
    async fn complete_inner(&self, model: &str, messages: &[Value]) -> Result<Completion> {
        #[cfg(test)]
        if let Some(script) = &self.script {
            let mut script = script.lock().unwrap();
            script.requests.push(messages.to_vec());
            if let Some(completion) = script.completions.pop_front() {
                return Ok(completion);
            }
            let usage = script.usages.pop_front().flatten();
            let content = match script.responder {
                Some(responder) => responder(messages),
                None => script
                    .responses
                    .pop_front()
                    .unwrap_or_else(|| Err("unexpected provider call".into())),
            }
            .map_err(anyhow::Error::msg)?;
            return Ok(Completion {
                failure: None,
                content: Ok(content),
                raw_usage: usage,
                response_id: None,
                response_model: None,
            });
        }
        let mut data = json!({"model":model,"messages":messages,"max_tokens":self.cfg.max_tokens,"stream":self.live.is_some()});
        if self.live.is_some() {
            data["stream_options"] = json!({"include_usage":true});
        }
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
        if status.is_success()
            && r.headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| {
                    v.split(';')
                        .next()
                        .is_some_and(|t| t.trim().eq_ignore_ascii_case("text/event-stream"))
                })
        {
            if let Some(live) = &self.live {
                return Ok(stream::read(r, live).await);
            }
        }
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
    ) -> (Provider, String, std::thread::JoinHandle<Vec<u8>>) {
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
            request
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
    #[tokio::test]
    async fn http_stream_updates_preview_before_completion_and_keeps_usage() {
        let body = concat!(
            "data: {\"id\":\"stream-1\",\"model\":\"fixture\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"action: done\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":20,\"completion_tokens\":3,\"prompt_tokens_details\":{\"cached_tokens\":5}}}\n\n",
            "data: [DONE]\n\n"
        );
        let (mut provider, key, server) = mock(200, body, "Content-Type: text/event-stream\r\n");
        let preview = stream::Preview::default();
        provider.live = Some(preview.clone());
        let completion = provider.complete("fixture", &[]).await.unwrap();
        assert_eq!(preview.lock().unwrap().text, "action: done");
        assert_eq!(completion.content.unwrap(), "action: done");
        assert_eq!(completion.raw_usage.unwrap()["completion_tokens"], 3);
        server.join().unwrap();
        std::env::remove_var(key);
    }
    #[tokio::test]
    async fn truncated_http_stream_is_billing_uncertain_and_preserves_reported_usage() {
        let (mut provider, key, server) = mock(
            200,
            "data: {\"choices\":[],\"usage\":{\"completion_tokens\":3}}\n\n",
            "Content-Type: text/event-stream\r\n",
        );
        provider.live = Some(stream::Preview::default());
        let completion = provider.complete("fixture", &[]).await.unwrap();
        let failure = completion.failure.unwrap();
        assert!(!failure.retryable);
        assert!(failure.billing_uncertain);
        assert!(completion.content.is_err());
        assert_eq!(completion.raw_usage.unwrap()["completion_tokens"], 3);
        server.join().unwrap();
        std::env::remove_var(key);
    }
    #[tokio::test]
    async fn chat_transport_sends_one_openai_compatible_request_in_both_modes() {
        for streaming in [false, true] {
            let (mut provider, key, server) = mock(
                200,
                r#"{"choices":[{"message":{"content":"fixture"}}]}"#,
                "Content-Type: application/json\r\n",
            );
            if streaming {
                provider.live = Some(stream::Preview::default());
            }
            let messages = vec![
                json!({"role":"system", "content":"fixture protocol"}),
                json!({"role":"user", "content":"café fixture"}),
            ];
            std::env::set_var(&key, "replacement-after-provider-start");
            provider.complete("fixture/model", &messages).await.unwrap();
            let request = server.join().unwrap();
            let end = request
                .windows(4)
                .position(|part| part == b"\r\n\r\n")
                .unwrap();
            let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
            assert!(headers.starts_with("post /v1/chat/completions http/1.1"));
            assert!(headers.contains("authorization: bearer fixture-key"));
            assert!(headers.contains("content-type: application/json"));
            let body: Value = serde_json::from_slice(&request[end + 4..]).unwrap();
            assert_eq!(body["model"], "fixture/model");
            assert_eq!(body["messages"], json!(messages));
            assert_eq!(body["max_tokens"], 4096);
            assert_eq!(body["stream"], streaming);
            if streaming {
                assert_eq!(body["stream_options"], json!({"include_usage":true}));
            } else {
                assert!(body.get("stream_options").is_none());
            }
            assert!(
                body.get("n").is_none(),
                "do not request multiple completions per turn"
            );
            std::env::remove_var(key);
        }
    }
}

#[cfg(test)]
mod stop_tests {
    use super::*;
    #[tokio::test]
    async fn stop_returns_before_http_response_and_marks_billing_uncertain() {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sent, received) = tokio::sync::oneshot::channel();
        let (finished, finish) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut buffer = [0; 4096];
            assert!(socket.read(&mut buffer).unwrap() > 0);
            sent.send(()).unwrap();
            // No response is sent until the client has reported cancellation.
            finish
                .recv_timeout(std::time::Duration::from_secs(3))
                .unwrap();
        });
        let name = format!("GREENLIGHT_STOP_TEST_{}", uuid::Uuid::new_v4().simple());
        std::env::set_var(&name, "fixture-key");
        let cfg = serde_yaml::from_str(&format!(
            "model: fixture\nbase_url: http://{address}/v1\napi_key_env: {name}\n"
        ))
        .unwrap();
        let mut provider = Provider::new(cfg);
        provider.client = Client::builder().no_proxy().build().unwrap();
        let cancel = provider.retry_cancel.clone();
        let trigger = tokio::spawn(async move {
            received.await.unwrap();
            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            provider.complete("fixture", &[]),
        )
        .await
        .unwrap()
        .err()
        .expect("request should be cancelled");
        let failure = error.downcast_ref::<Failure>().unwrap();
        assert_eq!(failure.kind, Kind::Cancelled);
        assert!(failure.billing_uncertain);
        trigger.await.unwrap();
        finished.send(()).unwrap();
        server.join().unwrap();
        std::env::remove_var(name);
    }
}
