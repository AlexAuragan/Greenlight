//! Explicit UTF-8 materialization budgets; never trim a full evidence record silently.
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
#[derive(Debug)]
pub struct Exceeded(pub String);
impl std::fmt::Display for Exceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Exceeded {}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub source_bytes: usize,
    pub expanded_prompt_bytes: usize,
    pub blob_bytes: usize,
    pub materialized_bytes: usize,
    pub history_events: usize,
    pub export_bytes: usize,
    pub export_events: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            source_bytes: 1 << 20,
            expanded_prompt_bytes: 1 << 20,
            blob_bytes: 32 << 20,
            materialized_bytes: 64 << 20,
            history_events: 100_000,
            export_bytes: 128 << 20,
            export_events: 100_000,
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<()> {
        if [
            self.source_bytes,
            self.expanded_prompt_bytes,
            self.blob_bytes,
            self.materialized_bytes,
            self.history_events,
            self.export_bytes,
            self.export_events,
        ]
        .contains(&0)
        {
            bail!("resource limits must be positive");
        }
        Ok(())
    }
}
pub struct Budget {
    pub used: usize,
    limit: usize,
}
impl Budget {
    pub fn new(limit: usize) -> Self {
        Self { used: 0, limit }
    }
    pub fn remaining(&self) -> usize {
        self.limit - self.used
    }
    pub fn charge(&mut self, bytes: usize) -> Result<()> {
        if bytes > self.remaining() {
            return Err(Exceeded(format!("Materialized evidence exceeds {} bytes; recorded data is unchanged. Increase limits.materialized_bytes/export_bytes and restart to read it in full.", self.limit)).into());
        }
        self.used += bytes;
        Ok(())
    }
}

/// Serialization refuses excess output before extending the allocation.
pub fn json_bytes(value: &impl Serialize, limit: usize) -> Result<Vec<u8>> {
    struct Writer {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl std::io::Write for Writer {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            if data.len() > self.limit.saturating_sub(self.bytes.len()) {
                return Err(std::io::Error::other(
                    "JSON exceeds configured byte budget; no full export was produced",
                ));
            }
            self.bytes.extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Writer {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut writer, value)?;
    Ok(writer.bytes)
}

pub struct Text {
    bytes: String,
    limit: usize,
    exceeded: bool,
}
impl Text {
    pub fn new(limit: usize) -> Self {
        Self {
            bytes: String::new(),
            limit,
            exceeded: false,
        }
    }
    pub fn push_str(&mut self, text: &str) {
        if text.len() > self.limit.saturating_sub(self.bytes.len()) {
            self.exceeded = true;
        } else if !self.exceeded {
            self.bytes.push_str(text);
        }
    }
    pub fn finish(self) -> Result<String> {
        if self.exceeded {
            bail!("Conversation rendering exceeds limits.export_bytes; no partial full export was produced. Increase the limit and restart.");
        }
        Ok(self.bytes)
    }
}
