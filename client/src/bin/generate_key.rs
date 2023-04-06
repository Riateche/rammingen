use aes_siv::KeyInit;
use aes_siv::{aead::OsRng, Aes256SivAead};
use base64::{prelude::BASE64_URL_SAFE_NO_PAD, Engine};
use generic_array::GenericArray;
use typenum::U64;

fn main() {
    let key: GenericArray<u8, U64> = Aes256SivAead::generate_key(&mut OsRng);
    println!("{}", BASE64_URL_SAFE_NO_PAD.encode(key));
}
