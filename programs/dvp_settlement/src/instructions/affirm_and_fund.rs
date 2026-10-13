//! `affirm_and_fund(side)` (contracts/dvp_settlement.md, FR-011–FR-014): one side places its leg
//! in escrow. For a subscription, the Fund side (the transfer agent) only affirms: its units are
//! created at settlement.

use anchor_lang::prelude::*;
use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use mmf_common::cash;
use mmf_common::enums::{SettlementKind, SettlementStatus, Side};
use mmf_common::seeds;
use mmf_common::views::{FundStateView, HolderStateView, RoleRegistryView};

use crate::errors::DvpError;
use crate::events::LegFunded;
use crate::state::{DvpConfig, SettlementInstruction};
use crate::token;

#[event_cpi]
#[derive(Accounts)]
pub struct AffirmAndFund<'info> {
    /// The side's party: the transfer agent for the Fund side, otherwise the seller or buyer.
    pub party: Signer<'info>,
    /// CHECK: decoded with `FundStateView`.
    pub fund_state: UncheckedAccount<'info>,
    /// CHECK: decoded with `RoleRegistryView` at the fund's `role_registry`.
    pub role_registry: UncheckedAccount<'info>,
    #[account(seeds = [seeds::DVP, fund_state.key().as_ref()], bump = dvp_config.bump)]
    pub dvp_config: Account<'info, DvpConfig>,
    #[account(
        mut,
        seeds = [seeds::SETTLEMENT, fund_state.key().as_ref(), settlement.created_by.as_ref(), &settlement.external_ref],
        bump = settlement.bump,
    )]
    pub settlement: Account<'info, SettlementInstruction>,
    /// CHECK: HolderState of the investor party; absent for the transfer agent's side. Writable:
    /// the hook updates it on a unit leg.
    #[account(mut)]
    pub party_holder: Option<UncheckedAccount<'info>>,
    /// CHECK: what moves into escrow: the party's cash account, the treasury cash account
    /// (redemption), or the seller's holding account (unit leg). Absent for the Fund's
    /// affirmation of a subscription.
    #[account(mut)]
    pub source: Option<UncheckedAccount<'info>>,
    /// CHECK: the escrow account the leg goes to (cash escrow or unit escrow).
    #[account(mut)]
    pub destination: Option<UncheckedAccount<'info>>,
    /// CHECK: the leg's mint (cash mint or fund mint).
    pub leg_mint: Option<UncheckedAccount<'info>>,
    /// CHECK: the leg's token program (the cash mint's owner, or Token-2022).
    pub token_program: Option<UncheckedAccount<'info>>,
    // --- unit leg only: the hook's extra accounts (shared-model §4.1) ---
    /// CHECK: the fund's Registry (writable: the hook maintains `holder_count`).
    #[account(mut)]
    pub registry: Option<UncheckedAccount<'info>>,
    /// CHECK: the escrow's HolderState (the hook's destination holder).
    #[account(mut)]
    pub escrow_holder: Option<UncheckedAccount<'info>>,
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

pub fn handle_affirm_and_fund(ctx: Context<AffirmAndFund>, side: u8) -> Result<()> {
    let a = &ctx.accounts;
    let fund_key = a.fund_state.key();
    let si = &a.settlement;

    // 1. Accounts belong to one fund.
    let fund = FundStateView::load(&a.fund_state, &fund_key)
        .map_err(|_| error!(DvpError::FundMismatch))?;
    let roles = RoleRegistryView::load(&a.role_registry, &fund.role_registry)
        .map_err(|_| error!(DvpError::FundMismatch))?;
    require!(
        roles.fund_state == fund_key && si.fund_state == fund_key,
        DvpError::FundMismatch
    );

    // 2–6.
    let side = Side::try_from(side).map_err(|_| error!(DvpError::InvalidSide))?;
    require!(si.status().is_open(), DvpError::InstructionFinal);
    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    require!(now < si.expires_at_ts, DvpError::InstructionExpired);
    let party = a.party.key();
    require_keys_eq!(
        party,
        si.side_party(side, &roles.transfer_agent),
        DvpError::Unauthorized
    );
    let funded_at = match side {
        Side::Seller => si.seller_funded_at,
        Side::Buyer => si.buyer_funded_at,
    };
    require!(funded_at == 0, DvpError::LegAlreadyFunded);

    // 7. An investor party must still be eligible (research R12).
    let kind = si.kind();
    let fund_side = matches!(
        (side, kind),
        (Side::Seller, SettlementKind::Subscription) | (Side::Buyer, SettlementKind::Redemption)
    );
    if !fund_side {
        let ok = a
            .party_holder
            .as_ref()
            .and_then(|h| HolderStateView::load(h, &fund.mint, &party).ok())
            .is_some_and(|v| v.eligible(now));
        require!(ok, DvpError::PartyNotEligible);
    }

    // The leg itself.
    let escrow = Pubkey::create_program_address(
        &[
            seeds::ESCROW,
            fund_key.as_ref(),
            &[a.dvp_config.escrow_bump],
        ],
        &crate::ID,
    )
    .map_err(|_| error!(DvpError::FundMismatch))?;
    let amount = match (side, kind) {
        // The Fund's affirmation: its units are created at settlement.
        (Side::Seller, SettlementKind::Subscription) => 0,
        // An investor's cash.
        (Side::Buyer, SettlementKind::Subscription | SettlementKind::SecondaryTransfer) => {
            // 9. The cash mint's own accounts, into the cash escrow.
            let mint = present(&a.leg_mint)?;
            let program = present(&a.token_program)?;
            let source = present(&a.source)?;
            let destination = present(&a.destination)?;
            require!(
                mint.key() == fund.cash_mint && program.key() == *mint.owner,
                DvpError::WrongAccount
            );
            require_keys_eq!(
                destination.key(),
                get_associated_token_address_with_program_id(&escrow, &fund.cash_mint, program.key),
                DvpError::WrongAccount
            );
            let source_ok = {
                let data = source.try_borrow_data()?;
                source.owner == program.key
                    && cash::token_account(source.owner, &data)
                        .is_some_and(|t| t.mint == fund.cash_mint)
            };
            require!(source_ok, DvpError::WrongAccount);
            token::transfer_cash(
                program,
                source,
                mint,
                destination,
                a.party.as_ref(),
                si.cash_amount,
                &[],
            )?;
            si.cash_amount
        }
        // The redemption's cash from the treasury: T048 (US2).
        (Side::Buyer, SettlementKind::Redemption) => return err!(DvpError::InvalidSide),
        // A unit leg: T047 (US2).
        (Side::Seller, SettlementKind::Redemption | SettlementKind::SecondaryTransfer) => {
            return err!(DvpError::InvalidSide)
        }
    };

    let si = &mut ctx.accounts.settlement;
    match side {
        Side::Seller => si.seller_funded_at = now,
        Side::Buyer => si.buyer_funded_at = now,
    }
    let status = match (si.seller_funded_at != 0, si.buyer_funded_at != 0) {
        (true, true) => SettlementStatus::Affirmed,
        (true, false) => SettlementStatus::SellerFunded,
        _ => SettlementStatus::BuyerFunded,
    };
    si.status = status.as_u8();

    emit_cpi!(LegFunded {
        fund: fund_key,
        seq: si.seq,
        side: side.as_u8(),
        amount,
        status_after: si.status,
        signer: party,
        external_ref: si.external_ref,
        slot: clock.slot,
        unix_ts: now,
    });
    Ok(())
}
