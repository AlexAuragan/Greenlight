//! Capture credentials once; only masked metadata may cross into the UI.
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Mutex, OnceLock},
};

struct Loaded {
    digest: [u8; 32],
    path: String,
}
static DOTENV: OnceLock<Mutex<HashMap<String, Loaded>>> = OnceLock::new();

pub(crate) fn record_dotenv(name: &str, value: &str, path: &Path) {
    DOTENV.get_or_init(Default::default).lock().unwrap().insert(
        name.to_owned(),
        Loaded {
            digest: Sha256::digest(value.as_bytes()).into(),
            path: path.display().to_string(),
        },
    );
}

#[derive(Clone, Debug)]
pub(crate) struct Summary {
    pub name: String,
    pub masked: String,
    pub source: String,
}

#[derive(Clone)]
pub(crate) struct Credential {
    value: Option<String>,
    pub summary: Summary,
}
impl Credential {
    pub fn capture(name: &str) -> Self {
        let raw = std::env::var(name).ok();
        let value = raw
            .as_ref()
            .filter(|key| !key.is_empty() && key.bytes().all(|b| b.is_ascii_graphic()))
            .cloned();
        let source = raw
            .as_ref()
            .and_then(|key| {
                let hash: [u8; 32] = Sha256::digest(key.as_bytes()).into();
                DOTENV
                    .get_or_init(Default::default)
                    .lock()
                    .unwrap()
                    .get(name)
                    .filter(|loaded| loaded.digest == hash)
                    .map(|loaded| format!("{} (loaded into this process)", loaded.path))
            })
            .unwrap_or_else(|| {
                if std::env::var_os(name).is_some() {
                    "Inherited/current process environment (shell/export)".into()
                } else {
                    "Not set in this process".into()
                }
            });
        let masked = match value.as_deref() {
            Some(key) if key.len() > 10 => format!("{}…{}", &key[..5], &key[key.len() - 5..]),
            Some(_) => "[set; short key fully hidden]".into(),
            None if std::env::var_os(name).is_some() => "[empty or invalid key]".into(),
            None => "[not set]".into(),
        };
        Self {
            value,
            summary: Summary {
                name: name.into(),
                masked,
                source,
            },
        }
    }
    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masking_never_displays_a_full_key_or_invalid_bytes() {
        let name = format!("GREENLIGHT_MASK_{}", uuid::Uuid::new_v4().simple());
        for value in [
            "x",
            "1234567890",
            "first-private-middle-last5",
            "invalid key\n",
        ] {
            std::env::set_var(&name, value);
            let credential = Credential::capture(&name);
            let display = format!("{:?}", credential.summary);
            if value.len() > 10 {
                assert!(!display.contains(value));
            } else {
                assert_eq!(credential.summary.masked, "[set; short key fully hidden]");
            }
            if value == "first-private-middle-last5" {
                assert_eq!(credential.summary.masked, "first…last5");
            }
        }
        std::env::remove_var(&name);
        assert_eq!(Credential::capture(&name).summary.masked, "[not set]");
    }

    #[test]
    fn dotenv_provenance_and_precedence_survive_file_edits() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(".greenlight")).unwrap();
        let shell = format!("GREENLIGHT_SHELL_{}", uuid::Uuid::new_v4().simple());
        let file = format!("GREENLIGHT_FILE_{}", uuid::Uuid::new_v4().simple());
        std::env::set_var(&shell, "shell-original-private-key");
        let local = root.path().join(".greenlight/.env");
        std::fs::write(
            &local,
            format!("{shell}=unused-file-secret\n{file}=local-original-private-key\n"),
        )
        .unwrap();
        std::fs::write(
            root.path().join(".env"),
            format!("{file}=unused-root-private-key\n"),
        )
        .unwrap();
        crate::project::load_environment(root.path()).unwrap();
        let inherited = Credential::capture(&shell);
        let captured = Credential::capture(&file);
        assert_eq!(inherited.value(), Some("shell-original-private-key"));
        assert!(inherited.summary.source.contains("process environment"));
        assert_eq!(captured.value(), Some("local-original-private-key"));
        assert!(captured.summary.source.contains(".greenlight/.env"));
        std::fs::write(&local, format!("{file}=local-replacement-private-key\n")).unwrap();
        crate::project::load_environment(root.path()).unwrap();
        assert_eq!(Credential::capture(&file).value(), captured.value());
        std::env::remove_var(&file);
        crate::project::load_environment(root.path()).unwrap();
        assert_eq!(
            Credential::capture(&file).value(),
            Some("local-replacement-private-key")
        );
        assert_eq!(captured.value(), Some("local-original-private-key"));
        std::env::remove_var(&shell);
        std::env::remove_var(&file);
    }
}
