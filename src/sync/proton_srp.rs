//! Proton's way of proving a password without sending it (SRP), worked out
//! from Proton's own MIT-licensed go-srp. All numbers travel little-endian.
//! The test values and the method are from go-srp, Copyright (c) 2019
//! Proton Technologies AG, MIT License.

use crate::strings::{tr, trf};
use num_bigint::BigUint;
use sha2::{Digest, Sha512};

/// How many bytes every number takes on the wire.
const LEN: usize = 256;

/// What goes to the server, and what it must answer.
pub struct Proofs {
    pub client_ephemeral: Vec<u8>,
    pub client_proof: Vec<u8>,
    pub expected_server_proof: Vec<u8>,
}

/// Four SHA-512 runs side by side: 256 bytes out of anything.
pub fn expand_hash(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(LEN);
    for i in 0u8..4 {
        let mut h = Sha512::new();
        h.update(data);
        h.update([i]);
        out.extend_from_slice(&h.finalize());
    }
    out
}

/// The bcrypt text for a password and sixteen bytes of salt.
fn bcrypt_text(password: &[u8], salt: &[u8]) -> Result<String, String> {
    let salt: [u8; 16] = salt.try_into().map_err(|_| tr("Proton sent a salt of the wrong length.").to_string())?;
    let parts = bcrypt::hash_with_salt(password, 10, salt).map_err(|e| e.to_string())?;
    Ok(parts.format_for_version(bcrypt::Version::TwoY))
}

/// The password as the sign-in maths wants it. Only today's accounts
/// (versions 3 and 4) are understood.
pub fn hash_password(version: i64, password: &[u8], salt: &[u8], modulus: &[u8]) -> Result<Vec<u8>, String> {
    if version < 3 {
        return Err(tr("This Proton account uses an old sign-in. Sign in once at proton.me, then try again.").into());
    }
    let mut salted = salt.to_vec();
    salted.extend_from_slice(b"proton");
    let mut data = bcrypt_text(password, &salted)?.into_bytes();
    data.extend_from_slice(modulus);
    Ok(expand_hash(&data))
}

/// What unlocks the account's keys: the tail of the bcrypt text.
pub fn key_passphrase(password: &[u8], salt: &[u8]) -> Result<String, String> {
    let text = bcrypt_text(password, salt)?;
    Ok(text[text.len() - 31..].to_string())
}

fn le(n: &BigUint) -> Vec<u8> {
    let mut b = n.to_bytes_le();
    b.resize(LEN, 0);
    b
}

fn random_below(limit: &BigUint) -> Result<BigUint, String> {
    let mut buf = [0u8; LEN + 8];
    getrandom::fill(&mut buf).map_err(|e| e.to_string())?;
    Ok(BigUint::from_bytes_le(&buf) % limit)
}

/// The multiplier both sides derive from the modulus.
fn multiplier(n: &BigUint) -> BigUint {
    let mut data = le(&BigUint::from(2u8));
    data.extend_from_slice(&le(n));
    BigUint::from_bytes_le(&expand_hash(&data)) % n
}

/// Works out the proofs for one sign-in. `secret` is only given by tests.
pub fn proofs(hashed: &[u8], modulus: &[u8], server_ephemeral: &[u8], secret: Option<BigUint>) -> Result<Proofs, String> {
    let bad = |what: &str| trf("Proton's sign-in numbers are wrong ({}).", &[&what]);
    let one = BigUint::from(1u8);
    let two = BigUint::from(2u8);
    let n = BigUint::from_bytes_le(modulus);
    if n.bits() != (LEN * 8) as u64 {
        return Err(bad("size"));
    }
    if &n % 8u8 != BigUint::from(3u8) {
        return Err(bad("shape"));
    }
    let n1 = &n - &one;
    // with (N-1)/2 prime this shows N prime and 2 a generator of the group
    if two.modpow(&(&n >> 1), &n) != n1 {
        return Err(bad("not prime"));
    }
    let b = BigUint::from_bytes_le(server_ephemeral);
    if b <= one || b >= n1 {
        return Err(bad("server number"));
    }
    let k = multiplier(&n);
    if k <= one || k >= n1 {
        return Err(bad("multiplier"));
    }
    let x = BigUint::from_bytes_le(hashed);
    loop {
        let a = match &secret {
            Some(a) => a.clone(),
            None => random_below(&n1)?,
        };
        if a <= BigUint::from((LEN * 16) as u32) {
            continue;
        }
        let big_a = le(&two.modpow(&a, &n));
        let mut ab = big_a.clone();
        ab.extend_from_slice(server_ephemeral);
        let u = BigUint::from_bytes_le(&expand_hash(&ab));
        if u == BigUint::from(0u8) {
            continue;
        }
        let kgx = (&k * two.modpow(&x, &n)) % &n;
        let base = (&b + &n - kgx) % &n;
        let exp = (&u * &x + &a) % &n1;
        let shared = le(&base.modpow(&exp, &n));
        let mut m1 = big_a.clone();
        m1.extend_from_slice(server_ephemeral);
        m1.extend_from_slice(&shared);
        let client_proof = expand_hash(&m1);
        let mut m2 = big_a.clone();
        m2.extend_from_slice(&client_proof);
        m2.extend_from_slice(&shared);
        return Ok(Proofs { client_ephemeral: big_a, client_proof, expected_server_proof: expand_hash(&m2) });
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use base64::Engine;

    /// The modulus from go-srp's own tests.
    pub const MODULUS: &str = "W2z5HBi8RvsfYzZTS7qBaUxxPhsfHJFZpu3Kd6s1JafNrCCH9rfvPLrfuqocxWPgWDH2R8neK7PkNvjxto9TStuY5z7jAzWRvFWN9cQhAKkdWgy0JY6ywVn22+HFpF4cYesHrqFIKUPDMSSIlWjBVmEJZ/MusD44ZT29xcPrOqeZvwtCffKtGAIjLYPZIEbZKnDM1Dm3q2K/xS5h+xdhjnndhsrkwm9U9oyA2wxzSXFL+pdfj2fOdRwuR5nW0J2NFrq3kJjkRmpO/Genq1UW+TEknIWAb6VzJJJA244K/H8cnSx2+nSNZO3bbo6Ys228ruV9A8m6DhxmS+bihN3ttQ==";

    fn b64(s: &str) -> Vec<u8> {
        base64::engine::general_purpose::STANDARD.decode(s).unwrap()
    }

    #[test]
    fn bcrypt_matches_protons_values() {
        // from go-srp's hash_test.go: the salt in bcrypt's own alphabet
        let alphabet = base64::alphabet::Alphabet::new("./ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789").unwrap();
        let engine = base64::engine::GeneralPurpose::new(
            &alphabet,
            base64::engine::GeneralPurposeConfig::new()
                .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent)
                .with_decode_allow_trailing_bits(true),
        );
        for (salt, want) in [
            ("PTTsDBs/mlLnSk6VmtFghe", "$2y$10$PTTsDBs/mlLnSk6VmtFgheNSiK/lSwtJsrBLLDK3kZYI7193nInqy"),
            ("4DZHd6WZX4fEaWKtCfYdde", "$2y$10$4DZHd6WZX4fEaWKtCfYddeZfcryISo9eEMgbA90O.Wnnz1s1VKmKC"),
            ("/.3KXCwRnsrxURMGxN7.R.", "$2y$10$/.3KXCwRnsrxURMGxN7.R.GLpVq0zyBbI9wgS0wB2U/g2btx1RYoy"),
        ] {
            let raw = engine.decode(salt).unwrap();
            assert_eq!(bcrypt_text(b"test!!!", &raw).unwrap(), want);
            assert_eq!(key_passphrase(b"test!!!", &raw).unwrap(), &want[29..]);
        }
    }

    #[test]
    fn a_server_agrees_with_the_proofs() {
        let modulus = b64(MODULUS);
        let n = BigUint::from_bytes_le(&modulus);
        let two = BigUint::from(2u8);
        let salt = [7u8; 10];
        let hashed = hash_password(4, "Password\nabc!!~~ä".as_bytes(), &salt, &modulus).unwrap();
        // the server's side: it only holds the verifier
        let x = BigUint::from_bytes_le(&hashed);
        let v = two.modpow(&x, &n);
        let b = BigUint::from_bytes_le(&[0x5a; 200]);
        let k = multiplier(&n);
        let big_b = le(&((&k * &v + two.modpow(&b, &n)) % &n));

        let p = proofs(&hashed, &modulus, &big_b, None).unwrap();
        let mut ab = p.client_ephemeral.clone();
        ab.extend_from_slice(&big_b);
        let u = BigUint::from_bytes_le(&expand_hash(&ab));
        let big_a = BigUint::from_bytes_le(&p.client_ephemeral);
        let shared = le(&((big_a * v.modpow(&u, &n)) % &n).modpow(&b, &n));
        let mut m1 = p.client_ephemeral.clone();
        m1.extend_from_slice(&big_b);
        m1.extend_from_slice(&shared);
        assert_eq!(expand_hash(&m1), p.client_proof);

        // a wrong password gives another proof
        let other = hash_password(4, b"nope", &salt, &modulus).unwrap();
        let q = proofs(&other, &modulus, &big_b, Some(BigUint::from(99999u32))).unwrap();
        assert_ne!(q.client_proof, p.client_proof);
    }

    #[test]
    fn bad_numbers_are_refused() {
        let modulus = b64(MODULUS);
        let hashed = vec![1u8; 256];
        assert!(proofs(&hashed, &modulus[..200], &[5u8; 256], None).is_err());
        assert!(proofs(&hashed, &modulus, &[0u8; 256], None).is_err());
        let mut even = modulus.clone();
        even[0] ^= 1;
        assert!(proofs(&hashed, &even, &[5u8; 256], None).is_err());
        assert!(hash_password(2, b"x", &[0; 10], &modulus).is_err());
    }
}
