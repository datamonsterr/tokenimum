//! RFC 1321 MD5 message-digest algorithm in safe, dependency-free Rust.

use std::fmt::Write;

const T: [u32; 64] = [
    // Round 1
    0xd76a_a478,
    0xe8c7_b756,
    0x2420_70db,
    0xc1bd_ceee,
    0xf57c_0faf,
    0x4787_c62a,
    0xa830_4613,
    0xfd46_9501,
    0x6980_98d8,
    0x8b44_f7af,
    0xffff_5bb1,
    0x895c_d7be,
    0x6b90_1122,
    0xfd98_7193,
    0xa679_438e,
    0x49b4_0821,
    // Round 2
    0xf61e_2562,
    0xc040_b340,
    0x265e_5a51,
    0xe9b6_c7aa,
    0xd62f_105d,
    0x0244_1453,
    0xd8a1_e681,
    0xe7d3_fbc8,
    0x21e1_cde6,
    0xc337_07d6,
    0xf4d5_0d87,
    0x455a_14ed,
    0xa9e3_e905,
    0xfcef_a3f8,
    0x676f_02d9,
    0x8d2a_4c8a,
    // Round 3
    0xfffa_3942,
    0x8771_f681,
    0x6d9d_6122,
    0xfde5_380c,
    0xa4be_ea44,
    0x4bde_cfa9,
    0xf6bb_4b60,
    0xbebf_bc70,
    0x289b_7ec6,
    0xeaa1_27fa,
    0xd4ef_3085,
    0x0488_1d05,
    0xd9d4_d039,
    0xe6db_99e5,
    0x1fa2_7cf8,
    0xc4ac_5665,
    // Round 4
    0xf429_2244,
    0x432a_ff97,
    0xab94_23a7,
    0xfc93_a039,
    0x655b_59c3,
    0x8f0c_cc92,
    0xffef_f47d,
    0x8584_5dd1,
    0x6fa8_7e4f,
    0xfe2c_e6e0,
    0xa301_4314,
    0x4e08_11a1,
    0xf753_7e82,
    0xbd3a_f235,
    0x2ad7_d2bb,
    0xeb86_d391,
];

#[inline]
fn f(x: u32, y: u32, z: u32) -> u32 {
    (x & y) | (!x & z)
}

#[inline]
fn g(x: u32, y: u32, z: u32) -> u32 {
    (x & z) | (y & !z)
}

#[inline]
fn h(x: u32, y: u32, z: u32) -> u32 {
    x ^ y ^ z
}

#[inline]
fn i_fn(x: u32, y: u32, z: u32) -> u32 {
    y ^ (x | !z)
}

/// Computes the MD5 hex digest of `data` according to RFC 1321.
///
/// # Examples
/// ```
/// use tkm_hosts::kimi::md5_hex;
/// assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
/// ```
// Variable names a/b/c/d/m follow RFC 1321 so the code can be checked against the spec.
#[allow(clippy::many_single_char_names)]
#[must_use]
pub fn md5_hex(data: &[u8]) -> String {
    let mut a0 = 0x6745_2301_u32;
    let mut b0 = 0xefcd_ab89_u32;
    let mut c0 = 0x98ba_dcfe_u32;
    let mut d0 = 0x1032_5476_u32;

    let bit_len = u64::try_from(data.len())
        .unwrap_or(u64::MAX)
        .wrapping_mul(8);
    let mod64 = data.len() % 64;
    let pad_len = if mod64 < 56 { 56 - mod64 } else { 120 - mod64 };

    let mut message = Vec::with_capacity(data.len().saturating_add(pad_len).saturating_add(8));
    message.extend_from_slice(data);
    message.push(0x80);
    message.resize(data.len().saturating_add(pad_len), 0);
    message.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in message.chunks_exact(64) {
        let mut m = [0_u32; 16];
        for (idx, word) in m.iter_mut().enumerate() {
            let start = idx.wrapping_mul(4);
            if let Some(bytes) = chunk.get(start..start + 4)
                && let Ok(b) = bytes.try_into()
            {
                *word = u32::from_le_bytes(b);
            }
        }

        let mut a = a0;
        let mut b = b0;
        let mut c = c0;
        let mut d = d0;

        for step in 0..64 {
            let (f_val, g_idx, shift) = match step {
                0..=15 => {
                    let shifts = [7, 12, 17, 22];
                    (f(b, c, d), step, *shifts.get(step % 4).unwrap_or(&0))
                }
                16..=31 => {
                    let shifts = [5, 9, 14, 20];
                    let idx = (1 + 5 * step) % 16;
                    (g(b, c, d), idx, *shifts.get(step % 4).unwrap_or(&0))
                }
                32..=47 => {
                    let shifts = [4, 11, 16, 23];
                    let idx = (5 + 3 * step) % 16;
                    (h(b, c, d), idx, *shifts.get(step % 4).unwrap_or(&0))
                }
                _ => {
                    let shifts = [6, 10, 15, 21];
                    let idx = (7 * step) % 16;
                    (i_fn(b, c, d), idx, *shifts.get(step % 4).unwrap_or(&0))
                }
            };

            let temp = d;
            d = c;
            c = b;
            let sum = a
                .wrapping_add(f_val)
                .wrapping_add(*T.get(step).unwrap_or(&0))
                .wrapping_add(*m.get(g_idx).unwrap_or(&0));
            b = b.wrapping_add(sum.rotate_left(shift));
            a = temp;
        }

        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut hex = String::with_capacity(32);
    for byte in a0
        .to_le_bytes()
        .into_iter()
        .chain(b0.to_le_bytes())
        .chain(c0.to_le_bytes())
        .chain(d0.to_le_bytes())
    {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}
