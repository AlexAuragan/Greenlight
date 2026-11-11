use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, io::{self, Write}, path::{Path, PathBuf}, time::Duration};
use tokio::process::Command;
use crate::config::Permissions;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentAction {
    pub action: String, // tool | artifact | finish | deviation
    pub tool: Option<String>,
    pub input: Option<String>,
    pub path: Option<String>,
    pub content: Option<String>,
    pub name: Option<String>,
    pub summary: Option<String>,
    pub deviation_id: Option<String>,
}

pub fn parse_action(text:&str)->Result<AgentAction> {
    let trimmed=text.trim();
    let yaml=if trimmed.starts_with("```yaml") || trimmed.starts_with("```yml") {
        let start=trimmed.find('\n').context("malformed YAML fence")?+1;
        let end=trimmed[start..].find("```").context("missing YAML fence end")?+start;
        &trimmed[start..end]
    } else { trimmed };
    let action:AgentAction=serde_yaml::from_str(yaml).context("model output must be a single YAML action")?;
    if !["tool","artifact","finish","deviation"].contains(&action.action.as_str()) { bail!("unknown action {}",action.action); }
    Ok(action)
}

pub struct NativeEdit { pub path: String, pub before: Option<String>, pub after: String }

pub struct Tools { pub root:PathBuf, pub policies:Permissions, approved:HashSet<String>, pub decisions:Vec<String>, pub client:reqwest::Client, pub pending_edit: Option<NativeEdit> }
impl Tools {
    pub fn new(root:PathBuf,policies:Permissions)->Self { Self {root,policies,approved:HashSet::new(),decisions:Vec::new(),client:reqwest::Client::new(),pending_edit:None} }
    pub fn permission(&mut self,kind:&str,detail:&str)->Result<()> {
        let policy=match kind {"shell"=>&self.policies.shell,"read"=>&self.policies.read,"write"=>&self.policies.write,"web"=>&self.policies.web,_=>bail!("unknown tool")}.clone();
        if policy=="deny" {
            self.decisions.push(format!("{kind}: denied by configuration"));
            bail!("{kind} forbidden by policy");
        }
        if policy=="allow" || self.approved.contains(kind) {
            self.decisions.push(format!("{kind}: authorized by {}",if policy=="allow"{"configuration"}else{"process approval"}));
            return Ok(());
        }
        println!("\n[approval required: {kind}]\n{detail}\n[y] once / [a] for this process / [n] deny (default)");
        print!("> ");io::stdout().flush()?;
        let mut answer=String::new();io::stdin().read_line(&mut answer)?;
        match answer.trim().to_lowercase().as_str() {
            "y"=>{self.decisions.push(format!("{kind}: approved once — {detail}"));Ok(())},
            "a"=>{self.approved.insert(kind.into());self.decisions.push(format!("{kind}: approved for process — {detail}"));Ok(())},
            _=>{self.decisions.push(format!("{kind}: denied by user — {detail}"));bail!("user denied {kind}")},
        }
    }
    fn safe_path(&self,relative:&str)->Result<PathBuf> {
        let p=Path::new(relative);
        if p.is_absolute() || p.components().any(|c|!matches!(c,std::path::Component::Normal(_))) {bail!("only paths beneath project root are allowed");}
        let sensitive=[".git", ".env", ".ssh"];
        if p.components().any(|c|sensitive.contains(&c.as_os_str().to_string_lossy().as_ref()))
            || relative.starts_with(".stepwright/state") {
            bail!("agent file access to sensitive internal path is denied");
        }
        let root=self.root.canonicalize()?;
        let candidate=root.join(p);
        if candidate.exists() {
            let resolved=candidate.canonicalize()?;
            if !resolved.starts_with(&root) {bail!("path escapes project root via symlink");}
            Ok(resolved)
        } else {
            let parent=candidate.parent().context("invalid file path")?.canonicalize().context("parent directory must exist")?;
            if !parent.starts_with(&root) {bail!("parent escapes project root");}
            Ok(candidate)
        }
    }
    pub async fn shell(&mut self,command:&str)->Result<String> {
        self.permission("shell",command)?;
        let mut process=Command::new("sh");
        process.arg("-c").arg(command).current_dir(&self.root).kill_on_drop(true);
        let output=tokio::time::timeout(Duration::from_secs(120),process.output()).await
            .context("command timed out after 120s")??;
        let stdout=String::from_utf8_lossy(&output.stdout);
        let stderr=String::from_utf8_lossy(&output.stderr);
        let mut result=format!("exit_code: {}\nstdout:\n{}\nstderr:\n{}",output.status.code().unwrap_or(-1),stdout,stderr);
        if result.len()>24_000 {
            let cut=result.char_indices().take_while(|(i,_)|*i<=24_000).last().map(|(i,_)|i).unwrap_or(0);
            result.truncate(cut);
            result.push_str("\n[output truncated by harness]");
        }
        Ok(result)
    }
    pub async fn call(&mut self,action:&AgentAction)->Result<String> {
        let tool=action.tool.as_deref().context("missing tool")?;
        match tool {
            "shell"=>self.shell(action.input.as_deref().context("shell input is required")?).await,
            "read"=>{
                let path=action.path.as_deref().context("read path required")?;
                self.permission("read",path)?;
                let file=self.safe_path(path)?;
                let bytes=tokio::fs::read(file).await?;
                if bytes.len()>100_000 {bail!("file exceeds read limit (100KB)");}
                Ok(String::from_utf8(bytes).context("read only supports UTF-8 files")?)
            },
            "write"=>{
                let path=action.path.as_deref().context("write path required")?;
                let content=action.content.as_deref().context("write content required")?;
                let file = self.safe_path(path)?;
                if content.len() > 100_000 { bail!("write exceeds 100KB limit"); }
                let before = match tokio::fs::read(&file).await {
                    Ok(bytes) => Some(String::from_utf8(bytes).context("write journaling requires UTF-8 original file")?),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                    Err(e) => return Err(e.into()),
                };
                if before.as_deref() == Some(content) {
                    return Ok(format!("unchanged {path}"));
                }
                let preview = crate::changes::unified(path, before.as_deref(), content);
                println!("\n[proposed write: {path}]\n{preview}");
                self.permission("write",&format!("path: {path}\nbytes: {}",content.len()))?;
                // Recheck to avoid overwriting changes made while approval was pending.
                let current = match tokio::fs::read(&file).await {
                    Ok(bytes) => Some(String::from_utf8(bytes).context("file changed to non-UTF-8")?),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                    Err(e) => return Err(e.into()),
                };
                if current != before { bail!("file changed during write approval; edit not applied"); }
                tokio::fs::write(&file, content).await?;
                self.pending_edit = Some(NativeEdit { path: path.to_owned(), before, after: content.to_owned() });
                Ok(format!("wrote {path} ({} bytes)",content.len()))
            },
            "web"=>{
                let query=action.input.as_deref().context("web query required")?;
                self.permission("web",query)?;
                let key=std::env::var("BRAVE_SEARCH_API_KEY").context("set BRAVE_SEARCH_API_KEY for web search")?;
                let response=self.client.get("https://api.search.brave.com/res/v1/web/search")
                    .header("X-Subscription-Token",key).query(&[("q",query),("count","5")]).send().await?;
                let status=response.status();let body=response.text().await?;
                if !status.is_success() {bail!("search HTTP {status}: {body}");}
                let value:serde_json::Value=serde_json::from_str(&body)?;
                let results=value["web"]["results"].as_array().context("web results missing")?;
                let text=results.iter().map(|item|format!("{}\n{}\n{}",item["title"].as_str().unwrap_or(""),item["url"].as_str().unwrap_or(""),item["description"].as_str().unwrap_or(""))).collect::<Vec<_>>().join("\n\n");
                Ok(text)
            },
            _=>bail!("unknown tool {tool}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_yaml_action() {
        let a=parse_action("```yaml\naction: finish\nsummary: Done\n```").unwrap();
        assert_eq!(a.summary.as_deref(),Some("Done"));
    }
    #[test]
    fn reject_invalid_action() { assert!(parse_action("action: break_everything").is_err()); }
    #[test]
    fn reject_path_traversal() {
        let d=tempfile::tempdir().unwrap();
        let tools=Tools::new(d.path().to_path_buf(),Permissions::default());
        assert!(tools.safe_path("../secret").is_err());
    }
}
