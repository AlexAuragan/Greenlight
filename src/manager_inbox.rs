//! Owner-advertised, durable control inbox. Producers never take execution authority.
use crate::{
    config,
    store::{Session, Store},
};
use anyhow::{bail, Context, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

pub(crate) struct Steering {
    pub token: String,
    pub step: Option<String>,
    pub attempt: Option<String>,
    pub workflow_digest: Option<String>,
    pub accepting: bool,
}
#[derive(Debug)]
pub(crate) struct GuidanceWithdrawn;
impl std::fmt::Display for GuidanceWithdrawn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "Pending guidance was withdrawn while preparing this request; rebuild before dispatch",
        )
    }
}
impl std::error::Error for GuidanceWithdrawn {}
impl Store {
    pub(crate) fn steering(&self, s: &Session) -> Result<Option<Steering>> {
        self.db.query_row("SELECT g.token,g.step,g.attempt,g.workflow_digest,g.accepting FROM execution_steering g JOIN project_owner p ON p.token=g.token JOIN execution_owners e ON e.token=g.token AND e.session=g.session WHERE g.session=?1 AND p.branch=?2 AND g.protocol=1",params![s.id,s.active_branch],|r|Ok(Steering {token:r.get(0)?,step:r.get(1)?,attempt:r.get(2)?,workflow_digest:r.get(3)?,accepting:r.get(4)?})).optional().map_err(Into::into)
    }
    pub(crate) fn advertise_steering(&self) -> Result<()> {
        let lease = self
            .active_project
            .borrow()
            .upgrade()
            .context("No active execution capability")?;
        if lease.record.operation != "workflow" {
            bail!("This operation cannot advertise coding steering");
        }
        self.db.execute(
            "INSERT INTO execution_steering(token,session,protocol,accepting) VALUES(?1,?2,1,0)",
            params![lease.record.token, lease.record.session],
        )?;
        Ok(())
    }
    pub(crate) fn open_guidance_boundary(
        &self,
        s: &Session,
        step: &str,
        wf: &config::LoadedWorkflow,
    ) -> Result<()> {
        let lease = self
            .active_project
            .borrow()
            .upgrade()
            .context("No active execution capability")?;
        self.check_project(&lease, s)?;
        let (attempt, _) = self.active_attempt(s, step)?;
        if self.db.execute("UPDATE execution_steering SET step=?2,attempt=?3,workflow_digest=?4,accepting=1 WHERE token=?1",params![lease.record.token,step,attempt,crate::manager::workflow_digest(wf)?])?!=1 {bail!("Steering ownership changed");}
        Ok(())
    }
    pub(crate) fn close_guidance_boundary(&self, s: &Session, step: &str) -> Result<bool> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.db,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let owner = self
            .steering(s)?
            .context("Steering owner disappeared before step completion")?;
        if owner.step.as_deref() != Some(step) {
            bail!("Steering step changed");
        }
        self.db.execute(
            "UPDATE execution_steering SET accepting=0 WHERE token=?1",
            [&owner.token],
        )?;
        let pending = self.manager_routes(s)?.iter().any(|r| {
            r.state == "queued"
                && r.attempt == owner.attempt
                && r.owner_token.as_deref() == Some(&owner.token)
        });
        if pending {
            self.invalidate_manager_routes(
                s,
                true,
                "Attempt ended without another coding request; instruction was not delivered",
            )?;
            self.event(s,Some(step),"manager.pending-review",&json!({"attempt":owner.attempt,"reason":"Step ended before pending guidance reached a coding request. Automatic progression is held; recorded outcomes remain intact."}).to_string())?;
        }
        tx.commit()?;
        Ok(pending)
    }
    pub(crate) fn hold_success_for_guidance(
        &self,
        s: &mut Session,
        step: &config::Step,
        wf: &config::LoadedWorkflow,
    ) -> Result<()> {
        let tx = self.db.unchecked_transaction()?;
        let (attempt, _) = self.latest_attempt(s, &step.id)?;
        self.event(s,Some(&step.id),"manager.progress-held",&json!({"step":step.id,"attempt":attempt,"next_step":step.transitions.success,"workflow_digest":crate::manager::workflow_digest(wf)?}).to_string())?;
        s.status = "instruction_review".into();
        self.save(s)?;
        tx.commit()?;
        Ok(())
    }
    pub fn continue_after_guidance(&self, root: &std::path::Path, session: &str) -> Result<()> {
        let s = self.session(session)?;
        self.with_project_control(&s,"guidance-review",|lease| {
            self.check_project(lease,&s)?;self.ensure_no_pending_writes()?;
            let current=self.session(session)?;
            if current.active_branch!=s.active_branch || current.current_step!=s.current_step || current.status!=s.status || current.workflow!=s.workflow {bail!("Session changed before guidance review; refresh first");}
            if s.status!="instruction_review" {bail!("No completed-step guidance hold to continue");}
            if self.manager_routes(&s)?.iter().any(|r|r.state=="needs_review") {bail!("Review/restart or withdraw undelivered instructions before continuing");}
            let history=self.history(&s)?;
            let raw=&history.iter().rev().find(|e|e.2=="manager.progress-held" && e.1==s.current_step).context("Recorded guidance hold is unavailable")?.3;
            let hold:Value=serde_json::from_str(raw)?;
            let (attempt, start)=self.latest_attempt(&s,&s.current_step)?;
            if hold["attempt"].as_str()!=Some(&attempt) || !history.iter().any(|e|e.0>start && e.1==s.current_step && e.2=="step.completed") {bail!("Recorded completed attempt no longer matches the guidance hold");}
            let wf=config::load_workflow(root,&s.workflow)?;
            if hold["workflow_digest"].as_str()!=Some(&crate::manager::workflow_digest(&wf)?) {bail!("Workflow changed; inspect a guarded restart instead");}
            let step=wf.steps.get(&s.current_step).context("Held step missing")?;
            if hold["next_step"].as_str()!=step.transitions.success.as_deref() {bail!("Recorded transition changed");}
            let tx=self.db.unchecked_transaction()?;
            let mut next=s.clone();
            if let Some(target)=&step.transitions.success {next.current_step=target.clone();next.status="ready".into();}
            else {next.status="completed".into();self.event(&next,None,"session.completed","Guidance review dismissed; recorded completed outcome retained")?;}
            self.save(&next)?;
            self.event(&next,None,"manager.progress-resumed","User explicitly continued the recorded success transition after guidance review; no step was repeated")?;
            tx.commit()?;
            Ok(())
        })
    }
    pub(crate) fn refresh_guidance_messages(
        &self,
        s: &Session,
        step: &str,
        messages: &mut Vec<Value>,
    ) -> Result<()> {
        let lease = self
            .active_project
            .borrow()
            .upgrade()
            .context("No active execution capability")?;
        self.check_project(&lease, s)?;
        let (attempt, _) = self.active_attempt(s, step)?;
        let routes = self.manager_routes(s)?;
        for route in routes.iter().filter(|r| {
            r.step == step
                && r.attempt.as_deref() == Some(&attempt)
                && r.state == "withdrawn"
                && r.owner_token.as_deref() == Some(&lease.record.token)
        }) {
            if let Some(index) = messages.iter().position(|m| m == &route.metadata(&attempt)) {
                let guidance = route.guidance(&attempt);
                if messages.get(index..index + guidance.len()) != Some(guidance.as_slice()) {
                    bail!("Guidance context mismatch");
                }
                messages.drain(index..index + guidance.len());
            }
        }
        for route in routes
            .into_iter()
            .filter(|r| r.state == "queued" && r.step == step)
        {
            if route.attempt.as_deref() != Some(&attempt)
                || route.owner_token.as_deref() != Some(&lease.record.token)
            {
                bail!("Pending guidance belongs to another owner/attempt");
            }
            if messages.iter().any(|m| m == &route.metadata(&attempt)) {
                continue;
            }
            messages.extend(route.guidance(&attempt));
        }
        Ok(())
    }
}
