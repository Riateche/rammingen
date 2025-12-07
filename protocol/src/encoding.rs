#![expect(clippy::absolute_paths, reason = "for clarity")]

//! Encoding used in client-server communication and in the client's local database.
//!
//! Compatible with bincode 1.x.

use {
    serde::{Serialize, de::DeserializeOwned},
    std::io::Write,
};

#[inline]
pub fn serialize<T: Serialize>(value: &T) -> Result<Vec<u8>, bincode::error::EncodeError> {
    bincode::serde::encode_to_vec(value, bincode::config::legacy())
}

#[inline]
pub fn serialize_into<T: Serialize>(
    mut writer: impl Write,
    value: &T,
) -> Result<usize, bincode::error::EncodeError> {
    bincode::serde::encode_into_std_write(value, &mut writer, bincode::config::legacy())
}

#[inline]
pub fn deserialize<T: DeserializeOwned>(data: &[u8]) -> Result<T, bincode::error::DecodeError> {
    bincode::serde::decode_from_slice(data, bincode::config::legacy()).map(|(data, _len)| data)
}
