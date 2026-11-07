//! Dispatch of the seventeen port operations.
use crate::{artifacts, digest, events, fail::Result, root, root::Context, tasks};
use hf_interfaces::fixture_port::{Request, Response};

pub fn execute(context: &Context, request: Request) -> Result<Response> {
    match request {
        Request::InitializeEmpty => root::initialize_empty(context),
        Request::RemoveRoot => root::remove_root(context),
        Request::MakeForeignRoot => root::make_foreign_root(context),
        Request::RootState => Ok(Response::Text(root::root_state(context).to_owned())),
        Request::AuthoritativeDigest => digest::authoritative_digest(context),
        Request::SeedWorld(world) => tasks::seed_world(context, &world),
        Request::AddTask(spec) => {
            context.with_store(|store| tasks::add_task(context, store, *spec))
        }
        Request::AddLiveCaller(caller) => {
            context.with_store(|store| tasks::add_live_caller(context, store, &caller))
        }
        Request::DamageTask(id) => context.with_store(|store| tasks::damage_task(store, &id)),
        Request::AppendEvent(id, event) => {
            context.with_store(|store| events::append_event(store, &id, &event))
        }
        Request::AppendMalformedLine(id) => {
            context.with_store(|store| events::append_malformed(store, &id))
        }
        Request::AppendPartialLine(id) => {
            context.with_store(|store| events::append_partial(store, &id))
        }
        Request::TruncateLog(id, keep) => {
            context.with_store(|store| events::truncate(store, &id, keep))
        }
        Request::RemoveLog(id) => context.with_store(|store| events::remove(store, &id)),
        Request::TamperPrompt(id) => context.with_store(|store| events::tamper_prompt(store, &id)),
        Request::WriteArtifacts {
            task,
            items,
            header_valid,
        } => context
            .with_store(|store| artifacts::write_artifacts(store, &task, &items, header_valid)),
        Request::ChangeArtifactTarget(id, name) => artifacts::change_target(context, &id, &name),
    }
}
