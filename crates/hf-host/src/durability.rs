//! Writer options of the scaffold binary's store handles.
use hf_storage::WriterOptions;

/// The options of `hf_host::task_runtime::store_options::suppressed`.
pub(crate) fn suppressed() -> WriterOptions {
    hf_host::task_runtime::store_options::suppressed()
}
