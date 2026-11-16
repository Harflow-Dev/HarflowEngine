//! Bounded canonical codecs for recovery journals and completed receipts.
mod writer;
pub use writer::encode_journal;

mod decode;
mod inventory_decode;
mod result;
mod tree;
pub use decode::decode_journal;
pub use result::{decode_result, encode_result};

#[derive(Clone, Copy, Debug, Default)]
pub struct JsonJournalCodec;
impl hf_application::ctl::state_recover::flow::JournalCodecPort for JsonJournalCodec {
    fn encode(
        &self,
        journal: &hf_application::ctl::state_recover::journal::RecoveryJournal,
    ) -> Result<Vec<u8>, hf_application::ctl::state_recover::journal::JournalError> {
        encode_journal(journal)
    }
    fn decode(
        &self,
        bytes: &[u8],
    ) -> Result<
        hf_application::ctl::state_recover::journal::RecoveryJournal,
        hf_application::ctl::state_recover::journal::JournalError,
    > {
        decode_journal(bytes)
    }
    fn encode_result(
        &self,
        result: &hf_application::ctl::state_recover::journal::RecoveryResult,
    ) -> Result<Vec<u8>, hf_application::ctl::state_recover::journal::JournalError> {
        encode_result(result)
    }
    fn decode_result(
        &self,
        bytes: &[u8],
    ) -> Result<
        hf_application::ctl::state_recover::journal::RecoveryResult,
        hf_application::ctl::state_recover::journal::JournalError,
    > {
        decode_result(bytes)
    }
}
