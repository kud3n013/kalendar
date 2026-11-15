//! The OpenPGP side of Proton: unlocking keys, decrypting what they
//! protect, and checking signatures.

use base64::Engine;
use pgp::composed::{CleartextSignedMessage, Deserializable, DetachedSignature, Message, MessageBuilder, PlainSessionKey, RawSessionKey, SignedPublicKey, SignedSecretKey};
use pgp::crypto::{hash::HashAlgorithm, sym::SymmetricKeyAlgorithm};
use pgp::packet::{Packet, PacketParser};
use pgp::types::{DecryptionKey, EskType, Password};

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

    /// A detached signature over `data`, armored.
    pub fn sign(&self, data: &[u8]) -> Result<String, String> {
        let sig = DetachedSignature::sign_binary_data(rand::thread_rng(), &self.key.primary_key, &self.pass, HashAlgorithm::Sha512, data).map_err(|e| e.to_string())?;
        sig.to_armored_string(Default::default()).map_err(|e| e.to_string())
    }

    /// The session key wrapped in `key_packet`, with its cipher.
    fn session_key(&self, key_packet: &[u8]) -> Result<(SymmetricKeyAlgorithm, RawSessionKey), String> {
        let wrapped = PacketParser::new(key_packet).filter_map(|p| match p {
            Ok(Packet::PublicKeyEncryptedSessionKey(p)) => Some(p),
            _ => None,
        });
        for p in wrapped {
            let Ok(values) = p.values() else { continue };
            let tries = std::iter::once(self.key.primary_key.decrypt(&self.pass, values, EskType::V3_4)).chain(self.key.secret_subkeys.iter().map(|k| k.key.decrypt(&self.pass, values, EskType::V3_4)));
            for t in tries {
                if let Ok(Ok(PlainSessionKey::V3_4 { sym_alg, key })) = &t {
                    return Ok((*sym_alg, key.clone()));
                }
            }
        }
        Err("The event's key could not be opened.".into())
    }

    fn encrypt(&self, data: &[u8], session: Option<(SymmetricKeyAlgorithm, RawSessionKey)>) -> Result<(Vec<u8>, Vec<u8>), String> {
        let public = self.key.to_public_key();
        let to = public.public_subkeys.first().ok_or("This key cannot encrypt.")?;
        let mut rng = rand::thread_rng();
        let alg = session.as_ref().map(|s| s.0).unwrap_or(SymmetricKeyAlgorithm::AES256);
        let mut b = MessageBuilder::from_bytes("", data.to_vec()).seipd_v1(&mut rng, alg);
        if let Some((_, key)) = session {
            b.set_session_key(key).map_err(|e| e.to_string())?;
        }
        b.encrypt_to_key(&mut rng, to).map_err(|e| e.to_string())?;
        let whole = b.to_vec(&mut rng).map_err(|e| e.to_string())?;
        let n = first_packet_len(&whole).ok_or("The encrypted event came out malformed.")?;
        Ok((whole[..n].to_vec(), whole[n..].to_vec()))
    }

    /// Encrypts `data` to this key under a fresh session key: the wrapped
    /// session key and the data, as two pieces.
    pub fn seal(&self, data: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
        self.encrypt(data, None)
    }

    /// Encrypts `data` under the session key already wrapped in
    /// `key_packet`, so that everyone holding that key can still read it.
    pub fn reseal(&self, key_packet: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
        let session = self.session_key(key_packet)?;
        Ok(self.encrypt(data, Some(session))?.1)
    }

    /// Whether `signature` (armored, detached) is this key's over `content`.
    pub fn signed(&self, signature: &str, content: &[u8]) -> bool {
        let Ok((sig, _)) = DetachedSignature::from_string(signature) else { return false };
        let public = self.key.to_public_key();
        sig.verify(&public, content).is_ok() || public.public_subkeys.iter().any(|k| sig.verify(k, content).is_ok())
    }
}

/// How many bytes the first OpenPGP packet takes, header included.
fn first_packet_len(b: &[u8]) -> Option<usize> {
    let tag = *b.first()?;
    let (head, body) = if tag & 0x40 != 0 {
        match *b.get(1)? {
            n @ 0..=191 => (2, n as usize),
            n @ 192..=223 => (3, ((n as usize - 192) << 8) + *b.get(2)? as usize + 192),
            255 => (6, u32::from_be_bytes(b.get(2..6)?.try_into().ok()?) as usize),
            // a length given piece by piece: not a session key packet
            _ => return None,
        }
    } else {
        match tag & 3 {
            0 => (2, *b.get(1)? as usize),
            1 => (3, u16::from_be_bytes(b.get(1..3)?.try_into().ok()?) as usize),
            2 => (5, u32::from_be_bytes(b.get(1..5)?.try_into().ok()?) as usize),
            _ => return None,
        }
    };
    (head + body <= b.len()).then_some(head + body)
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
    use pgp::composed::{EncryptionCaps, KeyType, SecretKeyParamsBuilder, SubkeyParamsBuilder};
    use pgp::crypto::ecc_curve::ECCCurve;

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
        let sig = DetachedSignature::sign_binary_data(rand::thread_rng(), &key.primary_key, &Password::from(pass), HashAlgorithm::Sha256, data).unwrap();
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

        // what the key seals and signs itself
        let (kp2, data2) = key.seal(b"first").unwrap();
        assert_eq!(first_packet_len(&kp2), Some(kp2.len()));
        assert_eq!(key.decrypt_split(&kp2, &data2).unwrap(), b"first");
        // new text under the same session key reads with the old key packet
        let data3 = key.reseal(&kp2, b"second").unwrap();
        assert_ne!(data3, data2);
        assert_eq!(key.decrypt_split(&kp2, &data3).unwrap(), b"second");
        assert!(key.signed(&key.sign(b"text").unwrap(), b"text"));
        assert!(!key.signed(&key.sign(b"text").unwrap(), b"texts"));

        let other = Key::open(&make_key("x"), b"x").unwrap();
        assert!(other.decrypt_split(&kp, &data).is_err());
        assert_eq!(any(&[other, key], |k| k.decrypt_split(&kp, &data)).unwrap(), b"BEGIN:VCALENDAR");
    }
}
