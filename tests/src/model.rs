//! Reference models for the randomized tests: F001's register model (T075, SC-001, SC-007) and
//! F002's accrual model (`AccrualModel`, research R11).
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

/// Independent reference for F002 interest (research R11). It deliberately shares no code with
/// `mmf_common::accrual`: it credits every holder **once per covered day** at that publish's
/// rate, which is the spec's "day-by-day calculation", rather than moving a counter.
///
/// Amounts are base units × 10^18, the same scale as `HolderState.accrued_e18`, so comparisons
/// are exact.
#[derive(Clone, Debug, Default)]
pub struct AccrualModel {
    /// Units held now, per holder (index = the test's holder number).
    pub balances: Vec<u64>,
    /// Interest earned so far, per holder.
    pub earned_e18: Vec<u128>,
    /// Interest one base unit has earned since the first publish.
    pub index_e18: u128,
    /// Last UTC day covered so far.
    pub period_end_day: Option<u32>,
    /// Last published price.
    pub last_nav: Option<u64>,
}

impl AccrualModel {
    pub fn new(holders: usize) -> Self {
        Self {
            balances: vec![0; holders],
            earned_e18: vec![0; holders],
            ..Self::default()
        }
    }

    /// First day the next publish covers: `today` for the first publish, else the day after the
    /// last covered day (FR-009).
    pub fn next_start(&self, today: u32) -> u32 {
        self.period_end_day.map_or(today, |end| end + 1)
    }

    /// A publish starting on `start_day` covering `days` days at `rate_e12`: each day, every
    /// holder earns `balance × rate × 10^6`.
    pub fn publish(&mut self, start_day: u32, nav: u64, rate_e12: u64, days: u16) {
        for _ in 0..days {
            let per_unit_per_day = u128::from(rate_e12) * 1_000_000;
            for (balance, earned) in self.balances.iter().zip(self.earned_e18.iter_mut()) {
                *earned += u128::from(*balance) * per_unit_per_day;
            }
            self.index_e18 += per_unit_per_day;
        }
        self.period_end_day = Some(start_day + u32::from(days) - 1);
        self.last_nav = Some(nav);
    }

    pub fn transfer(&mut self, from: usize, to: usize, amount: u64) {
        self.balances[from] -= amount;
        self.balances[to] += amount;
    }

    /// Units arriving from outside the register (test seeding).
    pub fn seed(&mut self, to: usize, amount: u64) {
        self.balances[to] += amount;
    }

    pub fn total_earned(&self) -> u128 {
        self.earned_e18.iter().sum()
    }
}

#[cfg(test)]
mod accrual_model_tests {
    use super::AccrualModel;

    #[test]
    fn three_days_by_hand() {
        let mut m = AccrualModel::new(2);
        m.seed(0, 100);
        assert_eq!(m.next_start(10), 10);
        // Day 10 at rate 2: holder 0 earns 100 × 2 × 10^6.
        m.publish(10, 100_000_000, 2, 1);
        assert_eq!(m.earned_e18, vec![200_000_000, 0]);
        m.transfer(0, 1, 40);
        // Days 11 and 12 at rate 5: holder 0 earns 60 × 5 × 10^6 twice, holder 1 40 × 5 × 10^6 twice.
        assert_eq!(m.next_start(11), 11);
        m.publish(11, 100_000_000, 5, 2);
        assert_eq!(m.earned_e18, vec![800_000_000, 400_000_000]);
        assert_eq!(m.total_earned(), 1_200_000_000);
        assert_eq!(m.index_e18, 12_000_000);
        assert_eq!(m.period_end_day, Some(12));
        assert_eq!(m.next_start(20), 13);
        // A zero rate covers days but earns nothing.
        m.publish(13, 100_000_000, 0, 3);
        assert_eq!(m.total_earned(), 1_200_000_000);
        assert_eq!(m.period_end_day, Some(15));
    }
}
