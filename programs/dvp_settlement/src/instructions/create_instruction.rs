//! `create_instruction` (contracts/dvp_settlement.md, FR-004–FR-010): a party agrees the terms of
//! one exchange. Creating does not fund or affirm anything (FR-005).

use anchor_lang::prelude::*;
use mmf_common::accrual::utc_day;
use mmf_common::cash;
use mmf_common::enums::{FundStatus, SettlementKind, SettlementStatus};
use mmf_common::ids::{DVP_SETTLEMENT_PROGRAM_ID, TOKEN_2022_PROGRAM_ID};
use mmf_common::seeds;
use mmf_common::settlement::{price_fresh, redemption_cash, subscription_cash};
use mmf_common::views::{FundStateView, HolderStateView, RoleRegistryView};

use crate::errors::DvpError;
use crate::events::SettlementCreated;
use crate::state::{DvpConfig, SettlementInstruction};

/// Terms of a new instruction (contracts/dvp_settlement.md `create_instruction`).
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreateArgs {
    /// `SettlementKind`: 0 Subscription, 1 Redemption, 2 SecondaryTransfer.
    pub kind: u8,
    /// Subscription: the fund's FundState; otherwise the unit owner.
    pub seller: Pubkey,
    /// Redemption: the fund's FundState; otherwise the unit receiver.
    pub buyer: Pubkey,
    pub units: u64,
    /// Secondary transfers only; 0 for subscriptions and redemptions (computed from the price).
    pub cash_amount: u64,
    /// Valid-from; 0 = now.
    pub not_before_ts: i64,
    /// 0 = now + the fund's default expiry.
    pub expires_at_ts: i64,
    /// The creator's own reference; part of the address (research R2).
    pub external_ref: [u8; 32],
}

#[event_cpi]
#[derive(Accounts)]
#[instruction(args: CreateArgs)]
pub struct CreateInstruction<'info> {
    pub creator: Signer<'info>,
    /// Pays the instruction's rent; any key (the creator may be a Squads vault).
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: decoded with `FundStateView` (owner and discriminator checked).
    pub fund_state: UncheckedAccount<'info>,
    /// CHECK: decoded with `RoleRegistryView` at the fund's `role_registry`.
    pub role_registry: UncheckedAccount<'info>,
    /// CHECK: PDA `["dvp", fund_state]`, read and written by hand so a missing config gives
    /// `EscrowNotReady` (FR-003).
    #[account(mut)]
    pub dvp_config: UncheckedAccount<'info>,
    /// CHECK: the escrow's HolderState; decoded with `HolderStateView`.
    pub escrow_holder: UncheckedAccount<'info>,
    /// CHECK: the unit escrow (escrow authority's Token-2022 ATA); must be thawed.
    pub unit_escrow: UncheckedAccount<'info>,
    /// CHECK: HolderState of `args.seller`; absent for a Subscription.
    pub seller_holder: Option<UncheckedAccount<'info>>,
    /// CHECK: HolderState of `args.buyer`; absent for a Redemption.
    pub buyer_holder: Option<UncheckedAccount<'info>>,
    #[account(
        init,
        payer = payer,
        space = 8 + SettlementInstruction::INIT_SPACE,
        seeds = [seeds::SETTLEMENT, fund_state.key().as_ref(), creator.key().as_ref(), &args.external_ref],
        bump,
    )]
    pub settlement: Account<'info, SettlementInstruction>,
    pub system_program: Program<'info, System>,
}

/// The settlement config for `fund_state`, if set up: owned by this program at its PDA.
fn load_config(info: &AccountInfo, fund_state: &Pubkey) -> Option<DvpConfig> {
    let expected = Pubkey::find_program_address(&[seeds::DVP, fund_state.as_ref()], &crate::ID).0;
    if info.key != &expected || info.owner != &crate::ID {
        return None;
    }
    let data = info.try_borrow_data().ok()?;
    let cfg = DvpConfig::try_deserialize(&mut &data[..]).ok()?;
    (cfg.fund_state == *fund_state).then_some(cfg)
}

/// An investor party's record is present and eligible (research R12).
fn eligible(holder: Option<&UncheckedAccount>, mint: &Pubkey, party: &Pubkey, now: i64) -> bool {
    holder
        .and_then(|h| HolderStateView::load(h, mint, party).ok())
        .is_some_and(|v| v.eligible(now))
}

pub fn handle_create_instruction(ctx: Context<CreateInstruction>, args: CreateArgs) -> Result<()> {
    let a = &ctx.accounts;
    let fund_key = a.fund_state.key();
    let creator = a.creator.key();

    // 1. FundState and its RoleRegistry belong together.
    let fund = FundStateView::load(&a.fund_state, &fund_key)
        .map_err(|_| error!(DvpError::FundMismatch))?;
    let roles = RoleRegistryView::load(&a.role_registry, &fund.role_registry)
        .map_err(|_| error!(DvpError::FundMismatch))?;
    require_keys_eq!(roles.fund_state, fund_key, DvpError::FundMismatch);

    // 2. Only while the fund is Open.
    require!(
        fund.status == FundStatus::Open.as_u8(),
        DvpError::FundNotOpen
    );

    // 3. Settlement is set up: config, an Approved escrow holder, a thawed unit escrow (FR-003).
    let mut cfg = load_config(&a.dvp_config, &fund_key).ok_or(error!(DvpError::EscrowNotReady))?;
    let escrow = Pubkey::create_program_address(
        &[seeds::ESCROW, fund_key.as_ref(), &[cfg.escrow_bump]],
        &DVP_SETTLEMENT_PROGRAM_ID,
    )
    .map_err(|_| error!(DvpError::EscrowNotReady))?;
    let escrow_ok = HolderStateView::load(&a.escrow_holder, &fund.mint, &escrow)
        .is_ok_and(|v| v.is_approved_escrow());
    let unit_escrow_ok = a.unit_escrow.key()
        == anchor_spl::associated_token::get_associated_token_address_with_program_id(
            &escrow,
            &fund.mint,
            &TOKEN_2022_PROGRAM_ID,
        )
        && {
            let data = a.unit_escrow.try_borrow_data()?;
            cash::token_account(a.unit_escrow.owner, &data).is_some_and(|t| !t.frozen)
        };
    require!(escrow_ok && unit_escrow_ok, DvpError::EscrowNotReady);

    // 4. A known kind, with the Fund exactly where the kind puts it.
    let kind = SettlementKind::try_from(args.kind).map_err(|_| error!(DvpError::InvalidKind))?;
    let shape_ok = match kind {
        SettlementKind::Subscription => args.seller == fund_key && args.buyer != fund_key,
        SettlementKind::Redemption => args.buyer == fund_key && args.seller != fund_key,
        SettlementKind::SecondaryTransfer => args.seller != fund_key && args.buyer != fund_key,
    };
    require!(shape_ok, DvpError::InvalidKind);

    // 5. Who may create (Clarification Q1).
    let allowed = match kind {
        SettlementKind::Subscription => creator == args.buyer || creator == roles.transfer_agent,
        SettlementKind::Redemption => creator == args.seller || creator == roles.transfer_agent,
        SettlementKind::SecondaryTransfer => creator == args.seller || creator == args.buyer,
    };
    require!(allowed, DvpError::Unauthorized);

    // 6–8.
    require!(args.units > 0, DvpError::ZeroUnits);
    require!(args.external_ref != [0; 32], DvpError::ZeroExternalRef);
    require!(
        kind != SettlementKind::SecondaryTransfer || args.seller != args.buyer,
        DvpError::SameParty
    );

    // 9–11. Price and cash (FR-007, FR-008; shared-model §8 rounding).
    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    let cash_amount = match kind {
        SettlementKind::Subscription | SettlementKind::Redemption => {
            require!(fund.nav_seq > 0, DvpError::NoPrice);
            let today = utc_day(now).ok_or(error!(DvpError::Overflow))?;
            require!(
                price_fresh(fund.nav_seq, fund.nav_period_end_day, today),
                DvpError::StalePrice
            );
            require!(args.cash_amount == 0, DvpError::CashAmountNotAllowed);
            let cash = if kind == SettlementKind::Subscription {
                subscription_cash(args.units, fund.nav)
            } else {
                redemption_cash(args.units, fund.nav)
            };
            cash.ok_or(error!(DvpError::Overflow))?
        }
        SettlementKind::SecondaryTransfer => args.cash_amount,
    };
    require!(cash_amount > 0, DvpError::ZeroCash);

    // 12. Window (FR-009, FR-010).
    let not_before_ts = if args.not_before_ts == 0 {
        now
    } else {
        args.not_before_ts
    };
    let expires_at_ts = if args.expires_at_ts == 0 {
        now.checked_add(i64::from(cfg.default_expiry_secs))
            .ok_or(error!(DvpError::Overflow))?
    } else {
        args.expires_at_ts
    };
    require!(
        expires_at_ts > now && expires_at_ts > not_before_ts,
        DvpError::InvalidExpiry
    );

    // 13. Every investor party is eligible now.
    for (party, holder) in [
        (args.seller, a.seller_holder.as_ref()),
        (args.buyer, a.buyer_holder.as_ref()),
    ] {
        if party != fund_key {
            require!(
                eligible(holder, &fund.mint, &party, now),
                DvpError::PartyNotEligible
            );
        }
    }

    // 14. Numbering.
    let seq = cfg.next_seq;
    cfg.next_seq = seq.checked_add(1).ok_or(error!(DvpError::Overflow))?;
    {
        let mut data = a.dvp_config.try_borrow_mut_data()?;
        let mut out: &mut [u8] = &mut data;
        cfg.try_serialize(&mut out)?;
    }

    let address = ctx.accounts.settlement.key();
    let si = &mut ctx.accounts.settlement;
    si.version = SettlementInstruction::VERSION;
    si.fund_state = fund_key;
    si.seq = seq;
    si.kind = args.kind;
    si.seller = args.seller;
    si.buyer = args.buyer;
    si.units = args.units;
    si.cash_amount = cash_amount;
    si.nav_seq = fund.nav_seq;
    si.not_before_ts = not_before_ts;
    si.expires_at_ts = expires_at_ts;
    si.status = SettlementStatus::Instructed.as_u8();
    si.seller_funded_at = 0;
    si.buyer_funded_at = 0;
    si.external_ref = args.external_ref;
    si.created_by = creator;
    si.created_at = now;
    si.closed_at = 0;
    si.cancel_requests = 0;
    si.units_to_return = 0;
    si.cash_to_return = 0;
    si.bump = ctx.bumps.settlement;

    emit_cpi!(SettlementCreated {
        fund: fund_key,
        seq,
        address,
        kind: args.kind,
        seller: args.seller,
        buyer: args.buyer,
        units: args.units,
        cash_amount,
        nav_seq: fund.nav_seq,
        not_before_ts,
        expires_at_ts,
        external_ref: args.external_ref,
        created_by: creator,
        slot: clock.slot,
        unix_ts: now,
    });
    Ok(())
}
