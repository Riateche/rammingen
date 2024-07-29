use aes_siv::aead::OsRng;
use aes_siv::{Aes256SivAead, KeyInit};
use base64::{prelude::BASE64_URL_SAFE_NO_PAD, Engine};
use derivative::Derivative;
use generic_array::{typenum::U64, GenericArray};
use serde::{de::Error, Deserialize, Serialize};

#[derive(Clone, Derivative)]
#[derivative(Debug)]
pub struct EncryptionKey(#[derivative(Debug = "ignore")] GenericArray<u8, U64>);

impl EncryptionKey {
    pub fn generate() -> Self {
        Self(Aes256SivAead::generate_key(&mut OsRng))
    }

    pub fn get(&self) -> &GenericArray<u8, U64> {
        &self.0
    }
}

impl<'de> Deserialize<'de> for EncryptionKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let string = String::deserialize(deserializer)?;
        let binary = BASE64_URL_SAFE_NO_PAD
            .decode(string)
            .map_err(D::Error::custom)?;
        let array = <[u8; 64]>::try_from(binary).map_err(|vec| {
            D::Error::custom(format!(
                "invalid encryption key length, expected 64, got {}",
                vec.len()
            ))
        })?;
        Ok(Self(array.into()))
    }
}

impl Serialize for EncryptionKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        BASE64_URL_SAFE_NO_PAD.encode(self.0).serialize(serializer)
    }
}
