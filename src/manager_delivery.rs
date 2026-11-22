//! Explicit idle delivery of a validated manager proposal. Never live steering.
use crate::{
    config,
    manager::{Decision, Reply, Snapshot},
    store::{Session, Store},
};
use anyhow::{bail, Context, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Route {
    pub message_id: String,
    pub session: String,
    pub branch: String,
    pub step: String,
    pub original: String,
    pub workflow_digest: String,
    pub decision_event: i64,
    pub state: String,
    pub attempt: Option<String>,
    pub call: Option<String>,
}
pub struct AcceptedRoute {
    pub route: Route,
    pub newly_accepted: bool,
    pub start_current: bool,
}
impl Route {
    pub fn display(&self) -> String {
        let mut text=format!("{} → {} · {}\nOriginal message: {}\nBranch: {}\n{}",self.message_id,self.step,self.state,self.original,self.branch,
            match self.state.as_str() {
                "queued" => "Reserved for the next matching attempt on this branch; no request has included it yet.",
                "included_in_request" => "Included in a persisted coding request. This does not prove provider acceptance or completed work.",
                "restart_proposed" => "This target requires restart review. Inspect the existing restart/restore flow; nothing was restarted or delivered.",
                "withdrawn" => "Withdrawn before request inclusion; original evidence retained.",
                _ => "Delivery needs review; no automatic replay. Inspect the original and create a fresh routing decision.",
            });
        text.push_str(&format!(
            "\nAttempt: {}\nFirst prepared call: {}",
            self.attempt.as_deref().unwrap_or("not bound"),
            self.call.as_deref().unwrap_or("not included")
        ));
        crate::markdown::raw(&text)
            .lines
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn metadata(&self, attempt: &str) -> Value {
        json!({"role":"user","content":json!({"manager_guidance":{"message_id":self.message_id,"step":self.step,"attempt":attempt,"note":"The next user message is the original instruction, verbatim. Existing tool permissions, input approvals and verification still apply."}}).to_string()})
    }
}
impl Store {
    pub fn manager_routes(&self, s: &Session) -> Result<Vec<Route>> {
        let (count,bytes,largest):(usize,usize,usize)=self.db.query_row("SELECT count(*),coalesce(sum(octet_length(original)+1024),0),coalesce(max(octet_length(original)),0) FROM manager_routes WHERE session=?1 AND branch=?2",params![s.id,s.active_branch],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        if count > self.limits.history_events || largest > crate::manager::MESSAGE_BYTES {
            bail!("Manager route collection exceeds record/original-message bounds; saved evidence is unchanged");
        }
        crate::resource_limits::Budget::new(self.limits.materialized_bytes).charge(bytes)?;
        let mut query=self.db.prepare("SELECT message_id,session,branch,step,original,workflow_digest,decision_event,state,attempt,call_id FROM manager_routes WHERE session=?1 AND branch=?2 ORDER BY rowid")?;
        let mut result = Vec::new();
        let mut budget = crate::resource_limits::Budget::new(self.limits.materialized_bytes);
        for row in query.query_map(params![s.id, s.active_branch], |r| {
            Ok(Route {
                message_id: r.get(0)?,
                session: r.get(1)?,
                branch: r.get(2)?,
                step: r.get(3)?,
                original: r.get(4)?,
                workflow_digest: r.get(5)?,
                decision_event: r.get(6)?,
                state: r.get(7)?,
                attempt: r.get(8)?,
                call: r.get(9)?,
            })
        })? {
            if result.len() >= self.limits.history_events {
                bail!("Manager route count exceeds limits.history_events");
            }
            let route = row?;
            budget.charge(route.original.len() + 1024)?;
            result.push(route);
        }
        Ok(result)
    }
    fn route_event(&self, route: &Route, reason: &str) -> Result<()> {
        let mut scope = self.session(&route.session)?;
        scope.active_branch = route.branch.clone();
        self.event(&scope,Some(&route.step),"manager.delivery",&json!({"message_id":route.message_id,"step":route.step,"state":route.state,"attempt":route.attempt,"call_id":route.call,"decision_event":route.decision_event,"reason":reason}).to_string())?;
        Ok(())
    }
    /// Accepts a destination once. Repeated clicks cannot start another attempt.
    pub fn accept_manager_route(
        &self,
        root: &Path,
        session: &str,
        decision_event: i64,
    ) -> Result<AcceptedRoute> {
        let s = self.session(session)?;
        self.with_project_control(&s, "manager-route", |lease| {
            self.check_project(lease, &s)?;
            self.ensure_no_pending_writes()?;
            let history = self.history(&s)?;
            let raw = &history.iter()
                .find(|e| e.0 == decision_event && e.2 == "manager.result")
                .context("Manager decision is not visible on this branch")?.3;
            let reply: Reply = serde_json::from_str(raw)?;
            let Decision::Forward { step_id, .. } = &reply.decision else {
                bail!("Only a validated forward proposal can be delivered");
            };
            let snapshot: Snapshot = history.iter().rev()
                .filter(|e| e.0 < decision_event && e.2 == "manager.snapshot")
                .filter_map(|e| serde_json::from_str::<Snapshot>(&e.3).ok())
                .find(|snapshot| snapshot.snapshot_id == reply.decision.snapshot_id())
                .context("Original routing snapshot unavailable")?;
            Decision::parse(&serde_json::to_string(&reply.decision)?, &snapshot)?;
            if snapshot.session.id != s.id || snapshot.session.active_branch != s.active_branch {
                bail!("Routing scope changed; ask for a fresh decision");
            }
            if let Some(existing) = self.manager_routes(&s)?.into_iter()
                .find(|r| r.message_id == reply.decision.message_id()) {
                return Ok(AcceptedRoute { route: existing, newly_accepted: false, start_current: false });
            }
            let wf = config::load_workflow(root, &s.workflow)?;
            if reply.snapshot_changed || crate::manager::control(self, &s, &wf)? != snapshot.control_fingerprint {
                bail!("Routing snapshot is stale; original retained. Ask for a fresh decision");
            }
            if wf.steps.get(step_id).is_none_or(|step| step.kind != "agent") {
                bail!("Destination is no longer an agent step");
            }
            let original = history.iter()
                .filter(|e| e.0 < decision_event && e.2 == "manager.message")
                .find_map(|e| {
                    let value: Value = serde_json::from_str(&e.3).ok()?;
                    if value["message_id"].as_str() != Some(reply.decision.message_id()) { return None; }
                    value["text"].as_str().map(str::to_owned)
                }).context("Saved original message unavailable; no replacement text is accepted")?;
            if original.trim().is_empty() || original.len() > crate::manager::MESSAGE_BYTES {
                bail!("Original message violates the routing byte budget");
            }
            let visited = self.attempts(&s)?.iter().any(|a| a.step == *step_id)
                || history.iter().any(|e| e.1 == *step_id && matches!(e.2.as_str(), "step.started" | "step.completed" | "step.blocked"));
            let current = step_id == &s.current_step && matches!(s.status.as_str(), "ready" | "blocked");
            let state = if current || !visited && s.status != "completed" { "queued" } else { "restart_proposed" };
            let route = Route {
                message_id: reply.decision.message_id().into(), session: s.id.clone(),
                branch: s.active_branch.clone(), step: step_id.clone(), original,
                workflow_digest: crate::manager::workflow_digest(&wf)?, decision_event,
                state: state.into(), attempt: None, call: None,
            };
            let tx = self.db.unchecked_transaction()?;
            self.db.execute("INSERT INTO manager_routes(message_id,session,branch,step,original,workflow_digest,decision_event,state) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![route.message_id,route.session,route.branch,route.step,route.original,route.workflow_digest,route.decision_event,route.state])?;
            self.route_event(&route, "User explicitly accepted the proposed destination; no tools, approvals or transitions were inferred")?;
            tx.commit()?;
            Ok(AcceptedRoute { route, newly_accepted: true, start_current: current && state == "queued" })
        })
    }
    pub fn withdraw_manager_route(&self, session: &str, message: &str) -> Result<()> {
        let s = self.session(session)?;
        self.with_project_control(&s, "manager-route", |_| {
            let mut route = self
                .manager_routes(&s)?
                .into_iter()
                .find(|r| r.message_id == message)
                .context("Route is not on this branch")?;
            if !matches!(
                route.state.as_str(),
                "queued" | "needs_review" | "restart_proposed"
            ) {
                bail!("Route cannot be withdrawn after request inclusion (or twice)");
            }
            let tx = self.db.unchecked_transaction()?;
            route.state = "withdrawn".into();
            self.db.execute(
                "UPDATE manager_routes SET state='withdrawn' WHERE message_id=?1",
                [message],
            )?;
            self.route_event(
                &route,
                "User withdrew the pending instruction; original evidence retained",
            )?;
            tx.commit()?;
            Ok(())
        })
    }
    pub(crate) fn invalidate_manager_routes(
        &self,
        s: &Session,
        bound_only: bool,
        reason: &str,
    ) -> Result<()> {
        for mut route in self
            .manager_routes(s)?
            .into_iter()
            .filter(|r| r.state == "queued" && (!bound_only || r.attempt.is_some()))
        {
            route.state = "needs_review".into();
            self.db.execute(
                "UPDATE manager_routes SET state='needs_review' WHERE message_id=?1",
                [&route.message_id],
            )?;
            self.route_event(&route, reason)?;
        }
        Ok(())
    }
    pub(crate) fn validate_manager_routes(
        &self,
        s: &Session,
        wf: &config::LoadedWorkflow,
    ) -> Result<()> {
        if self
            .manager_routes(s)?
            .iter()
            .any(|r| r.state == "needs_review")
        {
            bail!("Manager instructions need explicit review/withdrawal before a fresh attempt");
        }
        let digest = crate::manager::workflow_digest(wf)?;
        if self
            .manager_routes(s)?
            .iter()
            .any(|r| r.state == "queued" && (r.workflow_digest != digest || r.attempt.is_some()))
        {
            let tx = self.db.unchecked_transaction()?;
            self.invalidate_manager_routes(s,false,"Workflow changed or a prior attempt ended before request inclusion; fresh routing review required")?;
            tx.commit()?;
            bail!("Pending manager instructions need review; inspect delivery records and withdraw or request fresh routing");
        }
        Ok(())
    }
    pub(crate) fn bind_manager_routes(&self, s: &Session, step: &str) -> Result<()> {
        let routes = self
            .manager_routes(s)?
            .into_iter()
            .filter(|r| r.state == "queued" && r.step == step)
            .collect::<Vec<_>>();
        if routes.is_empty() {
            return Ok(());
        }
        let lease = self
            .active_project
            .borrow()
            .upgrade()
            .context("Only the executing project owner can bind manager guidance")?;
        self.check_project(&lease, s)?;
        let (attempt, _) = self.active_attempt(s, step)?;
        let tx = if self.db.is_autocommit() {
            Some(self.db.unchecked_transaction()?)
        } else {
            None
        };
        for mut route in routes {
            if route.attempt.is_some() {
                bail!("Instruction was already bound to an attempt");
            }
            self.db.execute(
                "UPDATE manager_routes SET attempt=?2 WHERE message_id=?1",
                params![route.message_id, attempt],
            )?;
            route.attempt = Some(attempt.clone());
            self.route_event(
                &route,
                "Bound to this attempt; not included until its outgoing request is durable",
            )?;
        }
        if let Some(tx) = tx {
            tx.commit()?;
        }
        Ok(())
    }
    pub(crate) fn append_manager_guidance(
        &self,
        s: &Session,
        step: &str,
        messages: &mut Vec<Value>,
    ) -> Result<()> {
        let (attempt, _) = self.active_attempt(s, step)?;
        for route in self
            .manager_routes(s)?
            .into_iter()
            .filter(|r| r.state == "queued" && r.step == step)
        {
            if route.attempt.as_deref() != Some(&attempt) {
                bail!("Manager instruction is not bound to this attempt");
            }
            messages.push(route.metadata(&attempt));
            messages.push(json!({"role":"user","content":route.original}));
        }
        Ok(())
    }
    /// Called inside the same transaction as the prepared model envelope/ledger.
    pub(crate) fn include_manager_guidance(
        &self,
        s: &Session,
        step: &str,
        call: &str,
        messages: &[Value],
    ) -> Result<()> {
        let routes = self
            .manager_routes(s)?
            .into_iter()
            .filter(|r| r.step == step)
            .collect::<Vec<_>>();
        let referenced = |r: &Route| {
            r.attempt.as_ref().is_some_and(|attempt| {
                messages
                    .iter()
                    .any(|message| message == &r.metadata(attempt))
            })
        };
        if !routes.iter().any(|r| r.state == "queued" || referenced(r)) {
            return Ok(());
        }
        let lease = self
            .active_project
            .borrow()
            .upgrade()
            .context("Only the executing project owner can include manager guidance")?;
        self.check_project(&lease, s)?;
        let (attempt, _) = self.active_attempt(s, step)?;
        for mut route in routes {
            let present = referenced(&route);
            if !present && route.state != "queued" {
                continue;
            }
            let exact = messages.windows(2).any(|pair| {
                pair[0] == route.metadata(&attempt)
                    && pair[1]["role"] == "user"
                    && pair[1]["content"].as_str() == Some(&route.original)
            });
            if !present
                || !exact
                || route.attempt.as_deref() != Some(&attempt)
                || !matches!(
                    route.state.as_str(),
                    "queued" | "needs_review" | "included_in_request"
                )
            {
                bail!("Manager guidance is withdrawn, belongs to another attempt, or is absent from the exact prepared request");
            }
            if route.state == "included_in_request" {
                continue;
            }
            route.state = "included_in_request".into();
            route.call = Some(call.into());
            self.db.execute("UPDATE manager_routes SET state='included_in_request',call_id=?2 WHERE message_id=?1",params![route.message_id,call])?;
            self.route_event(&route,"Included in persisted coding request; provider acceptance and work outcomes remain separate evidence")?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "manager_delivery_tests.rs"]
mod tests;
