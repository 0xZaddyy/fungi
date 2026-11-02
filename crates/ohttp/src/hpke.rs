//! Fixed-size payloads between a sender and a recipient: one HPKE payload
//! that gives both a key for each direction, then symmetric payloads under
//! those keys.
//!
//! Every payload is [`PAYLOAD_BYTES`], the size of a BIP77 mailbox message,
//! and encrypts a length-prefixed, zero-padded message.
//!
//! A payload from [`seal`] is an ElligatorSwift-encoded encapsulated key
//! followed by the HPKE ChaCha20Poly1305 ciphertext. The keys are exported
//! from the HPKE context.
//!
//! A payload from [`seal_symmetric`] is a random nonce followed by the
//! XChaCha20Poly1305 ciphertext.

use bitcoin_hpke::aead::ChaCha20Poly1305;
use bitcoin_hpke::kdf::HkdfSha256;
use bitcoin_hpke::kem::SecpK256HkdfSha256;
use bitcoin_hpke::rand_core::{OsRng, RngCore as _};
use bitcoin_hpke::{Deserializable, HpkeError, Kem as _, OpModeR, OpModeS, Serializable};
use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use secp256k1::ellswift::ElligatorSwift;

/// Size of a payload.
pub const PAYLOAD_BYTES: usize = 7168;

/// Largest message that fits in a payload from [`seal`].
pub const MAX_MESSAGE_BYTES: usize = HANDSHAKE_PLAINTEXT_BYTES - LENGTH_BYTES;

/// Largest message that fits in a payload from [`seal_symmetric`].
pub const MAX_SYMMETRIC_MESSAGE_BYTES: usize = SYMMETRIC_PLAINTEXT_BYTES - LENGTH_BYTES;

/// Size of each exported key.
pub const KEY_BYTES: usize = 32;

const ENCAPSULATED_KEY_BYTES: usize = 64;
const NONCE_BYTES: usize = 24;
const TAG_BYTES: usize = 16;
const LENGTH_BYTES: usize = 2;
const HANDSHAKE_PLAINTEXT_BYTES: usize = PAYLOAD_BYTES - ENCAPSULATED_KEY_BYTES - TAG_BYTES;
const SYMMETRIC_PLAINTEXT_BYTES: usize = PAYLOAD_BYTES - NONCE_BYTES - TAG_BYTES;

/// HPKE exporter contexts of the keys. The values are arbitrary.
const SENDER_TO_RECIPIENT: &[u8] = b"sender to recipient";
const RECIPIENT_TO_SENDER: &[u8] = b"recipient to sender";

type Kem = SecpK256HkdfSha256;
type EncappedKey = <Kem as bitcoin_hpke::Kem>::EncappedKey;

/// A secp256k1 HPKE secret key.
pub type SecretKey = <Kem as bitcoin_hpke::Kem>::PrivateKey;

/// A secp256k1 HPKE public key.
pub type PublicKey = <Kem as bitcoin_hpke::Kem>::PublicKey;

/// One party's keys, one for each direction.
pub struct Keys {
    /// Key of the payloads this party sends.
    pub sending: [u8; KEY_BYTES],
    /// Key of the payloads this party receives.
    pub receiving: [u8; KEY_BYTES],
}

/// Failure to seal or open a payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The message is longer than [`MAX_MESSAGE_BYTES`] or
    /// [`MAX_SYMMETRIC_MESSAGE_BYTES`].
    #[error("message has {0} bytes, more than fits in a payload")]
    MessageTooLarge(usize),
    /// The payload is not [`PAYLOAD_BYTES`] long.
    #[error("payload has {0} bytes, expected {PAYLOAD_BYTES}")]
    PayloadSize(usize),
    /// The payload was modified, or was not sealed with this key and `info`
    /// or `aad`.
    #[error("payload cannot be decrypted")]
    Decryption,
    /// The decrypted length prefix exceeds the rest of the plaintext.
    #[error("decrypted message length is invalid")]
    MessageLength,
}

/// Derive a keypair from `ikm` with the KEM's `DeriveKeyPair`.
pub fn derive_keypair(ikm: &[u8; 32]) -> (SecretKey, PublicKey) {
    Kem::derive_keypair(ikm)
}

/// Encrypt `message` to `public_key` and return the payload and the sender's
/// keys. [`open`] needs the same `info`.
pub fn seal(public_key: &PublicKey, info: &[u8], message: &[u8]) -> Result<(Vec<u8>, Keys), Error> {
    let plaintext = pad(message, HANDSHAKE_PLAINTEXT_BYTES)?;
    let (encapsulated_key, mut context) = bitcoin_hpke::setup_sender::<
        ChaCha20Poly1305,
        HkdfSha256,
        Kem,
        _,
    >(&OpModeS::Base, public_key, info, &mut OsRng)
    .expect("setting up to a valid public key cannot fail");
    let ciphertext = context
        .seal(&plaintext, &[])
        .expect("the first message of a context cannot fail to seal");

    let mut payload = encode_key(&encapsulated_key).to_vec();
    payload.extend_from_slice(&ciphertext);
    debug_assert_eq!(payload.len(), PAYLOAD_BYTES);
    let keys = export_keys(
        |exporter_context, key| context.export(exporter_context, key),
        SENDER_TO_RECIPIENT,
        RECIPIENT_TO_SENDER,
    );
    Ok((payload, keys))
}

/// Decrypt `payload` with `secret_key` and return the message and the
/// recipient's keys.
pub fn open(secret_key: &SecretKey, info: &[u8], payload: &[u8]) -> Result<(Vec<u8>, Keys), Error> {
    if payload.len() != PAYLOAD_BYTES {
        return Err(Error::PayloadSize(payload.len()));
    }
    let (key, ciphertext) = payload.split_at(ENCAPSULATED_KEY_BYTES);
    let encapsulated_key = decode_key(key.try_into().expect("split at the key size"));
    let mut context = bitcoin_hpke::setup_receiver::<ChaCha20Poly1305, HkdfSha256, Kem>(
        &OpModeR::Base,
        secret_key,
        &encapsulated_key,
        info,
    )
    .map_err(|_| Error::Decryption)?;
    let plaintext = context
        .open(ciphertext, &[])
        .map_err(|_| Error::Decryption)?;
    let message = unpad(&plaintext)?;
    let keys = export_keys(
        |exporter_context, key| context.export(exporter_context, key),
        RECIPIENT_TO_SENDER,
        SENDER_TO_RECIPIENT,
    );
    Ok((message, keys))
}

/// Export the keys with `export`, given the exporter contexts of the
/// directions this party sends and receives in.
fn export_keys(
    export: impl Fn(&[u8], &mut [u8]) -> Result<(), HpkeError>,
    sending: &[u8],
    receiving: &[u8],
) -> Keys {
    let mut keys = Keys {
        sending: [0; KEY_BYTES],
        receiving: [0; KEY_BYTES],
    };
    export(sending, &mut keys.sending).expect("KEY_BYTES is a valid export length");
    export(receiving, &mut keys.receiving).expect("KEY_BYTES is a valid export length");
    keys
}

/// Encrypt `message` with `key` and return the payload. [`open_symmetric`]
/// needs the same `aad`.
pub fn seal_symmetric(key: &[u8; KEY_BYTES], aad: &[u8], message: &[u8]) -> Result<Vec<u8>, Error> {
    let plaintext = pad(message, SYMMETRIC_PLAINTEXT_BYTES)?;
    let mut nonce = [0; NONCE_BYTES];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = XChaCha20Poly1305::new(Key::from_slice(key))
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &plaintext,
                aad,
            },
        )
        .expect("a payload-sized plaintext cannot fail to encrypt");

    let mut payload = nonce.to_vec();
    payload.extend_from_slice(&ciphertext);
    debug_assert_eq!(payload.len(), PAYLOAD_BYTES);
    Ok(payload)
}

/// Decrypt `payload` with `key` and return the message.
pub fn open_symmetric(key: &[u8; KEY_BYTES], aad: &[u8], payload: &[u8]) -> Result<Vec<u8>, Error> {
    if payload.len() != PAYLOAD_BYTES {
        return Err(Error::PayloadSize(payload.len()));
    }
    let (nonce, ciphertext) = payload.split_at(NONCE_BYTES);
    let plaintext = XChaCha20Poly1305::new(Key::from_slice(key))
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| Error::Decryption)?;
    unpad(&plaintext)
}

/// Prefix `message` with its length and pad it with zeros to `size` bytes.
fn pad(message: &[u8], size: usize) -> Result<Vec<u8>, Error> {
    if message.len() > size - LENGTH_BYTES {
        return Err(Error::MessageTooLarge(message.len()));
    }
    let length = u16::try_from(message.len()).expect("PAYLOAD_BYTES fits in u16");
    let mut plaintext = Vec::with_capacity(size);
    plaintext.extend_from_slice(&length.to_le_bytes());
    plaintext.extend_from_slice(message);
    plaintext.resize(size, 0);
    Ok(plaintext)
}

/// Undo [`pad`].
fn unpad(plaintext: &[u8]) -> Result<Vec<u8>, Error> {
    let (length, rest) = plaintext.split_at(LENGTH_BYTES);
    let length = u16::from_le_bytes(length.try_into().expect("split at the length size"));
    Ok(rest
        .get(..usize::from(length))
        .ok_or(Error::MessageLength)?
        .to_vec())
}

fn encode_key(key: &EncappedKey) -> [u8; ENCAPSULATED_KEY_BYTES] {
    let key = secp256k1::PublicKey::from_slice(&key.to_bytes())
        .expect("an HPKE encapsulated key is a valid secp256k1 point");
    let mut randomness = [0; 32];
    OsRng.fill_bytes(&mut randomness);
    ElligatorSwift::from_pubkey_with_rnd(key, randomness).to_array()
}

fn decode_key(encoded: [u8; ENCAPSULATED_KEY_BYTES]) -> EncappedKey {
    let key = secp256k1::PublicKey::from_ellswift(ElligatorSwift::from_array(encoded));
    EncappedKey::from_bytes(&key.serialize_uncompressed())
        .expect("a decoded ElligatorSwift point is a valid secp256k1 point")
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    const INFO: &[u8] = b"info";

    fn keypair(seed: u8) -> (SecretKey, PublicKey) {
        derive_keypair(&[seed; 32])
    }

    #[test]
    fn messages_and_keys_round_trip() {
        let (secret, public) = keypair(1);
        for message in [&b""[..], b"message", &[0xff; MAX_MESSAGE_BYTES]] {
            let (payload, sender) = seal(&public, INFO, message).unwrap();
            assert_eq!(payload.len(), PAYLOAD_BYTES);
            let (opened, recipient) = open(&secret, INFO, &payload).unwrap();
            assert_eq!(opened, message);
            assert_eq!(sender.sending, recipient.receiving);
            assert_eq!(sender.receiving, recipient.sending);
        }
    }

    #[test]
    fn directions_have_different_keys() {
        let (_, public) = keypair(1);
        let (_, keys) = seal(&public, INFO, b"message").unwrap();
        assert_ne!(keys.sending, keys.receiving);
    }

    #[test]
    fn each_handshake_has_its_own_keys() {
        let (_, public) = keypair(1);
        let (_, first) = seal(&public, INFO, b"message").unwrap();
        let (_, second) = seal(&public, INFO, b"message").unwrap();
        assert_ne!(first.sending, second.sending);
        assert_ne!(first.receiving, second.receiving);
    }

    #[test]
    fn oversized_message_is_rejected() {
        let (_, public) = keypair(1);
        assert_eq!(
            seal(&public, INFO, &[0; MAX_MESSAGE_BYTES + 1]).err(),
            Some(Error::MessageTooLarge(MAX_MESSAGE_BYTES + 1))
        );
    }

    #[test]
    fn wrong_size_payload_is_rejected() {
        let (secret, public) = keypair(1);
        let (payload, _) = seal(&public, INFO, b"message").unwrap();
        for size in [PAYLOAD_BYTES - 1, PAYLOAD_BYTES + 1] {
            let mut payload = payload.clone();
            payload.resize(size, 0);
            assert_eq!(
                open(&secret, INFO, &payload).err(),
                Some(Error::PayloadSize(size))
            );
        }
    }

    #[test]
    fn wrong_key_or_info_is_rejected() {
        let (secret, public) = keypair(1);
        let (other_secret, _) = keypair(2);
        let (payload, _) = seal(&public, INFO, b"message").unwrap();
        assert_eq!(
            open(&other_secret, INFO, &payload).err(),
            Some(Error::Decryption)
        );
        assert_eq!(
            open(&secret, b"other", &payload).err(),
            Some(Error::Decryption)
        );
    }

    #[test]
    fn invalid_length_prefix_is_rejected() {
        let (secret, public) = keypair(1);
        let mut plaintext = vec![0; HANDSHAKE_PLAINTEXT_BYTES];
        plaintext[..LENGTH_BYTES].copy_from_slice(&u16::MAX.to_le_bytes());
        let (encapsulated_key, ciphertext) =
            bitcoin_hpke::single_shot_seal::<ChaCha20Poly1305, HkdfSha256, Kem, _>(
                &OpModeS::Base,
                &public,
                INFO,
                &plaintext,
                &[],
                &mut OsRng,
            )
            .unwrap();
        let mut payload = encode_key(&encapsulated_key).to_vec();
        payload.extend_from_slice(&ciphertext);
        assert_eq!(
            open(&secret, INFO, &payload).err(),
            Some(Error::MessageLength)
        );
    }

    /// Re-encoding the payload's key without randomness does not reproduce
    /// its bytes.
    #[test]
    fn encapsulated_key_encoding_is_randomized() {
        let (_, public) = keypair(1);
        let (payload, _) = seal(&public, INFO, b"message").unwrap();
        let encoded: [u8; ENCAPSULATED_KEY_BYTES] =
            payload[..ENCAPSULATED_KEY_BYTES].try_into().unwrap();
        let key = secp256k1::PublicKey::from_ellswift(ElligatorSwift::from_array(encoded));
        let deterministic = ElligatorSwift::from_pubkey_with_rnd(key, [0; 32]).to_array();
        assert_ne!(deterministic, encoded);
    }

    /// Every bit position takes both values across payloads from `seal`, as
    /// in rust-payjoin's `test_encrypted_payload_bit_uniformity`.
    ///
    /// Fails by chance with negligible probability.
    fn assert_payload_bits_vary(seal: impl Fn() -> Vec<u8>) {
        let first = seal();
        let mut differing = vec![0u8; PAYLOAD_BYTES];
        for _ in 0..80 {
            for (bits, (a, b)) in differing.iter_mut().zip(first.iter().zip(&seal())) {
                *bits |= a ^ b;
            }
        }
        assert!(differing.iter().all(|&bits| bits == 0xff));
    }

    #[test]
    fn payload_bits_vary() {
        let (_, public) = keypair(1);
        let message = [0; MAX_MESSAGE_BYTES];
        assert_payload_bits_vary(|| seal(&public, INFO, &message).unwrap().0);
    }

    const AAD: &[u8] = b"aad";

    fn handshake() -> (Keys, Keys) {
        let (secret, public) = keypair(1);
        let (payload, sender) = seal(&public, INFO, b"").unwrap();
        let (_, recipient) = open(&secret, INFO, &payload).unwrap();
        (sender, recipient)
    }

    #[test]
    fn symmetric_messages_round_trip_in_both_directions() {
        let (sender, recipient) = handshake();
        for message in [&b""[..], b"message", &[0xff; MAX_SYMMETRIC_MESSAGE_BYTES]] {
            for (from, to) in [(&sender, &recipient), (&recipient, &sender)] {
                let payload = seal_symmetric(&from.sending, AAD, message).unwrap();
                assert_eq!(payload.len(), PAYLOAD_BYTES);
                assert_eq!(
                    open_symmetric(&to.receiving, AAD, &payload).unwrap(),
                    message
                );
            }
        }
    }

    #[test]
    fn symmetric_wrong_key_or_aad_is_rejected() {
        let (sender, _) = handshake();
        let payload = seal_symmetric(&sender.sending, AAD, b"message").unwrap();
        assert_eq!(
            open_symmetric(&sender.receiving, AAD, &payload),
            Err(Error::Decryption)
        );
        assert_eq!(
            open_symmetric(&sender.sending, b"other", &payload),
            Err(Error::Decryption)
        );
    }

    #[test]
    fn symmetric_modified_payload_is_rejected() {
        let (sender, _) = handshake();
        let mut payload = seal_symmetric(&sender.sending, AAD, b"message").unwrap();
        payload[PAYLOAD_BYTES - 1] ^= 1;
        assert_eq!(
            open_symmetric(&sender.sending, AAD, &payload),
            Err(Error::Decryption)
        );
    }

    #[test]
    fn symmetric_oversized_message_is_rejected() {
        let (sender, _) = handshake();
        assert_eq!(
            seal_symmetric(&sender.sending, AAD, &[0; MAX_SYMMETRIC_MESSAGE_BYTES + 1]),
            Err(Error::MessageTooLarge(MAX_SYMMETRIC_MESSAGE_BYTES + 1))
        );
    }

    #[test]
    fn symmetric_wrong_size_payload_is_rejected() {
        let (sender, _) = handshake();
        let payload = seal_symmetric(&sender.sending, AAD, b"message").unwrap();
        for size in [PAYLOAD_BYTES - 1, PAYLOAD_BYTES + 1] {
            let mut payload = payload.clone();
            payload.resize(size, 0);
            assert_eq!(
                open_symmetric(&sender.sending, AAD, &payload),
                Err(Error::PayloadSize(size))
            );
        }
    }

    #[test]
    fn symmetric_payload_bits_vary() {
        let (sender, _) = handshake();
        let message = [0; MAX_SYMMETRIC_MESSAGE_BYTES];
        assert_payload_bits_vary(|| seal_symmetric(&sender.sending, AAD, &message).unwrap());
    }
}
