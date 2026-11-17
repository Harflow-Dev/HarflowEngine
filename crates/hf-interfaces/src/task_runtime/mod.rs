//! Production task codecs, separate from the HFOP fixture invitation.
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

mod terminal;
pub use terminal::{CanonicalTerminalProjection, task_finished, terminal_projection};

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
