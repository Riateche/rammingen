use {
    serde::{de::DeserializeOwned, Serialize},
    std::io::Write,
};

pub fn serialize<T: Serialize>(value: &T) -> Result<Vec<u8>, bincode::error::EncodeError> {
    bincode::serde::encode_to_vec(value, bincode::config::legacy())
}

pub fn serialize_into<T: Serialize>(
    mut writer: impl Write,
    value: &T,
) -> Result<usize, bincode::error::EncodeError> {
    bincode::serde::encode_into_std_write(value, &mut writer, bincode::config::legacy())
}

pub fn deserialize<T: DeserializeOwned>(data: &[u8]) -> Result<T, bincode::error::DecodeError> {
    bincode::serde::decode_from_slice(data, bincode::config::legacy()).map(|(data, _len)| data)
}
