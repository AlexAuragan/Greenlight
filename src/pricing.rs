//! Public authoritative catalog discovery. Never sends API keys or inference data.
use crate::{
    config::ProviderConfig,
    usage::{CacheCreationPricing, ModelMetadata, Pricing},
};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::Path,
    time::Duration,
};

const SOURCE: &str = "https://api.deepinfra.com/models/list";
const LIMIT: u64 = 8 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    source: String,
    checked_at: DateTime<Utc>,
    pub models: BTreeMap<String, Pricing>,
    pub note: Option<String>,
}

fn supported(cfg: &ProviderConfig) -> bool {
    // Match the exact transport, not model prefixes or a look-alike proxy host.
    cfg.base_url.trim_end_matches('/') == "https://api.deepinfra.com/v1/openai"
}

fn number(v: &Value) -> Option<f64> {
    let n = v.as_f64().or_else(|| v.as_str()?.parse().ok())?;
    (n.is_finite() && n >= 0.0).then_some(n)
}

fn parse_catalog(raw: &str, now: DateTime<Utc>) -> Result<BTreeMap<String, Pricing>> {
    let value: Value = serde_json::from_str(raw).context("Invalid public pricing catalog JSON")?;
    let rows = value
        .as_array()
        .context("Invalid public pricing catalog shape")?;
    let mut models = BTreeMap::new();
    for row in rows {
        if row["type"] != "text-generation" {
            continue;
        }
        let Some(id) = row["model_name"]
            .as_str()
            .filter(|v| !v.is_empty() && v.len() < 512 && !v.chars().any(char::is_control))
        else {
            continue;
        };
        let p = &row["pricing"];
        // A prose table/discount or explicit-cache granularity needs a different
        // billing adapter; never silently flatten it into regular token prices.
        if p["type"] != "tokens"
            || [
                "table",
                "discount",
                "discount_ends_at",
                "rate_per_explicit_cache_write_token",
                "explicit_cache_granularity_tokens",
            ]
            .iter()
            .any(|key| !p[*key].is_null())
        {
            continue;
        }
        let Some(input) = number(&p["cents_per_input_token"]).map(|n| n * 10_000.0) else {
            continue;
        };
        let Some(output) = number(&p["cents_per_output_token"]).map(|n| n * 10_000.0) else {
            continue;
        };
        let cached = number(&p["rate_per_input_token_cached"]).map(|multiplier| input * multiplier);
        let creation =
            number(&p["rate_per_input_token_cache_write"]).map(|multiplier| CacheCreationPricing {
                default_per_million: Some(input * multiplier),
                ..Default::default()
            });
        let pricing = Pricing {
            input_per_million: Some(input),
            cached_input_per_million: cached,
            output_per_million: Some(output),
            cache_creation: creation,
            tiers: Vec::new(),
            currency: "USD".into(),
            source: SOURCE.into(),
            effective_date: None,
            retrieved_at: Some(now.to_rfc3339()),
        };
        let metadata = ModelMetadata {
            pricing: Some(pricing.clone()),
            ..Default::default()
        };
        if metadata.validate().is_ok() && models.insert(id.to_owned(), pricing).is_some() {
            bail!("Duplicate model ID in public pricing catalog");
        }
    }
    if models.is_empty() {
        bail!("Public catalog has no supported token prices");
    }
    Ok(models)
}

async fn fetch(url: &str) -> Result<BTreeMap<String, Pricing>> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none());
    // Rustls' built-in public roots do not include locally configured CAs.
    // Honor an explicit trust bundle without bypassing certificate validation.
    if let Some(path) = std::env::var_os("SSL_CERT_FILE") {
        let file = std::fs::File::open(path).context("Configured TLS CA bundle unavailable")?;
        let mut pem = Vec::new();
        file.take(2 * 1024 * 1024 + 1).read_to_end(&mut pem)?;
        if pem.len() > 2 * 1024 * 1024 {
            bail!("Configured TLS CA bundle exceeds size limit");
        }
        let certificates = reqwest::Certificate::from_pem_bundle(&pem)
            .context("Configured TLS CA bundle is invalid")?;
        if certificates.is_empty() {
            bail!("Configured TLS CA bundle is empty");
        }
        for cert in certificates {
            builder = builder.add_root_certificate(cert);
        }
    }
    let client = builder.build()?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Public pricing catalog unavailable (network failure)"))?;
    if !response.status().is_success() {
        bail!(
            "Public pricing catalog unavailable (HTTP {})",
            response.status().as_u16()
        );
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("Public pricing catalog body unavailable"))?
    {
        if bytes.len() as u64 + chunk.len() as u64 > LIMIT {
            bail!("Public pricing catalog exceeds size limit");
        }
        bytes.extend_from_slice(&chunk);
    }
    let text = std::str::from_utf8(&bytes).context("Public pricing catalog is not UTF-8")?;
    parse_catalog(text, Utc::now())
}

fn read_cache(path: &Path) -> Result<Catalog> {
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > LIMIT {
        bail!("Pricing cache exceeds size limit");
    }
    let mut raw = String::new();
    file.take(LIMIT + 1).read_to_string(&mut raw)?;
    let c: Catalog = serde_json::from_str(&raw)?;
    if c.source != SOURCE || c.checked_at > Utc::now() + chrono::Duration::minutes(5) {
        bail!("Invalid pricing cache provenance");
    }
    for p in c.models.values() {
        ModelMetadata {
            pricing: Some(p.clone()),
            ..Default::default()
        }
        .validate()?;
        if p.source != SOURCE || p.retrieved_at.is_none() {
            bail!("Invalid cached rate provenance");
        }
    }
    Ok(c)
}

pub async fn catalog(root: &Path, cfg: &ProviderConfig, force: bool) -> Result<Catalog> {
    update_catalog(root, cfg, force, SOURCE).await
}

async fn update_catalog(
    root: &Path,
    cfg: &ProviderConfig,
    force: bool,
    url: &str,
) -> Result<Catalog> {
    if !supported(cfg) {
        bail!("Pricing discovery is unsupported for this provider URL; configure explicit model rates");
    }
    let path = crate::paths::state_directory(root)?.join("deepinfra-pricing.json");
    let prior = read_cache(&path).ok();
    if !force
        && prior.as_ref().is_some_and(|c| {
            Utc::now().signed_duration_since(c.checked_at).num_minutes()
                < if c.note.is_some() { 15 } else { 24 * 60 }
        })
    {
        return Ok(prior.unwrap());
    }
    let c = match fetch(url).await {
        Ok(models) => Catalog {source:SOURCE.into(), checked_at:Utc::now(),models,note:None},
        Err(_) => Catalog {source:SOURCE.into(), checked_at:Utc::now(), models:prior.map(|c|c.models).unwrap_or_default(),
            note:Some("Public pricing refresh failed; retained cached rates if available. Use refresh-pricing to retry; no inference request was made.".into())},
    };
    // A private atomic cache cannot rewrite already recorded request snapshots.
    let saved = (|| -> Result<()> {
        let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
        serde_json::to_writer(temp.as_file_mut(), &c)?;
        temp.flush()?;
        temp.as_file().sync_all()?;
        temp.persist(&path).map_err(|e| e.error)?;
        Ok(())
    })();
    if saved.is_err() {
        return Ok(Catalog {
            note: Some(format!(
                "{} Pricing cache could not be saved; rates apply only to this lookup.",
                c.note.as_deref().unwrap_or("Public catalog retrieved.")
            )),
            ..c
        });
    }
    Ok(c)
}

pub async fn resolve(root: &Path, cfg: &ProviderConfig, model: &str, force: bool) -> ModelMetadata {
    let mut metadata = cfg.model_metadata.get(model).cloned().unwrap_or_default();
    if let Some(p) = &metadata.pricing {
        metadata.pricing_note = Some(format!(
            "Explicit configured pricing overrides discovery, including unknown/null rates. {}",
            p.freshness(Utc::now())
        ));
        return metadata;
    }
    if !cfg.pricing_discovery && !force {
        metadata.pricing_note =
            Some("Automatic pricing discovery is disabled; rates remain unknown.".into());
        return metadata;
    }
    match catalog(root,cfg,force).await {
        Ok(c) => {
            metadata.pricing=c.models.get(model).cloned();
            metadata.pricing_note=c.note.or_else(|| metadata.pricing.is_none().then(|| "No supported public rate for this exact model ID; configure explicit rates. Nonstandard billing is not flattened.".into()));
            if let Some(p)=&metadata.pricing {metadata.pricing_note=Some(format!("{} {}",metadata.pricing_note.as_deref().unwrap_or("Default on-demand public catalog rates."),p.freshness(Utc::now())));}
        }
        Err(_) => metadata.pricing_note=Some("Pricing discovery unavailable for this provider; configure explicit rates. No guessed prices were applied.".into()),
    }
    metadata
}

#[cfg(test)]
mod tests {
    use super::*;
    const FIXTURE: &str = r#"[{"model_name":"fixture/model","type":"text-generation","pricing":{"type":"tokens","cents_per_input_token":0.0002,"cents_per_output_token":0.001,"rate_per_input_token_cached":0.25,"rate_per_input_token_cache_write":1.25}}]"#;
    fn cfg() -> ProviderConfig {
        serde_yaml::from_str("model: fixture/model\n").unwrap()
    }
    fn mock(status: u16, body: &str) -> (String, std::thread::JoinHandle<()>) {
        use std::net::TcpListener;
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/catalog", server.local_addr().unwrap());
        let body = body.to_owned();
        let worker = std::thread::spawn(move || {
            let (mut connection, _) = server.accept().unwrap();
            connection
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 1024];
            while !request.windows(4).any(|v| v == b"\r\n\r\n") {
                let n = connection.read(&mut buffer).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&buffer[..n]);
            }
            let head = String::from_utf8(request).unwrap().to_ascii_lowercase();
            assert!(head.starts_with("get /catalog "));
            assert!(!head.contains("authorization"));
            write!(
                connection,
                "HTTP/1.1 {status} Mock\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        (url, worker)
    }
    #[test]
    fn catalog_units_multipliers_and_unsupported_billing_are_conservative() {
        let p = parse_catalog(FIXTURE, Utc::now())
            .unwrap()
            .remove("fixture/model")
            .unwrap();
        assert_eq!(p.input_per_million, Some(2.0));
        assert_eq!(p.output_per_million, Some(10.0));
        assert_eq!(p.cached_input_per_million, Some(0.5));
        assert_eq!(p.cache_creation.unwrap().default_per_million, Some(2.5));
        assert!(p.effective_date.is_none());
        for (field, value) in [
            ("table", serde_json::json!([[1, 2]])),
            ("discount", serde_json::json!(0.2)),
            ("cents_per_input_token", serde_json::json!(-1)),
            ("cents_per_output_token", serde_json::json!("invalid")),
            ("explicit_cache_granularity_tokens", serde_json::json!(1024)),
        ] {
            let mut data: Value = serde_json::from_str(FIXTURE).unwrap();
            data[0]["pricing"][field] = value;
            assert!(parse_catalog(&data.to_string(), Utc::now()).is_err());
        }
        let v: Value = serde_json::from_str(FIXTURE).unwrap();
        assert!(parse_catalog(&serde_json::json!([v[0], v[0]]).to_string(), Utc::now()).is_err());
        let mut c = cfg();
        c.base_url = "https://api.deepinfra.com.evil/v1/openai".into();
        assert!(!supported(&c));
        c.base_url = "https://api.deepinfra.com/v1/openai?key=secret".into();
        assert!(!supported(&c));
    }
    #[tokio::test]
    async fn cache_refresh_failure_overrides_and_unknown_models_preserve_provenance() {
        let root = tempfile::tempdir().unwrap();
        let c = cfg();
        let (url, worker) = mock(200, FIXTURE);
        let first = update_catalog(root.path(), &c, true, &url).await.unwrap();
        worker.join().unwrap();
        assert_eq!(first.models.len(), 1);
        assert!(first.note.is_none());
        let cached = resolve(root.path(), &c, "fixture/model", false).await;
        assert_eq!(cached.pricing.unwrap().input_per_million, Some(2.0));
        assert!(resolve(root.path(), &c, "unknown/model", false)
            .await
            .pricing
            .is_none());
        // HTTP error bodies are never stored or displayed; old snapshots survive.
        let (url, worker) = mock(503, "fixture-secret not pricing");
        let failed = update_catalog(root.path(), &c, true, &url).await.unwrap();
        worker.join().unwrap();
        assert!(failed.note.as_ref().unwrap().contains("refresh failed"));
        assert_eq!(
            failed.models["fixture/model"].retrieved_at,
            first.models["fixture/model"].retrieved_at
        );
        assert!(!serde_json::to_string(&failed)
            .unwrap()
            .contains("fixture-secret"));
        let mut c = c;
        let mut explicit = ModelMetadata {
            pricing: Some(first.models["fixture/model"].clone()),
            ..Default::default()
        };
        explicit.pricing.as_mut().unwrap().input_per_million = None;
        c.model_metadata.insert("fixture/model".into(), explicit);
        assert!(resolve(root.path(), &c, "fixture/model", true)
            .await
            .pricing
            .unwrap()
            .input_per_million
            .is_none());
        c.model_metadata.clear();
        c.pricing_discovery = false;
        assert!(resolve(root.path(), &c, "fixture/model", false)
            .await
            .pricing
            .is_none());
        let path = crate::paths::state_directory(root.path())
            .unwrap()
            .join("deepinfra-pricing.json");
        std::fs::write(&path, "corrupt").unwrap();
        assert!(read_cache(&path).is_err());
    }
}
