//! Streaming preview is ephemeral. Only a complete response reaches the action parser.
use super::{Completion, Failure, Kind};
use serde_json::Value;
use std::sync::{Arc, Mutex};

pub type Preview = Arc<Mutex<LiveOutput>>;
#[derive(Default)]
pub struct LiveOutput {
    pub model: String,
    pub connection: Option<super::ConnectionInfo>,
    pub text: String,
    pub state: String,
    pub truncated: bool,
}
impl LiveOutput {
    pub fn start(&mut self, model: &str) {
        *self = Self {
            model: model.into(),
            connection: self.connection.clone(),
            state: "Waiting for model…".into(),
            ..Self::default()
        };
    }
    fn append(&mut self, text: &str) {
        self.state = "Receiving model output…".into();
        self.text.push_str(text);
        if self.text.len() > 65536 {
            let mut boundary = self.text.len() - 65536;
            while !self.text.is_char_boundary(boundary) {
                boundary += 1;
            }
            self.text.drain(..boundary);
            self.truncated = true;
        }
    }
}
fn protocol() -> Failure {
    Failure {
        billing_uncertain: true,
        ..Failure::local(Kind::Protocol)
    }
}
#[derive(Default)]
struct Stream {
    pending: Vec<u8>,
    event: Vec<String>,
    content: String,
    usage: Option<Value>,
    id: Option<String>,
    model: Option<String>,
    finished: bool,
    done: bool,
    saw_content: bool,
    failure: Option<Failure>,
}
impl Stream {
    fn feed(&mut self, bytes: &[u8], preview: &Preview) -> Result<(), Failure> {
        self.pending.extend_from_slice(bytes);
        while let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
            let line =
                String::from_utf8(self.pending.drain(..=end).collect()).map_err(|_| protocol())?;
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                self.dispatch(preview)?;
            } else if let Some(data) = line.strip_prefix("data:") {
                self.event
                    .push(data.strip_prefix(' ').unwrap_or(data).into());
            }
        }
        Ok(())
    }
    fn dispatch(&mut self, preview: &Preview) -> Result<(), Failure> {
        if self.event.is_empty() {
            return Ok(());
        }
        let data = std::mem::take(&mut self.event).join("\n");
        if self.done {
            return Err(protocol());
        }
        if data == "[DONE]" {
            self.done = true;
            return Ok(());
        }
        let v: Value = serde_json::from_str(&data).map_err(|_| protocol())?;
        if let Some(id) = v["id"].as_str() {
            self.id = Some(id.into());
        }
        if let Some(model) = v["model"].as_str() {
            self.model = Some(model.into());
        }
        if let Some(usage) = v.get("usage").filter(|u| !u.is_null()) {
            self.usage = Some(usage.clone());
        }
        if v.get("error").is_some_and(|e| !e.is_null()) {
            let mut failure = Failure::http(200, &v, None);
            // An accepted streaming request may already have produced billable tokens.
            failure.retryable = false;
            failure.billing_uncertain = true;
            self.failure = Some(failure);
            return Ok(());
        }
        if self.failure.is_some() {
            return Err(protocol());
        }
        let choices = v["choices"].as_array().ok_or_else(protocol)?;
        for choice in choices {
            if choice["index"].as_u64() != Some(0) {
                continue;
            }
            if let Some(text) = choice["delta"]["content"].as_str() {
                if self.finished && !text.is_empty() {
                    return Err(protocol());
                }
                self.saw_content = true;
                self.content.push_str(text);
                preview.lock().unwrap().append(text);
            }
            if !choice["finish_reason"].is_null() {
                self.finished = true;
            }
        }
        Ok(())
    }
    fn completion(self, read_error: Option<Failure>) -> Completion {
        let failure = read_error.or(self.failure).or_else(|| {
            if !self.done || !self.finished || !self.saw_content {
                Some(protocol())
            } else {
                None
            }
        });
        Completion {
            content: match &failure {
                Some(error) => Err(error.to_string()),
                None => Ok(self.content),
            },
            failure,
            raw_usage: self.usage,
            response_id: self.id,
            response_model: self.model,
        }
    }
}
pub async fn read(mut response: reqwest::Response, preview: &Preview) -> Completion {
    let mut stream = Stream::default();
    let mut total = 0usize;
    let mut error = None;
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                total += chunk.len();
                if total > 8 * 1024 * 1024 {
                    error = Some(protocol());
                    break;
                }
                if let Err(failure) = stream.feed(&chunk, preview) {
                    error = Some(failure);
                    break;
                }
                if stream.done || stream.failure.is_some() {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => {
                error = Some(protocol());
                break;
            }
        }
    }
    stream.completion(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(value: Value) -> String {
        format!("data: {value}\r\n\r\n")
    }
    #[test]
    fn split_utf8_frames_keep_live_preview_and_final_usage() {
        let preview = Preview::default();
        let mut stream = Stream::default();
        let content = event(
            serde_json::json!({"id":"abc","model":"actual","choices":[{"index":0,"delta":{"content":"café👩‍💻"},"finish_reason":null}]}),
        );
        for byte in content.as_bytes() {
            stream.feed(&[*byte], &preview).unwrap();
        }
        assert_eq!(preview.lock().unwrap().text, "café👩‍💻");
        assert!(!stream.done);
        stream
            .feed(
                event(
                    serde_json::json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
                )
                .as_bytes(),
                &preview,
            )
            .unwrap();
        stream.feed(event(serde_json::json!({"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":8,"prompt_tokens_details":{"cached_tokens":4}}})).as_bytes(),&preview).unwrap();
        stream.feed(b"data: [DONE]\n\n", &preview).unwrap();
        let result = stream.completion(None);
        assert!(result.failure.is_none());
        assert_eq!(result.content.unwrap(), "café👩‍💻");
        assert_eq!(
            result.raw_usage.unwrap()["prompt_tokens_details"]["cached_tokens"],
            4
        );
    }
    #[test]
    fn truncated_or_error_stream_never_returns_an_executable_action_or_retries() {
        for body in [
            event(
                serde_json::json!({"choices":[{"index":0,"delta":{"content":"action: shell"},"finish_reason":null}]}),
            ),
            event(
                serde_json::json!({"error":{"code":"rate_limit_exceeded","message":"secret"},"usage":{"completion_tokens":9}}),
            ),
        ] {
            let mut stream = Stream::default();
            stream.feed(body.as_bytes(), &Preview::default()).unwrap();
            let completion = stream.completion(None);
            let failure = completion.failure.unwrap();
            if failure.kind == Kind::RateLimit {
                assert_eq!(completion.raw_usage.unwrap()["completion_tokens"], 9);
            }
            assert!(completion.content.is_err());
            assert!(!failure.retryable);
            assert!(failure.billing_uncertain);
            assert!(!failure.to_string().contains("secret"));
        }
    }
    #[test]
    fn malformed_stream_retains_usage_and_preview_is_bounded() {
        let mut stream = Stream::default();
        let preview = Preview::default();
        stream
            .feed(
                event(serde_json::json!({"choices":[],"usage":{"completion_tokens":7}})).as_bytes(),
                &preview,
            )
            .unwrap();
        let error = stream
            .feed(b"data: invalid-secret\n\n", &preview)
            .unwrap_err();
        let completion = stream.completion(Some(error));
        assert_eq!(completion.raw_usage.unwrap()["completion_tokens"], 7);
        preview.lock().unwrap().append(&"é".repeat(40000));
        let p = preview.lock().unwrap();
        assert!(p.text.len() <= 65536);
        assert!(p.truncated);
    }
}
