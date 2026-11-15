//! The OpenPGP side of Proton: unlocking keys, decrypting what they
//! protect, and checking signatures.

use base64::Engine;
use pgp::composed::{CleartextSignedMessage, Deserializable, DetachedSignature, Message, SignedPublicKey, SignedSecretKey};
use pgp::types::Password;

/// The key Proton signs its sign-in modulus with; it is part of every
/// Proton client (from go-srp, Copyright (c) 2019 Proton Technologies AG,
/// MIT License).
const MODULUS_KEY: &str = "-----BEGIN PGP PUBLIC KEY BLOCK-----\r\n\r\nxjMEXAHLgxYJKwYBBAHaRw8BAQdAFurWXXwjTemqjD7CXjXVyKf0of7n9Ctm\r\nL8v9enkzggHNEnByb3RvbkBzcnAubW9kdWx1c8J3BBAWCgApBQJcAcuDBgsJ\r\nBwgDAgkQNQWFxOlRjyYEFQgKAgMWAgECGQECGwMCHgEAAPGRAP9sauJsW12U\r\nMnTQUZpsbJb53d0Wv55mZIIiJL2XulpWPQD/V6NglBd96lZKBmInSXX/kXat\r\nSv+y0io+LR8i2+jV+AbOOARcAcuDEgorBgEEAZdVAQUBAQdAeJHUz1c9+KfE\r\nkSIgcBRE3WuXC4oj5a2/U3oASExGDW4DAQgHwmEEGBYIABMFAlwBy4MJEDUF\r\nhcTpUY8mAhsMAAD/XQD8DxNI6E78meodQI+wLsrKLeHn32iLvUqJbVDhfWSU\r\nWO4BAMcm1u02t4VKw++ttECPt+HUgPUq5pqQWe5Q2cW4TMsE\r\n=Y4Mw\r\n-----END PGP PUBLIC KEY BLOCK-----";

pub fn b64(text: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD.decode(text.trim()).map_err(|_| "Proton sent something that is not base64.".to_string())
}

pub fn to_b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// The sign-in modulus out of its signed wrapping, once the signature
/// is shown to be Proton's.
pub fn signed_modulus(signed: &str) -> Result<Vec<u8>, String> {
    let bad = || "Proton's sign-in numbers are not signed by Proton.".to_string();
    let (key, _) = SignedPublicKey::from_string(MODULUS_KEY).map_err(|e| e.to_string())?;
    let (msg, _) = CleartextSignedMessage::from_string(signed.trim()).map_err(|_| bad())?;
    msg.verify(&key).map_err(|_| bad())?;
    b64(&msg.signed_text())
}

/// A private key with what unlocks it.
pub struct Key {
    key: SignedSecretKey,
    pass: Password,
}

impl Key {
    /// Reads an armored private key and makes sure `passphrase` opens it.
    pub fn open(armored: &str, passphrase: &[u8]) -> Result<Key, String> {
        let (key, _) = SignedSecretKey::from_string(armored).map_err(|e| format!("A Proton key could not be read ({e})."))?;
        let pass = Password::from(passphrase);
        match key.primary_key.unlock(&pass, |_, _| Ok(())) {
            Ok(Ok(())) => Ok(Key { key, pass }),
            _ => Err("The password does not open this key.".into()),
        }
    }

    fn read(&self, msg: Message) -> Result<Vec<u8>, String> {
        let mut plain = msg.decrypt(&self.pass, &self.key).map_err(|e| e.to_string())?;
        if plain.is_compressed() {
            plain = plain.decompress().map_err(|e| e.to_string())?;
        }
        plain.as_data_vec().map_err(|e| e.to_string())
    }

    /// Decrypts an armored message.
    pub fn decrypt(&self, armored: &str) -> Result<Vec<u8>, String> {
        let (msg, _) = Message::from_string(armored).map_err(|e| e.to_string())?;
        self.read(msg)
    }

    /// Decrypts a message that travels in two pieces: the wrapped session
    /// key and the data.
    pub fn decrypt_split(&self, key_packet: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
        let mut whole = key_packet.to_vec();
        whole.extend_from_slice(data);
        let msg = Message::from_bytes(&whole[..]).map_err(|e| e.to_string())?;
        self.read(msg)
    }

    /// Whether `signature` (armored, detached) is this key's over `content`.
    pub fn signed(&self, signature: &str, content: &[u8]) -> bool {
        let Ok((sig, _)) = DetachedSignature::from_string(signature) else { return false };
        let public = self.key.to_public_key();
        sig.verify(&public, content).is_ok() || public.public_subkeys.iter().any(|k| sig.verify(k, content).is_ok())
    }
}

/// The first answer any of the keys gives.
pub fn any<'a, T>(keys: impl IntoIterator<Item = &'a Key>, mut f: impl FnMut(&Key) -> Result<T, String>) -> Result<T, String> {
    let mut last = "No key is at hand.".to_string();
    for k in keys {
        match f(k) {
            Ok(v) => return Ok(v),
            Err(e) => last = e,
        }
    }
    Err(last)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use pgp::composed::{EncryptionCaps, KeyType, MessageBuilder, SecretKeyParamsBuilder, SubkeyParamsBuilder};
    use pgp::crypto::{ecc_curve::ECCCurve, sym::SymmetricKeyAlgorithm};

    /// A fresh key like Proton's, locked with `pass`, armored.
    pub fn make_key(pass: &str) -> String {
        let mut params = SecretKeyParamsBuilder::default();
        params
            .key_type(KeyType::Ed25519Legacy)
            .can_certify(true)
            .can_sign(true)
            .primary_user_id("Test <test@example.com>".into())
            .passphrase(Some(pass.to_string()))
            .subkeys(vec![SubkeyParamsBuilder::default()
                .key_type(KeyType::ECDH(ECCCurve::Curve25519Legacy))
                .can_encrypt(EncryptionCaps::All)
                .passphrase(Some(pass.to_string()))
                .build()
                .unwrap()]);
        let key = params.build().unwrap().generate(rand::thread_rng()).unwrap();
        key.to_armored_string(Default::default()).unwrap()
    }

    /// `data` encrypted to the key, as (session key piece, data piece).
    pub fn seal(armored_key: &str, data: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let (key, _) = SignedSecretKey::from_string(armored_key).unwrap();
        let public = key.to_public_key();
        let mut rng = rand::thread_rng();
        let mut b = MessageBuilder::from_bytes("", data.to_vec()).seipd_v1(&mut rng, SymmetricKeyAlgorithm::AES256);
        b.encrypt_to_key(&mut rng, &public.public_subkeys[0]).unwrap();
        let whole = b.to_vec(&mut rng).unwrap();
        // the first packet is the wrapped session key: a new-format header
        // with a one-byte length
        assert_eq!(whole[0], 0xC1);
        let n = 2 + whole[1] as usize;
        (whole[..n].to_vec(), whole[n..].to_vec())
    }

    /// The same as one armored message.
    pub fn seal_armored(armored_key: &str, data: &[u8]) -> String {
        let (key, _) = SignedSecretKey::from_string(armored_key).unwrap();
        let public = key.to_public_key();
        let mut rng = rand::thread_rng();
        let mut b = MessageBuilder::from_bytes("", data.to_vec()).seipd_v1(&mut rng, SymmetricKeyAlgorithm::AES256);
        b.encrypt_to_key(&mut rng, &public.public_subkeys[0]).unwrap();
        b.to_armored_string(&mut rng, Default::default()).unwrap()
    }

    /// A detached signature by the key over `data`, armored.
    pub fn sign(armored_key: &str, pass: &str, data: &[u8]) -> String {
        let (key, _) = SignedSecretKey::from_string(armored_key).unwrap();
        let sig = DetachedSignature::sign_binary_data(rand::thread_rng(), &key.primary_key, &Password::from(pass), pgp::crypto::hash::HashAlgorithm::Sha256, data).unwrap();
        sig.to_armored_string(Default::default()).unwrap()
    }

    #[test]
    fn the_modulus_must_be_protons() {
        // from go-srp's own tests
        let signed = "-----BEGIN PGP SIGNED MESSAGE-----\nHash: SHA256\n\nW2z5HBi8RvsfYzZTS7qBaUxxPhsfHJFZpu3Kd6s1JafNrCCH9rfvPLrfuqocxWPgWDH2R8neK7PkNvjxto9TStuY5z7jAzWRvFWN9cQhAKkdWgy0JY6ywVn22+HFpF4cYesHrqFIKUPDMSSIlWjBVmEJZ/MusD44ZT29xcPrOqeZvwtCffKtGAIjLYPZIEbZKnDM1Dm3q2K/xS5h+xdhjnndhsrkwm9U9oyA2wxzSXFL+pdfj2fOdRwuR5nW0J2NFrq3kJjkRmpO/Genq1UW+TEknIWAb6VzJJJA244K/H8cnSx2+nSNZO3bbo6Ys228ruV9A8m6DhxmS+bihN3ttQ==\n-----BEGIN PGP SIGNATURE-----\nVersion: ProtonMail\nComment: https://protonmail.com\n\nwl4EARYIABAFAlwB1j0JEDUFhcTpUY8mAAD8CgEAnsFnF4cF0uSHKkXa1GIa\nGO86yMV4zDZEZcDSJo0fgr8A/AlupGN9EdHlsrZLmTA1vhIx+rOgxdEff28N\nkvNM7qIK\n=q6vu\n-----END PGP SIGNATURE-----";
        let modulus = signed_modulus(signed).unwrap();
        assert_eq!(modulus.len(), 256);
        assert_eq!(to_b64(&modulus), crate::sync::proton_srp::tests::MODULUS);
        // one letter of the modulus changed
        assert!(signed_modulus(&signed.replacen("W2z5", "W2z6", 1)).is_err());
        assert!(signed_modulus("hello").is_err());
    }

    #[test]
    fn keys_open_decrypt_and_check() {
        let armored = make_key("secret");
        assert!(Key::open(&armored, b"wrong").is_err());
        let key = Key::open(&armored, b"secret").unwrap();

        let (kp, data) = seal(&armored, b"BEGIN:VCALENDAR");
        assert_eq!(key.decrypt_split(&kp, &data).unwrap(), b"BEGIN:VCALENDAR");
        assert_eq!(key.decrypt(&seal_armored(&armored, b"pass")).unwrap(), b"pass");

        let sig = sign(&armored, "secret", b"pass");
        assert!(key.signed(&sig, b"pass"));
        assert!(!key.signed(&sig, b"other"));

        let other = Key::open(&make_key("x"), b"x").unwrap();
        assert!(other.decrypt_split(&kp, &data).is_err());
        assert_eq!(any(&[other, key], |k| k.decrypt_split(&kp, &data)).unwrap(), b"BEGIN:VCALENDAR");
    }
}
