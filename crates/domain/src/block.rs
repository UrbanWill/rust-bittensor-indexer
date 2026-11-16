//! Block numbers, hashes, and the runtime specs of a block.

use std::fmt;
use std::num::IntErrorKind;
use std::str::FromStr;

/// A block number. The chain counts blocks in `u32`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub struct BlockNumber(u32);

impl BlockNumber {
    /// Wraps a block number.
    pub const fn new(n: u32) -> Self {
        Self(n)
    }

    /// Returns the number.
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Returns the block before this one, or `None` at block 0.
    pub fn prev(self) -> Option<BlockNumber> {
        self.0.checked_sub(1).map(Self)
    }

    /// Returns the block after this one, or `None` at `u32::MAX`.
    pub fn next(self) -> Option<BlockNumber> {
        self.0.checked_add(1).map(Self)
    }
}

/// Why a value is not a block number.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BlockNumberError {
    /// The text is not a non-negative integer.
    #[error("block number is not a number")]
    NotANumber,
    /// The value is above `u32::MAX`.
    #[error("block number does not fit in u32")]
    TooLarge,
}

impl FromStr for BlockNumber {
    type Err = BlockNumberError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<u32>().map(Self).map_err(|e| match e.kind() {
            IntErrorKind::PosOverflow => BlockNumberError::TooLarge,
            _ => BlockNumberError::NotANumber,
        })
    }
}

impl TryFrom<u64> for BlockNumber {
    type Error = BlockNumberError;

    fn try_from(n: u64) -> Result<Self, Self::Error> {
        u32::try_from(n)
            .map(Self)
            .map_err(|_| BlockNumberError::TooLarge)
    }
}

impl From<BlockNumber> for u64 {
    fn from(n: BlockNumber) -> u64 {
        u64::from(n.0)
    }
}

impl fmt::Display for BlockNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A runtime spec version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SpecVersion(pub u32);

impl SpecVersion {
    /// Wraps a spec version.
    pub const fn new(v: u32) -> Self {
        Self(v)
    }

    /// Returns the version.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for SpecVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A 32-byte block hash. It prints as `0x` and 64 hex digits.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockHash(pub [u8; 32]);

impl fmt::Debug for BlockHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("0x")?;
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

/// The two runtime specs that matter for one block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpecAt {
    /// The spec that executed the block. It is the spec in the parent's state.
    /// Storage and events of the block decode with its metadata.
    pub executed_by: SpecVersion,
    /// The spec the block's own state reports. Runtime API calls at the block run it.
    pub state: SpecVersion,
}

/// One block: its number, its hash, and its specs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockRef {
    /// The block number.
    pub number: BlockNumber,
    /// The block hash.
    pub hash: BlockHash,
    /// The specs of the block.
    pub spec: SpecAt,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_block_number() {
        assert_eq!("4920350".parse(), Ok(BlockNumber::new(4_920_350)));
        assert_eq!("4294967295".parse(), Ok(BlockNumber::new(u32::MAX)));
    }

    #[test]
    fn rejects_a_block_number_above_u32() {
        assert_eq!(
            "5000000000".parse::<BlockNumber>(),
            Err(BlockNumberError::TooLarge)
        );
        assert_eq!(
            "99999999999999999999999".parse::<BlockNumber>(),
            Err(BlockNumberError::TooLarge)
        );
    }

    #[test]
    fn rejects_text_that_is_not_a_number() {
        for s in ["", "abc", "-1", "1.5", " 1"] {
            assert_eq!(
                s.parse::<BlockNumber>(),
                Err(BlockNumberError::NotANumber),
                "{s:?}"
            );
        }
    }

    #[test]
    fn converts_from_and_to_u64() {
        assert_eq!(BlockNumber::try_from(7_u64), Ok(BlockNumber::new(7)));
        assert_eq!(
            BlockNumber::try_from(u64::from(u32::MAX) + 1),
            Err(BlockNumberError::TooLarge)
        );
        assert_eq!(u64::from(BlockNumber::new(u32::MAX)), u64::from(u32::MAX));
    }

    #[test]
    fn prev_and_next_stop_at_the_ends() {
        assert_eq!(BlockNumber::new(0).prev(), None);
        assert_eq!(BlockNumber::new(1).prev(), Some(BlockNumber::new(0)));
        assert_eq!(BlockNumber::new(u32::MAX).next(), None);
        assert_eq!(BlockNumber::new(1).next(), Some(BlockNumber::new(2)));
    }

    #[test]
    fn displays_numbers_plainly() {
        assert_eq!(BlockNumber::new(4_920_350).to_string(), "4920350");
        assert_eq!(SpecVersion::new(233).to_string(), "233");
    }

    #[test]
    fn hash_debug_is_hex() {
        let mut bytes = [0_u8; 32];
        bytes[0] = 0x93;
        bytes[31] = 0x1f;
        let s = format!("{:?}", BlockHash(bytes));
        assert_eq!(s.len(), 66);
        assert!(s.starts_with("0x93"));
        assert!(s.ends_with("1f"));
    }
}
