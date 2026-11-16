//! Bounded canonical codec for precommit recovery journals.
mod writer;
pub use writer::encode_journal;

mod decode;
mod inventory_decode;
mod tree;
pub use decode::decode_journal;

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
}
