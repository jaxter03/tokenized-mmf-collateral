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
