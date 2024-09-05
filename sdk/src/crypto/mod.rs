//! All encryption operations use AES-SIV.
//!
//! Data that will be stored in the server's database is encrypted using zero nonce,
//! so the result is deterministic. This is important, e.g., for paths, because we
//! should be able to encrypt the path again and retrieve it from the server.
//! For file content, a random nonce is used for each block.
//!
//! File hash and size are encrypted using a single pass of AES-SIV with a zero nonce.
//!
//! When encrypting an archive path, it's split into components, and each component
//! is encrypted individually using a single pass of AES-SIV with a zero nonce, and then
//! encoded in base64. An encrypted path is then reconstructed from the encrypted components.
//! Thus, encrypted path is still a valid archive path, and
//! parent-child relationships are preserved even in encrypted form. This is important for
//! certain server operations. For example, if a MovePath or RemovePath command is issued,
//! the server should be able to find all paths nested in the specified path.
//!
//! When encrypting file content, it's first compressed using deflate and then split into fixed-size blocks.
//! For each block, a random nonce is chosen. The nonce and encrypted block data are written to the encrypted file
//! in the following form:
//!
//! - block size (32 bits, little endian) - length of the following block (nonce + encrypted content)
//! - nonce (128 bits) - the random nonce used to encrypt this block
//! - encrypted content
//!
//! Integrity of the file content is ensured on decryption by checking the resulting file content hash.

mod cipher;
mod io;

pub use cipher::Cipher;
pub use io::{encrypt_file, DecryptingWriter};
