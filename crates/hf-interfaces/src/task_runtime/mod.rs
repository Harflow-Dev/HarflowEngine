//! Production task codecs, separate from the HFOP fixture invitation.
mod environment;
mod event_bounds;
mod events;
pub use events::{NormalizedEvent, backend_events};
mod protocol;
mod protocol_finish;
mod protocol_refusal;
pub use environment::{decode_environment, encode_environment};
pub use protocol::{
    decode_started, decode_writer_ack, decode_writer_command, encode_started, encode_writer_ack,
    encode_writer_command,
};
pub use protocol_refusal::{decode_not_started, encode_not_started};
mod metadata;
pub use metadata::{
    CanonicalLaunchValidator, InitialLaunchMetadata, decode_metadata, encode_metadata,
    intent_payload,
};
/// Exactly the canonical Python encode_live_message JSONL representation.
pub fn encode_live_message(
    prompt: &str,
) -> Result<Vec<u8>, hf_application::task_runtime::InvalidIntent> {
    if prompt.len() > 4 * 1024 * 1024 {
        return Err(hf_application::task_runtime::InvalidIntent);
    }
    let text =
        serde_json::to_string(prompt).map_err(|_| hf_application::task_runtime::InvalidIntent)?;
    Ok(format!("{{\"type\": \"user\", \"message\": {{\"role\": \"user\", \"content\": [{{\"type\": \"text\", \"text\": {text}}}]}}}}\n").into_bytes())
}

mod chunks;
pub use chunks::{BlobChunk, BlobTransfer, CHUNK_BYTES, FRAME_BYTES, decode_chunk, encode_chunk};

mod summary;
mod terminal;
pub use summary::{
    CanonicalTerminalSummaryProjection, successful_result_summary, terminal_projection_with_summary,
};
pub use terminal::{
    CanonicalTerminalProjection, task_finished, task_finished_observed, terminal_projection,
};

mod projection;
pub use projection::spawn_projection;

mod invitation;
pub use invitation::{decode_invitation, encode_invitation};

mod control;
pub use control::{cancel_response, decode_control, encode_control};

mod writer;
pub use writer::{
    WRITER_PAYLOAD_BYTES, decode_writer_bootstrap, decode_writer_ready, encode_writer_bootstrap,
    encode_writer_ready,
};
