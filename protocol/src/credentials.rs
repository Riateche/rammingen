use {
    aes_siv::{aead::array::Array, Aes256SivAead, Key, KeyInit},
    anyhow::{anyhow, bail, ensure, format_err, Error},
    base64::{display::Base64Display, prelude::BASE64_URL_SAFE_NO_PAD, Engine},
    generic_array::typenum::U64,
    rand::{
        distr::{Alphanumeric, SampleString},
        rand_core,
        rngs::OsRng,
        CryptoRng,
    },
    serde::{de, Deserialize, Deserializer, Serialize, Serializer},
    std::{
        any::Any,
        borrow::Cow,
        fmt::{self, Debug, Display},
        panic::catch_unwind,
        str::FromStr,
    },
};

#[derive(Clone, Deserialize, Serialize)]
pub struct AccessToken(String);

const ACCESS_TOKEN_LENGTH: usize = 64;

fn format_panic_message(err: Box<dyn Any + Send + 'static>) -> String {
    err.downcast_ref::<&'static str>()
        .map(|s| s.to_string())
        .or_else(|| err.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| format!("{err:?}"))
}

impl AccessToken {
    pub fn generate() -> anyhow::Result<Self> {
        catch_unwind(|| {
            Self(Alphanumeric.sample_string(&mut rand_core::UnwrapErr(OsRng), ACCESS_TOKEN_LENGTH))
        })
        .map_err(|err| anyhow!(format_panic_message(err)))
    }

    pub fn as_unmasked_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for AccessToken {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ensure!(
            s.len() == ACCESS_TOKEN_LENGTH,
            "invalid length; got {}, expected {ACCESS_TOKEN_LENGTH}",
            s.len(),
        );
        if let Some(c) = s.chars().find(|c| !c.is_ascii_alphanumeric()) {
            bail!("must be alphanumeric but contains invalid character `{c}`");
        }
        Ok(Self(s.to_owned()))
    }
}

impl Debug for AccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessToken").finish()
    }
}

#[derive(Clone)]
#[allow(deprecated)]
pub struct EncryptionKey(Array<u8, U64>);

impl EncryptionKey {
    pub fn generate() -> anyhow::Result<Self> {
        Ok(Self(Aes256SivAead::generate_key()?))
    }

    pub fn generate_with_rng<R: CryptoRng + ?Sized>(rng: &mut R) -> Self {
        let mut key = Key::<Aes256SivAead>::default();
        rng.fill_bytes(&mut key);
        Self(key)
    }

    #[allow(deprecated)]
    pub fn get(&self) -> &Array<u8, U64> {
        &self.0
    }

    pub fn display_unmasked(&self) -> impl Display + '_ {
        Base64Display::new(self.0.as_ref(), &BASE64_URL_SAFE_NO_PAD)
    }
}

impl<'de> Deserialize<'de> for EncryptionKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Cow::<'_, str>::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

impl Serialize for EncryptionKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        BASE64_URL_SAFE_NO_PAD.encode(self.0).serialize(serializer)
    }
}

impl FromStr for EncryptionKey {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        const KEY_LENGTH: usize = 64;

        let bytes = BASE64_URL_SAFE_NO_PAD.decode(s)?;
        let array = <[u8; KEY_LENGTH]>::try_from(bytes).map_err(|bytes| {
            format_err!("invalid length; got {}, expected {KEY_LENGTH}", bytes.len())
        })?;
        Ok(Self(array.into()))
    }
}

impl Debug for EncryptionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EncryptionKey").finish()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn access_token_from_str() {
        static TOKEN: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789ab";
        assert_eq!(
            AccessToken::from_str(TOKEN).unwrap().as_unmasked_str(),
            TOKEN,
        );
        assert!(AccessToken::from_str("").is_err());
        assert!(AccessToken::from_str(&TOKEN[1..]).is_err());
        assert!(AccessToken::from_str(&format!("{TOKEN}c")).is_err());
        assert!(AccessToken::from_str(&format!("{}:", &TOKEN[1..])).is_err());
    }

    #[test]
    fn encryption_key_from_str() {
        static KEY: &str = "qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqg";
        assert_eq!(
            EncryptionKey::from_str(KEY)
                .unwrap()
                .display_unmasked()
                .to_string(),
            KEY,
        );
        assert!(EncryptionKey::from_str("").is_err());
        assert!(EncryptionKey::from_str("qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqo").is_err());
        assert!(EncryptionKey::from_str(&format!("{KEY}:")).is_err());
    }
}
