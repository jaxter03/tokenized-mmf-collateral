//! `settle` (contracts/dvp_settlement.md, FR-015–FR-022): both legs move in one instruction, or
//! nothing does. **Constitution IV exception** (plan.md): atomicity is the point; splitting the
//! legs would bring back the risk of paying without receiving.

use anchor_lang::prelude::*;
use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use mmf_common::enums::{FundStatus, SettlementKind, SettlementStatus};
use mmf_common::ids::{COMPLIANCE_HOOK_PROGRAM_ID, FUND_PROGRAM_ID, TOKEN_2022_PROGRAM_ID};
use mmf_common::seeds;
use mmf_common::views::{FundStateView, HolderStateView, RegistryView};

use crate::errors::DvpError;
use crate::events::Settled;
use crate::state::{DvpConfig, SettlementInstruction};
use crate::token::{self, UnitTransfer};

#[event_cpi]
#[derive(Accounts)]
pub struct Settle<'info> {
    /// Anyone (normally the keeper); recorded only (FR-015).
    pub triggered_by: Signer<'info>,
    /// CHECK: decoded with `FundStateView`; written by `fund` on a mint or burn.
    #[account(mut)]
    pub fund_state: UncheckedAccount<'info>,
    /// CHECK: the fund's Registry (writable: the hook and `accrue` run).
    #[account(mut)]
    pub registry: UncheckedAccount<'info>,
    #[account(seeds = [seeds::DVP, fund_state.key().as_ref()], bump = dvp_config.bump)]
    pub dvp_config: Account<'info, DvpConfig>,
    #[account(
        mut,
        seeds = [seeds::SETTLEMENT, fund_state.key().as_ref(), settlement.created_by.as_ref(), &settlement.external_ref],
        bump = settlement.bump,
    )]
    pub settlement: Account<'info, SettlementInstruction>,
    /// CHECK: PDA `["escrow", fund_state]`; signs every movement out of escrow.
    #[account(seeds = [seeds::ESCROW, fund_state.key().as_ref()], bump = dvp_config.escrow_bump)]
    pub escrow_authority: UncheckedAccount<'info>,
    /// CHECK: the escrow's HolderState (hook source holder; `accrue` target).
    #[account(mut)]
    pub escrow_holder: UncheckedAccount<'info>,
    /// CHECK: unit escrow.
    #[account(mut)]
    pub unit_escrow: UncheckedAccount<'info>,
    /// CHECK: cash escrow.
    #[account(mut)]
    pub cash_escrow: UncheckedAccount<'info>,
    /// CHECK: the fund mint.
    #[account(mut)]
    pub mint: UncheckedAccount<'info>,
    /// CHECK: the fund's cash mint.
    pub cash_mint: UncheckedAccount<'info>,
    /// CHECK: HolderState of the seller; absent for a Subscription.
    pub seller_holder: Option<UncheckedAccount<'info>>,
    /// CHECK: HolderState of the buyer (the hook's destination holder); absent for a Redemption.
    #[account(mut)]
    pub buyer_holder: Option<UncheckedAccount<'info>>,
    /// CHECK: the buyer's holding account (Subscription, Secondary).
    #[account(mut)]
    pub buyer_units: Option<UncheckedAccount<'info>>,
    /// CHECK: the seller's cash ATA (Redemption, Secondary).
    #[account(mut)]
    pub seller_cash: Option<UncheckedAccount<'info>>,
    /// CHECK: the fund's treasury cash account (Subscription).
    #[account(mut)]
    pub cash_treasury: Option<UncheckedAccount<'info>>,
    /// CHECK: the mint's ExtraAccountMetaList (unit delivery).
    pub extra_account_meta_list: UncheckedAccount<'info>,
    /// CHECK: governance delegate PDA (hook extra account).
    pub delegate: UncheckedAccount<'info>,
    /// CHECK: Token-2022.
    pub token_2022_program: UncheckedAccount<'info>,
    /// CHECK: the cash mint's token program.
    pub cash_token_program: UncheckedAccount<'info>,
    /// CHECK: compliance_hook program.
    pub hook_program: UncheckedAccount<'info>,
    /// CHECK: fund program (mint and burn CPIs).
    pub fund_program: UncheckedAccount<'info>,
    /// CHECK: fund's `["__event_authority"]` PDA.
    pub fund_event_authority: UncheckedAccount<'info>,
}

fn present<'a, 'info>(a: &'a Option<UncheckedAccount<'info>>) -> Result<&'a AccountInfo<'info>> {
    a.as_ref()
        .map(|x| x.as_ref())
        .ok_or(error!(DvpError::WrongAccount))
}

/// An investor party's record is present and eligible now (research R12).
fn eligible(holder: &Option<UncheckedAccount>, mint: &Pubkey, party: &Pubkey, now: i64) -> bool {
    holder
        .as_ref()
        .and_then(|h| HolderStateView::load(h, mint, party).ok())
        .is_some_and(|v| v.eligible(now))
}

pub fn handle_settle(ctx: Context<Settle>) -> Result<()> {
    let a = &ctx.accounts;
    let fund_key = a.fund_state.key();
    let si = &a.settlement;

    // 1. Accounts belong to one fund.
    let fund = FundStateView::load(&a.fund_state, &fund_key)
        .map_err(|_| error!(DvpError::FundMismatch))?;
    let registry_key = Pubkey::find_program_address(
        &[seeds::REGISTRY, fund.mint.as_ref()],
        &COMPLIANCE_HOOK_PROGRAM_ID,
    )
    .0;
    let registry = RegistryView::load(&a.registry, &registry_key)
        .map_err(|_| error!(DvpError::FundMismatch))?;
    require!(
        registry.fund_state == fund_key && si.fund_state == fund_key,
        DvpError::FundMismatch
    );

    // 2. Affirmed (a final instruction is reported as such).
    let status = si.status();
    require!(status.is_open(), DvpError::InstructionFinal);
    require!(status == SettlementStatus::Affirmed, DvpError::NotAffirmed);

    // 3–4. Inside its window.
    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    require!(now >= si.not_before_ts, DvpError::NotYetValid);
    require!(now < si.expires_at_ts, DvpError::InstructionExpired);

    // 5–6. The fund is Open and transfers aren't paused (a burn runs no hook, so this is checked
    // here for every kind; research R12).
    require!(
        fund.status == FundStatus::Open.as_u8(),
        DvpError::FundNotOpen
    );
    require!(!registry.transfers_paused, DvpError::TransfersPaused);

    // 7. Every investor party is eligible now.
    for (party, holder) in [(si.seller, &a.seller_holder), (si.buyer, &a.buyer_holder)] {
        if party != fund_key {
            require!(
                eligible(holder, &fund.mint, &party, now),
                DvpError::PartyNotEligible
            );
        }
    }

    // 8. The fund's own accounts and programs.
    let escrow = a.escrow_authority.key();
    let cash_program = a.cash_token_program.key();
    require!(
        a.mint.key() == fund.mint
            && a.cash_mint.key() == fund.cash_mint
            && cash_program == *a.cash_mint.owner
            && a.token_2022_program.key() == TOKEN_2022_PROGRAM_ID
            && a.hook_program.key() == COMPLIANCE_HOOK_PROGRAM_ID
            && a.fund_program.key() == FUND_PROGRAM_ID
            && a.unit_escrow.key()
                == get_associated_token_address_with_program_id(
                    &escrow,
                    &fund.mint,
                    &TOKEN_2022_PROGRAM_ID
                )
            && a.cash_escrow.key()
                == get_associated_token_address_with_program_id(
                    &escrow,
                    &fund.cash_mint,
                    &cash_program
                ),
        DvpError::WrongAccount
    );

    let fund_bytes = fund_key.to_bytes();
    let bump = [a.dvp_config.escrow_bump];
    let signer: &[&[&[u8]]] = &[&[seeds::ESCROW, &fund_bytes, &bump]];

    match si.kind() {
        SettlementKind::Subscription => {
            // 8 (cont.). Units to the buyer's own holding account, cash to the treasury.
            let buyer_units = present(&a.buyer_units)?;
            let cash_treasury = present(&a.cash_treasury)?;
            let buyer_holder = present(&a.buyer_holder)?;
            require!(
                buyer_units.key()
                    == get_associated_token_address_with_program_id(
                        &si.buyer,
                        &fund.mint,
                        &TOKEN_2022_PROGRAM_ID
                    )
                    && cash_treasury.key() == fund.cash_treasury,
                DvpError::WrongAccount
            );

            // Create the units in escrow (fund checks the settlement itself, research R7) …
            fund::cpi::mint_for_subscription(
                CpiContext::new_with_signer(
                    FUND_PROGRAM_ID,
                    fund::cpi::accounts::MintForSubscription {
                        escrow_authority: a.escrow_authority.to_account_info(),
                        fund_state: a.fund_state.to_account_info(),
                        mint: a.mint.to_account_info(),
                        unit_escrow: a.unit_escrow.to_account_info(),
                        cash_escrow: a.cash_escrow.to_account_info(),
                        settlement: a.settlement.to_account_info(),
                        registry: a.registry.to_account_info(),
                        escrow_holder: a.escrow_holder.to_account_info(),
                        compliance_hook_program: a.hook_program.to_account_info(),
                        token_2022_program: a.token_2022_program.to_account_info(),
                        event_authority: a.fund_event_authority.to_account_info(),
                        program: a.fund_program.to_account_info(),
                    },
                    signer,
                ),
                si.units,
            )?;
            // … deliver them through the hook …
            token::transfer_units(
                UnitTransfer {
                    token_2022_program: &a.token_2022_program,
                    source: &a.unit_escrow,
                    mint: &a.mint,
                    destination: buyer_units,
                    authority: &a.escrow_authority,
                    registry: &a.registry,
                    fund_state: &a.fund_state,
                    source_holder: &a.escrow_holder,
                    destination_holder: buyer_holder,
                    delegate: &a.delegate,
                    hook_program: &a.hook_program,
                    extra_account_meta_list: &a.extra_account_meta_list,
                },
                si.units,
                signer,
            )?;
            // … and pay the cash to the treasury, all in this one instruction.
            token::transfer_cash(
                &a.cash_token_program,
                &a.cash_escrow,
                &a.cash_mint,
                cash_treasury,
                &a.escrow_authority,
                si.cash_amount,
                signer,
            )?;
        }
        SettlementKind::Redemption => {
            // 8 (cont.). Cash to the seller's own cash ATA.
            let seller_cash = present(&a.seller_cash)?;
            require_keys_eq!(
                seller_cash.key(),
                get_associated_token_address_with_program_id(
                    &si.seller,
                    &fund.cash_mint,
                    &cash_program
                ),
                DvpError::WrongAccount
            );
            // Destroy the units in escrow within the daily limit (fund checks the settlement
            // itself and the limit, research R7, R9) …
            fund::cpi::burn_for_redemption(
                CpiContext::new_with_signer(
                    FUND_PROGRAM_ID,
                    fund::cpi::accounts::BurnForRedemption {
                        escrow_authority: a.escrow_authority.to_account_info(),
                        fund_state: a.fund_state.to_account_info(),
                        mint: a.mint.to_account_info(),
                        unit_escrow: a.unit_escrow.to_account_info(),
                        settlement: a.settlement.to_account_info(),
                        registry: a.registry.to_account_info(),
                        escrow_holder: a.escrow_holder.to_account_info(),
                        compliance_hook_program: a.hook_program.to_account_info(),
                        token_2022_program: a.token_2022_program.to_account_info(),
                        event_authority: a.fund_event_authority.to_account_info(),
                        program: a.fund_program.to_account_info(),
                    },
                    signer,
                ),
                si.units,
            )?;
            // … and pay the seller, in this one instruction.
            token::transfer_cash(
                &a.cash_token_program,
                &a.cash_escrow,
                &a.cash_mint,
                seller_cash,
                &a.escrow_authority,
                si.cash_amount,
                signer,
            )?;
        }
        SettlementKind::SecondaryTransfer => {
            // 8 (cont.). Units to the buyer's own holding account, cash to the seller's own ATA.
            let buyer_units = present(&a.buyer_units)?;
            let seller_cash = present(&a.seller_cash)?;
            let buyer_holder = present(&a.buyer_holder)?;
            require!(
                buyer_units.key()
                    == get_associated_token_address_with_program_id(
                        &si.buyer,
                        &fund.mint,
                        &TOKEN_2022_PROGRAM_ID
                    )
                    && seller_cash.key()
                        == get_associated_token_address_with_program_id(
                            &si.seller,
                            &fund.cash_mint,
                            &cash_program
                        ),
                DvpError::WrongAccount
            );
            // The units through the hook, then the cash, in this one instruction.
            token::transfer_units(
                UnitTransfer {
                    token_2022_program: &a.token_2022_program,
                    source: &a.unit_escrow,
                    mint: &a.mint,
                    destination: buyer_units,
                    authority: &a.escrow_authority,
                    registry: &a.registry,
                    fund_state: &a.fund_state,
                    source_holder: &a.escrow_holder,
                    destination_holder: buyer_holder,
                    delegate: &a.delegate,
                    hook_program: &a.hook_program,
                    extra_account_meta_list: &a.extra_account_meta_list,
                },
                si.units,
                signer,
            )?;
            token::transfer_cash(
                &a.cash_token_program,
                &a.cash_escrow,
                &a.cash_mint,
                seller_cash,
                &a.escrow_authority,
                si.cash_amount,
                signer,
            )?;
        }
    }

    let triggered_by = a.triggered_by.key();
    let si = &mut ctx.accounts.settlement;
    si.status = SettlementStatus::Settled.as_u8();
    si.closed_at = now;
    emit_cpi!(Settled {
        fund: fund_key,
        seq: si.seq,
        kind: si.kind,
        units: si.units,
        cash_amount: si.cash_amount,
        seller: si.seller,
        buyer: si.buyer,
        nav_seq: si.nav_seq,
        triggered_by,
        external_ref: si.external_ref,
        slot: clock.slot,
        unix_ts: now,
    });
    Ok(())
}
