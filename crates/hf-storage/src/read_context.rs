use crate::{Error, Identity, Limits, Result, Store};
pub(crate) struct ReadContext<'a> {
    pub identity: &'a Identity,
    pub limits: Limits,
}
impl ReadContext<'_> {
    pub(crate) fn check_bounds(&self, rows: usize, bytes: usize) -> Result<()> {
        if rows > self.limits.max_rows || bytes > self.limits.max_bytes {
            Err(Error::LimitExceeded)
        } else {
            Ok(())
        }
    }
}
impl Store {
    pub(crate) fn read_context(&self) -> ReadContext<'_> {
        ReadContext {
            identity: &self.identity,
            limits: self.limits,
        }
    }
}
