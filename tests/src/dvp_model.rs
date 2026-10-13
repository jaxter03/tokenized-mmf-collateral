//! Independent reference model of settlement (F003 research R18), the oracle for the property
//! tests (T071, SC-001–SC-005).
//!
//! Written from the spec and the contracts' check order. It shares **no** code with the programs:
//! no `mmf_common::settlement`, no program types, its own rounding. Every operation returns `Ok`
//! or the name of the error the programs must report first (the variant name in `DvpError`,
//! `FundError`, `HookError` or `GovernanceError`, or `InsufficientFunds` for the token program,
//! `AlreadyInUse` for a second creation at the same address).
//!
//! Not modelled: interest (F002's model covers it), the holder cap, missing cash accounts and
//! wrong-account errors (the generator never produces them), and stray escrow deposits.

use std::collections::HashSet;

pub type ModelResult<T = ()> = Result<T, &'static str>;

const DAY: i64 = 86_400;
const PRICE_SCALE: u128 = 100_000_000;

/// A party to an instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Who {
    Fund,
    Inv(usize),
}

/// Who signs a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Actor {
    Inv(usize),
    TransferAgent,
    Keeper,
    Compliance,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Sub,
    Red,
    Sec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Instructed,
    SellerFunded,
    BuyerFunded,
    Affirmed,
    Settled,
    Cancelled,
    Expired,
    Rejected,
}

impl Status {
    pub fn is_open(self) -> bool {
        matches!(
            self,
            Self::Instructed | Self::SellerFunded | Self::BuyerFunded | Self::Affirmed
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Seller,
    Buyer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leg {
    Units,
    Cash,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HolderStatus {
    Approved,
    Suspended,
    Revoked,
}

#[derive(Clone, Debug)]
pub struct Investor {
    pub units: u64,
    pub cash: u64,
    pub status: HolderStatus,
    pub kyc_expiry: i64,
}

#[derive(Clone, Debug)]
pub struct Instr {
    pub kind: Kind,
    pub seller: Who,
    pub buyer: Who,
    pub units: u64,
    pub cash: u64,
    pub nav_seq: u64,
    pub not_before: i64,
    pub expires: i64,
    pub status: Status,
    pub seller_funded: bool,
    pub buyer_funded: bool,
    /// (seller side, buyer side) asked to cancel while Affirmed.
    pub cancel_req: (bool, bool),
    pub units_to_return: u64,
    pub cash_to_return: u64,
}

impl Instr {
    fn units_held(&self) -> u64 {
        if self.status.is_open() {
            if self.seller_funded && self.kind != Kind::Sub {
                self.units
            } else {
                0
            }
        } else {
            self.units_to_return
        }
    }

    fn cash_held(&self) -> u64 {
        if self.status.is_open() {
            if self.buyer_funded {
                self.cash
            } else {
                0
            }
        } else {
            self.cash_to_return
        }
    }
}

/// Arguments of `create`.
#[derive(Clone, Copy, Debug)]
pub struct CreateArgs {
    pub kind: Kind,
    pub seller: Who,
    pub buyer: Who,
    pub units: u64,
    /// Secondary only; 0 for subscriptions and redemptions.
    pub cash: u64,
    /// 0 = now.
    pub not_before: i64,
    /// 0 = now + default expiry.
    pub expires: i64,
    pub external_ref: [u8; 32],
}

#[derive(Clone, Debug)]
pub struct Model {
    pub now: i64,
    pub investors: Vec<Investor>,
    pub treasury_cash: u64,
    pub escrow_units: u64,
    pub escrow_cash: u64,
    pub units_outstanding: u64,
    pub paused: bool,
    pub nav: u64,
    pub nav_seq: u64,
    /// Last UTC day covered by a publish (meaningful when `nav_seq > 0`).
    pub period_end_day: i64,
    pub gate_bps: u64,
    pub default_expiry: i64,
    /// UTC day the redemption counter belongs to; `None` before the first mint or burn.
    pub redemption_day: Option<i64>,
    pub redeemed_today: u64,
    pub units_at_day_start: u64,
    pub instrs: Vec<Instr>,
    used_refs: HashSet<(Actor, [u8; 32])>,
}

fn ceil_cash(units: u64, nav: u64) -> Option<u64> {
    let p = units as u128 * nav as u128;
    u64::try_from(p.div_ceil(PRICE_SCALE)).ok()
}

fn floor_cash(units: u64, nav: u64) -> Option<u64> {
    let p = units as u128 * nav as u128;
    u64::try_from(p / PRICE_SCALE).ok()
}

impl Model {
    /// A fund at `now` with these investors (all Approved, KYC valid for a year), no price yet,
    /// `gate_bps` daily redemption limit, 24 h default expiry, units already outstanding equal
    /// to the investors' units.
    pub fn new(now: i64, investors: &[(u64, u64)], gate_bps: u64) -> Self {
        let investors: Vec<Investor> = investors
            .iter()
            .map(|&(units, cash)| Investor {
                units,
                cash,
                status: HolderStatus::Approved,
                kyc_expiry: now + 365 * DAY,
            })
            .collect();
        let units_outstanding = investors.iter().map(|i| i.units).sum();
        Self {
            now,
            investors,
            treasury_cash: 0,
            escrow_units: 0,
            escrow_cash: 0,
            units_outstanding,
            paused: false,
            nav: 0,
            nav_seq: 0,
            period_end_day: 0,
            gate_bps,
            default_expiry: DAY,
            redemption_day: None,
            redeemed_today: 0,
            units_at_day_start: 0,
            instrs: Vec::new(),
            used_refs: HashSet::new(),
        }
    }

    pub fn today(&self) -> i64 {
        self.now.div_euclid(DAY)
    }

    fn eligible(&self, who: Who) -> bool {
        match who {
            Who::Fund => true,
            Who::Inv(i) => {
                let inv = &self.investors[i];
                inv.status == HolderStatus::Approved && inv.kyc_expiry > self.now
            }
        }
    }

    fn side_party(&self, ins: &Instr, side: Side) -> Actor {
        let who = match side {
            Side::Seller => ins.seller,
            Side::Buyer => ins.buyer,
        };
        match who {
            Who::Fund => Actor::TransferAgent,
            Who::Inv(i) => Actor::Inv(i),
        }
    }

    // ---- price and admin --------------------------------------------------------------------

    /// Days the next publish must cover to end today (F002 contiguous coverage).
    pub fn days_to_cover_today(&self) -> i64 {
        let start = if self.nav_seq == 0 {
            self.today()
        } else {
            self.period_end_day + 1
        };
        self.today() - start + 1
    }

    /// Publishes `nav` covering `days` days (F002 rules that matter here).
    pub fn publish(&mut self, nav: u64, days: i64) -> ModelResult {
        let start = if self.nav_seq == 0 {
            self.today()
        } else {
            self.period_end_day + 1
        };
        if start > self.today() {
            return Err("PeriodNotStarted");
        }
        self.nav = nav;
        self.nav_seq += 1;
        self.period_end_day = start + days - 1;
        Ok(())
    }

    pub fn suspend(&mut self, i: usize) {
        self.investors[i].status = HolderStatus::Suspended;
    }

    pub fn reinstate(&mut self, i: usize) {
        self.investors[i].status = HolderStatus::Approved;
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    pub fn advance(&mut self, secs: i64) {
        self.now += secs;
    }

    // ---- settlement -------------------------------------------------------------------------

    /// `create_instruction` (contract checks 4–15, in order). Returns the instruction index.
    pub fn create(&mut self, creator: Actor, a: CreateArgs) -> ModelResult<usize> {
        // 4. kind and where the Fund sits
        let shape_ok = match a.kind {
            Kind::Sub => a.seller == Who::Fund && matches!(a.buyer, Who::Inv(_)),
            Kind::Red => matches!(a.seller, Who::Inv(_)) && a.buyer == Who::Fund,
            Kind::Sec => matches!(a.seller, Who::Inv(_)) && matches!(a.buyer, Who::Inv(_)),
        };
        if !shape_ok {
            return Err("InvalidKind");
        }
        // 5. creator
        let allowed = match a.kind {
            Kind::Sub => creator == Actor::TransferAgent || Some(creator) == inv_actor(a.buyer),
            Kind::Red => creator == Actor::TransferAgent || Some(creator) == inv_actor(a.seller),
            Kind::Sec => {
                Some(creator) == inv_actor(a.seller) || Some(creator) == inv_actor(a.buyer)
            }
        };
        if !allowed {
            return Err("Unauthorized");
        }
        // 6–8
        if a.units == 0 {
            return Err("ZeroUnits");
        }
        if a.external_ref == [0; 32] {
            return Err("ZeroExternalRef");
        }
        if a.kind == Kind::Sec && a.seller == a.buyer {
            return Err("SameParty");
        }
        // 9–11
        let cash = match a.kind {
            Kind::Sub | Kind::Red => {
                if self.nav_seq == 0 {
                    return Err("NoPrice");
                }
                if self.period_end_day + 1 < self.today() {
                    return Err("StalePrice");
                }
                if a.cash != 0 {
                    return Err("CashAmountNotAllowed");
                }
                let c = if a.kind == Kind::Sub {
                    ceil_cash(a.units, self.nav)
                } else {
                    floor_cash(a.units, self.nav)
                };
                c.ok_or("Overflow")?
            }
            Kind::Sec => a.cash,
        };
        if cash == 0 {
            return Err("ZeroCash");
        }
        // 12
        let not_before = if a.not_before == 0 {
            self.now
        } else {
            a.not_before
        };
        let expires = if a.expires == 0 {
            self.now + self.default_expiry
        } else {
            a.expires
        };
        if expires <= self.now || expires <= not_before {
            return Err("InvalidExpiry");
        }
        // 13
        if !self.eligible(a.seller) || !self.eligible(a.buyer) {
            return Err("PartyNotEligible");
        }
        // 15
        if !self.used_refs.insert((creator, a.external_ref)) {
            return Err("AlreadyInUse");
        }
        self.instrs.push(Instr {
            kind: a.kind,
            seller: a.seller,
            buyer: a.buyer,
            units: a.units,
            cash,
            nav_seq: self.nav_seq,
            not_before,
            expires,
            status: Status::Instructed,
            seller_funded: false,
            buyer_funded: false,
            cancel_req: (false, false),
            units_to_return: 0,
            cash_to_return: 0,
        });
        Ok(self.instrs.len() - 1)
    }

    /// `affirm_and_fund(side)` (contract checks 3–9, then the token program and hook).
    pub fn fund(&mut self, id: usize, side: Side, actor: Actor) -> ModelResult {
        let ins = self.instrs[id].clone();
        if !ins.status.is_open() {
            return Err("InstructionFinal");
        }
        if self.now >= ins.expires {
            return Err("InstructionExpired");
        }
        if actor != self.side_party(&ins, side) {
            return Err("Unauthorized");
        }
        let funded = match side {
            Side::Seller => ins.seller_funded,
            Side::Buyer => ins.buyer_funded,
        };
        if funded {
            return Err("LegAlreadyFunded");
        }
        let party = match side {
            Side::Seller => ins.seller,
            Side::Buyer => ins.buyer,
        };
        if !self.eligible(party) {
            return Err("PartyNotEligible");
        }
        match (side, party) {
            // A subscription's Fund side affirms; nothing moves.
            (Side::Seller, Who::Fund) => {}
            (Side::Seller, Who::Inv(i)) => {
                // Token-2022 checks the balance before calling the hook; the hook then checks
                // the pause first.
                if self.investors[i].units < ins.units {
                    return Err("InsufficientFunds");
                }
                if self.paused {
                    return Err("TransfersPaused");
                }
                self.investors[i].units -= ins.units;
                self.escrow_units += ins.units;
            }
            (Side::Buyer, Who::Fund) => {
                if self.treasury_cash < ins.cash {
                    return Err("InsufficientFunds");
                }
                self.treasury_cash -= ins.cash;
                self.escrow_cash += ins.cash;
            }
            (Side::Buyer, Who::Inv(i)) => {
                if self.investors[i].cash < ins.cash {
                    return Err("InsufficientFunds");
                }
                self.investors[i].cash -= ins.cash;
                self.escrow_cash += ins.cash;
            }
        }
        let s = &mut self.instrs[id];
        match side {
            Side::Seller => s.seller_funded = true,
            Side::Buyer => s.buyer_funded = true,
        }
        s.status = match (s.seller_funded, s.buyer_funded) {
            (true, true) => Status::Affirmed,
            (true, false) => Status::SellerFunded,
            (false, true) => Status::BuyerFunded,
            (false, false) => unreachable!("one leg was just funded"),
        };
        Ok(())
    }

    fn roll_day(&mut self) {
        let today = self.today();
        if self.redemption_day != Some(today) {
            self.redemption_day = Some(today);
            self.redeemed_today = 0;
            self.units_at_day_start = self.units_outstanding;
        }
    }

    /// `settle` (contract checks 2–7, then `fund`'s mint/burn and the hook on delivery).
    pub fn settle(&mut self, id: usize) -> ModelResult {
        let ins = self.instrs[id].clone();
        if !ins.status.is_open() {
            return Err("InstructionFinal");
        }
        if ins.status != Status::Affirmed {
            return Err("NotAffirmed");
        }
        if self.now < ins.not_before {
            return Err("NotYetValid");
        }
        if self.now >= ins.expires {
            return Err("InstructionExpired");
        }
        if self.paused {
            return Err("TransfersPaused");
        }
        if !self.eligible(ins.seller) || !self.eligible(ins.buyer) {
            return Err("PartyNotEligible");
        }
        match ins.kind {
            Kind::Sub => {
                let Who::Inv(b) = ins.buyer else {
                    unreachable!()
                };
                self.roll_day();
                self.units_outstanding += ins.units;
                self.investors[b].units += ins.units;
                self.escrow_cash -= ins.cash;
                self.treasury_cash += ins.cash;
            }
            Kind::Red => {
                let Who::Inv(s) = ins.seller else {
                    unreachable!()
                };
                // The limit is checked after the day rolls over; a refused burn aborts the
                // transaction, so the rollover itself is only kept on success.
                let (redeemed, base) = if self.redemption_day == Some(self.today()) {
                    (self.redeemed_today, self.units_at_day_start)
                } else {
                    (0, self.units_outstanding)
                };
                if (redeemed as u128 + ins.units as u128) * 10_000
                    > base as u128 * self.gate_bps as u128
                {
                    return Err("RedemptionLimitExceeded");
                }
                self.roll_day();
                self.redeemed_today += ins.units;
                self.units_outstanding -= ins.units;
                self.escrow_units -= ins.units;
                self.escrow_cash -= ins.cash;
                self.investors[s].cash += ins.cash;
            }
            Kind::Sec => {
                let (Who::Inv(s), Who::Inv(b)) = (ins.seller, ins.buyer) else {
                    unreachable!()
                };
                self.escrow_units -= ins.units;
                self.investors[b].units += ins.units;
                self.escrow_cash -= ins.cash;
                self.investors[s].cash += ins.cash;
            }
        }
        self.instrs[id].status = Status::Settled;
        Ok(())
    }

    fn earmark(&mut self, id: usize, status: Status) {
        let s = &mut self.instrs[id];
        s.units_to_return = s.units_held();
        s.cash_to_return = s.cash_held();
        s.status = status;
    }

    pub fn cancel(&mut self, id: usize, side: Side, actor: Actor) -> ModelResult {
        let ins = self.instrs[id].clone();
        if !ins.status.is_open() {
            return Err("InstructionFinal");
        }
        if actor != self.side_party(&ins, side) {
            return Err("Unauthorized");
        }
        if ins.status == Status::Affirmed {
            let s = &mut self.instrs[id];
            let bit = match side {
                Side::Seller => &mut s.cancel_req.0,
                Side::Buyer => &mut s.cancel_req.1,
            };
            if *bit {
                return Err("CancelAlreadyRequested");
            }
            *bit = true;
            if s.cancel_req == (true, true) {
                self.earmark(id, Status::Cancelled);
            }
        } else {
            self.earmark(id, Status::Cancelled);
        }
        Ok(())
    }

    pub fn withdraw_cancel(&mut self, id: usize, side: Side, actor: Actor) -> ModelResult {
        let ins = self.instrs[id].clone();
        if !ins.status.is_open() {
            return Err("InstructionFinal");
        }
        if actor != self.side_party(&ins, side) {
            return Err("Unauthorized");
        }
        let (own, other) = match side {
            Side::Seller => (ins.cancel_req.0, ins.cancel_req.1),
            Side::Buyer => (ins.cancel_req.1, ins.cancel_req.0),
        };
        if ins.status != Status::Affirmed || !own || other {
            return Err("NoCancelRequest");
        }
        let s = &mut self.instrs[id];
        match side {
            Side::Seller => s.cancel_req.0 = false,
            Side::Buyer => s.cancel_req.1 = false,
        }
        Ok(())
    }

    pub fn expire(&mut self, id: usize) -> ModelResult {
        let ins = &self.instrs[id];
        if !ins.status.is_open() {
            return Err("InstructionFinal");
        }
        if self.now < ins.expires {
            return Err("NotExpired");
        }
        self.earmark(id, Status::Expired);
        Ok(())
    }

    /// `governance::reject_settlement` then `dvp_settlement::reject`. `reason` as stored (0–6
    /// valid codes; 0, 1, 2, 3, 6 allowed).
    pub fn reject(&mut self, id: usize, actor: Actor, reason: u8) -> ModelResult {
        if actor != Actor::Compliance {
            return Err("Unauthorized");
        }
        if !matches!(reason, 0 | 1 | 2 | 3 | 6) {
            return Err("InvalidReason");
        }
        if !self.instrs[id].status.is_open() {
            return Err("InstructionFinal");
        }
        self.earmark(id, Status::Rejected);
        Ok(())
    }

    /// `return_leg(leg)`.
    pub fn return_leg(&mut self, id: usize, leg: Leg) -> ModelResult {
        let ins = self.instrs[id].clone();
        if !matches!(
            ins.status,
            Status::Cancelled | Status::Expired | Status::Rejected
        ) {
            return Err("NotReturnable");
        }
        match leg {
            Leg::Units => {
                if ins.units_to_return == 0 {
                    return Err("NothingToReturn");
                }
                let Who::Inv(s) = ins.seller else {
                    unreachable!("units only come from investors")
                };
                if self.paused {
                    return Err("TransfersPaused");
                }
                if !self.eligible(ins.seller) {
                    return Err("ReceiverNotApproved");
                }
                self.escrow_units -= ins.units_to_return;
                self.investors[s].units += ins.units_to_return;
                self.instrs[id].units_to_return = 0;
            }
            Leg::Cash => {
                if ins.cash_to_return == 0 {
                    return Err("NothingToReturn");
                }
                self.escrow_cash -= ins.cash_to_return;
                match ins.buyer {
                    Who::Fund => self.treasury_cash += ins.cash_to_return,
                    Who::Inv(b) => self.investors[b].cash += ins.cash_to_return,
                }
                self.instrs[id].cash_to_return = 0;
            }
        }
        Ok(())
    }

    // ---- invariants -------------------------------------------------------------------------

    /// I13 with no stray deposits: escrow holds exactly what instructions hold.
    pub fn escrow_matches_instructions(&self) -> bool {
        let units: u64 = self.instrs.iter().map(Instr::units_held).sum();
        let cash: u64 = self.instrs.iter().map(Instr::cash_held).sum();
        units == self.escrow_units && cash == self.escrow_cash
    }

    /// I14 for the current redemption day.
    pub fn redemption_limit_holds(&self) -> bool {
        self.redeemed_today as u128 * 10_000
            <= self.units_at_day_start as u128 * self.gate_bps as u128
    }

    /// Units in existence = held by investors + in escrow (I4's model side).
    pub fn units_conserved(&self) -> bool {
        let held: u64 = self.investors.iter().map(|i| i.units).sum();
        held + self.escrow_units == self.units_outstanding
    }
}

fn inv_actor(who: Who) -> Option<Actor> {
    match who {
        Who::Inv(i) => Some(Actor::Inv(i)),
        Who::Fund => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: u64 = 1_000_000; // one unit or one dollar, 6 dp
    const NAV_1: u64 = 100_000_000;
    const T0: i64 = 20_739 * DAY + 3_600;

    fn r(n: u8) -> [u8; 32] {
        [n; 32]
    }

    fn sub(buyer: usize, units: u64, ext: u8) -> CreateArgs {
        CreateArgs {
            kind: Kind::Sub,
            seller: Who::Fund,
            buyer: Who::Inv(buyer),
            units,
            cash: 0,
            not_before: 0,
            expires: 0,
            external_ref: r(ext),
        }
    }

    fn red(seller: usize, units: u64, ext: u8) -> CreateArgs {
        CreateArgs {
            kind: Kind::Red,
            seller: Who::Inv(seller),
            buyer: Who::Fund,
            ..sub(0, units, ext)
        }
    }

    #[test]
    fn us1_subscribe_two_million() {
        let mut m = Model::new(T0, &[(0, 2_000_000 * ONE)], 1_000);
        m.publish(NAV_1, 1).unwrap();
        let id = m.create(Actor::Inv(0), sub(0, 2_000_000 * ONE, 1)).unwrap();
        assert_eq!(m.instrs[id].cash, 2_000_000 * ONE);
        m.fund(id, Side::Seller, Actor::TransferAgent).unwrap();
        m.fund(id, Side::Buyer, Actor::Inv(0)).unwrap();
        assert_eq!(m.instrs[id].status, Status::Affirmed);
        m.settle(id).unwrap();
        assert_eq!(m.investors[0].units, 2_000_000 * ONE);
        assert_eq!(m.investors[0].cash, 0);
        assert_eq!(m.treasury_cash, 2_000_000 * ONE);
        assert_eq!(m.units_outstanding, 2_000_000 * ONE);
        assert!(m.escrow_matches_instructions() && m.units_conserved());
        assert_eq!(m.settle(id), Err("InstructionFinal"), "settle twice");
    }

    #[test]
    fn us2_redeem_half_a_million_within_the_limit() {
        let mut m = Model::new(T0, &[(5_000_000 * ONE, 0)], 1_000);
        m.treasury_cash = 1_000_000 * ONE;
        m.publish(NAV_1, 1).unwrap();
        let id = m.create(Actor::Inv(0), red(0, 500_000 * ONE, 1)).unwrap();
        m.fund(id, Side::Seller, Actor::Inv(0)).unwrap();
        m.fund(id, Side::Buyer, Actor::TransferAgent).unwrap();
        m.settle(id).unwrap();
        assert_eq!(m.investors[0].units, 4_500_000 * ONE);
        assert_eq!(m.investors[0].cash, 500_000 * ONE);
        assert_eq!(m.units_outstanding, 4_500_000 * ONE);
        assert_eq!(m.redeemed_today, 500_000 * ONE);
        assert!(m.redemption_limit_holds() && m.units_conserved());

        // 10 % of 5M = 500k is used up today; one more unit waits for tomorrow. It needs an
        // expiry past the default 24 h to still be open then.
        let next_day_ok = CreateArgs {
            expires: T0 + 2 * DAY,
            ..red(0, 1, 2)
        };
        let id2 = m.create(Actor::Inv(0), next_day_ok).unwrap();
        m.fund(id2, Side::Seller, Actor::Inv(0)).unwrap();
        m.fund(id2, Side::Buyer, Actor::TransferAgent).unwrap();
        assert_eq!(m.settle(id2), Err("RedemptionLimitExceeded"));
        assert_eq!(m.instrs[id2].status, Status::Affirmed);
        m.advance(DAY);
        m.publish(NAV_1, m.days_to_cover_today()).unwrap();
        m.settle(id2).unwrap();
        assert_eq!(m.units_at_day_start, 4_500_000 * ONE);
    }

    #[test]
    fn us3_secondary_and_us4_unwinds() {
        let mut m = Model::new(T0, &[(1_000_000 * ONE, 0), (0, 1_000_000 * ONE)], 1_000);
        let sec = |ext| CreateArgs {
            kind: Kind::Sec,
            seller: Who::Inv(0),
            buyer: Who::Inv(1),
            units: 1_000_000 * ONE,
            cash: 999_500 * ONE,
            not_before: 0,
            expires: 0,
            external_ref: r(ext),
        };
        let id = m.create(Actor::Inv(1), sec(1)).unwrap();
        m.fund(id, Side::Seller, Actor::Inv(0)).unwrap();
        assert_eq!(m.settle(id), Err("NotAffirmed"));
        m.fund(id, Side::Buyer, Actor::Inv(1)).unwrap();
        m.settle(id).unwrap();
        assert_eq!(
            (m.investors[0].units, m.investors[0].cash),
            (0, 999_500 * ONE)
        );
        assert_eq!(m.investors[1].units, 1_000_000 * ONE);

        // Back the other way: one side funds, then expire, then return.
        let back = CreateArgs {
            seller: Who::Inv(1),
            buyer: Who::Inv(0),
            external_ref: r(2),
            ..sec(2)
        };
        let id = m.create(Actor::Inv(1), back).unwrap();
        m.fund(id, Side::Seller, Actor::Inv(1)).unwrap();
        assert_eq!(m.expire(id), Err("NotExpired"));
        m.advance(DAY);
        m.expire(id).unwrap();
        assert_eq!(m.instrs[id].units_to_return, 1_000_000 * ONE);
        m.suspend(1);
        assert_eq!(m.return_leg(id, Leg::Units), Err("ReceiverNotApproved"));
        m.reinstate(1);
        m.return_leg(id, Leg::Units).unwrap();
        assert_eq!(m.return_leg(id, Leg::Units), Err("NothingToReturn"));
        assert_eq!(m.investors[1].units, 1_000_000 * ONE);
        assert!(m.escrow_matches_instructions() && m.units_conserved());
    }

    #[test]
    fn creation_checks_in_order() {
        let mut m = Model::new(T0, &[(ONE, ONE)], 1_000);
        assert_eq!(m.create(Actor::Keeper, sub(0, ONE, 1)), Err("Unauthorized"));
        assert_eq!(m.create(Actor::Inv(0), sub(0, 0, 1)), Err("ZeroUnits"));
        assert_eq!(
            m.create(Actor::Inv(0), sub(0, ONE, 0)),
            Err("ZeroExternalRef")
        );
        assert_eq!(m.create(Actor::Inv(0), sub(0, ONE, 1)), Err("NoPrice"));
        m.publish(NAV_1, 1).unwrap();
        m.advance(2 * DAY);
        assert_eq!(m.create(Actor::Inv(0), sub(0, ONE, 1)), Err("StalePrice"));
        m.publish(99_999_999, m.days_to_cover_today()).unwrap();
        assert_eq!(m.create(Actor::Inv(0), red(0, 1, 1)), Err("ZeroCash"));
        let id = m.create(Actor::Inv(0), sub(0, 1, 1)).unwrap();
        assert_eq!(m.instrs[id].cash, 1, "rounded up");
        assert_eq!(m.create(Actor::Inv(0), sub(0, 1, 1)), Err("AlreadyInUse"));
        m.create(Actor::TransferAgent, sub(0, 1, 1)).unwrap();
        m.suspend(0);
        assert_eq!(
            m.create(Actor::Inv(0), sub(0, 1, 2)),
            Err("PartyNotEligible")
        );
    }

    #[test]
    fn affirmed_cancel_needs_both_sides() {
        let mut m = Model::new(T0, &[(0, ONE)], 1_000);
        m.publish(NAV_1, 1).unwrap();
        let id = m.create(Actor::TransferAgent, sub(0, ONE, 1)).unwrap();
        m.fund(id, Side::Buyer, Actor::Inv(0)).unwrap();
        m.fund(id, Side::Seller, Actor::TransferAgent).unwrap();
        m.cancel(id, Side::Buyer, Actor::Inv(0)).unwrap();
        assert_eq!(
            m.cancel(id, Side::Buyer, Actor::Inv(0)),
            Err("CancelAlreadyRequested")
        );
        m.withdraw_cancel(id, Side::Buyer, Actor::Inv(0)).unwrap();
        assert_eq!(
            m.withdraw_cancel(id, Side::Buyer, Actor::Inv(0)),
            Err("NoCancelRequest")
        );
        m.cancel(id, Side::Buyer, Actor::Inv(0)).unwrap();
        m.cancel(id, Side::Seller, Actor::TransferAgent).unwrap();
        assert_eq!(m.instrs[id].status, Status::Cancelled);
        assert_eq!(m.instrs[id].cash_to_return, ONE);
        assert_eq!(m.reject(id, Actor::Compliance, 1), Err("InstructionFinal"));
        m.return_leg(id, Leg::Cash).unwrap();
        assert_eq!(m.investors[0].cash, ONE, "restored exactly");
        assert_eq!(m.units_outstanding, 0, "nothing was minted");
    }
}
