//! Lunii **V3** pack ciphering (pure bytes-in/bytes-out, no I/O).
//!
//! V3 firmware ciphers only the **first 512 bytes** of each content file with
//! **AES-128-CBC** (the remainder is plaintext — which is why the tails of
//! MP3/BMP assets stay directly readable). The per-device content key + IV are
//! stored in the device root `.md` (metadata v7): a 16-byte key at offset
//! `0x40` and a 16-byte IV at `0x50`, each stored little-endian per 32-bit word
//! — so they must be byte-swapped within each 4-byte group before use
//! (`reverse_bytes_per_u32`).
//!
//! No hardware key-dump is needed to (re)cipher YOUR OWN packs for a device:
//! the content key is read from that device's own `.md`. (Decrypting a
//! store-bought pack's per-pack key would need the device's secret key, which
//! is out of scope — this module only handles the content key.)
//!
//! Cross-checked against Lunii.QT (`pkg/api/stories.py`, `pkg/api/aes_keys.py`)
//! and STUdio (`AESCBCCipher.java`): AES/CBC/NoPadding, key+IV word-swapped,
//! first-512 region. Validated empirically against a real device by
//! `deciphers_a_real_device_ri_to_the_000_marker` (a deciphered `ri` begins
//! with the ASCII resource-index marker `"000"`).

use aes::cipher::block_padding::NoPadding;
use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use aes::Aes128;

type Aes128CbcEnc = cbc::Encryptor<Aes128>;
type Aes128CbcDec = cbc::Decryptor<Aes128>;

/// Bytes ciphered at the head of a V3 content file; the rest is plaintext.
pub const V3_CIPHER_REGION: usize = 512;

/// The device `.md` offset of the V3 story key (16 bytes) and, right after it,
/// the story IV (16 bytes).
const MD_STORY_KEY_OFFSET: usize = 0x40;
const MD_STORY_IV_OFFSET: usize = 0x50;
const AES_KEY_LEN: usize = 16;

/// Swap the bytes WITHIN each 4-byte group: `[b0 b1 b2 b3 | b4 …] →
/// [b3 b2 b1 b0 | …]`. The V3 key/IV are stored little-endian per 32-bit word
/// on the device; AES wants them big-endian, so every 4-byte word is reversed.
/// A trailing partial group (len not a multiple of 4) is reversed in place too,
/// matching Lunii.QT's `reverse_bytes` (it never happens for a 16-byte key).
pub fn reverse_bytes_per_u32(bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    for group in out.chunks_mut(4) {
        group.reverse();
    }
    out
}

/// Read the V3 (metadata v7) content key + IV from a device `.md` payload:
/// 16 bytes at `0x40` (key) and 16 at `0x50` (IV), each word-swapped. `None`
/// when the payload is too short to carry them (a malformed / non-v7 `.md`).
pub fn v3_story_key_iv(md: &[u8]) -> Option<([u8; AES_KEY_LEN], [u8; AES_KEY_LEN])> {
    if md.len() < MD_STORY_IV_OFFSET + AES_KEY_LEN {
        return None;
    }
    let key = reverse_bytes_per_u32(&md[MD_STORY_KEY_OFFSET..MD_STORY_KEY_OFFSET + AES_KEY_LEN]);
    let iv = reverse_bytes_per_u32(&md[MD_STORY_IV_OFFSET..MD_STORY_IV_OFFSET + AES_KEY_LEN]);
    Some((key.try_into().ok()?, iv.try_into().ok()?))
}

/// The 16-byte-aligned prefix actually ciphered for a file of length `len`:
/// `min(512, len)` rounded DOWN to a multiple of the AES block size. Rounding
/// down (never padding up) keeps the file length unchanged so the region
/// round-trips exactly; the few trailing bytes below the 512 boundary stay
/// plaintext (the device treats them the same way).
fn ciphered_prefix_len(len: usize) -> usize {
    let region = len.min(V3_CIPHER_REGION);
    region - (region % 16)
}

/// Decipher, in place, the ciphered head of a V3 content file. Bytes past the
/// first-512 aligned region are left untouched. A no-op for a file shorter than
/// one AES block.
pub fn v3_decipher_in_place(data: &mut [u8], key: &[u8; AES_KEY_LEN], iv: &[u8; AES_KEY_LEN]) {
    let n = ciphered_prefix_len(data.len());
    if n == 0 {
        return;
    }
    // NoPadding on an exact block-multiple slice never errors.
    let _ =
        Aes128CbcDec::new(key.into(), iv.into()).decrypt_padded_mut::<NoPadding>(&mut data[..n]);
}

/// Cipher, in place, the head of a V3 content file — the exact inverse of
/// [`v3_decipher_in_place`], over the same first-512 aligned region.
pub fn v3_cipher_in_place(data: &mut [u8], key: &[u8; AES_KEY_LEN], iv: &[u8; AES_KEY_LEN]) {
    let n = ciphered_prefix_len(data.len());
    if n == 0 {
        return;
    }
    let _ =
        Aes128CbcEnc::new(key.into(), iv.into()).encrypt_padded_mut::<NoPadding>(&mut data[..n], n);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverse_bytes_per_u32_swaps_within_each_word() {
        assert_eq!(
            reverse_bytes_per_u32(&[0x96, 0x77, 0x56, 0xd5, 0xd5, 0xd9, 0x15, 0x2a]),
            vec![0xd5, 0x56, 0x77, 0x96, 0x2a, 0x15, 0xd9, 0xd5],
        );
    }

    #[test]
    fn v3_story_key_iv_reads_and_word_swaps_the_md_slots() {
        let mut md = vec![0u8; 0x60];
        md[0x40..0x50].copy_from_slice(&[
            0x96, 0x77, 0x56, 0xd5, 0xd5, 0xd9, 0x15, 0x2a, 0xbf, 0x13, 0x30, 0x00, 0xcd, 0x91,
            0xc0, 0x3b,
        ]);
        md[0x50..0x60].copy_from_slice(&[
            0x06, 0x6b, 0x10, 0x91, 0xf0, 0x57, 0xca, 0x17, 0xcb, 0x2c, 0x4d, 0xd0, 0x45, 0x93,
            0x8d, 0x31,
        ]);
        let (key, iv) = v3_story_key_iv(&md).expect("v7 md carries key+iv");
        assert_eq!(
            key,
            [
                0xd5, 0x56, 0x77, 0x96, 0x2a, 0x15, 0xd9, 0xd5, 0x00, 0x30, 0x13, 0xbf, 0x3b, 0xc0,
                0x91, 0xcd
            ]
        );
        assert_eq!(
            iv,
            [
                0x91, 0x10, 0x6b, 0x06, 0x17, 0xca, 0x57, 0xf0, 0xd0, 0x4d, 0x2c, 0xcb, 0x31, 0x8d,
                0x93, 0x45
            ]
        );
    }

    #[test]
    fn v3_story_key_iv_refuses_a_short_md() {
        assert!(v3_story_key_iv(&[0u8; 0x40]).is_none());
    }

    #[test]
    fn cipher_then_decipher_is_identity_and_leaves_the_tail_untouched() {
        let key = [7u8; 16];
        let iv = [9u8; 16];
        // 540 bytes like a real `ri`: first 512 ciphered, last 28 plaintext.
        let original: Vec<u8> = (0..540u32).map(|i| (i * 7 % 256) as u8).collect();
        let mut buf = original.clone();
        v3_cipher_in_place(&mut buf, &key, &iv);
        // The first block changed; the tail (past 512) is byte-identical.
        assert_ne!(buf[..16], original[..16]);
        assert_eq!(&buf[512..], &original[512..]);
        v3_decipher_in_place(&mut buf, &key, &iv);
        assert_eq!(buf, original, "cipher∘decipher round-trips exactly");
    }

    #[test]
    fn a_file_shorter_than_one_block_is_left_as_is() {
        let key = [1u8; 16];
        let iv = [2u8; 16];
        let mut buf = vec![0xAB; 10];
        let before = buf.clone();
        v3_cipher_in_place(&mut buf, &key, &iv);
        assert_eq!(buf, before, "nothing is ciphered below one AES block");
    }

    /// Empirical ground-truth check against a REAL device (read-only): point
    /// `RUSTORY_TEST_MD` at the device `.md` and `RUSTORY_TEST_RI` at the `ri`
    /// of a **custom / tool-written** pack; deciphering its first 512 bytes with
    /// the `.md` key/IV must yield a resource index beginning with the ASCII
    /// marker `"000"`. NOTE: an OFFICIAL store-bought pack won't match — it uses
    /// a per-story key wrapped (under the hardware device key) in its `bt`, not
    /// the `.md` content key. Confirmed against a real V3 (FW 3.3.3) custom pack.
    /// Ignored by default (needs a real device).
    #[test]
    #[ignore = "manual: set RUSTORY_TEST_MD + RUSTORY_TEST_RI to a real device's CUSTOM pack"]
    fn deciphers_a_real_device_ri_to_the_000_marker() {
        let md = std::fs::read(std::env::var("RUSTORY_TEST_MD").expect("RUSTORY_TEST_MD"))
            .expect("read .md");
        let mut ri = std::fs::read(std::env::var("RUSTORY_TEST_RI").expect("RUSTORY_TEST_RI"))
            .expect("read ri");
        let (key, iv) = v3_story_key_iv(&md).expect("v7 md");
        v3_decipher_in_place(&mut ri, &key, &iv);
        let head = String::from_utf8_lossy(&ri[..16.min(ri.len())]);
        eprintln!("[cipher-smoke] deciphered ri head = {head:?}");
        assert!(
            ri.starts_with(b"000"),
            "a correctly deciphered ri begins with the ASCII '000' marker; got {:?}",
            &ri[..8.min(ri.len())]
        );
    }
}
