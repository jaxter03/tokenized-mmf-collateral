//! Status, class and reason codes shared by the programs (stored as `u8`, docs/shared-model.md §4).

/// Defines a `#[repr(u8)]` enum with `TryFrom<u8>` and `as_u8`. Unknown values are an error,
/// never a default (Constitution VII).
macro_rules! u8_enum {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $variant:ident = $value:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u8)]
        pub enum $name { $($(#[$vm])* $variant = $value),+ }

        impl $name {
            pub const fn as_u8(self) -> u8 { self as u8 }
        }

        impl TryFrom<u8> for $name {
            type Error = u8;
            fn try_from(v: u8) -> Result<Self, u8> {
                match v { $($value => Ok(Self::$variant),)+ other => Err(other) }
            }
        }
    };
}

u8_enum! {
    /// `FundState.status`. F001 uses only Setup and Open.
    FundStatus { Setup = 0, Open = 1, Gated = 2, Suspended = 3, WindingDown = 4, Closed = 5 }
}

u8_enum! {
    /// `HolderState.status`.
    HolderStatus { Pending = 0, Approved = 1, Suspended = 2, Revoked = 3 }
}

u8_enum! {
    /// `HolderState.class`. `CollateralVault` is reserved for the backlog and is never accepted.
    HolderClass { Investor = 0, FundTreasury = 1, Escrow = 2, CollateralVault = 3 }
}

u8_enum! {
    /// `ActionRecord.reason` (shared-model §4.5).
    ReasonCode {
        CourtOrder = 0, SanctionsHit = 1, LostKey = 2, OperationalError = 3,
        CollateralDefault = 4, IssuerWindDown = 5, Other = 6,
    }
}

u8_enum! {
    /// Holder status changes routed governance → compliance_hook `set_status`.
    StatusAction { Suspend = 0, Reinstate = 1, Revoke = 2 }
}

u8_enum! {
    /// `SettlementInstruction.kind` (shared-model §4.3). [F003]
    SettlementKind { Subscription = 0, Redemption = 1, SecondaryTransfer = 2 }
}

u8_enum! {
    /// `SettlementInstruction.status` (shared-model §4.3). [F003]
    SettlementStatus {
        Instructed = 0, SellerFunded = 1, BuyerFunded = 2, Affirmed = 3,
        Settled = 4, Cancelled = 5, Expired = 6, Rejected = 7,
    }
}

u8_enum! {
    /// Side of a settlement instruction: the seller delivers units, the buyer pays cash. [F003]
    Side { Seller = 0, Buyer = 1 }
}

u8_enum! {
    /// Leg of a settlement instruction, for `return_leg`. [F003]
    Leg { Units = 0, Cash = 1 }
}

u8_enum! {
    /// `PledgeAgreement.mode` / `Pledge.mode` (shared-model §4.4). Only Lien is accepted until the
    /// backlog. [F004]
    PledgeMode { Lien = 0, TitleTransfer = 1 }
}

u8_enum! {
    /// `PledgeAgreement.income_to`. Only Giver is accepted until the backlog. [F004]
    IncomeTo { Giver = 0, Taker = 1, TakerOwesGiver = 2 }
}

u8_enum! {
    /// `PledgeAgreement.status`. Terminating and Terminated are reserved. [F004]
    AgreementStatus { Proposed = 0, Active = 1, Terminating = 2, Terminated = 3 }
}

u8_enum! {
    /// `Pledge.status`. Enforced arrives with F005. [F004]
    PledgeStatus { Active = 0, Released = 1, Enforced = 2, Substituted = 3 }
}

u8_enum! {
    /// `CollateralCall.kind`: the taker demands more (Delivery) or the giver asks for excess back
    /// (Return). [F004]
    CallKind { Delivery = 0, Return = 1 }
}

u8_enum! {
    /// `CollateralCall.status`. A Delivery call stored Open or Disputed below the agreement's
    /// `met_below_seq` is Met (derived, `collateral::delivery_met`). Defaulted and Enforced arrive
    /// with F005. [F004]
    CallStatus { Open = 0, Met = 1, Disputed = 2, Defaulted = 3, Enforced = 4, Cancelled = 5 }
}

u8_enum! {
    /// `ExposureUpdated.outcome`: applied at once, held for the giver, or confirmed by the giver.
    /// [F004]
    ExposureOutcome { Applied = 0, Pending = 1, Confirmed = 2 }
}

impl SettlementStatus {
    /// Not yet final: Instructed, SellerFunded, BuyerFunded or Affirmed.
    pub const fn is_open(self) -> bool {
        matches!(
            self,
            Self::Instructed | Self::SellerFunded | Self::BuyerFunded | Self::Affirmed
        )
    }

    /// Final without settling, so funded legs are earmarked for return (research R3).
    pub const fn is_returnable(self) -> bool {
        matches!(self, Self::Cancelled | Self::Expired | Self::Rejected)
    }
}

impl HolderClass {
    /// Classes a transfer may deliver to (hook `class_allowed`).
    pub const fn transfer_allowed(self) -> bool {
        matches!(self, Self::Investor | Self::FundTreasury | Self::Escrow)
    }

    /// FundTreasury and Escrow: approved without a KYC statement, never counted toward the cap.
    pub const fn is_fund_operated(self) -> bool {
        matches!(self, Self::FundTreasury | Self::Escrow)
    }
}

impl ReasonCode {
    /// Reasons F001 accepts for suspend, reinstate, revoke, pause and unpause (FR-031).
    /// CollateralDefault and IssuerWindDown belong to later features.
    pub const fn allowed_in_f001(self) -> bool {
        matches!(
            self,
            Self::CourtOrder
                | Self::SanctionsHit
                | Self::LostKey
                | Self::OperationalError
                | Self::Other
        )
    }

    /// Reasons a price override may give (F002, research R5): correcting an operational error, or
    /// "other" with a case reference to the valuation decision.
    pub const fn allowed_for_nav_override(self) -> bool {
        matches!(self, Self::OperationalError | Self::Other)
    }

    /// Reasons compliance may give when rejecting a settlement (F003, research R11): the F001
    /// list.
    pub const fn allowed_for_reject_settlement(self) -> bool {
        self.allowed_in_f001()
    }

    /// Reasons compliance may give when resolving a call dispute (F004, research R10): the F001
    /// list.
    pub const fn allowed_for_resolve_dispute(self) -> bool {
        self.allowed_in_f001()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_unknown_values() {
        for v in 0..=5u8 {
            assert_eq!(FundStatus::try_from(v).unwrap().as_u8(), v);
        }
        assert_eq!(FundStatus::try_from(6), Err(6));
        assert_eq!(HolderStatus::try_from(3), Ok(HolderStatus::Revoked));
        assert_eq!(HolderStatus::try_from(4), Err(4));
        assert_eq!(HolderClass::try_from(4), Err(4));
        assert_eq!(StatusAction::try_from(3), Err(3));
        assert_eq!(ReasonCode::try_from(7), Err(7));
        for v in 0..=2u8 {
            assert_eq!(SettlementKind::try_from(v).unwrap().as_u8(), v);
        }
        assert_eq!(SettlementKind::try_from(3), Err(3));
        for v in 0..=7u8 {
            assert_eq!(SettlementStatus::try_from(v).unwrap().as_u8(), v);
        }
        assert_eq!(SettlementStatus::try_from(8), Err(8));
        assert_eq!(Side::try_from(1), Ok(Side::Buyer));
        assert_eq!(Side::try_from(2), Err(2));
        assert_eq!(Leg::try_from(1), Ok(Leg::Cash));
        assert_eq!(Leg::try_from(2), Err(2));
        assert_eq!(PledgeMode::try_from(1), Ok(PledgeMode::TitleTransfer));
        assert_eq!(PledgeMode::try_from(2), Err(2));
        assert_eq!(IncomeTo::try_from(2), Ok(IncomeTo::TakerOwesGiver));
        assert_eq!(IncomeTo::try_from(3), Err(3));
        for v in 0..=3u8 {
            assert_eq!(AgreementStatus::try_from(v).unwrap().as_u8(), v);
            assert_eq!(PledgeStatus::try_from(v).unwrap().as_u8(), v);
        }
        assert_eq!(AgreementStatus::try_from(4), Err(4));
        assert_eq!(PledgeStatus::try_from(4), Err(4));
        assert_eq!(CallKind::try_from(1), Ok(CallKind::Return));
        assert_eq!(CallKind::try_from(2), Err(2));
        for v in 0..=5u8 {
            assert_eq!(CallStatus::try_from(v).unwrap().as_u8(), v);
        }
        assert_eq!(CallStatus::try_from(6), Err(6));
        assert_eq!(ExposureOutcome::try_from(2), Ok(ExposureOutcome::Confirmed));
        assert_eq!(ExposureOutcome::try_from(3), Err(3));
    }

    #[test]
    fn resolve_dispute_reasons() {
        let allowed: Vec<u8> = (0..=6)
            .filter(|v| {
                ReasonCode::try_from(*v)
                    .unwrap()
                    .allowed_for_resolve_dispute()
            })
            .collect();
        assert_eq!(allowed, vec![0, 1, 2, 3, 6]);
    }

    #[test]
    fn settlement_status_groups() {
        let open: Vec<u8> = (0..=7)
            .filter(|v| SettlementStatus::try_from(*v).unwrap().is_open())
            .collect();
        assert_eq!(open, vec![0, 1, 2, 3]);
        let returnable: Vec<u8> = (0..=7)
            .filter(|v| SettlementStatus::try_from(*v).unwrap().is_returnable())
            .collect();
        assert_eq!(returnable, vec![5, 6, 7]);
    }

    #[test]
    fn reject_settlement_reasons() {
        let allowed: Vec<u8> = (0..=6)
            .filter(|v| {
                ReasonCode::try_from(*v)
                    .unwrap()
                    .allowed_for_reject_settlement()
            })
            .collect();
        assert_eq!(allowed, vec![0, 1, 2, 3, 6]);
    }

    #[test]
    fn f001_reasons() {
        let allowed: Vec<u8> = (0..=6)
            .filter(|v| ReasonCode::try_from(*v).unwrap().allowed_in_f001())
            .collect();
        assert_eq!(allowed, vec![0, 1, 2, 3, 6]);
    }

    #[test]
    fn nav_override_reasons() {
        let allowed: Vec<u8> = (0..=6)
            .filter(|v| ReasonCode::try_from(*v).unwrap().allowed_for_nav_override())
            .collect();
        assert_eq!(allowed, vec![3, 6]);
    }

    #[test]
    fn classes() {
        assert!(HolderClass::Investor.transfer_allowed());
        assert!(!HolderClass::CollateralVault.transfer_allowed());
        assert!(HolderClass::FundTreasury.is_fund_operated());
        assert!(!HolderClass::Investor.is_fund_operated());
    }
}
