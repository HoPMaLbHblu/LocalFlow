//! End-to-end encryption between LocalFlow and a paired phone (see PROTOCOL.md in LocalFlow Remote).
//!
//! Each connection: both sides send an ephemeral X25519 key. The session key comes from the
//! static-static secret (who you are) and the ephemeral-ephemeral secret (forward secrecy):
//! `HKDF-SHA256(ss || ee, salt "lfremote v1", info pc_id || device_id || e_phone || e_pc)`.
//! Messages use XChaCha20-Poly1305 with a per-direction counter as nonce; the receiver only
//! accepts strictly increasing counters, so recorded messages can't be replayed.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use chacha20poly1305::{aead::Aead, KeyInit, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

pub const SALT: &[u8] = b"lfremote v1";

pub fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).expect("system random number generator");
    b
}

pub fn b64(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

pub fn unb64(text: &str) -> Option<Vec<u8>> {
    B64.decode(text.trim()).ok()
}

pub fn unb64_32(text: &str) -> Option<[u8; 32]> {
    unb64(text)?.try_into().ok()
}

pub fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// A static key pair (the PC's or a phone's).
pub struct KeyPair {
    pub secret: StaticSecret,
    pub public: [u8; 32],
}

impl KeyPair {
    pub fn generate() -> Self {
        Self::from_secret(random())
    }

    pub fn from_secret(bytes: [u8; 32]) -> Self {
        let secret = StaticSecret::from(bytes);
        let public = PublicKey::from(&secret).to_bytes();
        KeyPair { secret, public }
    }

    pub fn secret_bytes(&self) -> [u8; 32] {
        self.secret.to_bytes()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum CryptoError {
    /// Wrong key, tampered message, or a replayed/out-of-order counter.
    Rejected,
    Malformed,
}

/// Which side of the session we are.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Pc,
    Phone,
}

/// An established, encrypted session in one direction pair.
pub struct Session {
    send: XChaCha20Poly1305,
    recv: XChaCha20Poly1305,
    send_counter: u64,
    /// The next counter we accept (anything lower is a replay).
    recv_next: u64,
}

impl Session {
    /// Derive the session from both static and both ephemeral keys.
    #[allow(clippy::too_many_arguments)]
    pub fn derive(
        side: Side,
        my_static: &StaticSecret,
        their_static: &[u8; 32],
        my_ephemeral: &StaticSecret,
        their_ephemeral: &[u8; 32],
        pc_id: &[u8],
        device_id: &[u8],
    ) -> Result<Self, CryptoError> {
        let ss = my_static.diffie_hellman(&PublicKey::from(*their_static));
        let ee = my_ephemeral.diffie_hellman(&PublicKey::from(*their_ephemeral));
        // An all-zero result means a low-order (malicious) public key.
        if !ss.was_contributory() || !ee.was_contributory() {
            return Err(CryptoError::Rejected);
        }
        let my_e = PublicKey::from(my_ephemeral).to_bytes();
        let (e_phone, e_pc) = match side {
            Side::Phone => (my_e, *their_ephemeral),
            Side::Pc => (*their_ephemeral, my_e),
        };
        let mut ikm = Vec::with_capacity(64);
        ikm.extend_from_slice(ss.as_bytes());
        ikm.extend_from_slice(ee.as_bytes());
        let mut info = Vec::with_capacity(pc_id.len() + device_id.len() + 64);
        info.extend_from_slice(pc_id);
        info.extend_from_slice(device_id);
        info.extend_from_slice(&e_phone);
        info.extend_from_slice(&e_pc);
        let mut okm = [0u8; 64];
        Hkdf::<Sha256>::new(Some(SALT), &ikm).expand(&info, &mut okm).map_err(|_| CryptoError::Malformed)?;
        let phone_to_pc = XChaCha20Poly1305::new((&okm[..32]).into());
        let pc_to_phone = XChaCha20Poly1305::new((&okm[32..]).into());
        let (send, recv) = match side {
            Side::Phone => (phone_to_pc, pc_to_phone),
            Side::Pc => (pc_to_phone, phone_to_pc),
        };
        Ok(Session { send, recv, send_counter: 0, recv_next: 0 })
    }

    fn nonce(counter: u64) -> XNonce {
        let mut n = [0u8; 24];
        n[16..].copy_from_slice(&counter.to_be_bytes());
        n.into()
    }

    /// Encrypt one message: base64url(nonce || ciphertext).
    pub fn seal(&mut self, plaintext: &[u8]) -> String {
        let nonce = Self::nonce(self.send_counter);
        self.send_counter += 1;
        let ct = self.send.encrypt(&nonce, plaintext).expect("encryption cannot fail");
        let mut out = nonce.to_vec();
        out.extend_from_slice(&ct);
        b64(&out)
    }

    /// Decrypt one message, enforcing strictly increasing counters.
    pub fn open(&mut self, frame: &str) -> Result<Vec<u8>, CryptoError> {
        let raw = unb64(frame).ok_or(CryptoError::Malformed)?;
        if raw.len() < 24 + 16 {
            return Err(CryptoError::Malformed);
        }
        let (nonce, ct) = raw.split_at(24);
        if nonce[..16].iter().any(|&b| b != 0) {
            return Err(CryptoError::Malformed);
        }
        let counter = u64::from_be_bytes(nonce[16..].try_into().unwrap());
        if counter < self.recv_next {
            return Err(CryptoError::Rejected);
        }
        let plain = self.recv.decrypt(XNonce::from_slice(nonce), ct).map_err(|_| CryptoError::Rejected)?;
        self.recv_next = counter + 1;
        Ok(plain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (Session, Session) {
        let pc = KeyPair::generate();
        let phone = KeyPair::generate();
        let pc_e = KeyPair::generate();
        let phone_e = KeyPair::generate();
        let pc_s = Session::derive(Side::Pc, &pc.secret, &phone.public, &pc_e.secret, &phone_e.public, b"pc", b"dev").unwrap();
        let ph_s = Session::derive(Side::Phone, &phone.secret, &pc.public, &phone_e.secret, &pc_e.public, b"pc", b"dev").unwrap();
        (pc_s, ph_s)
    }

    #[test]
    fn both_directions_round_trip() {
        let (mut pc, mut phone) = pair();
        let a = phone.seal(b"hello pc");
        assert_eq!(pc.open(&a).unwrap(), b"hello pc");
        let b = pc.seal(b"hello phone");
        assert_eq!(phone.open(&b).unwrap(), b"hello phone");
    }

    #[test]
    fn replays_and_tampering_are_rejected() {
        let (mut pc, mut phone) = pair();
        let a = phone.seal(b"one");
        assert!(pc.open(&a).is_ok());
        assert_eq!(pc.open(&a), Err(CryptoError::Rejected), "replay");
        let mut raw = unb64(&phone.seal(b"two")).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 1;
        assert_eq!(pc.open(&b64(&raw)), Err(CryptoError::Rejected), "tampered");
        // Its own messages can't be fed back to it (different direction key).
        let mine = pc.seal(b"x");
        assert!(pc.open(&mine).is_err());
    }

    #[test]
    fn a_stranger_cannot_join() {
        let pc = KeyPair::generate();
        let phone = KeyPair::generate();
        let stranger = KeyPair::generate();
        let (pc_e, ph_e) = (KeyPair::generate(), KeyPair::generate());
        // PC thinks it talks to `phone`; the stranger uses its own static key.
        let mut pc_s = Session::derive(Side::Pc, &pc.secret, &phone.public, &pc_e.secret, &ph_e.public, b"pc", b"d").unwrap();
        let mut st = Session::derive(Side::Phone, &stranger.secret, &pc.public, &ph_e.secret, &pc_e.public, b"pc", b"d").unwrap();
        assert!(pc_s.open(&st.seal(b"run everything")).is_err());
    }

    #[test]
    fn low_order_keys_are_refused() {
        let me = KeyPair::generate();
        let e = KeyPair::generate();
        assert!(Session::derive(Side::Pc, &me.secret, &[0u8; 32], &e.secret, &[0u8; 32], b"p", b"d").is_err());
    }
}
