//! Canonical nonterminal task blobs for production lifecycle preparation.
use hf_application::task_lifecycle::{
    InitialProjectionFacts, InitialProjectionValidator, InvalidInitialProjection,
};
use serde_json::{Value, json};
pub struct CanonicalInitialProjection;
fn document(f: &InitialProjectionFacts<'_>) -> Result<Value, InvalidInitialProjection> {
    if !matches!(f.status, "queued" | "running") {
        return Err(InvalidInitialProjection);
    }
    Ok(
        json!({"task_id":f.task.as_str(),"backend":f.backend,"repo_path":f.repo_path,
        "session_id":f.session_id,"status":f.status,"started_at":f.started_at,
        "title":f.title,"model":f.model,"continuation_of_task_id":f.continuation_of.map(|id|id.as_str()),
        "chain_root_task_id":f.chain_root,"exit_code":null,"finished_at":null,"output_drained":false}),
    )
}
/// Both served blobs contain exactly the typed initial facts; no unvalidated extra keys survive.
pub fn initial_projection(
    facts: &InitialProjectionFacts<'_>,
) -> Result<(Vec<u8>, Vec<u8>), InvalidInitialProjection> {
    let bytes = serde_json::to_vec(&document(facts)?).map_err(|_| InvalidInitialProjection)?;
    if bytes.len() > 2048 {
        return Err(InvalidInitialProjection);
    }
    Ok((bytes.clone(), bytes))
}
impl InitialProjectionValidator for CanonicalInitialProjection {
    fn validate(
        &self,
        facts: &InitialProjectionFacts<'_>,
        header: &[u8],
        status: &[u8],
    ) -> Result<(), InvalidInitialProjection> {
        if header.len() > 2048 || status.len() > 2048 {
            return Err(InvalidInitialProjection);
        }
        let expected = document(facts)?;
        for bytes in [header, status] {
            // Byte equality also refuses duplicate keys and alternate unbounded structure.
            let canonical = serde_json::to_vec(&expected).map_err(|_| InvalidInitialProjection)?;
            if bytes != canonical {
                return Err(InvalidInitialProjection);
            }
        }
        Ok(())
    }
}
