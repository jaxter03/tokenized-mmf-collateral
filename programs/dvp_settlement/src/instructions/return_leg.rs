//! `return_leg(leg)` (contracts/dvp_settlement.md, FR-026, research R3): anyone delivers one
//! earmarked leg of a cancelled, expired or rejected instruction back to the party that funded
//! it. If the delivery can't happen yet (the seller is suspended, transfers are paused, a cash
//! account is missing), the transaction fails, the earmark stays, and it can be retried.

use anchor_lang::prelude::*;
use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use mmf_common::enums::Leg;
use mmf_common::ids::{COMPLIANCE_HOOK_PROGRAM_ID, TOKEN_2022_PROGRAM_ID};
use mmf_common::seeds;
use mmf_common::views::FundStateView;

use crate::errors::DvpError;
use crate::escrow;
use crate::events::LegReturned;
use crate::state::{DvpConfig, SettlementInstruction};
use crate::token::{self, UnitTransfer};

#[event_cpi]
#[derive(Accounts)]
pub struct ReturnLeg<'info> {
    /// Anyone; recorded only.
    pub triggered_by: Signer<'info>,
    /// CHECK: decoded with `FundStateView` (hook extra account for units).
    pub fund_state: UncheckedAccount<'info>,
    #[account(seeds = [seeds::DVP, fund_state.key().as_ref()], bump = dvp_config.bump)]
    pub dvp_config: Account<'info, DvpConfig>,
    /// A genuine instruction (owner and discriminator checked by Anchor); the handler checks it
    /// belongs to `fund_state` (`FundMismatch`).
    #[account(mut)]
    pub settlement: Account<'info, SettlementInstruction>,
    /// CHECK: PDA `["escrow", fund_state]`; signs the return.
    #[account(seeds = [seeds::ESCROW, fund_state.key().as_ref()], bump = dvp_config.escrow_bump)]
    pub escrow_authority: UncheckedAccount<'info>,
    /// CHECK: the escrow account the leg leaves (unit escrow or cash escrow).
    #[account(mut)]
    pub escrow_account: UncheckedAccount<'info>,
    /// CHECK: the funder's account: the seller's holding account (units), the buyer's cash ATA,
    /// or the treasury cash account (a redemption's cash).
    #[account(mut)]
    pub recipient: UncheckedAccount<'info>,
    /// CHECK: the leg's mint (fund mint or cash mint).
    pub leg_mint: UncheckedAccount<'info>,
    /// CHECK: the leg's token program.
    pub token_program: UncheckedAccount<'info>,
    // --- units only: the hook's extra accounts ---
    /// CHECK: the fund's Registry.
    #[account(mut)]
    pub registry: Option<UncheckedAccount<'info>>,
    /// CHECK: the escrow's HolderState (hook source holder).
    #[account(mut)]
    pub escrow_holder: Option<UncheckedAccount<'info>>,
    /// CHECK: the seller's HolderState (hook destination holder).
    #[account(mut)]
    pub recipient_holder: Option<UncheckedAccount<'info>>,
    /// CHECK: governance delegate PDA.
    pub delegate: Option<UncheckedAccount<'info>>,
    /// CHECK: compliance_hook program.
    pub hook_program: Option<UncheckedAccount<'info>>,
    /// CHECK: the mint's ExtraAccountMetaList.
    pub extra_account_meta_list: Option<UncheckedAccount<'info>>,
}

fn present<'a, 'info>(a: &'a Option<UncheckedAccount<'info>>) -> Result<&'a AccountInfo<'info>> {
    a.as_ref()
        .map(|x| x.as_ref())
        .ok_or(error!(DvpError::WrongAccount))
}

pub fn handle_return_leg(ctx: Context<ReturnLeg>, leg: u8) -> Result<()> {
    let a = &ctx.accounts;
    let fund_key = a.fund_state.key();
    let si = &a.settlement;

    // 1. The instruction belongs to this fund.
    escrow::require_fund(si, &fund_key)?;
    let fund = FundStateView::load(&a.fund_state, &fund_key)
        .map_err(|_| error!(DvpError::FundMismatch))?;
    // 2–4. A known leg, of a cancelled, expired or rejected instruction, with something earmarked.
    let leg = Leg::try_from(leg).map_err(|_| error!(DvpError::InvalidLeg))?;
    require!(si.status().is_returnable(), DvpError::NotReturnable);
    let amount = match leg {
        Leg::Units => si.units_to_return,
        Leg::Cash => si.cash_to_return,
    };
    require!(amount > 0, DvpError::NothingToReturn);

    let escrow = a.escrow_authority.key();
    let fund_bytes = fund_key.to_bytes();
    let bump = [a.dvp_config.escrow_bump];
    let signer: &[&[&[u8]]] = &[&[seeds::ESCROW, &fund_bytes, &bump]];
    let recipient = a.recipient.key();
    match leg {
        Leg::Units => {
            // 5. Back to the seller's own holding account, from the unit escrow, through the hook.
            require!(
                a.leg_mint.key() == fund.mint
                    && a.token_program.key() == TOKEN_2022_PROGRAM_ID
                    && a.escrow_account.key()
                        == get_associated_token_address_with_program_id(
                            &escrow,
                            &fund.mint,
                            &TOKEN_2022_PROGRAM_ID
                        )
                    && recipient
                        == get_associated_token_address_with_program_id(
                            &si.seller,
                            &fund.mint,
                            &TOKEN_2022_PROGRAM_ID
                        ),
                DvpError::WrongAccount
            );
            let hook_program = present(&a.hook_program)?;
            require_keys_eq!(
                hook_program.key(),
                COMPLIANCE_HOOK_PROGRAM_ID,
                DvpError::WrongAccount
            );
            token::transfer_units(
                UnitTransfer {
                    token_2022_program: &a.token_program,
                    source: &a.escrow_account,
                    mint: &a.leg_mint,
                    destination: &a.recipient,
                    authority: &a.escrow_authority,
                    registry: present(&a.registry)?,
                    fund_state: &a.fund_state,
                    source_holder: present(&a.escrow_holder)?,
                    destination_holder: present(&a.recipient_holder)?,
                    delegate: present(&a.delegate)?,
                    hook_program,
                    extra_account_meta_list: present(&a.extra_account_meta_list)?,
                },
                amount,
                signer,
            )?;
        }
        Leg::Cash => {
            // 5. Back to whoever funded it: the buyer's own cash ATA, or the treasury cash account
            // for a redemption (the transfer agent paid it from there).
            let cash_program = a.token_program.key();
            let expected = if si.buyer == fund_key {
                fund.cash_treasury
            } else {
                get_associated_token_address_with_program_id(
                    &si.buyer,
                    &fund.cash_mint,
                    &cash_program,
                )
            };
            require!(
                a.leg_mint.key() == fund.cash_mint
                    && cash_program == *a.leg_mint.owner
                    && a.escrow_account.key()
                        == get_associated_token_address_with_program_id(
                            &escrow,
                            &fund.cash_mint,
                            &cash_program
                        )
                    && recipient == expected,
                DvpError::WrongAccount
            );
            token::transfer_cash(
                &a.token_program,
                &a.escrow_account,
                &a.leg_mint,
                &a.recipient,
                &a.escrow_authority,
                amount,
                signer,
            )?;
        }
    }

    let si = &mut ctx.accounts.settlement;
    match leg {
        Leg::Units => si.units_to_return = 0,
        Leg::Cash => si.cash_to_return = 0,
    }
    let clock = Clock::get()?;
    emit_cpi!(LegReturned {
        fund: fund_key,
        seq: si.seq,
        leg: leg.as_u8(),
        amount,
        recipient,
        triggered_by: ctx.accounts.triggered_by.key(),
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}
