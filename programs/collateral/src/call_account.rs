//! Opening delivery calls (contracts/collateral.md "Opening a delivery call", research R7).
//! `update_exposure` and `confirm_exposure` create the call account only when a raise needs one,
//! so they create it by hand at its PDA; `issue_call` uses Anchor `init` and the same builder.

use anchor_lang::prelude::*;
use anchor_lang::system_program;
use mmf_common::collateral::units_for_value_up;
use mmf_common::enums::{CallKind, CallStatus};
use mmf_common::seeds;
use mmf_common::views::FundStateView;

use crate::errors::CollateralError;
use crate::events::CallOpened;
use crate::state::{CollateralCall, PledgeAgreement};

/// A Delivery call for `value` issued now, numbered `a.call_count` (not yet counted).
pub fn delivery_call(
    a: &PledgeAgreement,
    agreement: Pubkey,
    fund: &FundStateView,
    now: i64,
    value: u64,
    external_ref: [u8; 32],
    bump: u8,
) -> Result<CollateralCall> {
    let overflow = || error!(CollateralError::Overflow);
    let units_required = units_for_value_up(value, fund.nav, a.haircut_bps).ok_or_else(overflow)?;
    let cure_deadline_ts = now
        .checked_add(i64::from(a.call_cure_secs))
        .ok_or_else(overflow)?;
    let enforce_after_ts = cure_deadline_ts
        .checked_add(i64::from(a.enforcement_grace_secs))
        .ok_or_else(overflow)?;
    Ok(CollateralCall {
        version: CollateralCall::VERSION,
        agreement,
        seq: a.call_count,
        kind: CallKind::Delivery.as_u8(),
        value_delta: value,
        units_required,
        nav_seq: fund.nav_seq,
        issued_at: now,
        cure_deadline_ts,
        enforce_after_ts,
        status: CallStatus::Open.as_u8(),
        enforced_units: 0,
        enforced_value: 0,
        external_ref,
        units_released: 0,
        dispute_ref: [0; 32],
        disputed_at: 0,
        closed_at: 0,
        bump,
        _reserved: [0; 64],
    })
}

/// Count a newly opened Delivery call on the agreement: it demands the uncalled shortfall, so
/// the demanded part grows by its value (and then equals the shortfall).
pub fn count_delivery(a: &mut PledgeAgreement, value: u64) -> Result<()> {
    let overflow = || error!(CollateralError::Overflow);
    a.call_count = a.call_count.checked_add(1).ok_or_else(overflow)?;
    a.open_delivery_calls = a.open_delivery_calls.checked_add(1).ok_or_else(overflow)?;
    a.called_outstanding = a.called_outstanding.checked_add(value).ok_or_else(overflow)?;
    Ok(())
}

/// The event's view of a call.
pub fn opened(call: &CollateralCall, key: Pubkey) -> CallOpened {
    CallOpened {
        call: key,
        seq: call.seq,
        kind: call.kind,
        value_delta: call.value_delta,
        units_required: call.units_required,
        nav_seq: call.nav_seq,
        cure_deadline_ts: call.cure_deadline_ts,
        enforce_after_ts: call.enforce_after_ts,
        external_ref: call.external_ref,
    }
}

/// Creates the call account at `["call", agreement, external_ref]` and writes `call` into it.
/// `CallRefInUse` if the agreement already has a call with this reference. Lamports someone sent
/// to the address beforehand are kept, as Anchor's `init` does.
pub fn create<'info>(
    call_info: &AccountInfo<'info>,
    payer: &AccountInfo<'info>,
    system: &AccountInfo<'info>,
    agreement: &Pubkey,
    call: &CollateralCall,
) -> Result<()> {
    require!(
        call_info.data_is_empty() && call_info.owner == &system_program::ID,
        CollateralError::CallRefInUse
    );
    let space = 8 + CollateralCall::INIT_SPACE;
    let rent = Rent::get()?.minimum_balance(space);
    let bump = [call.bump];
    let signer_seeds: &[&[u8]] = &[
        seeds::CALL,
        agreement.as_ref(),
        &call.external_ref,
        &bump,
    ];
    let have = call_info.lamports();
    if have == 0 {
        system_program::create_account(
            CpiContext::new_with_signer(
                system.key(),
                system_program::CreateAccount {
                    from: payer.clone(),
                    to: call_info.clone(),
                },
                &[signer_seeds],
            ),
            rent,
            space as u64,
            &crate::ID,
        )?;
    } else {
        if have < rent {
            system_program::transfer(
                CpiContext::new(
                    system.key(),
                    system_program::Transfer {
                        from: payer.clone(),
                        to: call_info.clone(),
                    },
                ),
                rent - have,
            )?;
        }
        system_program::allocate(
            CpiContext::new_with_signer(
                system.key(),
                system_program::Allocate {
                    account_to_allocate: call_info.clone(),
                },
                &[signer_seeds],
            ),
            space as u64,
        )?;
        system_program::assign(
            CpiContext::new_with_signer(
                system.key(),
                system_program::Assign {
                    account_to_assign: call_info.clone(),
                },
                &[signer_seeds],
            ),
            &crate::ID,
        )?;
    }
    let mut data = call_info.try_borrow_mut_data()?;
    call.try_serialize(&mut &mut data[..])?;
    Ok(())
}
