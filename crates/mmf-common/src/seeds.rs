//! PDA seeds (docs/shared-model.md §4, §4.5).

/// fund: `["fund", mint]` → FundState (mint authority).
pub const FUND: &[u8] = b"fund";
/// governance: `["roles", fund_state]` → RoleRegistry.
pub const ROLES: &[u8] = b"roles";
/// compliance_hook: `["registry", mint]` → Registry (freeze authority).
pub const REGISTRY: &[u8] = b"registry";
/// compliance_hook: `["holder", mint, owner]` → HolderState.
pub const HOLDER: &[u8] = b"holder";
/// compliance_hook: `["extra-account-metas", mint]` → ExtraAccountMetaList.
pub const EXTRA_ACCOUNT_METAS: &[u8] = b"extra-account-metas";
/// governance: `["delegate", mint]` → permanent delegate.
pub const DELEGATE: &[u8] = b"delegate";
/// governance: `["token-admin", mint]` → transfer hook authority.
pub const TOKEN_ADMIN: &[u8] = b"token-admin";
/// governance: `["hook-admin"]` → signs compliance_hook set_status / set_paused.
pub const HOOK_ADMIN: &[u8] = b"hook-admin";
/// fund: `["treasury", fund_state]` → owner of the fund treasury holding account.
pub const TREASURY: &[u8] = b"treasury";
/// fund: `["nav", fund_state, seq.to_le_bytes()]` → NavRecord. [F002]
pub const NAV: &[u8] = b"nav";
/// governance: `["fund-caller"]` → signs fund `set_nav_override` / `clear_nav_override`. [F002]
pub const FUND_CALLER: &[u8] = b"fund-caller";
/// dvp_settlement: `["dvp", fund_state]` → DvpConfig. [F003]
pub const DVP: &[u8] = b"dvp";
/// dvp_settlement: `["escrow", fund_state]` → escrow authority (owns the unit and cash escrow). [F003]
pub const ESCROW: &[u8] = b"escrow";
/// dvp_settlement: `["settlement", fund_state, created_by, external_ref]` → SettlementInstruction. [F003]
pub const SETTLEMENT: &[u8] = b"settlement";
/// governance: `["dvp-caller"]` → signs `dvp_settlement::reject`. [F003]
pub const DVP_CALLER: &[u8] = b"dvp-caller";
