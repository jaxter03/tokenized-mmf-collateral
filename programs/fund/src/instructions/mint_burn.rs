//! `mint_for_subscription` (and, from US2, `burn_for_redemption`) (contracts/fund.md, F003
//! FR-019–FR-022, research R7, R9). Called only by `dvp_settlement` with its escrow PDA as signer;
//! `fund` also checks the settlement instruction itself, so a bug in the caller can't create
//! units for an unfunded subscription.

use anchor_lang::prelude::*;
use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use anchor_spl::token_2022::{self, MintTo};
use mmf_common::cash;
use mmf_common::enums::{FundStatus, SettlementKind, SettlementStatus};
use mmf_common::ids::{
    COMPLIANCE_HOOK_PROGRAM_ID, DVP_SETTLEMENT_PROGRAM_ID, TOKEN_2022_PROGRAM_ID,
};
use mmf_common::seeds;
use mmf_common::settlement;
use mmf_common::views::SettlementView;

use crate::errors::FundError;
use crate::events::SubscriptionMinted;
use crate::state::FundState;

#[event_cpi]
#[derive(Accounts)]
pub struct MintForSubscription<'info> {
    /// Must be dvp_settlement's escrow PDA `["escrow", fund_state]` (`UnauthorizedCaller`).
    pub escrow_authority: Signer<'info>,
    /// Also the mint authority.
    #[account(mut)]
    pub fund_state: Account<'info, FundState>,
    /// CHECK: must be `fund_state.mint`.
    #[account(mut)]
    pub mint: UncheckedAccount<'info>,
    /// CHECK: the escrow's unit account (Token-2022 ATA of the escrow PDA).
    #[account(mut)]
    pub unit_escrow: UncheckedAccount<'info>,
    /// CHECK: the escrow's cash account; must hold at least the subscription's cash (I5).
    pub cash_escrow: UncheckedAccount<'info>,
    /// CHECK: the SettlementInstruction; decoded with `SettlementView`.
    pub settlement: UncheckedAccount<'info>,
    /// CHECK: compliance_hook's Registry for this mint (passed to `accrue`).
    pub registry: UncheckedAccount<'info>,
    /// CHECK: the escrow's HolderState (settled by `accrue`).
    #[account(mut)]
    pub escrow_holder: UncheckedAccount<'info>,
    /// CHECK: compliance_hook program.
    pub compliance_hook_program: UncheckedAccount<'info>,
    /// CHECK: Token-2022.
    pub token_2022_program: UncheckedAccount<'info>,
}

/// Checks the SettlementInstruction passed to a mint or burn (research R7): owned by
/// dvp_settlement at its own PDA, for this fund, of `kind`, Affirmed, for exactly `units`.
/// Public so the tests can show it rejects forged accounts that no transaction can deliver.
pub fn check_settlement(
    settlement: &AccountInfo,
    fund_state: &Pubkey,
    kind: SettlementKind,
    units: u64,
) -> Result<()> {
    settlement_view(settlement, fund_state, kind, units).map(|_| ())
}

fn settlement_view(
    settlement: &AccountInfo,
    fund_state: &Pubkey,
    kind: SettlementKind,
    units: u64,
) -> Result<SettlementView> {
    let v = SettlementView::load(settlement).map_err(|_| error!(FundError::SettlementMismatch))?;
    require!(
        v.fund_state == *fund_state
            && v.kind == kind.as_u8()
            && v.status == SettlementStatus::Affirmed.as_u8()
            && v.units == units,
        FundError::SettlementMismatch
    );
    Ok(v)
}

/// Starts a new redemption day at the first mint or burn of each UTC day (research R9).
/// `units_outstanding` changes only through mint and burn, so this records the units outstanding
/// at the start of the day.
pub(crate) fn roll_day(fs: &mut FundState, now: i64) -> Result<()> {
    let start = settlement::day_start(now).ok_or(error!(FundError::Overflow))?;
    if fs.day_start_ts < start {
        fs.day_start_ts = start;
        fs.redeemed_today = 0;
        fs.units_at_day_start = fs.units_outstanding;
    }
    Ok(())
}

/// The caller is this fund's settlement escrow (shared-model §5.2).
fn require_escrow(signer: &Pubkey, fund_state: &Pubkey) -> Result<()> {
    let escrow = Pubkey::find_program_address(
        &[seeds::ESCROW, fund_state.as_ref()],
        &DVP_SETTLEMENT_PROGRAM_ID,
    )
    .0;
    require_keys_eq!(*signer, escrow, FundError::UnauthorizedCaller);
    Ok(())
}

/// The fund's mint, the escrow's unit account, and the two programs `fund` signs into. The
/// FundState PDA signs the `accrue` call and is the mint authority, so it is never handed to a
/// program other than compliance_hook and Token-2022.
fn require_unit_accounts(
    fs: &FundState,
    escrow: &Pubkey,
    mint: &AccountInfo,
    unit_escrow: &AccountInfo,
    hook_program: &AccountInfo,
    token_program: &AccountInfo,
) -> Result<()> {
    require!(
        mint.key() == fs.mint
            && unit_escrow.key()
                == get_associated_token_address_with_program_id(
                    escrow,
                    &fs.mint,
                    &TOKEN_2022_PROGRAM_ID
                )
            && hook_program.key() == COMPLIANCE_HOOK_PROGRAM_ID
            && token_program.key() == TOKEN_2022_PROGRAM_ID,
        FundError::SettlementMismatch
    );
    Ok(())
}

/// Balance of the escrow's unit account.
fn unit_balance(unit_escrow: &AccountInfo) -> Result<u64> {
    let data = unit_escrow.try_borrow_data()?;
    cash::token_account(unit_escrow.owner, &data)
        .map(|t| t.amount)
        .ok_or(error!(FundError::SettlementMismatch))
}

pub fn handle_mint_for_subscription(ctx: Context<MintForSubscription>, units: u64) -> Result<()> {
    let a = &ctx.accounts;
    let fund_key = a.fund_state.key();
    let escrow = a.escrow_authority.key();

    // 1. Only this fund's settlement escrow.
    require_escrow(&escrow, &fund_key)?;
    // 2. Only while Open.
    require!(
        a.fund_state.status == FundStatus::Open.as_u8(),
        FundError::FundNotOpen
    );
    // 3–4. An Affirmed subscription of this fund, for exactly these units.
    let si = settlement_view(
        &a.settlement,
        &fund_key,
        SettlementKind::Subscription,
        units,
    )?;
    require_unit_accounts(
        &a.fund_state,
        &escrow,
        &a.mint,
        &a.unit_escrow,
        &a.compliance_hook_program,
        &a.token_2022_program,
    )?;
    // 5. Its cash is in escrow (I5).
    let escrowed = {
        let data = a.cash_escrow.try_borrow_data()?;
        cash::token_account(a.cash_escrow.owner, &data)
            .filter(|t| t.owner == escrow && t.mint == a.fund_state.terms.cash_mint)
            .map(|t| t.amount)
    };
    require!(
        escrowed.is_some_and(|amount| amount >= si.cash_amount),
        FundError::CashNotEscrowed
    );
    // 6.
    let pre = unit_balance(&a.unit_escrow)?;
    let post = pre.checked_add(units).ok_or(error!(FundError::Overflow))?;
    let outstanding = a
        .fund_state
        .units_outstanding
        .checked_add(units)
        .ok_or(error!(FundError::Overflow))?;

    let clock = Clock::get()?;
    let mint_key = a.fund_state.mint;
    let bump = [a.fund_state.bump];
    let signer: &[&[&[u8]]] = &[&[seeds::FUND, mint_key.as_ref(), &bump]];

    // Settle the escrow's interest first (F002 FR-013), then create the units.
    compliance_hook::cpi::accrue(
        CpiContext::new_with_signer(
            COMPLIANCE_HOOK_PROGRAM_ID,
            compliance_hook::cpi::accounts::Accrue {
                fund_state: a.fund_state.to_account_info(),
                registry: a.registry.to_account_info(),
                holder_state: a.escrow_holder.to_account_info(),
                holding_account: a.unit_escrow.to_account_info(),
            },
            signer,
        ),
        post,
    )?;
    token_2022::mint_to(
        CpiContext::new_with_signer(
            TOKEN_2022_PROGRAM_ID,
            MintTo {
                mint: a.mint.to_account_info(),
                to: a.unit_escrow.to_account_info(),
                authority: a.fund_state.to_account_info(),
            },
            signer,
        ),
        units,
    )?;

    let fs = &mut ctx.accounts.fund_state;
    roll_day(fs, clock.unix_timestamp)?;
    fs.units_outstanding = outstanding;
    emit_cpi!(SubscriptionMinted {
        fund: fund_key,
        settlement: ctx.accounts.settlement.key(),
        units,
        units_outstanding: outstanding,
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}
