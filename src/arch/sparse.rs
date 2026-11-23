// Copyright 2026 crc-turbo contributors. Licensed under MIT or Apache-2.0.

//! Table-light software reduction by sparse multiples of the generator, for WebAssembly.
//!
//! A CRC is the remainder of the message polynomial modulo the generator `G`. When
//! `x^n + x^(n - d_1) + ... + 1` is a multiple of `G`, replacing a message bit by bits `d` places
//! further on, for every offset `d` of the multiple (`n` among them), leaves that remainder
//! unchanged. Sam Russell's Chorba (arXiv:2412.16398) does this for CRC-32/ISO-HDLC with the zero
//! polynomial `x^300 + x^211 + x^183 + x^145 + 1`; this module does it for every generator that
//! has a short enough multiple, on whole 64-bit words: every offset is at least 64, so a word
//! moves wholly past itself, and each word costs one or two shifts and XORs per offset instead of
//! eight table lookups. The words still pending at the end, a few dozen bytes, finish through the
//! slice-by-16 tables from a zero state, the incoming state having been XORed onto the first word.
//!
//! The offsets come from [`crate::generate::sparse_multiple`], which a test checks against the
//! lists here. WebAssembly bounds-checks every table load, so there the shifts win: at 64 KiB they
//! run 1.2x to 4.3x the tables' speed in both V8 and wasmtime, by generator and bit order. On
//! native CPUs the tables win, so only `target_family = "wasm"` dispatches here; the tests run the
//! kernels on every target.

#![cfg_attr(not(target_family = "wasm"), allow(dead_code))]

/// Bytes the tail buffer can hold: the pending words of the longest multiple plus a partial word.
pub(crate) const TAIL_MAX: usize = 8 * MAX_WORDS + 7;

/// Pending words of the longest multiple below (`n` = 306).
const MAX_WORDS: usize = 5;

/// A reduction for one generator.
#[derive(Clone, Copy)]
pub(crate) struct Kernel {
    /// Reduces all but the last words of `bytes` onto them, writing those words into the tail
    /// buffer and returning their length. The arguments are the state, the CRC width and whether the
    /// CRC is reflected.
    pub(crate) reduce: fn(u64, u8, bool, &[u8], &mut [u8; TAIL_MAX]) -> usize,
    /// The shortest input to reduce: below it the tables are faster.
    pub(crate) min_len: usize,
    /// The offsets, for the tests.
    #[cfg(test)]
    pub(crate) offsets: &'static [u32],
}

/// The 64-bit words a reduction keeps pending: the furthest word an offset reaches.
const fn pending_words(offsets: &[u32]) -> usize {
    let mut words = 0;
    let mut i = 0;
    while i < offsets.len() {
        let d = offsets[i] as usize;
        let reach = d.div_ceil(64);
        if reach > words {
            words = reach;
        }
        i += 1;
    }
    words
}

/// The reduction itself, inlined into one out-of-line function per generator so the offsets fold
/// into constant shifts and the pending words stay in registers. V8 optimizes a large function
/// poorly, so the per-generator functions must stay out of line.
#[inline(always)]
fn reduce<const W: usize, const REFLECTED: bool>(
    offsets: &[u32],
    state: u64,
    width: u8,
    bytes: &[u8],
    tail: &mut [u8; TAIL_MAX],
) -> usize {
    let (words, _) = bytes.as_chunks::<8>();
    debug_assert!(words.len() > W);
    let reduced = words.len() - W;

    // A ring of W slots: slot j % W collects what is XORed onto word j. Words go W at a time so
    // every slot index is a constant and no slot moves.
    let mut slot = [0u64; W];
    slot[0] = if REFLECTED {
        state
    } else {
        state << (64 - width as u32)
    };

    #[inline(always)]
    fn step<const W: usize, const REFLECTED: bool>(
        offsets: &[u32],
        slot: &mut [u64; W],
        k: usize,
        word: &[u8; 8],
    ) {
        let w = if REFLECTED {
            u64::from_le_bytes(*word)
        } else {
            u64::from_be_bytes(*word)
        } ^ slot[k];
        slot[k] = 0;
        for &d in offsets {
            let q = (d / 64) as usize;
            let r = d % 64;
            if REFLECTED {
                slot[(k + q) % W] ^= w << r;
                if r != 0 {
                    slot[(k + q + 1) % W] ^= w >> (64 - r);
                }
            } else {
                slot[(k + q) % W] ^= w >> r;
                if r != 0 {
                    slot[(k + q + 1) % W] ^= w << (64 - r);
                }
            }
        }
    }

    let first = if W <= 2 {
        // With two slots, moving them each word measures faster than unrolling by two.
        for word in &words[..reduced] {
            step::<W, REFLECTED>(offsets, &mut slot, 0, word);
            slot.rotate_left(1);
        }
        0
    } else {
        let (blocks, rest_words) = words[..reduced].as_chunks::<W>();
        for block in blocks {
            for (k, word) in block.iter().enumerate() {
                step::<W, REFLECTED>(offsets, &mut slot, k, word);
            }
        }
        for k in 0..W {
            if k < rest_words.len() {
                step::<W, REFLECTED>(offsets, &mut slot, k, &rest_words[k]);
            }
        }
        rest_words.len()
    };

    let rest = &bytes[reduced * 8..];
    let len = rest.len();
    tail[..len].copy_from_slice(rest);
    // slot k holds what goes onto pending word (k - first) mod W
    for (k, word) in slot.iter().enumerate() {
        let j = (k + W - first) % W;
        let word = if REFLECTED {
            word.to_le_bytes()
        } else {
            word.to_be_bytes()
        };
        for (t, b) in tail[j * 8..j * 8 + 8].iter_mut().zip(word) {
            *t ^= b;
        }
    }
    len
}

macro_rules! kernels {
    ($($width:literal, $poly:literal => $name:ident [$($d:literal),+] ($rmin:expr, $fmin:expr);)+) => {
        $(
            #[inline(never)]
            fn $name(state: u64, width: u8, reflected: bool, bytes: &[u8], tail: &mut [u8; TAIL_MAX]) -> usize {
                const OFFSETS: &[u32] = &[$($d),+];
                const W: usize = pending_words(OFFSETS);
                const _: () = assert!(W <= MAX_WORDS && W >= 1);
                if reflected {
                    reduce::<W, true>(OFFSETS, state, width, bytes, tail)
                } else {
                    reduce::<W, false>(OFFSETS, state, width, bytes, tail)
                }
            }
        )+

        /// The reduction for a generator in one bit order, or `None` when it has no sparse multiple
        /// short enough.
        pub(crate) const fn kernel(width: u8, poly: u64, reflected: bool) -> Option<Kernel> {
            match (width, poly) {
                $(
                    ($width, $poly) => {
                        const OFFSETS: &[u32] = &[$($d),+];
                        const FLOOR: usize = 8 * (pending_words(OFFSETS) + 1);
                        Some(Kernel {
                            reduce: $name,
                            min_len: if reflected { max(FLOOR, $rmin) } else { max(FLOOR, $fmin) },
                            #[cfg(test)]
                            offsets: OFFSETS,
                        })
                    }
                )+
                _ => None,
            }
        }

        /// The shortest input any kernel reduces, checked before looking a kernel up.
        pub(crate) const MIN_LEN: usize = {
            let mut min = usize::MAX;
            $(
                {
                    const FLOOR: usize = 8 * (pending_words(&[$($d),+]) + 1);
                    let m = if $rmin < $fmin { max(FLOOR, $rmin) } else { max(FLOOR, $fmin) };
                    if m < min {
                        min = m;
                    }
                }
            )+
            min
        };

        /// Every generator with a reduction, for the tests.
        #[cfg(test)]
        pub(crate) const GENERATORS: &[(u8, u64)] = &[$(($width, $poly)),+];
    };
}

/// A bit order no catalogue algorithm uses, or one the tables stay faster in: never reduced.
const NONE: usize = usize::MAX;

/// The reduction worth dispatching to for a generator in one bit order: [`kernel`], less the bit
/// orders the tables stay faster in. Built-in algorithms take it as a constant.
pub(crate) const fn dispatch(width: u8, poly: u64, reflected: bool) -> Option<Kernel> {
    match kernel(width, poly, reflected) {
        Some(k) if k.min_len != NONE => Some(k),
        _ => None,
    }
}

const fn max(a: usize, b: usize) -> usize {
    if a > b {
        a
    } else {
        b
    }
}

kernels! {
    16, 0x0589 => crc16_0589 [254] (NONE, 224);
    16, 0x080b => crc16_080b [119, 121, 124] (80, NONE);
    16, 0x1021 => crc16_1021 [76, 102, 103] (80, 448);
    16, 0x1dcf => crc16_1dcf [81, 87, 147] (NONE, 512);
    16, 0x3d65 => crc16_3d65 [151] (112, 256);
    16, 0x5935 => crc16_5935 [257] (NONE, 384);
    16, 0x6f63 => crc16_6f63 [255] (NONE, 224);
    16, 0x755b => crc16_755b [74, 97, 148] (NONE, 512);
    16, 0x8005 => crc16_8005 [90, 112, 118] (80, 384);
    16, 0x8bb7 => crc16_8bb7 [84, 101, 106] (NONE, 384);
    16, 0xa097 => crc16_a097 [68, 77, 83] (NONE, 448);
    16, 0xc867 => crc16_c867 [81, 123, 126] (NONE, 384);
    32, 0x000000af => crc32_000000af [66, 100, 108, 126, 130] (NONE, NONE);
    32, 0x04c11db7 => crc32_04c11db7 [145, 183, 211, 300] (192, 512);
    32, 0x1edc6f41 => crc32_1edc6f41 [65, 155, 170, 195, 209] (160, NONE);
    32, 0x741b8cd7 => crc32_741b8cd7 [84, 135, 203, 218, 222] (160, NONE);
    32, 0x8001801b => crc32_8001801b [64, 112, 124] (80, NONE);
    32, 0x814141ab => crc32_814141ab [92, 132, 169, 223, 239] (NONE, 768);
    32, 0xa833982b => crc32_a833982b [82, 83, 138, 139, 221] (160, NONE);
    32, 0xf4acfb13 => crc32_f4acfb13 [67, 101, 102, 142, 306] (224, NONE);
    64, 0x000000000000001b => crc64_1b [120, 122, 126, 128] (64, NONE);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::software;
    use crate::test::consts::TEST_ALL_CONFIGS;
    use rand::{rng, RngExt};

    /// The offsets here are the generator's: what `sparse_multiple` derives, for every generator
    /// in the catalogue, and nothing for those it finds none for.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_offsets_are_generated() {
        for config in TEST_ALL_CONFIGS {
            let (width, poly) = (config.get_width(), config.get_poly());
            let generated = crate::generate::sparse_multiple(width, poly);
            let here = kernel(width, poly, config.get_refin()).map(|k| k.offsets.to_vec());
            assert_eq!(here, generated, "{}", config.get_name());
        }
        for &(width, poly) in GENERATORS {
            assert!(
                TEST_ALL_CONFIGS
                    .iter()
                    .any(|c| c.get_width() == width && c.get_poly() == poly),
                "{width} {poly:#x} is not in the catalogue"
            );
        }
    }

    /// Each kernel, forced on whatever the target, against the table path over every algorithm
    /// of the catalogue, both bit orders, lengths around the pending words and the word size,
    /// unaligned starts and a carried state.
    #[test]
    fn test_kernels_match_software() {
        let mut data = vec![0u8; 4096 + 64];
        rng().fill(&mut data[..]);

        let mut lengths: Vec<usize> = (0..=160).collect();
        lengths.extend([255, 256, 257, 511, 512, 513, 1000, 4096]);
        if cfg!(miri) {
            lengths = vec![0, 7, 48, 49, 56, 57, 63, 64, 65, 120, 121, 300];
        }

        let mut covered = 0;
        for config in TEST_ALL_CONFIGS {
            let params = config.get_params();
            let Some(k) = kernel(params.width, params.poly, params.refin) else {
                continue;
            };
            covered += 1;
            let init = config.get_init_algorithm();
            let carried = software::update(init, &data[4096..4096 + 7], params);
            let words = pending_words(k.offsets);
            for offset in [0usize, 1, 7, 13] {
                for &len in &lengths {
                    if len / 8 <= words {
                        continue;
                    }
                    let input = &data[offset..offset + len];
                    for state in [init, carried] {
                        let expected = software::update(state, input, params);
                        let actual = software::update_sparse(state, input, params, k);
                        assert_eq!(
                            actual,
                            expected,
                            "{} offset {} len {} state {:#x}",
                            config.get_name(),
                            offset,
                            len,
                            state
                        );
                    }
                }
            }
        }
        assert!(covered > 40, "only {covered} algorithms have a kernel");
    }
}
