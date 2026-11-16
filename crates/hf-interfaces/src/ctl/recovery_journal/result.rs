//! Completed receipts contain no current-store evidence.
use super::{
    tree::{Tree, field, fields},
    writer::Writer,
};
use hf_application::ctl::state_recover::journal::{JOURNAL_BOUND, JournalError, RecoveryResult};
use hf_domain::ctl::{identity::PRODUCT_VERSION, state_recovery::is_operation_id};

pub fn encode_result(result: &RecoveryResult) -> Result<Vec<u8>, JournalError> {
    validate(result)?;
    let mut out = Writer(Vec::new());
    write(&mut out, result)?;
    out.raw("\n")?;
    Ok(out.0)
}
pub(super) fn write(out: &mut Writer, r: &RecoveryResult) -> Result<(), JournalError> {
    validate(r)?;
    out.raw("{\"backup_id\":")?;
    out.string(&r.operation_id)?;
    out.raw(",\"backup_path\":")?;
    out.string(&r.backup_path)?;
    out.raw(",\"engine_version\":")?;
    out.string(PRODUCT_VERSION)?;
    out.raw(",\"operation_id\":")?;
    out.string(&r.operation_id)?;
    out.raw(",\"phase\":\"completed\",\"recovery_supported\":true,\"state_root\":")?;
    out.string(&r.state_root)?;
    out.raw(",\"status\":\"completed\"}")
}
pub fn decode_result(bytes: &[u8]) -> Result<RecoveryResult, JournalError> {
    if bytes.len() > JOURNAL_BOUND {
        return Err(JournalError::TooLarge);
    }
    let t = Tree::parse(bytes)?;
    decode_tree(&t, 0)
}
pub(super) fn decode_tree(t: &Tree, id: usize) -> Result<RecoveryResult, JournalError> {
    let m = t.object(id)?;
    fields(
        m,
        &[
            "backup_id",
            "backup_path",
            "engine_version",
            "operation_id",
            "phase",
            "recovery_supported",
            "state_root",
            "status",
        ],
    )?;
    let get = |key| field(m, key);
    let r = RecoveryResult {
        operation_id: t.text(get("operation_id")?)?,
        state_root: t.text(get("state_root")?)?,
        backup_path: t.text(get("backup_path")?)?,
    };
    if t.text(get("backup_id")?)? != r.operation_id
        || t.text(get("engine_version")?)? != PRODUCT_VERSION
        || t.text(get("phase")?)? != "completed"
        || t.text(get("status")?)? != "completed"
        || t.scalar(get("recovery_supported")?)?.as_bool() != Some(true)
    {
        return Err(JournalError::Invalid);
    }
    validate(&r)?;
    Ok(r)
}
fn validate(r: &RecoveryResult) -> Result<(), JournalError> {
    if !is_operation_id(&r.operation_id)
        || !r.state_root.starts_with('/')
        || r.state_root.contains('\0')
        || !r.backup_path.starts_with('/')
        || r.backup_path.contains('\0')
    {
        return Err(JournalError::Invalid);
    }
    Ok(())
}
