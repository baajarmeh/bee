// Copyright 2020 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::{
    constants::INPUT_OUTPUT_INDEX_RANGE,
    payload::transaction::{TransactionId, TRANSACTION_ID_LENGTH},
    Error,
};

use bee_common::packable::{Packable, Read, Write};

use core::{
    convert::{From, TryFrom, TryInto},
    str::FromStr,
};

pub const OUTPUT_ID_LENGTH: usize = TRANSACTION_ID_LENGTH + std::mem::size_of::<u16>();

#[derive(Clone, Copy, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct OutputId {
    transaction_id: TransactionId,
    index: u16,
}

#[cfg(feature = "serde")]
string_serde_impl!(OutputId);

impl TryFrom<[u8; OUTPUT_ID_LENGTH]> for OutputId {
    type Error = Error;

    fn try_from(bytes: [u8; OUTPUT_ID_LENGTH]) -> Result<Self, Self::Error> {
        let (transaction_id, index) = bytes.split_at(TRANSACTION_ID_LENGTH);

        Self::new(
            // Unwrap is fine because size is already known and valid.
            From::<[u8; TRANSACTION_ID_LENGTH]>::from(transaction_id.try_into().unwrap()),
            // Unwrap is fine because size is already known and valid.
            u16::from_le_bytes(index.try_into().unwrap()),
        )
    }
}

impl FromStr for OutputId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes: [u8; OUTPUT_ID_LENGTH] = hex::decode(s)
            .map_err(|_| Self::Err::InvalidHexadecimalChar(s.to_owned()))?
            .try_into()
            .map_err(|_| Self::Err::InvalidHexadecimalLength(OUTPUT_ID_LENGTH * 2, s.len()))?;

        bytes.try_into()
    }
}

impl OutputId {
    pub fn new(transaction_id: TransactionId, index: u16) -> Result<Self, Error> {
        if !INPUT_OUTPUT_INDEX_RANGE.contains(&index) {
            return Err(Error::InvalidInputOutputIndex(index));
        }

        Ok(Self { transaction_id, index })
    }

    pub fn transaction_id(&self) -> &TransactionId {
        &self.transaction_id
    }

    pub fn index(&self) -> u16 {
        self.index
    }

    pub fn split(self) -> (TransactionId, u16) {
        (self.transaction_id, self.index)
    }
}

impl core::fmt::Display for OutputId {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{}{}", self.transaction_id, hex::encode(self.index.to_le_bytes()))
    }
}

impl core::fmt::Debug for OutputId {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "OutputId({})", self)
    }
}

impl Packable for OutputId {
    type Error = Error;

    fn packed_len(&self) -> usize {
        self.transaction_id.packed_len() + self.index.packed_len()
    }

    fn pack<W: Write>(&self, writer: &mut W) -> Result<(), Self::Error> {
        self.transaction_id.pack(writer)?;
        self.index.pack(writer)?;

        Ok(())
    }

    fn unpack<R: Read + ?Sized>(reader: &mut R) -> Result<Self, Self::Error> {
        let transaction_id = TransactionId::unpack(reader)?;
        let index = u16::unpack(reader)?;

        Self::new(transaction_id, index)
    }
}
