use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use crate::config::ProviderConfig;

#[derive(Clone)]
pub struct Provider { client: Client, pub cfg: ProviderConfig,
    #[cfg(test)]
    pub script: Option<std::sync::Arc<std::sync::Mutex<Script>>>,
}
#[cfg(test)]
#[derive(Default)]
pub struct Script {
    pub responses: std::collections::VecDeque<Result<String, String>>,
    pub requests: Vec<Vec<Value>>,
    pub usages: std::collections::VecDeque<Option<Value>>,
}
pub struct Completion {
    pub content: std::result::Result<String,String>,
    pub raw_usage: Option<Value>,
    pub response_id: Option<String>,
    pub response_model: Option<String>,
}
impl Completion {
    pub(crate) fn parse(status:u16,body:&str)->Result<Self> {
        let v:Value=serde_json::from_str(body).context("invalid provider API response")?;
        let content=if !(200..300).contains(&status) {
            Err(format!("provider returned HTTP {status}: {}",body.chars().take(1000).collect::<String>()))
        } else {v["choices"][0]["message"]["content"].as_str().map(str::to_owned).ok_or_else(||"provider did not return text content".into())};
        Ok(Self {content,raw_usage:v.get("usage").cloned(),response_id:v["id"].as_str().map(str::to_owned),response_model:v["model"].as_str().map(str::to_owned)})
    }
}
impl Provider {
    pub fn new(cfg: ProviderConfig) -> Self { Self {client:Client::new(),cfg,
        #[cfg(test)]
        script: None,
    } }
    fn key(&self) -> Result<String> { std::env::var(&self.cfg.api_key_env).with_context(|| format!("missing provider key env {}",self.cfg.api_key_env)) }
    pub async fn complete(&self,model:&str,messages:&[Value]) -> Result<Completion> {
        #[cfg(test)]
        if let Some(script) = &self.script {
            let mut script = script.lock().unwrap();
            script.requests.push(messages.to_vec());
            let usage=script.usages.pop_front().flatten();
            let content=script.responses.pop_front().unwrap_or_else(||Err("unexpected provider call".into())).map_err(anyhow::Error::msg)?;
            return Ok(Completion {content:Ok(content),raw_usage:usage,response_id:None,response_model:None});
        }
        let data = json!({"model":model,"messages":messages,"max_tokens":self.cfg.max_tokens,"stream":false});
        let r=self.client.post(format!("{}/chat/completions",self.cfg.base_url.trim_end_matches('/')))
            .bearer_auth(self.key()?).json(&data).send().await.context("provider request failed")?;
        let status=r.status(); let body=r.text().await?;
        Completion::parse(status.as_u16(),&body)
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
