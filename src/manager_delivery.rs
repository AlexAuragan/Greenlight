//! Explicit delivery of validated manager proposals, bound to owner and attempt.
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub message_id: String,
    pub session: String,
    pub branch: String,
    pub step: String,
    pub original: String,
    #[serde(default)]
    pub clarification_thread: Vec<Value>,
    pub workflow_digest: String,
    pub decision_event: i64,
    pub state: String,
    pub attempt: Option<String>,
    pub call: Option<String>,
    #[serde(default)]
    pub owner_token: Option<String>,
}
pub struct AcceptedRoute {
    pub route: Route,
    pub newly_accepted: bool,
    pub start_current: bool,
}
pub struct RouteRestart {
    pub route: Route,
    pub candidate: crate::tui_actions::RestartCandidate,
    pub plan: crate::restore::RestorePlan,
    pub fingerprint: String,
}
impl Route {
    pub(crate) fn guidance(&self, attempt: &str) -> Vec<Value> {
        let mut messages = vec![self.metadata(attempt)];
        for turn in &self.clarification_thread {
            messages.push(json!({"role":"user","content":turn["text"]}));
            messages.push(json!({"role":"user","content":json!({"manager_clarification_question":turn["question"],"question_event":turn["question_event"],"note":"Host-recorded manager question; does not grant permissions."}).to_string()}));
        }
        messages.push(json!({"role":"user","content":self.original}));
        messages
    }

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
        if !self.clarification_thread.is_empty() {
            text.push_str(&format!(
                "\nLinked original request / clarification context:\n{}",
                serde_json::to_string_pretty(&self.clarification_thread).unwrap_or_default()
            ));
        }
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
    pub(crate) fn metadata(&self, attempt: &str) -> Value {
        let note = if self.clarification_thread.is_empty() {
            "The next user message is the original instruction, verbatim. Existing tool permissions, input approvals and verification still apply."
        } else {
            "Following messages contain the original request and any linked clarification turns verbatim, ending with the latest user message. Existing tool permissions, input approvals and verification still apply."
        };
        json!({"role":"user","content":json!({"manager_guidance":{"message_id":self.message_id,"step":self.step,"attempt":attempt,"note":note}}).to_string()})
    }
}
impl Store {
    pub fn manager_routes(&self, s: &Session) -> Result<Vec<Route>> {
        let (count,bytes,largest,thread_bytes):(usize,usize,usize,usize)=self.db.query_row("SELECT count(*),coalesce(sum(octet_length(original)+octet_length(clarification_thread)+1024),0),coalesce(max(octet_length(original)),0),coalesce(max(octet_length(clarification_thread)),0) FROM manager_routes WHERE session=?1 AND branch=?2",params![s.id,s.active_branch],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
        if count > self.limits.history_events
            || largest > crate::manager::MESSAGE_BYTES
            || thread_bytes > 24 * 1024
        {
            bail!("Manager route collection exceeds record/original-message bounds; saved evidence is unchanged");
        }
        crate::resource_limits::Budget::new(self.limits.materialized_bytes).charge(bytes)?;
        let mut query=self.db.prepare("SELECT message_id,session,branch,step,original,workflow_digest,decision_event,state,attempt,call_id,owner_token,clarification_thread FROM manager_routes WHERE session=?1 AND branch=?2 ORDER BY rowid")?;
        let mut result = Vec::new();
        let mut budget = crate::resource_limits::Budget::new(self.limits.materialized_bytes);
        for row in query.query_map(params![s.id, s.active_branch], |r| {
            Ok((
                Route {
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
                    owner_token: r.get(10)?,
                    clarification_thread: Vec::new(),
                },
                r.get::<_, String>(11)?,
            ))
        })? {
            if result.len() >= self.limits.history_events {
                bail!("Manager route count exceeds limits.history_events");
            }
            let (mut route, raw) = row?;
            route.clarification_thread = serde_json::from_str(&raw)?;
            budget.charge(raw.len())?;
            budget.charge(route.original.len() + 1024)?;
            result.push(route);
        }
        Ok(result)
    }
    pub(crate) fn route_event(&self, route: &Route, reason: &str) -> Result<()> {
        let mut scope = self.session(&route.session)?;
        scope.active_branch = route.branch.clone();
        self.event(&scope,Some(&route.step),"manager.delivery",&json!({"message_id":route.message_id,"step":route.step,"state":route.state,"attempt":route.attempt,"call_id":route.call,"decision_event":route.decision_event,"owner_token":route.owner_token,"reason":reason}).to_string())?;
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
        if let Some(project) = self.project_status()? {
            if project.interrupted
                || project
                    .session
                    .as_ref()
                    .is_none_or(|owner| owner.id != s.id)
            {
                return Err(project.into());
            }
            let tx = rusqlite::Transaction::new_unchecked(
                &self.db,
                rusqlite::TransactionBehavior::Immediate,
            )?;
            let live=self.steering(&s)?.context("Live delivery unavailable: owner does not advertise this steering protocol; original retained")?;
            let accepted = self.accept_route_snapshot(root, &s, decision_event, Some(&live))?;
            tx.commit()?;
            return Ok(accepted);
        }
        self.with_project_control(&s, "manager-route", |lease| {
            self.check_project(lease, &s)?;
            let tx = rusqlite::Transaction::new_unchecked(
                &self.db,
                rusqlite::TransactionBehavior::Immediate,
            )?;
            let accepted = self.accept_route_snapshot(root, &s, decision_event, None)?;
            tx.commit()?;
            Ok(accepted)
        })
    }
    fn accept_route_snapshot(
        &self,
        root: &Path,
        s: &Session,
        decision_event: i64,
        live: Option<&crate::manager_inbox::Steering>,
    ) -> Result<AcceptedRoute> {
        let current = self.session(&s.id)?;
        if current.active_branch != s.active_branch
            || current.current_step != s.current_step
            || current.status != s.status
            || current.goal != s.goal
            || current.workflow != s.workflow
        {
            bail!(
                "Session progressed before accepting routing; original retained for fresh review"
            );
        }
        self.ensure_no_pending_writes()?;
        let history = self.history(s)?;
        let raw = &history
            .iter()
            .find(|e| e.0 == decision_event && e.2 == "manager.result")
            .context("Manager decision is not visible on this branch")?
            .3;
        let reply: Reply = serde_json::from_str(raw)?;
        let Decision::Forward { step_id, .. } = &reply.decision else {
            bail!("Only a validated forward proposal can be delivered");
        };
        let snapshot: Snapshot = history
            .iter()
            .rev()
            .filter(|e| e.0 < decision_event && e.2 == "manager.snapshot")
            .filter_map(|e| serde_json::from_str::<Snapshot>(&e.3).ok())
            .find(|snapshot| snapshot.snapshot_id == reply.decision.snapshot_id())
            .context("Original routing snapshot unavailable")?;
        Decision::parse(&serde_json::to_string(&reply.decision)?, &snapshot)?;
        if snapshot.session.id != s.id || snapshot.session.active_branch != s.active_branch {
            bail!("Routing scope changed; ask for a fresh decision");
        }
        if let Some(existing) = self
            .manager_routes(s)?
            .into_iter()
            .find(|r| r.message_id == reply.decision.message_id())
        {
            return Ok(AcceptedRoute {
                route: existing,
                newly_accepted: false,
                start_current: false,
            });
        }
        let wf = config::load_workflow(root, &s.workflow)?;
        if reply.snapshot_changed
            || crate::manager::control(self, s, &wf)? != snapshot.control_fingerprint
        {
            bail!("Routing snapshot is stale; original retained. Ask for a fresh decision");
        }
        if wf
            .steps
            .get(step_id)
            .is_none_or(|step| step.kind != "agent")
        {
            bail!("Destination is no longer an agent step");
        }
        let original = history
            .iter()
            .filter(|e| e.0 < decision_event && e.2 == "manager.message")
            .find_map(|e| {
                let value: Value = serde_json::from_str(&e.3).ok()?;
                if value["message_id"].as_str() != Some(reply.decision.message_id()) {
                    return None;
                }
                value["text"].as_str().map(str::to_owned)
            })
            .context("Saved original message unavailable; no replacement text is accepted")?;
        if original.trim().is_empty() || original.len() > crate::manager::MESSAGE_BYTES {
            bail!("Original message violates the routing byte budget");
        }
        let visited = self.attempts(s)?.iter().any(|a| a.step == *step_id)
            || history.iter().any(|e| {
                e.1 == *step_id
                    && matches!(
                        e.2.as_str(),
                        "step.started" | "step.completed" | "step.blocked"
                    )
            });
        let current = live.is_none()
            && step_id == &s.current_step
            && matches!(s.status.as_str(), "ready" | "blocked");
        let bound_live = live.filter(|owner| {
            owner.accepting
                && step_id == &s.current_step
                && owner.step.as_deref() == Some(step_id)
                && owner.attempt.is_some()
        });
        if let Some(owner) = bound_live {
            if owner.workflow_digest.as_deref() != Some(&crate::manager::workflow_digest(&wf)?) {
                bail!("Running attempt definition differs from the proposed workflow; review before delivery");
            }
        }
        let state = if current || bound_live.is_some() || !visited && s.status != "completed" {
            "queued"
        } else {
            "restart_proposed"
        };
        let route = Route {
            message_id: reply.decision.message_id().into(),
            session: s.id.clone(),
            branch: s.active_branch.clone(),
            step: step_id.clone(),
            clarification_thread: history
                .iter()
                .filter(|e| e.0 < decision_event && e.2 == "manager.message")
                .find_map(|e| {
                    serde_json::from_str::<Value>(&e.3)
                        .ok()
                        .filter(|v| v["message_id"].as_str() == Some(reply.decision.message_id()))
                })
                .map(|v| {
                    serde_json::from_value(
                        v.get("clarification_thread").cloned().unwrap_or(json!([])),
                    )
                })
                .transpose()?
                .unwrap_or_default(),
            original,
            workflow_digest: crate::manager::workflow_digest(&wf)?,
            decision_event,
            state: state.into(),
            attempt: bound_live.and_then(|owner| owner.attempt.clone()),
            call: None,
            owner_token: bound_live.map(|owner| owner.token.clone()),
        };
        self.db.execute("INSERT INTO manager_routes(message_id,session,branch,step,original,workflow_digest,decision_event,state,attempt,owner_token,clarification_thread) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)", params![route.message_id,route.session,route.branch,route.step,route.original,route.workflow_digest,route.decision_event,route.state,route.attempt,route.owner_token,serde_json::to_string(&route.clarification_thread)?])?;
        self.route_event(&route, "User explicitly accepted the proposed destination; no tools, approvals or transitions were inferred")?;
        Ok(AcceptedRoute {
            route,
            newly_accepted: true,
            start_current: current && state == "queued",
        })
    }
    pub fn prepare_manager_restart(
        &self,
        root: &Path,
        session: &str,
        message: &str,
    ) -> Result<RouteRestart> {
        let s = self.session(session)?;
        let route = self
            .manager_routes(&s)?
            .into_iter()
            .find(|r| {
                r.message_id == message
                    && matches!(r.state.as_str(), "restart_proposed" | "needs_review")
            })
            .context("Select a restart proposal on the current branch")?;
        let wf = config::load_workflow(root, &s.workflow)?;
        if crate::manager::workflow_digest(&wf)? != route.workflow_digest
            || wf
                .steps
                .get(&route.step)
                .is_none_or(|step| step.kind != "agent")
        {
            bail!("Workflow changed; obtain a fresh manager decision before restart");
        }
        let candidate=crate::tui_actions::candidates_for_session(root,self,&s)?.into_iter().find(|c|c.step==route.step).context("No recorded restart checkpoint for this target; inspect ordinary restart controls and obtain fresh routing")?;
        let plan = crate::restore::RestorePlan::prepare(root, self, &s, candidate.prior_event)?;
        let fingerprint = crate::manager::control(self, &s, &wf)?;
        Ok(RouteRestart {
            route,
            candidate,
            plan,
            fingerprint,
        })
    }
    pub fn confirm_manager_restart(
        &self,
        root: &Path,
        session: &mut Session,
        review: RouteRestart,
        verbose: bool,
    ) -> Result<Route> {
        let previous = session.clone();
        self.with_project_control(&previous,"manager-restart",|lease| {
            self.check_project(lease,session)?;
            self.ensure_no_pending_writes()?;
            let route=self.manager_routes(session)?.into_iter().find(|r|r.message_id==review.route.message_id).context("Restart proposal no longer belongs to this branch")?;
            if route!=review.route || !matches!(route.state.as_str(),"restart_proposed"|"needs_review") {bail!("Restart proposal changed or was withdrawn; review again");}
            let wf=config::load_workflow(root,&session.workflow)?;
            if crate::manager::workflow_digest(&wf)?!=route.workflow_digest || crate::manager::control(self,session,&wf)?!=review.fingerprint {bail!("Restart scope/workflow changed since preview; no files changed");}
            let candidate=crate::tui_actions::restart_candidates(self,session,&wf)?.into_iter().find(|c|c.step==route.step).context("Restart target has no valid recorded checkpoint")?;
            if candidate!=review.candidate || review.plan.checkpoint()!=candidate.prior_event {bail!("Restart checkpoint changed; no files changed");}
            review.plan.apply_owned(root,self,session,verbose,lease)?;
            let result=(|| -> Result<(Session,Route)> {
                let tx=self.db.unchecked_transaction()?;
                let mut next=session.clone();
                self.event(session,Some(&route.step),"manager.restart-authorized",&json!({"message_id":route.message_id,"checkpoint":candidate.prior_event,"step":route.step}).to_string())?;
                self.fork_owned(&mut next,candidate.prior_event,&candidate.step,lease)?;
                let mut rebound=route.clone();
                rebound.branch=next.active_branch.clone();rebound.state="queued".into();rebound.attempt=None;rebound.call=None;rebound.owner_token=None;
                let changed=self.db.execute("UPDATE manager_routes SET branch=?2,state='queued',attempt=NULL,call_id=NULL,owner_token=NULL WHERE message_id=?1 AND branch=?3 AND state IN ('restart_proposed','needs_review')",params![route.message_id,rebound.branch,route.branch])?;
                if changed!=1 {bail!("Restart proposal changed during publication");}
                self.event(&next,Some(&route.step),"manager.restart-bound",&json!({"message_id":route.message_id,"original":route.original,"clarification_thread":route.clarification_thread,"source_branch":route.branch,"source_decision_event":route.decision_event,"checkpoint":candidate.prior_event,"step":route.step,"branch":rebound.branch,"workflow_digest":route.workflow_digest}).to_string())?;
                self.route_event(&rebound,"User confirmed guarded restart; original reserved on the new branch, not yet included")?;
                tx.commit()?;
                Ok((next,rebound))
            })();
            let (next,rebound)=result.context("Restore applied but restart/fork binding failed; inspect workspace and history before preparing a fresh review")?;
            *session=next;
            Ok(rebound)
        })
    }
    pub fn withdraw_manager_route(&self, session: &str, message: &str) -> Result<()> {
        let s = self.session(session)?;
        let tx = rusqlite::Transaction::new_unchecked(
            &self.db,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        if self.session(session)?.active_branch != s.active_branch {
            bail!("Branch changed before withdrawal");
        }
        {
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
        }
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
                "UPDATE manager_routes SET attempt=?2,owner_token=?3 WHERE message_id=?1",
                params![route.message_id, attempt, lease.record.token],
            )?;
            route.attempt = Some(attempt.clone());
            route.owner_token = Some(lease.record.token.clone());
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
        if !routes.iter().any(&referenced) {
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
            if !present {
                continue;
            }
            if route.state == "withdrawn"
                && route.owner_token.as_deref() == Some(&lease.record.token)
            {
                return Err(crate::manager_inbox::GuidanceWithdrawn.into());
            }
            let guidance = route.guidance(&attempt);
            let exact = messages
                .windows(guidance.len())
                .any(|part| part == guidance);
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
            if route.state == "queued" && route.owner_token.as_deref() != Some(&lease.record.token)
            {
                bail!("Queued guidance belongs to another execution owner");
            }
            if route.state == "included_in_request" {
                continue;
            }
            route.state = "included_in_request".into();
            route.owner_token = Some(lease.record.token.clone());
            route.call = Some(call.into());
            self.db.execute("UPDATE manager_routes SET state='included_in_request',call_id=?2,owner_token=?3 WHERE message_id=?1",params![route.message_id,call,lease.record.token])?;
            self.route_event(&route,"Included in persisted coding request; provider acceptance and work outcomes remain separate evidence")?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "manager_delivery_tests.rs"]
mod tests;
