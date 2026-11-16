//! Completion is control flow, not evidence that the user's objective was achieved.
use super::*;
pub(super) fn summary(
    store: &Store,
    session: &Session,
    history: &[(i64, String, String, String)],
) -> String {
    let end = history
        .iter()
        .rev()
        .find(|e| e.2 == "session.completed")
        .map(|e| e.0)
        .unwrap_or(i64::MAX);
    let start = history
        .iter()
        .rev()
        .find(|e| e.0 <= end && e.1 == session.current_step && e.2 == "step.started")
        .map(|e| e.0)
        .unwrap_or(0);
    let changed = history
        .iter()
        .filter(|e| e.2 == "file.changed")
        .map(|e| e.3.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut out = format!(
        "Workflow ended · {}\nSource edits: {} file(s) recorded · 3 opens diffs/revert.\n",
        session.workflow,
        changed.len()
    );
    let in_final = |e: &&(i64, String, String, String)| {
        e.0 > start && e.0 <= end && e.1 == session.current_step
    };
    let checks = if history
        .iter()
        .filter(in_final)
        .any(|e| e.2 == "verification.none")
        && !history.iter().filter(in_final).any(|e| {
            matches!(
                e.2.as_str(),
                "verification.command" | "verification.agent" | "verification.human"
            )
        }) {
        "Verification: no checks required (policy accepted, not measured validation)."
    } else if history
        .iter()
        .filter(in_final)
        .any(|e| e.2 == "verification.passed")
    {
        "Verification: configured policy passed; only its checks were validated."
    } else {
        "Verification: no recorded aggregate verdict."
    };
    out.push_str(&format!("{checks}\n"));
    if history
        .iter()
        .any(|e| e.0 > end && e.2 == "workspace.restored")
    {
        out.push_str("Workspace restored after this result: reports/checks describe the historical run, not the restored files.\n");
    }
    if let Some((id, _, _, text)) = history
        .iter()
        .filter(in_final)
        .rev()
        .find(|e| e.2 == "step.completed")
    {
        out.push_str(&format!(
            "\nAgent-reported conclusion · event #{id}:\n{text}\n"
        ));
    }
    out.push_str("\nWorkflow completion alone does not establish that the goal was achieved.\n");
    let required = history
        .iter()
        .rev()
        .find(|e| e.0 <= start && e.1 == session.current_step && e.2 == "step.definition")
        .and_then(|e| serde_yaml::from_str::<crate::config::Step>(&e.3).ok())
        .map(|step| step.outputs.required)
        .unwrap_or_default();
    if let Some((id, _, _, name)) = history
        .iter()
        .filter(in_final)
        .rev()
        .find(|e| e.2 == "artifact.created" && (required.is_empty() || required.contains(&e.3)))
    {
        match store.artifact_version(session,name,*id) {
            Ok(report)=>{
                let excerpt=report.content.chars().take(10000).collect::<String>();
                out.push_str(&format!("\nFinal report · {} · event #{}\nModel-authored assessment; inspect its evidence.\n\n{}{}\n",report.name,report.event_id,excerpt,if excerpt.len()<report.content.len(){"\n[Preview truncated; open full report in 2 Artifacts / version history.]"}else{""}));
            }
            Err(error)=>out.push_str(&format!("\nFinal report · {name} · event #{id}\nRecorded report body unavailable: {error:#}\n")),
        }
    } else {
        out.push_str(
            "\nNo final-step report was registered. Review the step summaries and artifacts.\n",
        );
    }
    out.push_str(&format!(
        "\nGoal: {}\n\nRecorded step summaries (including earlier attempts):\n",
        session.goal
    ));
    for (id, step, kind, text) in history {
        if kind == "step.completed" && *id <= end {
            out.push_str(&format!(
                "\n{step} · event #{id} · agent reported:\n{text}\n"
            ));
        }
    }
    out.push_str("\n2: artifacts · 3: source edits/revert · h/Esc: activity · s: reopen result\n");
    out
}
