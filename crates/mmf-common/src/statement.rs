//! KYC statement: the fixed 95-byte message a provider signs with ed25519
//! (contracts/kyc-statement.md, data-model.md "KYC statement").
//!
//! ```text
//! offset  size  field
//!  0      20    domain         b"MMF-KYC-STATEMENT-V1"
//! 20      32    mint           the fund mint (scopes the statement to one fund)
//! 52      32    owner          the owner account it names
//! 84       2    jurisdiction   ISO 3166-1 alpha-2, ASCII
//! 86       1    investor_type  provider code (must be non-zero to be approvable)
//! 87       8    expiry_ts      i64, little-endian, Unix seconds
//! ```
//!
//! This module only encodes and parses the layout. Checking the provider, owner, expiry,
//! jurisdiction and investor type is the program's job, in the contract's order.

use anchor_lang::prelude::Pubkey;

/// Domain tag; separates these signatures from anything else the provider key signs.
pub const DOMAIN: &[u8; 20] = b"MMF-KYC-STATEMENT-V1";
/// Exact message length.
pub const LEN: usize = 95;

const MINT: usize = 20;
const OWNER: usize = 52;
const JURISDICTION: usize = 84;
const INVESTOR_TYPE: usize = 86;
const EXPIRY: usize = 87;

/// The fields a statement carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Statement {
    pub mint: Pubkey,
    pub owner: Pubkey,
    pub jurisdiction: [u8; 2],
    pub investor_type: u8,
    pub expiry_ts: i64,
}

/// Why a message isn't a statement. Both map to `StatementMalformed` in the program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    WrongLength,
    WrongDomain,
}

impl Statement {
    /// The 95 bytes a provider signs.
    pub fn encode(&self) -> [u8; LEN] {
        let mut m = [0u8; LEN];
        m[..MINT].copy_from_slice(DOMAIN);
        m[MINT..OWNER].copy_from_slice(self.mint.as_ref());
        m[OWNER..JURISDICTION].copy_from_slice(self.owner.as_ref());
        m[JURISDICTION..INVESTOR_TYPE].copy_from_slice(&self.jurisdiction);
        m[INVESTOR_TYPE] = self.investor_type;
        m[EXPIRY..].copy_from_slice(&self.expiry_ts.to_le_bytes());
        m
    }

    /// Parses a signed message. Checks only the length and the domain tag.
    pub fn parse(message: &[u8]) -> Result<Self, ParseError> {
        if message.len() != LEN {
            return Err(ParseError::WrongLength);
        }
        if &message[..MINT] != DOMAIN {
            return Err(ParseError::WrongDomain);
        }
        let key = |r: core::ops::Range<usize>| {
            Pubkey::new_from_array(message[r].try_into().expect("32-byte slice"))
        };
        Ok(Self {
            mint: key(MINT..OWNER),
            owner: key(OWNER..JURISDICTION),
            jurisdiction: [message[JURISDICTION], message[JURISDICTION + 1]],
            investor_type: message[INVESTOR_TYPE],
            expiry_ts: i64::from_le_bytes(message[EXPIRY..].try_into().expect("8-byte slice")),
        })
    }
}

/// `attestation_hash`: SHA-256 of the whole message (uses the sha256 syscall on-chain).
pub fn fingerprint(message: &[u8]) -> [u8; 32] {
    solana_sha256_hasher::hash(message).to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Statement {
        Statement {
            mint: Pubkey::new_unique(),
            owner: Pubkey::new_unique(),
            jurisdiction: *b"US",
            investor_type: 3,
            expiry_ts: 1_800_000_000,
        }
    }

    #[test]
    fn round_trip_and_layout() {
        let s = sample();
        let m = s.encode();
        assert_eq!(m.len(), 95);
        assert_eq!(&m[..20], b"MMF-KYC-STATEMENT-V1");
        assert_eq!(&m[20..52], s.mint.as_ref());
        assert_eq!(&m[52..84], s.owner.as_ref());
        assert_eq!(&m[84..86], b"US");
        assert_eq!(m[86], 3);
        assert_eq!(&m[87..], &1_800_000_000i64.to_le_bytes());
        assert_eq!(Statement::parse(&m), Ok(s));
    }

    #[test]
    fn negative_expiry_round_trips() {
        let s = Statement {
            expiry_ts: -1,
            ..sample()
        };
        assert_eq!(Statement::parse(&s.encode()).unwrap().expiry_ts, -1);
    }

    #[test]
    fn parse_errors() {
        let m = sample().encode();
        assert_eq!(Statement::parse(&m[..94]), Err(ParseError::WrongLength));
        let mut long = m.to_vec();
        long.push(0);
        assert_eq!(Statement::parse(&long), Err(ParseError::WrongLength));
        let mut bad = m;
        bad[0] ^= 1;
        assert_eq!(Statement::parse(&bad), Err(ParseError::WrongDomain));
    }

    #[test]
    fn fingerprint_is_sha256() {
        // SHA-256("abc") test vector.
        assert_eq!(
            fingerprint(b"abc"),
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad
            ]
        );
        let m = sample().encode();
        assert_ne!(fingerprint(&m), fingerprint(&sample().encode()));
    }
}
