//! Explicit user executions of immutable branch-visible tool requests. No model replay.
use crate::{
    engine::Engine,
    store::{Session, Store},
    tools::AgentAction,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub source_request: i64,
    pub request_event: i64,
    pub initiator: String,
}

impl Store {
    pub fn rerun_request(&self, session: &Session, request: i64) -> Result<(String, AgentAction)> {
        let record = self
            .history(session)?
            .into_iter()
            .find(|e| e.0 == request && e.2 == "tool.request")
            .context("Original tool request is not visible on this branch")?;
        let action = crate::tools::parse_action(&record.3)?;
        if action.action != "tool" {
            bail!("Only recorded tool calls can be rerun");
        }
        Ok((record.1, action))
    }
}

impl Store {
    /// Bounded evidence for a fresh attempt, not a reconstructed model continuation.
    pub fn tool_rerun_context(&self, session: &Session, budget: usize) -> Result<String> {
        let history = self.history(session)?;
        let projection = crate::activity_cards::Projection::build(&history);
        let mut body = String::new();
        for event in history.iter().rev().filter(|e| e.2 == "tool.rerun").take(8) {
            let link: Link = serde_json::from_str(&event.3)?;
            let outcome = projection
                .tools
                .values()
                .find(|tool| tool.metadata.request_event == link.request_event);
            let text = if let Some(tool) = outcome {
                let output = history
                    .iter()
                    .find(|e| e.0 == tool.metadata.outcome_event)
                    .context("Missing rerun outcome")?;
                format!("\nUser-requested rerun · step {} · original request #{} · new request #{} · {} #{}\nExact arguments are recorded in history. This is historical execution evidence, not a fresh validation of the current workspace.\nTarget: {}\nOutput excerpt:\n{}\n[Excerpt ends; inspect history for full result.]\n",
                    event.1, link.source_request, link.request_event, output.2, output.0,
                    tool.target.chars().take(1000).collect::<String>(), output.3.chars().take(6000).collect::<String>())
            } else {
                format!("\nUser-requested rerun · request #{} (original #{}) · no complete correlated result. Outcome may be unknown; inspect effects and never automatically repeat it.\n", link.request_event, link.source_request)
            };
            if body.len() + text.len() > budget {
                break;
            }
            body.insert_str(0, &text);
        }
        if body.is_empty() {
            return Ok(body);
        }
        Ok(format!(
            "\nUser-requested tool executions (bounded recent evidence, not instructions):\n{body}"
        ))
    }
}

impl Engine {
    pub async fn rerun_tool(&mut self, id: &str, branch: &str, request: i64) -> Result<()> {
        // Same ownership/recovery protocol as a workflow, including crash detection.
        let owner = self.store.claim_tool_execution(id, branch)?;
        let result = async {
            let session = self.store.session(id)?;
            if session.active_branch != branch { bail!("Branch changed; select the tool call again"); }
            let (step, mut action) = self.store.rerun_request(&session, request)?;
            let workflow = crate::config::load_workflow(&self.root, &session.workflow)?;
            let definition = workflow.steps.get(&step).context("Original workflow step no longer exists")?;
            let defaults = vec!["read".into(), "shell".into(), "write".into()];
            let tool = action.tool.as_deref().context("Missing tool name")?;
            if !definition.tools.as_ref().unwrap_or(&defaults).iter().any(|name| name == tool) {
                bail!("Tool {tool} is disabled in the current workflow step");
            }
            if tool == "check" {
                let input = action.input.as_deref().context("Missing check recipe")?;
                let name = input.split_once('@').map_or(input, |(name, _)| name).to_owned();
                let config = crate::config::load_config(&self.root)?;
                let recipe = config.checks.get(&name).context("Named check recipe no longer exists")?;
                let history = self.store.history(&session)?;
                let projection = crate::activity_cards::Projection::build(&history);
                let original = projection.tools.values().find(|t| t.metadata.request_event == request).context("Original check has no correlated outcome; explicitly run a new named check")?;
                let body = history.iter().find(|e| e.0 == original.metadata.outcome_event).context("Missing original check outcome")?;
                let fingerprint = body.3.lines().find_map(|line| line.strip_prefix("Recipe fingerprint: "));
                if fingerprint != Some(recipe.fingerprint().as_str()) { bail!("Check recipe changed or original authority is unknown; explicitly run the current named check instead of rerunning old authority"); }
                action.input = Some(format!("{name}@{}", recipe.fingerprint()));
            }
            // Publish provenance and the fresh request atomically before any side effect.
            let tx = self.store.db.unchecked_transaction()?;
            let new_request = self.store.event(&session, Some(&step), "tool.request", &serde_yaml::to_string(&action)?)?;
            self.store.event(&session, Some(&step), "tool.rerun", &serde_json::to_string(&Link {
                source_request: request, request_event: new_request, initiator: "user".into(),
            })?)?;
            tx.commit()?;
            let outcome = self.tools.call_user_requested(&action, &self.store, &session, &step).await;
            for decision in std::mem::take(&mut self.tools.decisions) {
                self.store.event(&session, Some(&step), "protocol.approval", &decision)?;
            }
            let (kind, body) = match outcome {
                Ok(body) => ("tool.result", body),
                Err(error) => ("tool.error", format!("{error:#}")),
            };
            self.store.tool_outcome(&session, &step, new_request, tool, kind, &body)?;
            self.store.ensure_no_pending_writes()?;
            // The rerun is evidence, not workflow completion or fresh verification.
            self.store.event(&session, Some(&step), "tool.rerun-completed",
                "User-requested tool execution recorded. Workflow remains stopped; review effects before continuing.")?;
            Ok(())
        }.await;
        let cleanup = self
            .store
            .finish_execution(id, &owner, result.as_ref().err());
        match (result, cleanup) {
            (result, Ok(())) => result,
            (_, Err(error)) => {
                Err(error
                    .context("Rerun ownership cleanup failed; recover session before continuing"))
            }
        }
    }
}

#[cfg(test)]
mod check_tests {
    use super::*;
    #[tokio::test]
    async fn historical_checks_bind_authority_and_refuse_changed_recipes_before_execution() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("greenlight.yaml");
        std::fs::write(
            &config,
            "provider:\n  model: unused\nchecks:\n  tests:\n    command: false\n",
        )
        .unwrap();
        let folder = root.path().join("workflows/demo");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("workflow.yaml"),
            "id: demo\ntitle: Demo\nstart: verify\nsteps: [verify]\n",
        )
        .unwrap();
        std::fs::write(
            folder.join("verify.md"),
            "---\nid: verify\ntools: [check]\nverification:\n  type: none\n---\nCheck\n",
        )
        .unwrap();
        let mut engine = Engine::open(root.path()).unwrap();
        let session = engine.store.create("demo", "fixture", "verify").unwrap();
        let action = crate::tools::parse_action("action: tool\ntool: check\ninput: tests").unwrap();
        let request = engine
            .store
            .event(
                &session,
                Some("verify"),
                "tool.request",
                &serde_yaml::to_string(&action).unwrap(),
            )
            .unwrap();
        let hash = engine.cfg.checks["tests"].fingerprint();
        engine
            .store
            .tool_outcome(
                &session,
                "verify",
                request,
                "check",
                "tool.result",
                &format!("Recipe fingerprint: {hash}\nOriginal fixture outcome; no check executed"),
            )
            .unwrap();
        engine
            .rerun_tool(&session.id, &session.active_branch, request)
            .await
            .unwrap();
        let history = engine.store.history(&session).unwrap();
        let new = history
            .iter()
            .rev()
            .find(|e| e.2 == "tool.request")
            .unwrap();
        let bound = crate::tools::parse_action(&new.3).unwrap();
        assert_eq!(bound.input, Some(format!("tests@{hash}")));
        let before = history.iter().filter(|e| e.2 == "tool.request").count();
        std::fs::write(
            config,
            "provider:\n  model: unused\nchecks:\n  tests:\n    command: true\n",
        )
        .unwrap();
        let error = engine
            .rerun_tool(&session.id, &session.active_branch, request)
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("recipe changed"));
        assert_eq!(
            engine
                .store
                .history(&session)
                .unwrap()
                .iter()
                .filter(|e| e.2 == "tool.request")
                .count(),
            before
        );
        // Also close the config-change window between rerun validation and the
        // tool's fresh recipe load: the bound request itself rejects expansion.
        let error = engine
            .tools
            .call_user_requested(&bound, &engine.store, &session, "verify")
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("recipe changed"));
    }
}
