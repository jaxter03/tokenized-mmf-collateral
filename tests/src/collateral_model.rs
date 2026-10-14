//! Independent reference model of collateral (F004 research R15), the oracle for the property
//! tests (T069; SC-002, SC-003, SC-005) and for the invariant checks I1, I2, I15 and I16.
//!
//! Written from the spec and the contracts' check order. It shares **no** code with the programs:
//! no `mmf_common::collateral`, no program types, its own formulas and its own `observe`. Every
//! operation returns `Ok` or the name of the error the programs must report first: the variant
//! name in `CollateralError` or `HookError`, `AlreadyInUse` for a second account at the same
//! address (Anchor `init`, checked before the handler runs), or `InsufficientFunds` for a
//! transfer larger than the balance (Token-2022, before the hook).
//!
//! Agreements are created Active (propose and sign are covered by `f004_agreement.rs`). Not
//! modelled: paused transfers, interest and the holder cap.

use std::collections::HashSet;

pub type ModelResult<T = ()> = Result<T, &'static str>;

const PRICE_SCALE: u128 = 100_000_000;
const BPS: u128 = 10_000;

/// Who signs a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Actor {
    Giver,
    Taker,
    Keeper,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallKind {
    Delivery,
    Return,
}

/// Stored call status (the derived Met rule is `Model::call_status`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallStatus {
    Open,
    Met,
    Disputed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PledgeStatus {
    Active,
    Released,
    Substituted,
}

#[derive(Clone, Debug)]
pub struct Holder {
    pub balance: u64,
    pub locked: u64,
    pub approved: bool,
}

#[derive(Clone, Debug)]
pub struct Terms {
    pub haircut_bps: u16,
    pub min_transfer_value: u64,
    pub cap_units: u64,
    pub max_age_secs: u32,
    pub cure_secs: u32,
    pub grace_secs: u32,
    pub unilateral: bool,
}

#[derive(Clone, Debug)]
pub struct Agreement {
    pub giver: usize,
    pub taker: usize,
    pub terms: Terms,
    pub required: u64,
    pub pending: Option<(u64, [u8; 32])>,
    pub pledged: u64,
    pub demanded: u64,
    pub open_delivery: u16,
    pub met_below: u64,
    pub call_count: u64,
    pub pledge_count: u64,
    pub open_return: Option<usize>,
    pub pledge_refs: HashSet<[u8; 32]>,
    pub call_refs: HashSet<[u8; 32]>,
}

#[derive(Clone, Debug)]
pub struct Pledge {
    pub agreement: usize,
    pub seq: u64,
    pub units: u64,
    pub released: u64,
    pub status: PledgeStatus,
}

impl Pledge {
    pub fn remaining(&self) -> u64 {
        self.units - self.released
    }
}

#[derive(Clone, Debug)]
pub struct Call {
    pub agreement: usize,
    pub seq: u64,
    pub kind: CallKind,
    pub value: u64,
    pub units_required: u64,
    pub issued_at: i64,
    pub deadline: i64,
    pub enforce_after: i64,
    pub status: CallStatus,
    pub units_released: u64,
}

#[derive(Clone, Debug)]
pub struct Model {
    pub holders: Vec<Holder>,
    pub agreements: Vec<Agreement>,
    pub pledges: Vec<Pledge>,
    pub calls: Vec<Call>,
    pub nav: u64,
    pub nav_seq: u64,
    pub nav_ts: i64,
    pub now: i64,
}

// --- formulas (own copies of shared-model §8) ---

pub fn value_of(units: u64, nav: u64, haircut_bps: u16) -> Option<u64> {
    let p = (units as u128) * (nav as u128) * (BPS - haircut_bps as u128);
    u64::try_from(p / (PRICE_SCALE * BPS)).ok()
}

pub fn units_up(value: u64, nav: u64, haircut_bps: u16) -> Option<u64> {
    let num = (value as u128) * PRICE_SCALE * BPS;
    let den = (nav as u128) * (BPS - haircut_bps as u128);
    u64::try_from(num.div_ceil(den)).ok()
}

pub fn units_down(value: u64, nav: u64, haircut_bps: u16) -> Option<u64> {
    let num = (value as u128) * PRICE_SCALE * BPS;
    let den = (nav as u128) * (BPS - haircut_bps as u128);
    u64::try_from(num / den).ok()
}

impl Model {
    pub fn new(balances: &[u64], nav: u64, now: i64) -> Self {
        Self {
            holders: balances
                .iter()
                .map(|&b| Holder {
                    balance: b,
                    locked: 0,
                    approved: true,
                })
                .collect(),
            agreements: Vec::new(),
            pledges: Vec::new(),
            calls: Vec::new(),
            nav,
            nav_seq: 1,
            nav_ts: now,
            now,
        }
    }

    /// An Active agreement (propose + sign succeeded on chain).
    pub fn add_agreement(&mut self, giver: usize, taker: usize, terms: Terms) -> usize {
        self.agreements.push(Agreement {
            giver,
            taker,
            terms,
            required: 0,
            pending: None,
            pledged: 0,
            demanded: 0,
            open_delivery: 0,
            met_below: 0,
            call_count: 1,
            pledge_count: 1,
            open_return: None,
            pledge_refs: HashSet::new(),
            call_refs: HashSet::new(),
        });
        self.agreements.len() - 1
    }

    // --- environment ---

    pub fn advance(&mut self, secs: i64) {
        self.now += secs;
    }

    pub fn publish(&mut self, nav: u64) {
        self.nav = nav;
        self.nav_seq += 1;
        self.nav_ts = self.now;
    }

    pub fn set_approved(&mut self, holder: usize, approved: bool) {
        self.holders[holder].approved = approved;
    }

    /// A transfer through the hook: Token-2022 balance check, then hook steps 4, 5, 6.
    pub fn transfer(&mut self, from: usize, to: usize, units: u64) -> ModelResult {
        let src = &self.holders[from];
        if units > src.balance {
            return Err("InsufficientFunds");
        }
        if !src.approved {
            return Err("SenderNotApproved");
        }
        if src.balance - units < src.locked {
            return Err("PledgedUnitsLocked");
        }
        if !self.holders[to].approved {
            return Err("ReceiverNotApproved");
        }
        if from != to {
            self.holders[from].balance -= units;
            self.holders[to].balance += units;
        }
        Ok(())
    }

    // --- valuation ---

    fn fresh(&self, a: usize) -> ModelResult {
        if self.nav_seq == 0 {
            return Err("NoPrice");
        }
        if self.now - self.nav_ts > i64::from(self.agreements[a].terms.max_age_secs) {
            return Err("StalePrice");
        }
        Ok(())
    }

    /// Coverage at the current price with `pledged` units and `required` exposure.
    fn coverage_with(&self, a: usize, pledged: u64, required: u64) -> ModelResult<i128> {
        let t = &self.agreements[a].terms;
        let v = value_of(pledged, self.nav, t.haircut_bps).ok_or("Overflow")?;
        Ok(v as i128 - required as i128)
    }

    pub fn coverage(&self, a: usize) -> ModelResult<i128> {
        let ag = &self.agreements[a];
        self.coverage_with(a, ag.pledged, ag.required)
    }

    fn shortfall(&self, a: usize) -> ModelResult<u64> {
        Ok((-self.coverage(a)?).max(0) as u64)
    }

    /// Research R5: cap the demanded part at the shortfall; at zero, every call so far is Met.
    fn observe_with(&mut self, a: usize, s: u64) {
        let ag = &mut self.agreements[a];
        if s == 0 {
            ag.demanded = 0;
            ag.open_delivery = 0;
            ag.met_below = ag.call_count;
        } else {
            ag.demanded = ag.demanded.min(s);
        }
    }

    fn observe(&mut self, a: usize) -> ModelResult {
        let s = self.shortfall(a)?;
        self.observe_with(a, s);
        Ok(())
    }

    /// Stored status, with Open/Disputed delivery calls below `met_below` reported as Met.
    pub fn call_status(&self, c: usize) -> CallStatus {
        let call = &self.calls[c];
        let ag = &self.agreements[call.agreement];
        if call.kind == CallKind::Delivery
            && matches!(call.status, CallStatus::Open | CallStatus::Disputed)
            && call.seq < ag.met_below
        {
            CallStatus::Met
        } else {
            call.status
        }
    }

    fn settle_status(&mut self, c: usize) {
        if self.call_status(c) == CallStatus::Met {
            self.calls[c].status = CallStatus::Met;
        }
    }

    fn eligible(&self, holder: usize) -> bool {
        self.holders[holder].approved
    }

    fn open_delivery_call(&mut self, a: usize, value: u64, external_ref: [u8; 32]) -> usize {
        let haircut = self.agreements[a].terms.haircut_bps;
        let (cure, grace) = {
            let t = &self.agreements[a].terms;
            (i64::from(t.cure_secs), i64::from(t.grace_secs))
        };
        let ag = &mut self.agreements[a];
        let seq = ag.call_count;
        ag.call_count += 1;
        ag.open_delivery += 1;
        ag.demanded += value;
        ag.call_refs.insert(external_ref);
        let deadline = self.now + cure;
        self.calls.push(Call {
            agreement: a,
            seq,
            kind: CallKind::Delivery,
            value,
            units_required: units_up(value, self.nav, haircut).expect("fits"),
            issued_at: self.now,
            deadline,
            enforce_after: deadline + grace,
            status: CallStatus::Open,
            units_released: 0,
        });
        self.calls.len() - 1
    }

    /// Applies a raise at a fresh price: observe, set the exposure, open the call if needed.
    /// Returns the opened call's index.
    fn apply_raise(&mut self, a: usize, value: u64, external_ref: [u8; 32]) -> ModelResult<Option<usize>> {
        self.fresh(a)?;
        let before = self.clone();
        self.observe(a)?;
        self.agreements[a].required = value;
        self.agreements[a].pending = None;
        let s = self.shortfall(a)?;
        let uncalled = s - self.agreements[a].demanded;
        if uncalled > self.agreements[a].terms.min_transfer_value {
            if self.agreements[a].call_refs.contains(&external_ref) {
                *self = before;
                return Err("CallRefInUse");
            }
            return Ok(Some(self.open_delivery_call(a, uncalled, external_ref)));
        }
        Ok(None)
    }

    // --- instructions ---

    /// `update_exposure`, signed by the taker. Returns the call it opened, if any.
    pub fn update_exposure(&mut self, a: usize, value: u64, external_ref: [u8; 32]) -> ModelResult<Option<usize>> {
        if external_ref == [0; 32] {
            return Err("ZeroExternalRef");
        }
        let ag = &self.agreements[a];
        if value <= ag.required {
            let fresh = self.fresh(a).is_ok();
            let ag = &mut self.agreements[a];
            ag.required = value;
            ag.pending = None;
            if fresh {
                self.observe(a)?;
            } else if value == 0 {
                self.observe_with(a, 0);
            }
            return Ok(None);
        }
        if !self.eligible(ag.taker) {
            return Err("PartyNotEligible");
        }
        if ag.terms.unilateral {
            return self.apply_raise(a, value, external_ref);
        }
        self.agreements[a].pending = Some((value, external_ref));
        Ok(None)
    }

    /// `confirm_exposure`, signed by the giver.
    pub fn confirm_exposure(&mut self, a: usize, value: u64, external_ref: [u8; 32]) -> ModelResult<Option<usize>> {
        let ag = &self.agreements[a];
        let Some(pending) = ag.pending else {
            return Err("NoPendingRaise");
        };
        if pending != (value, external_ref) {
            return Err("PendingRaiseMismatch");
        }
        if !self.eligible(ag.giver) {
            return Err("PartyNotEligible");
        }
        self.apply_raise(a, value, external_ref)
    }

    /// `issue_call`: Delivery by the taker, Return by the giver.
    pub fn issue_call(&mut self, a: usize, kind: CallKind, external_ref: [u8; 32]) -> ModelResult<usize> {
        if self.agreements[a].call_refs.contains(&external_ref) {
            return Err("AlreadyInUse");
        }
        if external_ref == [0; 32] {
            return Err("ZeroExternalRef");
        }
        let signer = match kind {
            CallKind::Delivery => self.agreements[a].taker,
            CallKind::Return => self.agreements[a].giver,
        };
        if !self.eligible(signer) {
            return Err("PartyNotEligible");
        }
        self.fresh(a)?;
        let before = self.clone();
        self.observe(a)?;
        let mta = self.agreements[a].terms.min_transfer_value;
        match kind {
            CallKind::Delivery => {
                let uncalled = self.shortfall(a)? - self.agreements[a].demanded;
                if uncalled <= mta {
                    *self = before;
                    return Err("NothingToCall");
                }
                Ok(self.open_delivery_call(a, uncalled, external_ref))
            }
            CallKind::Return => {
                if self.agreements[a].open_return.is_some() {
                    *self = before;
                    return Err("ReturnCallAlreadyOpen");
                }
                let excess = self.coverage(a)?.max(0) as u64;
                if excess <= mta {
                    *self = before;
                    return Err("NoExcess");
                }
                let units = units_down(excess, self.nav, self.agreements[a].terms.haircut_bps)
                    .ok_or("Overflow")?;
                if units == 0 {
                    *self = before;
                    return Err("ReturnTooSmall");
                }
                let ag = &mut self.agreements[a];
                let seq = ag.call_count;
                ag.call_count += 1;
                ag.call_refs.insert(external_ref);
                let deadline = self.now + i64::from(ag.terms.cure_secs);
                self.calls.push(Call {
                    agreement: a,
                    seq,
                    kind: CallKind::Return,
                    value: excess,
                    units_required: units,
                    issued_at: self.now,
                    deadline,
                    enforce_after: 0,
                    status: CallStatus::Open,
                    units_released: 0,
                });
                let c = self.calls.len() - 1;
                self.agreements[a].open_return = Some(c);
                Ok(c)
            }
        }
    }

    /// `check_coverage`, by anyone.
    pub fn check_coverage(&mut self, a: usize) -> ModelResult {
        self.fresh(a)?;
        self.observe(a)
    }

    /// `pledge`, signed by `actor`. Returns the pledge index.
    pub fn pledge(&mut self, a: usize, actor: Actor, units: u64, pledge_ref: [u8; 32]) -> ModelResult<usize> {
        if self.agreements[a].pledge_refs.contains(&pledge_ref) {
            return Err("AlreadyInUse");
        }
        if actor != Actor::Giver {
            return Err("Unauthorized");
        }
        if units == 0 {
            return Err("ZeroUnits");
        }
        if pledge_ref == [0; 32] {
            return Err("ZeroPledgeRef");
        }
        let giver = self.agreements[a].giver;
        if !self.eligible(giver) {
            return Err("PartyNotEligible");
        }
        let ag = &self.agreements[a];
        if ag.pledged.checked_add(units).is_none_or(|p| p > ag.terms.cap_units) {
            return Err("PledgeLimitExceeded");
        }
        let h = &self.holders[giver];
        if h.locked.checked_add(units).is_none_or(|l| l > h.balance) {
            return Err("InsufficientUnlockedUnits");
        }
        self.holders[giver].locked += units;
        let ag = &mut self.agreements[a];
        ag.pledged += units;
        let seq = ag.pledge_count;
        ag.pledge_count += 1;
        ag.pledge_refs.insert(pledge_ref);
        self.pledges.push(Pledge {
            agreement: a,
            seq,
            units,
            released: 0,
            status: PledgeStatus::Active,
        });
        if self.fresh(a).is_ok() {
            self.observe(a)?;
        }
        Ok(self.pledges.len() - 1)
    }

    /// `release`, signed by `actor`, optionally against the open return call `return_call`.
    /// Returns whether the return call was Met.
    pub fn release(
        &mut self,
        p: usize,
        actor: Actor,
        units: u64,
        return_call: Option<usize>,
    ) -> ModelResult<bool> {
        let a = self.pledges[p].agreement;
        if !matches!(actor, Actor::Taker | Actor::Keeper) {
            return Err("Unauthorized");
        }
        if self.pledges[p].status != PledgeStatus::Active {
            return Err("PledgeNotActive");
        }
        if units == 0 {
            return Err("ZeroUnits");
        }
        if units > self.pledges[p].remaining() {
            return Err("ExceedsPledgeRemaining");
        }
        if actor == Actor::Keeper && return_call.is_none() {
            return Err("KeeperNeedsReturnCall");
        }
        if let Some(c) = return_call {
            if self.agreements[a].open_return != Some(c) || self.calls[c].status != CallStatus::Open {
                return Err("WrongReturnCall");
            }
            let call = &self.calls[c];
            if actor == Actor::Keeper && units > call.units_required - call.units_released {
                return Err("ExceedsReturnCall");
            }
        }
        let required = self.agreements[a].required;
        if required > 0 {
            self.fresh(a)?;
            let after = self.coverage_with(a, self.agreements[a].pledged - units, required)?;
            if after < 0 {
                return Err("WouldUndercollateralise");
            }
        }
        let giver = self.agreements[a].giver;
        self.holders[giver].locked -= units;
        self.agreements[a].pledged -= units;
        let pl = &mut self.pledges[p];
        pl.released += units;
        if pl.remaining() == 0 {
            pl.status = PledgeStatus::Released;
        }
        if required > 0 {
            self.observe(a)?;
        } else {
            self.observe_with(a, 0);
        }
        let mut met = false;
        if let Some(c) = return_call {
            let call = &mut self.calls[c];
            call.units_released += units;
            if call.units_released >= call.units_required {
                call.status = CallStatus::Met;
                self.agreements[a].open_return = None;
                met = true;
            }
        }
        Ok(met)
    }

    /// `substitute`, signed by `actor`. Returns the new pledge index.
    pub fn substitute(&mut self, old: usize, actor: Actor, new_units: u64, pledge_ref: [u8; 32]) -> ModelResult<usize> {
        let a = self.pledges[old].agreement;
        if self.agreements[a].pledge_refs.contains(&pledge_ref) {
            return Err("AlreadyInUse");
        }
        if actor != Actor::Giver {
            return Err("Unauthorized");
        }
        if self.pledges[old].status != PledgeStatus::Active {
            return Err("PledgeNotActive");
        }
        if pledge_ref == [0; 32] {
            return Err("ZeroPledgeRef");
        }
        let remaining = self.pledges[old].remaining();
        if new_units < remaining {
            return Err("SubstituteTooSmall");
        }
        let giver = self.agreements[a].giver;
        if !self.eligible(giver) {
            return Err("PartyNotEligible");
        }
        let ag = &self.agreements[a];
        if (ag.pledged - remaining).checked_add(new_units).is_none_or(|p| p > ag.terms.cap_units) {
            return Err("PledgeLimitExceeded");
        }
        let h = &self.holders[giver];
        let new_locked = (h.locked - remaining)
            .checked_add(new_units)
            .ok_or("InsufficientUnlockedUnits")?;
        if new_units > remaining && new_locked > h.balance {
            return Err("InsufficientUnlockedUnits");
        }
        self.holders[giver].locked = new_locked;
        let pl = &mut self.pledges[old];
        pl.released += remaining;
        pl.status = PledgeStatus::Substituted;
        let ag = &mut self.agreements[a];
        ag.pledged = ag.pledged - remaining + new_units;
        let seq = ag.pledge_count;
        ag.pledge_count += 1;
        ag.pledge_refs.insert(pledge_ref);
        self.pledges.push(Pledge {
            agreement: a,
            seq,
            units: new_units,
            released: 0,
            status: PledgeStatus::Active,
        });
        if self.fresh(a).is_ok() {
            self.observe(a)?;
        }
        Ok(self.pledges.len() - 1)
    }

    /// `cancel_call`, signed by `actor`.
    pub fn cancel_call(&mut self, c: usize, actor: Actor) -> ModelResult {
        let before = self.clone();
        self.settle_status(c);
        let call = self.calls[c].clone();
        let a = call.agreement;
        let allowed = match call.kind {
            CallKind::Delivery => actor == Actor::Taker,
            CallKind::Return => actor == Actor::Giver,
        };
        if !allowed {
            *self = before;
            return Err("Unauthorized");
        }
        let open = match call.kind {
            CallKind::Delivery => matches!(call.status, CallStatus::Open | CallStatus::Disputed),
            CallKind::Return => call.status == CallStatus::Open,
        };
        if !open {
            *self = before;
            return Err("CallNotOpen");
        }
        self.calls[c].status = CallStatus::Cancelled;
        let ag = &mut self.agreements[a];
        match call.kind {
            CallKind::Delivery => {
                ag.open_delivery -= 1;
                ag.demanded -= ag.demanded.min(call.value);
                if ag.open_delivery == 0 {
                    ag.demanded = 0;
                }
            }
            CallKind::Return => ag.open_return = None,
        }
        Ok(())
    }

    /// `dispute_call`, signed by `actor`.
    pub fn dispute(&mut self, c: usize, actor: Actor, dispute_ref: [u8; 32]) -> ModelResult {
        if actor != Actor::Giver {
            return Err("Unauthorized");
        }
        if self.calls[c].kind != CallKind::Delivery {
            return Err("WrongCallKind");
        }
        if self.call_status(c) != CallStatus::Open {
            return Err("CallNotOpen");
        }
        if dispute_ref == [0; 32] {
            return Err("ZeroExternalRef");
        }
        self.calls[c].status = CallStatus::Disputed;
        Ok(())
    }

    /// `withdraw_dispute`, signed by `actor`.
    pub fn withdraw_dispute(&mut self, c: usize, actor: Actor) -> ModelResult {
        if actor != Actor::Giver {
            return Err("Unauthorized");
        }
        if self.call_status(c) != CallStatus::Disputed {
            return Err("NotDisputed");
        }
        self.calls[c].status = CallStatus::Open;
        Ok(())
    }

    // --- invariants ---

    /// I1, I2, I15 and I16 over the whole model.
    pub fn check_invariants(&self) {
        for (i, h) in self.holders.iter().enumerate() {
            let locked: u64 = self
                .pledges
                .iter()
                .filter(|p| {
                    p.status == PledgeStatus::Active && self.agreements[p.agreement].giver == i
                })
                .map(Pledge::remaining)
                .sum();
            assert_eq!(h.locked, locked, "I1: holder {i}");
            assert!(h.balance >= h.locked, "I2: holder {i}");
        }
        for (a, ag) in self.agreements.iter().enumerate() {
            let pledged: u64 = self
                .pledges
                .iter()
                .filter(|p| p.agreement == a && p.status == PledgeStatus::Active)
                .map(Pledge::remaining)
                .sum();
            assert_eq!(ag.pledged, pledged, "I15: agreement {a}");
            assert!(ag.pledged <= ag.terms.cap_units, "I15 cap: agreement {a}");
            let open: Vec<&Call> = self
                .calls
                .iter()
                .filter(|c| {
                    c.agreement == a
                        && c.kind == CallKind::Delivery
                        && matches!(c.status, CallStatus::Open | CallStatus::Disputed)
                        && c.seq >= ag.met_below
                })
                .collect();
            assert_eq!(usize::from(ag.open_delivery), open.len(), "I16 count: agreement {a}");
            let total: u64 = open.iter().map(|c| c.value).sum();
            assert!(ag.demanded <= total, "I16 demanded ≤ Σ values: agreement {a}");
            if open.is_empty() {
                assert_eq!(ag.demanded, 0, "I16 zero: agreement {a}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: u64 = 100_000_000;

    fn worked() -> Terms {
        Terms {
            haircut_bps: 200,
            min_transfer_value: 10_000_000_000,
            cap_units: 5_000_000_000_000,
            max_age_secs: 93_600,
            cure_secs: 14_400,
            grace_secs: 7_200,
            unilateral: true,
        }
    }

    #[test]
    fn worked_figures_us2_to_us4() {
        let mut m = Model::new(&[2_000_000_000_000, 0], ONE, 1_000);
        let a = m.add_agreement(0, 1, worked());
        // US2: $1M with nothing pledged opens a $1M call.
        let c = m.update_exposure(a, 1_000_000_000_000, [1; 32]).unwrap().unwrap();
        assert_eq!(m.calls[c].value, 1_000_000_000_000);
        assert_eq!(m.calls[c].units_required, 1_020_408_163_266);
        assert_eq!(m.calls[c].deadline, 1_000 + 14_400);
        assert_eq!(m.calls[c].enforce_after, 1_000 + 21_600);
        assert_eq!(m.issue_call(a, CallKind::Delivery, [2; 32]), Err("NothingToCall"));
        // US3: pledging the units required meets it.
        let p = m.pledge(a, Actor::Giver, 1_020_408_163_266, [3; 32]).unwrap();
        assert_eq!(m.call_status(c), CallStatus::Met);
        assert_eq!(m.transfer(0, 1, 979_591_836_735), Err("PledgedUnitsLocked"));
        // US4: lower to $400 000; the return call releases 612,244.897959 units.
        m.update_exposure(a, 400_000_000_000, [4; 32]).unwrap();
        let r = m.issue_call(a, CallKind::Return, [5; 32]).unwrap();
        assert_eq!(m.calls[r].value, 600_000_000_000);
        assert_eq!(m.calls[r].units_required, 612_244_897_959);
        assert_eq!(m.release(p, Actor::Keeper, 612_244_897_959, Some(r)), Ok(true));
        assert_eq!(m.holders[0].locked, 408_163_265_307);
        assert_eq!(m.coverage(a), Ok(0));
        m.check_invariants();
    }

    #[test]
    fn demanded_part_shrinks_with_partial_pledges() {
        let mut m = Model::new(&[5_000_000_000_000, 0], ONE, 0);
        let a = m.add_agreement(0, 1, worked());
        m.update_exposure(a, 1_000_000_000_000, [1; 32]).unwrap();
        // Pledge $600 000 of value: shortfall $400 000, demanded part capped to it.
        let units = units_up(600_000_000_000, ONE, 200).unwrap();
        m.pledge(a, Actor::Giver, units, [2; 32]).unwrap();
        assert_eq!(m.agreements[a].demanded, 400_000_000_000);
        // A $500 000 raise opens a new $500 000 call instead of folding into the first.
        let c = m.update_exposure(a, 1_500_000_000_000, [3; 32]).unwrap().unwrap();
        assert_eq!(m.calls[c].value, 500_000_000_000);
        assert_eq!(m.agreements[a].open_delivery, 2);
        m.check_invariants();
    }

    #[test]
    fn price_drop_at_and_above_the_mta() {
        let mut m = Model::new(&[2_000_000_000_000, 0], ONE, 0);
        let a = m.add_agreement(0, 1, worked());
        m.update_exposure(a, 1_000_000_000_000, [1; 32]).unwrap();
        m.pledge(a, Actor::Giver, 1_020_408_163_266, [2; 32]).unwrap();
        m.publish(99_000_000);
        assert_eq!(m.issue_call(a, CallKind::Delivery, [3; 32]), Err("NothingToCall"));
        m.publish(98_500_000);
        let c = m.issue_call(a, CallKind::Delivery, [4; 32]).unwrap();
        assert_eq!(m.calls[c].value, 15_000_000_000);
        assert_eq!(m.calls[c].units_required, 15_539_210_609);
    }
}
