/// Call only in the fresh dedicated single-thread fixture host, at entry and
/// immediately before spawn. This is not a concurrent-process launch barrier.
pub fn check_entry() -> std::io::Result<()> {
    crate::task_runtime::bootstrap::check_empty_entry(false)
}
