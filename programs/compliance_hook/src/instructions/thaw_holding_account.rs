//! `thaw_holding_account` — permissionless and state-checked (contracts/compliance_hook.md,
//! research R9): thaws an Approved owner's one holding account, its Token-2022 ATA.

use anchor_lang::prelude::*;
use anchor_spl::token_2022::spl_token_2022::extension::StateWithExtensions;
use anchor_spl::token_2022::spl_token_2022::state::{Account as TokenAccount, AccountState};
use anchor_spl::token_2022::{self, ThawAccount, Token2022};
use mmf_common::ids::{ASSOCIATED_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID};
use mmf_common::seeds;

use crate::errors::HookError;
use crate::events::HoldingAccountThawed;
use crate::state::{HolderState, Registry};

#[event_cpi]
#[derive(Accounts)]
pub struct ThawHoldingAccount<'info> {
    /// Anyone; adds no discretion (Constitution III).
    pub payer: Signer<'info>,
    /// The mint's freeze authority; signs the thaw CPI.
    pub registry: Account<'info, Registry>,
    #[account(
        seeds = [seeds::HOLDER, registry.mint.as_ref(), holder_state.owner.as_ref()],
        bump = holder_state.bump
    )]
    pub holder_state: Account<'info, HolderState>,
    /// CHECK: must be the owner's ATA for the fund mint; checked in the handler.
    #[account(mut)]
    pub token_account: UncheckedAccount<'info>,
    /// CHECK: the fund mint (`registry.mint`).
    #[account(address = registry.mint)]
    pub mint: UncheckedAccount<'info>,
    pub token_2022_program: Program<'info, Token2022>,
}

pub fn handle_thaw_holding_account(ctx: Context<ThawHoldingAccount>) -> Result<()> {
    let a = &ctx.accounts;
    let now = Clock::get()?.unix_timestamp;

    // 1. The owner is Approved with unexpired KYC (fund-operated accounts never expire).
    require!(a.holder_state.approved(now), HookError::HolderNotApproved);

    // 2. Only the owner's one holding account: its Token-2022 ATA for this mint (research R9).
    let owner = a.holder_state.owner;
    let mint = a.registry.mint;
    let ata = Pubkey::find_program_address(
        &[
            owner.as_ref(),
            TOKEN_2022_PROGRAM_ID.as_ref(),
            mint.as_ref(),
        ],
        &ASSOCIATED_TOKEN_PROGRAM_ID,
    )
    .0;
    require_keys_eq!(a.token_account.key(), ata, HookError::NotHoldingAccount);

    // 3. It must actually be frozen.
    let frozen = {
        let data = a.token_account.try_borrow_data()?;
        let state = StateWithExtensions::<TokenAccount>::unpack(&data)
            .map_err(|_| error!(HookError::NotHoldingAccount))?;
        state.base.state == AccountState::Frozen
    };
    require!(frozen, HookError::AlreadyThawed);

    let bump = [a.registry.bump];
    let signer: &[&[&[u8]]] = &[&[seeds::REGISTRY, mint.as_ref(), &bump]];
    token_2022::thaw_account(
        CpiContext::new(
            a.token_2022_program.key(),
            ThawAccount {
                account: a.token_account.to_account_info(),
                mint: a.mint.to_account_info(),
                authority: a.registry.to_account_info(),
            },
        )
        .with_signer(signer),
    )?;

    let clock = Clock::get()?;
    emit_cpi!(HoldingAccountThawed {
        fund: a.registry.fund_state,
        owner,
        token_account: ata,
        signer: a.payer.key(),
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}
