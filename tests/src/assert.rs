//! Error assertions (T021). Every rejection test asserts the exact error (Constitution VI).

use anchor_spl::token_2022::spl_token_2022::error::TokenError;
use solana_transaction::InstructionError;
use solana_transaction_error::TransactionError;

use crate::harness::TxResult;

/// The custom error code the transaction failed with, and the failing instruction's index.
/// Panics (with logs) if the transaction succeeded or failed some other way.
pub fn custom_error(res: &TxResult) -> (u8, u32) {
    match res {
        Ok(meta) => panic!(
            "expected the transaction to fail, but it succeeded; logs:\n{}",
            meta.logs.join("\n")
        ),
        Err(f) => match &f.err {
            TransactionError::InstructionError(ix, InstructionError::Custom(code)) => (*ix, *code),
            other => panic!(
                "expected a custom program error, got {other:?}; logs:\n{}",
                f.meta.logs.join("\n")
            ),
        },
    }
}

/// Asserts the transaction failed with this Anchor error (program error enum or
/// `anchor_lang::error::ErrorCode`), e.g. `assert_anchor_error(&res, HookError::Unauthorized)`.
#[track_caller]
pub fn assert_anchor_error<E: Into<u32> + core::fmt::Debug + Copy>(res: &TxResult, expected: E) {
    let (_, code) = custom_error(res);
    let want: u32 = expected.into();
    if code != want {
        let logs = res
            .as_ref()
            .err()
            .map(|f| f.meta.logs.join("\n"))
            .unwrap_or_default();
        panic!("expected {expected:?} ({want}), got custom error {code}; logs:\n{logs}");
    }
}

/// Asserts the transaction failed with this Token-2022 error, e.g. `TokenError::AccountFrozen`.
#[track_caller]
pub fn assert_token_error(res: &TxResult, expected: TokenError) {
    let (_, code) = custom_error(res);
    let name = format!("{expected:?}");
    assert_eq!(
        code, expected as u32,
        "expected Token-2022 {name}, got custom error {code}"
    );
}

/// Asserts the transaction failed with this Squads v4 error code (e.g. 6008 InvalidProposalStatus).
#[track_caller]
pub fn assert_squads_error(res: &TxResult, expected: u32) {
    let (_, code) = custom_error(res);
    assert_eq!(
        code, expected,
        "expected Squads error {expected}, got {code}"
    );
}

/// Asserts the transaction failed with a non-custom instruction error (e.g.
/// `MissingRequiredSignature`).
#[track_caller]
pub fn assert_instruction_error(res: &TxResult, expected: InstructionError) {
    match res {
        Err(f) => match &f.err {
            TransactionError::InstructionError(_, e) if *e == expected => {}
            other => panic!("expected {expected:?}, got {other:?}"),
        },
        Ok(_) => panic!("expected {expected:?}, but the transaction succeeded"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Harness, Ix};

    #[test]
    fn asserts_anchor_framework_error() {
        let mut h = Harness::new();
        // Unknown instruction → Anchor's InstructionFallbackNotFound (101).
        let ix = Ix {
            program_id: mmf_common::ids::FUND_PROGRAM_ID,
            accounts: vec![],
            data: vec![0; 8],
        };
        let res = h.send(&[ix], &[]);
        assert_anchor_error(
            &res,
            anchor_lang::error::ErrorCode::InstructionFallbackNotFound,
        );
        assert_eq!(custom_error(&res), (0, 101));
    }

    #[test]
    #[should_panic(expected = "expected AccountNotSigner (3010), got custom error 101")]
    fn reports_wrong_error() {
        let mut h = Harness::new();
        let ix = Ix {
            program_id: mmf_common::ids::FUND_PROGRAM_ID,
            accounts: vec![],
            data: vec![0; 8],
        };
        let res = h.send(&[ix], &[]);
        assert_anchor_error(&res, anchor_lang::error::ErrorCode::AccountNotSigner);
    }
}
