use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use crate::config::ProviderConfig;

#[derive(Clone)]
pub struct Provider { client: Client, pub cfg: ProviderConfig }
impl Provider {
    pub fn new(cfg: ProviderConfig) -> Self { Self {client:Client::new(),cfg} }
    fn key(&self) -> Result<String> { std::env::var(&self.cfg.api_key_env).with_context(|| format!("missing provider key env {}",self.cfg.api_key_env)) }
    pub async fn complete(&self,model:&str,messages:&[Value]) -> Result<String> {
        let data = json!({"model":model,"messages":messages,"max_tokens":self.cfg.max_tokens,"stream":false});
        let r=self.client.post(format!("{}/chat/completions",self.cfg.base_url.trim_end_matches('/')))
            .bearer_auth(self.key()?).json(&data).send().await.context("provider request failed")?;
        let status=r.status(); let body=r.text().await?;
        if !status.is_success() { bail!("provider returned HTTP {}: {}",status,body.chars().take(1000).collect::<String>()); }
        let v:Value=serde_json::from_str(&body).context("invalid provider API response")?;
        let msg=v["choices"][0]["message"]["content"].as_str().context("provider did not return text content")?;
        Ok(msg.to_owned())
    }
    pub async fn models(&self) -> Result<Vec<String>> {
        let r=self.client.get(format!("{}/models",self.cfg.base_url.trim_end_matches('/'))).bearer_auth(self.key()?).send().await?;
        let status=r.status(); let body=r.text().await?;
        if !status.is_success(){bail!("models endpoint HTTP {status}: {body}");}
        let v:Value=serde_json::from_str(&body)?;
        let ids=v["data"].as_array().context("model list missing data array")?.iter()
            .filter_map(|m|m["id"].as_str().map(str::to_owned)).collect();
        Ok(ids)
    }
}
