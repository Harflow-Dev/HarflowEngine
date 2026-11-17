//! Task projection construction shared by the original writer's T2 and T5.
use super::Error;
use hf_application::{
    task_lifecycle::InitialProjectionFacts, task_runtime::TerminalProjectionFacts,
};
use hf_domain::process::ProcessIdentity;
use hf_interfaces::task_runtime::{
    InitialLaunchMetadata, spawn_projection, terminal_projection_with_summary,
};
use hf_storage::{TaskCreate, TaskFacts, TaskProcess};
pub fn create(
    m: &InitialLaunchMetadata,
    prompt: String,
    child: &ProcessIdentity,
    started_at: &str,
) -> Result<TaskCreate, Error> {
    let initial = InitialProjectionFacts {
        task: &m.task,
        backend: "claude",
        repo_path: &m.repo,
        session_id: m.session.as_str(),
        status: "running",
        started_at,
        title: m.title.as_deref(),
        model: m.model.as_deref(),
        continuation_of: None,
        chain_root: None,
    };
    let (header, status) = spawn_projection(&initial).map_err(|_| Error::Refused)?;
    let facts = TaskFacts {
        id: m.task.clone(),
        backend: "claude".into(),
        repo_path: m.repo.clone(),
        session_id: m.session.as_str().into(),
        status: "running".into(),
        exit_code: None,
        started_at: started_at.into(),
        started_at_us: hf_domain::timestamp::parse_micros(started_at).ok_or(Error::Refused)?,
        finished_at: None,
        finished_at_us: None,
        output_drained: false,
        title: m.title.clone(),
        model: m.model.clone(),
        continuation_of: None,
        chain_root: None,
    };
    Ok(TaskCreate {
        facts,
        header,
        status,
        prompt,
        process: Some(TaskProcess {
            pid: child.pid(),
            start: Some(child.clone()),
        }),
    })
}
pub fn finish(
    task: &mut TaskCreate,
    status: &str,
    exit: i64,
    at: &str,
    summary: Option<&str>,
) -> Result<(), Error> {
    task.facts.status = status.into();
    task.facts.exit_code = Some(exit);
    task.facts.finished_at = Some(at.into());
    task.facts.finished_at_us = hf_domain::timestamp::parse_micros(at);
    task.facts.output_drained = true;
    let (header, status) =
        terminal_projection_with_summary(&terminal_facts(task, exit, at), summary)
            .map_err(|_| Error::Refused)?;
    task.header = header;
    task.status = status;
    Ok(())
}
pub fn terminal_facts<'a>(
    task: &'a TaskCreate,
    exit: i64,
    at: &'a str,
) -> TerminalProjectionFacts<'a> {
    let f = &task.facts;
    TerminalProjectionFacts {
        initial: InitialProjectionFacts {
            task: &f.id,
            backend: &f.backend,
            repo_path: &f.repo_path,
            session_id: &f.session_id,
            status: &f.status,
            started_at: &f.started_at,
            title: f.title.as_deref(),
            model: f.model.as_deref(),
            continuation_of: None,
            chain_root: None,
        },
        exit_code: exit,
        finished_at: at,
        output_drained: true,
    }
}
