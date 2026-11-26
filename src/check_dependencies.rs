//! Pinned artifact broker. Fetch bytes only; all installation runs inside the
//! offline verification sandbox. Cache objects never execute on the host.
use crate::{
    anchored_fs::Dir,
    verification_workspace::{Recipe, Request},
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    net::IpAddr,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

const MAX_FILE: usize = 32 * 1024 * 1024;
const MAX_TOTAL: usize = 128 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub name: String,
    pub url: String,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preparation {
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    /// Executed afresh inside the network-off check copy, not by the broker.
    #[serde(default)]
    pub run: Option<String>,
}
impl Preparation {
    pub fn validate(&self) -> Result<()> {
        if self.artifacts.is_empty() && self.run.is_none() {
            bail!("Dependency preparation needs artifacts or an offline command");
        }
        if self.artifacts.len() > 16 {
            bail!("At most 16 dependency artifacts per recipe");
        }
        if self
            .run
            .as_ref()
            .is_some_and(|s| s.trim().is_empty() || s.len() > 16_384)
        {
            bail!("Offline preparation command must contain 1..16384 bytes");
        }
        let mut names = BTreeSet::new();
        for artifact in &self.artifacts {
            if artifact.name.is_empty()
                || artifact.name.len() > 128
                || artifact.name.starts_with('.')
                || !artifact
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
                || !names.insert(&artifact.name)
            {
                bail!("Dependency filenames must be unique short basenames, never paths/private names");
            }
            if !digest_name(&artifact.sha256) {
                bail!("Dependency SHA-256 must contain 64 lowercase hex digits");
            }
            checked_url(&artifact.url)?;
        }
        Ok(())
    }
    pub fn description(&self) -> String {
        let mut text = String::from("Dependency fetch profile: exact HTTPS URLs, no credentials, redirects or proxies; public addresses only. SHA-256 verified; never executed on host.\n");
        for artifact in &self.artifacts {
            text.push_str(&format!(
                "{}\n  {}\n  sha256: {}\n",
                artifact.name, artifact.url, artifact.sha256
            ));
        }
        text.push_str("Limits: 32 MiB per artifact / 128 MiB total / 60 seconds per fetch. No automatic retries. Checks remain network-off.\n");
        text
    }
}
fn digest_name(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn checked_url(text: &str) -> Result<reqwest::Url> {
    if text.len() > 2048 || text.chars().any(char::is_control) {
        bail!("Dependency URL exceeds 2048 bytes");
    }
    let url = reqwest::Url::parse(text)?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default() != Some(443)
    {
        bail!("Dependencies require public HTTPS port 443 without credentials/query/fragment");
    }
    let host = url
        .host_str()
        .context("Dependency URL requires a hostname")?;
    if host.len() > 253
        || host.starts_with('-')
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
        || host.parse::<IpAddr>().is_ok()
        || !host.contains('.')
        || host.ends_with('.')
        || host.ends_with(".localhost")
        || host.ends_with(".local")
    {
        bail!("Dependency URLs require a public DNS hostname");
    }
    Ok(url)
}
fn public(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_unspecified()
                || ip.is_multicast()
                || a == 0
                || a >= 224
                || a == 100 && (64..=127).contains(&b)
                || a == 192 && (b == 0 && (c == 0 || c == 2) || b == 88 && c == 99)
                || a == 198 && (b == 18 || b == 19 || b == 51 && c == 100)
                || a == 203 && b == 0 && c == 113)
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            s[0] & 0xe000 == 0x2000
                && !(s[0] == 0x2001 && (s[1] < 0x200 || s[1] == 0xdb8))
                && !(s[0] == 0x2002 || s[0] == 0x3ffe || s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn directories(state: &Dir) -> Result<(Dir, Dir)> {
    let base = state.create_dirs(Path::new("check-dependencies"))?;
    Ok((
        base.create_dirs(Path::new("objects"))?,
        base.create_dirs(Path::new("manifests"))?,
    ))
}
fn verified(objects: &Dir, artifact: &Artifact) -> Result<Vec<u8>> {
    let mut file = objects.open_file(Path::new(&format!("{}.bin", artifact.sha256)))?;
    if file.metadata()?.len() > MAX_FILE as u64 {
        bail!("Dependency object exceeds 32 MiB");
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_FILE as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_FILE || hash(&bytes) != artifact.sha256 {
        bail!("Dependency cache integrity mismatch for {}", artifact.name);
    }
    Ok(bytes)
}
#[derive(Serialize, Deserialize)]
struct Manifest {
    fingerprint: String,
    artifacts: Vec<Artifact>,
    bytes: usize,
    prepared_at: String,
}
pub fn stage(state: &Dir, recipe: &Recipe, destination: &Dir, stop: &AtomicBool) -> Result<()> {
    let Some(preparation) = &recipe.preparation else {
        return Ok(());
    };
    if preparation.artifacts.is_empty() {
        return Ok(());
    }
    let (objects, manifests) = directories(state)?;
    let fingerprint = recipe.fingerprint();
    let manifest: Manifest = serde_json::from_str(&manifests.read_string(Path::new(&format!("{fingerprint}.json")), 256 * 1024).context("Dependencies not prepared for this exact recipe. Ask the user to run check-prepare, or request prepare_check when enabled")?)?;
    if manifest.fingerprint != fingerprint || manifest.artifacts != preparation.artifacts {
        bail!("Dependency provenance does not match the current recipe; prepare it explicitly");
    }
    let mut total: usize = 0;
    for artifact in &preparation.artifacts {
        if stop.load(Ordering::SeqCst) {
            bail!("Dependency staging cancelled");
        }
        let bytes = verified(&objects, artifact)?;
        total += bytes.len();
        if total > MAX_TOTAL {
            bail!("Dependencies exceed total 128 MiB");
        }
        // Copy verified bytes into this invocation. No writable shared cache or
        // host pathname can be substituted after integrity validation.
        let mut out = destination.create_new(Path::new(&artifact.name))?;
        out.write_all(&bytes)?;
    }
    Ok(())
}

// DNS results are validated then pinned into this client. Redirects and system
// proxies cannot turn the approved URL into an internal/credentialed request.
async fn fetch(
    artifact: &Artifact,
    stop: &Arc<AtomicBool>,
    limit: usize,
    jobs: &Dir,
) -> Result<Vec<u8>> {
    let started = tokio::time::Instant::now();
    let url = checked_url(&artifact.url)?;
    let host = url.host_str().unwrap();
    // getaddrinfo on Tokio's blocking pool cannot be cancelled. Resolve with a
    // fixed trusted system program under the per-invocation supervisor instead.
    // No project/configured executable, shell, HOME or credentials is involved.
    if !Path::new("/usr/bin/getent").is_file() {
        bail!("Dependency DNS requires /usr/bin/getent with ahosts support; install the system resolver utility. No unvalidated DNS fallback.");
    }
    let mut command = crate::sandbox_job::Job::new("/usr/bin/getent", jobs)?;
    command
        .args(["ahosts", "--", host])
        .env_clear()
        .current_dir("/");
    let resolution = crate::sandbox::pipeline::run(
        vec![command],
        vec!["dependency hostname resolver".into()],
        vec![],
        stop,
        Duration::from_secs(10),
    )
    .await?;
    if !resolution.output.status.success() {
        bail!(
            "Dependency DNS resolver failed: {}\n{}",
            resolution.output.status,
            String::from_utf8_lossy(&resolution.output.stderr)
        );
    }
    let mut addresses = BTreeSet::new();
    for line in String::from_utf8(resolution.output.stdout)?.lines() {
        let address: IpAddr = line
            .split_whitespace()
            .next()
            .context("Malformed dependency resolver output")?
            .parse()?;
        addresses.insert(std::net::SocketAddr::new(address, 443));
    }
    let addresses = addresses.into_iter().collect::<Vec<_>>();
    if addresses.is_empty() || addresses.iter().any(|a| !public(a.ip())) {
        bail!("Dependency DNS resolved to a non-public or unsupported address");
    }
    let remaining = Duration::from_secs(60).saturating_sub(started.elapsed());
    if remaining.is_zero() {
        bail!("Dependency DNS/download deadline exceeded");
    }
    let client = transport(host, &addresses, remaining)?;
    // The resolver has finished and been reaped. Cancelling the asynchronous
    // HTTP phase cannot strand a resolver process or blocking-pool task.
    tokio::select! {
        biased;
        _ = crate::provider_failure::cancelled(stop) => bail!("Dependency fetch cancelled; requests may already have started"),
        result = tokio::time::timeout(remaining, async {
            let response = client.get(url).send().await?;
            receive(response, artifact, stop, limit).await
        }) => result.context("Dependency DNS/download deadline exceeded")?,
    }
}
fn transport(
    host: &str,
    addresses: &[std::net::SocketAddr],
    timeout: Duration,
) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        .timeout(timeout)
        .resolve_to_addrs(host, addresses)
        .build()?)
}
async fn receive(
    mut response: reqwest::Response,
    artifact: &Artifact,
    stop: &Arc<AtomicBool>,
    limit: usize,
) -> Result<Vec<u8>> {
    if stop.load(Ordering::SeqCst) {
        bail!("Dependency fetch cancelled; no new manifest published");
    }
    if !response.status().is_success() {
        let status = response.status();
        let mut body = Vec::new();
        while body.len() < 4096 {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    let keep = (4096 - body.len()).min(chunk.len());
                    body.extend_from_slice(&chunk[..keep]);
                }
                _ => break,
            }
        }
        bail!("Dependency fetch returned HTTP {status}; redirects/authentication are not followed. Body excerpt (max 4096 bytes):\n{}",String::from_utf8_lossy(&body));
    }
    if response.content_length().is_some_and(|n| n > limit as u64) {
        bail!("Dependency download exceeds the remaining byte budget (32 MiB per file / 128 MiB total)");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if stop.load(Ordering::SeqCst) {
            bail!("Dependency fetch cancelled; no preparation manifest published");
        }
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            bail!("Dependency download exceeds the remaining byte budget (32 MiB per file / 128 MiB total)");
        }
        bytes.extend_from_slice(&chunk);
    }
    if hash(&bytes) != artifact.sha256 {
        bail!(
            "SHA-256 mismatch for {}; download was not published",
            artifact.name
        );
    }
    Ok(bytes)
}

pub async fn prepare(
    request: Request<'_>,
    authorize: impl FnOnce() -> Result<()>,
) -> Result<String> {
    let Request {
        store,
        session,
        step,
        name,
        recipe,
        stop,
        ..
    } = request;
    recipe.validate()?;
    let lease = store
        .active_project
        .borrow()
        .upgrade()
        .context("Dependency preparation requires owned project execution")?;
    store.check_project(&lease, session)?;
    let preparation = recipe
        .preparation
        .as_ref()
        .context("No dependency preparation configured for this recipe")?;
    if preparation.artifacts.is_empty() {
        bail!("Recipe has no downloadable artifacts; its configured offline bootstrap runs during check execution");
    }
    if stop.load(Ordering::SeqCst) {
        bail!("Dependency preparation cancelled before authorization");
    }
    authorize()?;
    let fingerprint = recipe.fingerprint();
    store.event(
        session,
        Some(step),
        "dependency.started",
        &format!(
            "Recipe: {name}\nRecipe fingerprint: {fingerprint}\n{}",
            preparation.description()
        ),
    )?;
    let (objects, manifests) = directories(&store.directory_fd)?;
    let mut total: usize = 0;
    for artifact in &preparation.artifacts {
        if stop.load(Ordering::SeqCst) {
            bail!("Dependency preparation cancelled");
        }
        let bytes = match verified(&objects, artifact) {
            Ok(bytes) => bytes,
            Err(_) => {
                store.event(
                    session,
                    Some(step),
                    "dependency.download",
                    &format!(
                        "Fetching {} from {} (pinned SHA-256)",
                        artifact.name, artifact.url
                    ),
                )?;
                let jobs = store.directory_fd.create_dirs(Path::new("sandbox-jobs"))?;
                let bytes = fetch(
                    artifact,
                    stop,
                    MAX_FILE.min(MAX_TOTAL.saturating_sub(total)),
                    &jobs,
                )
                .await?;
                total = total
                    .checked_add(bytes.len())
                    .context("Dependency size overflow")?;
                if total > MAX_TOTAL {
                    bail!("Dependencies exceed 128 MiB; no preparation manifest published");
                }
                // Replacing a corrupt object is safe: checks consume private
                // verified copies, never execute this cache's inodes on host.
                objects
                    .target(Path::new(&format!("{}.bin", artifact.sha256)))?
                    .atomic_write(&bytes)?;
                store.event(
                    session,
                    Some(step),
                    "dependency.artifact",
                    &format!(
                        "{} · {} bytes · sha256 {}",
                        artifact.name,
                        bytes.len(),
                        artifact.sha256
                    ),
                )?;
                continue;
            }
        };
        total += bytes.len();
        if total > MAX_TOTAL {
            bail!("Dependencies exceed 128 MiB");
        }
        store.event(
            session,
            Some(step),
            "dependency.artifact",
            &format!(
                "{} · {} verified cached bytes · sha256 {}",
                artifact.name,
                bytes.len(),
                artifact.sha256
            ),
        )?;
    }
    if stop.load(Ordering::SeqCst) {
        bail!("Dependency preparation cancelled; no new manifest published");
    }
    let manifest = Manifest {
        fingerprint: fingerprint.clone(),
        artifacts: preparation.artifacts.clone(),
        bytes: total,
        prepared_at: chrono::Utc::now().to_rfc3339(),
    };
    manifests
        .target(Path::new(&format!("{fingerprint}.json")))?
        .atomic_write(&serde_json::to_vec(&manifest)?)?;
    let report=format!("Dependency artifacts ready · {name}\nRecipe fingerprint: {fingerprint}\n{} pinned artifact(s), {total} bytes. Artifact bytes verified; no package/project code executed on host. Offline installation will run afresh in a disposable check workspace.\n",preparation.artifacts.len());
    store.event(session, Some(step), "dependency.prepared", &report)?;
    Ok(report)
}

pub fn clear(store: &crate::store::Store, session: &crate::store::Session) -> Result<String> {
    let lease = store
        .active_project
        .borrow()
        .upgrade()
        .context("Dependency cleanup requires owned execution")?;
    store.check_project(&lease, session)?;
    crate::sandbox_job::ensure_clean(&store.directory_fd)?;
    let (objects, manifests) = directories(&store.directory_fd)?;
    let mut selected = Vec::new();
    for (directory, suffix) in [(&manifests, ".json"), (&objects, ".bin")] {
        for entry in directory.entries()? {
            let entry = entry?;
            let name = entry
                .name
                .to_str()
                .context("Invalid dependency cache filename")?;
            if !entry.regular || !name.strip_suffix(suffix).is_some_and(digest_name) {
                bail!("Unexpected dependency cache entry; refusing cleanup");
            }
            selected.push(directory.target(Path::new(name))?);
        }
    }
    let count = selected.len();
    for target in selected {
        target.unlink()?;
    }
    store.event(
        session,
        Some(&session.current_step),
        "dependency.cleared",
        "User cleared prepared dependency manifests and objects; live source unchanged",
    )?;
    Ok(format!("Cleared {count} dependency cache file(s). Recipes need explicit preparation again; source unchanged."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Session, Store};
    fn recipe() -> Recipe {
        Recipe {
            command: "cat /deps/pinned.bin".into(),
            requires: vec![],
            git: false,
            preparation: Some(Preparation {
                artifacts: vec![Artifact {
                    name: "pinned.bin".into(),
                    url: "https://artifacts.example.com/pinned.bin".into(),
                    sha256: hash(b"pinned"),
                }],
                run: Some("cp /deps/pinned.bin /cache/input".into()),
            }),
        }
    }
    fn fixture() -> (tempfile::TempDir, Store, Session) {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let session = store.create("demo", "dependencies", "verify").unwrap();
        (root, store, session)
    }
    fn seed(state: &Dir, recipe: &Recipe) {
        let (objects, _) = directories(state).unwrap();
        let artifact = &recipe.preparation.as_ref().unwrap().artifacts[0];
        objects
            .target(Path::new(&format!("{}.bin", artifact.sha256)))
            .unwrap()
            .atomic_write(b"pinned")
            .unwrap();
    }
    #[test]
    fn validates_closed_artifact_manifest_and_rejects_credentials_or_ambiguous_urls() {
        let mut recipe = recipe();
        recipe.validate().unwrap();
        let original = recipe.fingerprint();
        recipe.preparation.as_mut().unwrap().run = Some("different setup".into());
        assert_ne!(original, recipe.fingerprint());
        for url in [
            "http://artifacts.example.com/a",
            "https://127.0.0.1/a",
            "https://2130706433/a",
            "https://[::1]/a",
            "https://localhost/a",
            "https://x.local/a",
            "https://a.example.com:8443/a",
            "https://user:secret@a.example.com/a",
            "https://a.example.com/a?token=x",
            "https://a.example.com/a#fragment",
            "https://a.example.com./a",
            "https://a.example.com/a\nspoof",
        ] {
            assert!(checked_url(url).is_err(), "{url}");
        }
        let prep = recipe.preparation.as_mut().unwrap();
        let valid = prep.artifacts[0].clone();
        for name in ["../escape", ".env", "a/b", "", ".", "..", "a\nspoof"] {
            prep.artifacts[0].name = name.into();
            assert!(prep.validate().is_err(), "{name}");
        }
        prep.artifacts[0] = valid.clone();
        prep.artifacts.push(valid.clone());
        assert!(prep.validate().is_err());
        prep.artifacts.pop();
        prep.artifacts[0].sha256 = "unverified".into();
        assert!(prep.validate().is_err());
        prep.artifacts[0] = valid;
        prep.run = Some(" ".into());
        assert!(prep.validate().is_err());
    }
    #[test]
    fn public_address_filter_rejects_local_reserved_and_transition_routes() {
        for ip in [
            "127.0.0.1",
            "0.0.0.0",
            "10.0.0.1",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "192.0.2.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "255.255.255.255",
            "::1",
            "::ffff:127.0.0.1",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
            "2002:7f00:1::1",
            "3fff::1",
        ] {
            assert!(!public(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "1.1.1.1",
            "8.8.8.8",
            "192.0.78.13",
            "192.2.1.1",
            "2606:4700:4700::1111",
        ] {
            assert!(public(ip.parse().unwrap()), "{ip}");
        }
    }
    // Only response-consumption tests bypass the public-HTTPS transport policy.
    // Their fixed loopback HTTP server never receives artifact URLs from config.
    async fn response(wire: &[u8]) -> reqwest::Response {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let wire = wire.to_vec();
        let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = server.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = server.accept().await.unwrap();
            let mut request = [0; 4096];
            let _ = socket.read(&mut request).await.unwrap();
            socket.write_all(&wire).await.unwrap();
            let _ = socket.shutdown().await;
        });
        transport("fixture.example.com", &[address], Duration::from_secs(2))
            .unwrap()
            .get(format!(
                "http://fixture.example.com:{}/fixture",
                address.port()
            ))
            .send()
            .await
            .unwrap()
    }
    #[tokio::test]
    async fn response_bytes_require_digest_and_budget_and_preserve_http_error_excerpt() {
        let recipe = recipe();
        let artifact = &recipe.preparation.as_ref().unwrap().artifacts[0];
        let stop = Arc::new(AtomicBool::new(false));
        let valid = b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\npinned";
        assert_eq!(
            receive(response(valid).await, artifact, &stop, 6)
                .await
                .unwrap(),
            b"pinned"
        );
        let error = receive(response(valid).await, artifact, &stop, 5)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("byte budget"));
        let wrong = b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\npoison";
        assert!(receive(response(wrong).await, artifact, &stop, 6)
            .await
            .unwrap_err()
            .to_string()
            .contains("SHA-256"));
        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n6\r\npinned\r\n0\r\n\r\n";
        assert!(receive(response(chunked).await, artifact, &stop, 5)
            .await
            .is_err());
        let redirect = b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1/forbidden\r\nContent-Length: 6\r\nConnection: close\r\n\r\nreason";
        let error = receive(response(redirect).await, artifact, &stop, 6)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("302"));
        assert!(error.to_string().contains("reason"));
        let body = "x".repeat(5000);
        let wire = format!("HTTP/1.1 429 Too Many Requests\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
        let error = receive(response(wire.as_bytes()).await, artifact, &stop, 6)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("429"));
        assert!(error.len() < 4500, "HTTP error body was not bounded");
        stop.store(true, Ordering::SeqCst);
        assert!(receive(response(valid).await, artifact, &stop, 6)
            .await
            .unwrap_err()
            .to_string()
            .contains("cancelled"));
    }
    #[tokio::test]
    async fn preparation_needs_explicit_authority_and_checks_consume_private_verified_copies() {
        let (root, store, session) = fixture();
        let recipe = recipe();
        seed(&store.directory_fd, &recipe);
        let stop = Arc::new(AtomicBool::new(false));
        let owner = store
            .claim_tool_execution(&session.id, &session.active_branch)
            .unwrap();
        let request = || Request {
            root: root.path(),
            store: &store,
            session: &session,
            step: "verify",
            name: "tests",
            recipe: &recipe,
            stop: &stop,
        };
        assert!(prepare(request(), || bail!("declined")).await.is_err());
        let target = tempfile::tempdir().unwrap();
        let target = Dir::open(target.path()).unwrap();
        assert!(stage(&store.directory_fd, &recipe, &target, &stop).is_err());
        let report = prepare(request(), || Ok(())).await.unwrap();
        assert!(report.contains("Recipe fingerprint:"));
        stage(&store.directory_fd, &recipe, &target, &stop).unwrap();
        assert_eq!(target.read(Path::new("pinned.bin"), 32).unwrap(), b"pinned");
        let (objects, _) = directories(&store.directory_fd).unwrap();
        let artifact = &recipe.preparation.as_ref().unwrap().artifacts[0];
        objects
            .target(Path::new(&format!("{}.bin", artifact.sha256)))
            .unwrap()
            .atomic_write(b"poison")
            .unwrap();
        assert_eq!(target.read(Path::new("pinned.bin"), 32).unwrap(), b"pinned");
        let next = tempfile::tempdir().unwrap();
        let next = Dir::open(next.path()).unwrap();
        assert!(stage(&store.directory_fd, &recipe, &next, &stop)
            .unwrap_err()
            .to_string()
            .contains("integrity"));
        let mut changed = recipe.clone();
        changed.command = "different check".into();
        assert!(stage(&store.directory_fd, &changed, &next, &stop).is_err());
        assert!(!store
            .history(&session)
            .unwrap()
            .iter()
            .any(|e| e.2 == "dependency.download"));
        store.finish_execution(&session.id, &owner, None).unwrap();
    }
    #[tokio::test]
    async fn cancellation_does_not_authorize_or_publish_and_cache_clear_preserves_source() {
        let (root, store, session) = fixture();
        let recipe = recipe();
        seed(&store.directory_fd, &recipe);
        std::fs::write(root.path().join("source.txt"), "live").unwrap();
        let stop = Arc::new(AtomicBool::new(true));
        let owner = store
            .claim_tool_execution(&session.id, &session.active_branch)
            .unwrap();
        let request = || Request {
            root: root.path(),
            store: &store,
            session: &session,
            step: "verify",
            name: "tests",
            recipe: &recipe,
            stop: &stop,
        };
        assert!(prepare(request(), || panic!("cancelled request prompted"))
            .await
            .is_err());
        stop.store(false, Ordering::SeqCst);
        prepare(request(), || Ok(())).await.unwrap();
        let observer = Store::open(root.path()).unwrap();
        assert!(clear(&observer, &session).is_err());
        clear(&store, &session).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.path().join("source.txt")).unwrap(),
            "live"
        );
        let destination = tempfile::tempdir().unwrap();
        assert!(stage(
            &store.directory_fd,
            &recipe,
            &Dir::open(destination.path()).unwrap(),
            &stop
        )
        .is_err());
        store.finish_execution(&session.id, &owner, None).unwrap();
    }
    #[test]
    fn cache_cleanup_prevalidates_every_entry_before_removing_anything() {
        let (_root, store, session) = fixture();
        let recipe = recipe();
        seed(&store.directory_fd, &recipe);
        let owner = store
            .claim_tool_execution(&session.id, &session.active_branch)
            .unwrap();
        let (objects, manifests) = directories(&store.directory_fd).unwrap();
        manifests
            .target(Path::new(&format!("{}.json", recipe.fingerprint())))
            .unwrap()
            .atomic_write(b"{}")
            .unwrap();
        objects
            .target(Path::new("unexpected"))
            .unwrap()
            .atomic_write(b"keep")
            .unwrap();
        assert!(clear(&store, &session).is_err());
        assert!(manifests
            .open_file(Path::new(&format!("{}.json", recipe.fingerprint())))
            .is_ok());
        assert_eq!(objects.entries().unwrap().count(), 2);
        store.finish_execution(&session.id, &owner, None).unwrap();
    }
    #[test]
    fn production_supervisor_rejects_arbitrary_host_programs_and_resolver_databases() {
        for args in [
            vec!["/bin/sh", "-c", "true"],
            vec!["/usr/bin/getent", "passwd"],
            vec!["/usr/bin/getent", "ahosts", "--", "bad host.example"],
        ] {
            let output =
                std::process::Command::new(Path::new(env!("OUT_DIR")).join("sandbox-supervisor"))
                    .args(["-1", "-1"])
                    .args(args)
                    .output()
                    .unwrap();
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr)
                .contains("fixed dependency hostname resolver"));
        }
    }
}
