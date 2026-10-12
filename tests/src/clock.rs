//! Clock helpers (T021). KYC expiry depends on `unix_timestamp`; `warp_to_slot` changes only the
//! slot, so time is moved by rewriting the Clock sysvar.

use anchor_lang::prelude::Clock;
use litesvm::LiteSVM;

/// Current `Clock::unix_timestamp`.
pub fn now(svm: &LiteSVM) -> i64 {
    svm.get_sysvar::<Clock>().unix_timestamp
}

/// Sets `Clock::unix_timestamp`, leaving the slot unchanged.
pub fn set_unix_timestamp(svm: &mut LiteSVM, ts: i64) {
    let mut c = svm.get_sysvar::<Clock>();
    c.unix_timestamp = ts;
    svm.set_sysvar(&c);
}

/// Moves time forward (or back, for negative `secs`).
pub fn advance(svm: &mut LiteSVM, secs: i64) {
    let t = now(svm);
    set_unix_timestamp(svm, t + secs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_and_advances_time() {
        let mut svm = LiteSVM::new();
        set_unix_timestamp(&mut svm, 1_000);
        assert_eq!(now(&svm), 1_000);
        advance(&mut svm, 25);
        assert_eq!(now(&svm), 1_025);
    }
}
