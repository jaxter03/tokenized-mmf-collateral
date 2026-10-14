//! `register_holder` (contracts/compliance_hook.md, FR-013, FR-014): creates a Pending holder
//! record for an owner account.

use anchor_lang::prelude::*;
use anchor_lang::system_program;
use mmf_common::enums::{FundStatus, HolderClass, HolderStatus};
use mmf_common::ids::{DVP_SETTLEMENT_PROGRAM_ID, FUND_PROGRAM_ID};
use mmf_common::seeds;
use mmf_common::views::{FundStateView, RoleRegistryView};

use crate::errors::HookError;
use crate::events::HolderRegistered;
use crate::state::{HolderState, Registry};

#[event_cpi]
#[derive(Accounts)]
#[instruction(owner: Pubkey)]
pub struct RegisterHolder<'info> {
    /// The owner itself (Investor class only) or compliance (any class).
    pub authority: Signer<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub registry: Account<'info, Registry>,
    /// CHECK: decoded with `FundStateView` at `registry.fund_state`.
    pub fund_state: UncheckedAccount<'info>,
    /// CHECK: decoded with `RoleRegistryView` at `registry.role_registry`.
    pub role_registry: UncheckedAccount<'info>,
    /// CHECK: PDA `["holder", mint, owner]`; created by hand in the handler so an existing record
    /// gives `HolderAlreadyRegistered` and a pre-funded address can still be registered.
    #[account(mut, seeds = [seeds::HOLDER, registry.mint.as_ref(), owner.as_ref()], bump)]
    pub holder_state: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

pub fn handle_register_holder(
    ctx: Context<RegisterHolder>,
    owner: Pubkey,
    class: u8,
) -> Result<()> {
    let reg = &ctx.accounts.registry;
    let authority = ctx.accounts.authority.key();

    // 1. Only while the fund is Open (a missing fund fails closed).
    let open = FundStateView::load(&ctx.accounts.fund_state, &reg.fund_state)
        .map(|f| f.status == FundStatus::Open.as_u8())
        .unwrap_or(false);
    require!(open, HookError::FundNotOpen);

    // 2. A class that may hold units (CollateralVault is backlog).
    let class = HolderClass::try_from(class)
        .ok()
        .filter(|c| c.transfer_allowed())
        .ok_or(error!(HookError::InvalidClass))?;

    // 3. Compliance, or the owner itself.
    let roles = RoleRegistryView::load(&ctx.accounts.role_registry, &reg.role_registry)
        .map_err(|_| error!(HookError::Unauthorized))?;
    let is_compliance = authority == roles.compliance;
    require!(is_compliance || authority == owner, HookError::Unauthorized);

    // 4. An institution may register itself only as an investor (Clarification Q1).
    require!(
        is_compliance || class == HolderClass::Investor,
        HookError::SelfRegistrationClassNotAllowed
    );

    // 5–6. Fund-operated accounts must be the system's own PDA (FR-019, research R17; F003
    // enables the settlement escrow).
    if let Some(system_owner) = system_owner(class, &reg.fund_state) {
        require_keys_eq!(owner, system_owner, HookError::NotSystemControlled);
    }

    // 7. One record per owner, ever (FR-014; a revoked record stays and blocks re-registration).
    let record = &ctx.accounts.holder_state;
    require!(
        record.data_is_empty() && record.owner == &system_program::ID,
        HookError::HolderAlreadyRegistered
    );

    let mint = reg.mint;
    let bump = ctx.bumps.holder_state;
    create_record(&ctx, &mint, &owner, bump)?;

    let state = HolderState {
        version: HolderState::VERSION,
        mint,
        owner,
        status: HolderStatus::Pending.as_u8(),
        class: class.as_u8(),
        jurisdiction: [0; 2],
        investor_type: 0,
        kyc_expiry_ts: 0,
        attestation_hash: [0; 32],
        attested_by: Pubkey::default(),
        pledged_amount: 0,
        active_liens: 0,
        index_at_last_touch: 0,
        accrued_e18: 0,
        last_balance: 0,
        last_slot: 0,
        last_paid_cycle: 0,
        bump,
        _reserved: [0; 64],
    };
    {
        let mut data = record.try_borrow_mut_data()?;
        state.try_serialize(&mut &mut data[..])?;
    }

    let clock = Clock::get()?;
    emit_cpi!(HolderRegistered {
        fund: reg.fund_state,
        owner,
        class: class.as_u8(),
        signer: authority,
        slot: clock.slot,
        unix_ts: clock.unix_timestamp,
    });
    Ok(())
}

/// Allocates the record at its PDA. Works even if someone already sent lamports to the address
/// (`create_account` refuses a funded address, so allocate + assign is used instead).
fn create_record(
    ctx: &Context<RegisterHolder>,
    mint: &Pubkey,
    owner: &Pubkey,
    bump: u8,
) -> Result<()> {
    let record = ctx.accounts.holder_state.to_account_info();
    let payer = ctx.accounts.payer.to_account_info();
    let sys = ctx.accounts.system_program.key();
    let space = 8 + HolderState::INIT_SPACE;
    let needed = Rent::get()?.minimum_balance(space);
    let bump = [bump];
    let signer: &[&[&[u8]]] = &[&[seeds::HOLDER, mint.as_ref(), owner.as_ref(), &bump]];

    if record.lamports() == 0 {
        return system_program::create_account(
            CpiContext::new(
                sys,
                system_program::CreateAccount {
                    from: payer,
                    to: record,
                },
            )
            .with_signer(signer),
            needed,
            space as u64,
            &crate::ID,
        );
    }
    let top_up = needed.saturating_sub(record.lamports());
    if top_up > 0 {
        system_program::transfer(
            CpiContext::new(
                sys,
                system_program::Transfer {
                    from: payer,
                    to: record.clone(),
                },
            ),
            top_up,
        )?;
    }
    system_program::allocate(
        CpiContext::new(
            sys,
            system_program::Allocate {
                account_to_allocate: record.clone(),
            },
        )
        .with_signer(signer),
        space as u64,
    )?;
    system_program::assign(
        CpiContext::new(
            sys,
            system_program::Assign {
                account_to_assign: record,
            },
        )
        .with_signer(signer),
        &crate::ID,
    )
}

/// The only owner a fund-operated class may have: the fund treasury PDA `["treasury",
/// fund_state]` of `fund`, or the settlement escrow PDA `["escrow", fund_state]` of
/// `dvp_settlement` (F003). `None` for investors.
pub(crate) fn system_owner(class: HolderClass, fund_state: &Pubkey) -> Option<Pubkey> {
    let (seed, program) = match class {
        HolderClass::FundTreasury => (seeds::TREASURY, FUND_PROGRAM_ID),
        HolderClass::Escrow => (seeds::ESCROW, DVP_SETTLEMENT_PROGRAM_ID),
        HolderClass::Investor | HolderClass::CollateralVault => return None,
    };
    Some(Pubkey::find_program_address(&[seed, fund_state.as_ref()], &program).0)
}
