//! Reference model for the randomized invariant tests (T075, SC-001, SC-007).
//!
//! It predicts, from the rules in spec.md / contracts/, whether each operation succeeds, and
//! tracks what the chain should then hold. Error codes are asserted elsewhere; here only
//! success vs failure and the resulting state are compared.

use mmf_common::enums::HolderStatus;

#[derive(Clone, Debug)]
pub struct Investor {
    pub registered: bool,
    pub status: HolderStatus,
    pub kyc_expiry: i64,
    /// Units in the investor's holding account.
    pub balance: u64,
    /// The holding account was thawed (the investor was approved at some point).
    pub thawed: bool,
}

impl Investor {
    pub fn unregistered() -> Self {
        Self {
            registered: false,
            status: HolderStatus::Pending,
            kyc_expiry: 0,
            balance: 0,
            thawed: false,
        }
    }

    /// `approved(h, now)`.
    pub fn approved(&self, now: i64) -> bool {
        self.registered && self.status == HolderStatus::Approved && now < self.kyc_expiry
    }
}

#[derive(Clone, Debug)]
pub struct Model {
    pub investors: Vec<Investor>,
    pub paused: bool,
    pub max_holders: u32,
    pub now: i64,
}

impl Model {
    /// Investors holding units: what `holder_count` must equal (I10).
    pub fn holder_count(&self) -> u32 {
        self.investors.iter().filter(|i| i.balance > 0).count() as u32
    }

    /// Whether a transfer of `amount` from `from` to `to` should succeed.
    pub fn transfer_ok(&self, from: usize, to: usize, amount: u64) -> bool {
        let (s, r) = (&self.investors[from], &self.investors[to]);
        // Token-2022: both holding accounts must exist and be thawed, and the source must cover it.
        if !s.thawed || !r.thawed || amount > s.balance {
            return false;
        }
        // Same account: Token-2022 returns before the hook (research R4).
        if from == to {
            return true;
        }
        if self.paused || !s.approved(self.now) || !r.approved(self.now) {
            return false;
        }
        let new_holder = r.balance == 0 && amount > 0;
        !(new_holder && self.max_holders > 0 && self.holder_count() >= self.max_holders)
    }

    pub fn apply_transfer(&mut self, from: usize, to: usize, amount: u64) {
        if from != to {
            self.investors[from].balance -= amount;
            self.investors[to].balance += amount;
        }
    }

    pub fn suspend_ok(&self, i: usize) -> bool {
        let x = &self.investors[i];
        x.registered && x.status == HolderStatus::Approved
    }

    pub fn reinstate_ok(&self, i: usize) -> bool {
        let x = &self.investors[i];
        x.registered && x.status == HolderStatus::Suspended
    }

    pub fn revoke_ok(&self, i: usize) -> bool {
        let x = &self.investors[i];
        x.registered && x.status != HolderStatus::Revoked
    }

    pub fn renew_ok(&self, i: usize, new_expiry: i64) -> bool {
        let x = &self.investors[i];
        x.registered
            && x.status == HolderStatus::Approved
            && new_expiry > self.now
            && new_expiry > x.kyc_expiry
    }
}
