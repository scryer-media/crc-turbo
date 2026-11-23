// Copyright 2025 Don MacAskill. Licensed under MIT or Apache-2.0.

//! This module contains the main algorithm for CRC calculation.
//!
//! This implementation is designed to work with both CRC-32 and CRC-64 algorithms.
//!
//! It uses SIMD instructions for performance optimization and is designed to be
//! platform-agnostic.
//!
//! The code is structured to allow for easy extension and modification for
//! different architectures and CRC algorithms.
//!
//! The main entry point is the `update` function, which takes the current CRC state,
//! the input data, CRC parameters, and architecture-specific operations.

#![cfg(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64"))]

use crate::consts::CRC_CHUNK_SIZE;
use crate::enums::{DataChunkProcessor, Reflector};
use crate::structs::CrcState;
use crate::traits::{ArchOps, EnhancedCrcWidth};
use crate::{crc16, crc32, crc64, CrcParams};

/// The fold keys of `params`
#[inline(always)]
fn extract_keys_array(params: &CrcParams) -> &[u64; 23] {
    // Borrowed, not copied: through a tier's out-of-line fold the parameters are not constant,
    // and copying the keys out one by one, each behind the storage's tag, was a fixed cost of
    // every call there.
    params.keys.fold_keys()
}

/// `data`, shorter than 16 bytes, copied into a zeroed 16-byte block at `offset`.
///
/// Written as two overlapping copies of a fixed size rather than one of `data.len()` bytes,
/// which the compiler made a call to `memcpy`: for a buffer this short the call cost more than
/// the fold. The overlap writes the same bytes twice.
#[inline(always)]
pub(crate) fn short_block(data: &[u8], offset: usize) -> [u8; CRC_CHUNK_SIZE] {
    let n = data.len();
    debug_assert!(n < CRC_CHUNK_SIZE && offset + n <= CRC_CHUNK_SIZE);
    let mut block = [0u8; CRC_CHUNK_SIZE];
    let dst = &mut block[offset..offset + n];
    macro_rules! ends {
        ($w:literal) => {{
            dst[..$w].copy_from_slice(&data[..$w]);
            dst[n - $w..].copy_from_slice(&data[n - $w..]);
        }};
    }
    if n >= 8 {
        ends!(8)
    } else if n >= 4 {
        ends!(4)
    } else if n >= 2 {
        ends!(2)
    } else if n == 1 {
        dst[0] = data[0];
    }
    block
}

/// Main entry point that works for both CRC-32 and CRC-64
#[inline(always)]
pub unsafe fn update<T: ArchOps, W: EnhancedCrcWidth>(
    state: W::Value,
    bytes: &[u8],
    params: &CrcParams,
    ops: &T,
) -> W::Value
where
    T::Vector: Copy,
{
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    let mut bytes = bytes;
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    let mut len = bytes.len();
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    let len = bytes.len();
    if len == 0 {
        return state;
    }

    // Create the appropriate reflector based on CRC type
    let reflector = if params.refin {
        Reflector::NoReflector
    } else {
        // Load mask for byte-swapping operations
        let smask = ops.load_aligned(&W::load_constants(params.refin)[0] as *const [u64; 2]);
        Reflector::ForwardReflector { smask }
    };

    // Create initial CRC state
    let mut crc_state = W::create_state(state, params.refin, ops);

    // Extract keys once and pass by reference to avoid repeated stack copies
    let keys = extract_keys_array(params);

    // Process data differently based on length
    // On ARM M4 Max, ARM c8g, x86 c7a, and x86 c7i, using 128 bytes is a measurably faster
    // threshold than 256 bytes...
    if len < 128 {
        // Select processor based on input length
        let processor = DataChunkProcessor::for_length(len);
        return process_by_strategy::<T, W>(processor, bytes, &mut crc_state, reflector, keys, ops);
    }

    // On aarch64, below `UNALIGNED_FOLD_END` the buffer is folded from wherever it starts and
    // reduced once.
    #[cfg(target_arch = "aarch64")]
    if len < UNALIGNED_FOLD_END {
        return process_unaligned::<T, W>(bytes, &crc_state, &reflector, keys, ops);
    }

    // On x86 a long buffer is taken from a 64-byte boundary, see `X86_ALIGN_MIN`.
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if len >= X86_ALIGN_MIN {
        let head = bytes.as_ptr().align_offset(64);
        if head != 0 {
            let (left, rest) = bytes.split_at(head);
            let left_crc = process_by_strategy::<T, W>(
                DataChunkProcessor::for_length(head),
                left,
                &mut crc_state,
                reflector,
                keys,
                ops,
            );
            crc_state = W::create_state(left_crc, params.refin, ops);
            bytes = rest;
            len = rest.len();
        }
    }

    // On x86, below `X86_UNALIGNED_FOLD_END`, whatever the alignment.
    #[cfg(not(target_arch = "aarch64"))]
    if len < X86_UNALIGNED_FOLD_END {
        return process_unaligned::<T, W>(bytes, &crc_state, &reflector, keys, ops);
    }

    // Process large inputs with SIMD-optimized approach
    process_large_aligned::<T, W>(bytes, &mut crc_state, reflector, keys, ops)
}

/// [`update`] for a buffer shorter than 128 bytes, without the folds that longer ones take, so
/// that a tier can keep those out of line: inlined, their vector registers are saved
/// and restored on every call, the shortest included.
///
/// # Safety
///
/// `bytes` must be shorter than 128 bytes.
#[inline(always)]
pub(crate) unsafe fn update_short<T: ArchOps, W: EnhancedCrcWidth>(
    state: W::Value,
    bytes: &[u8],
    params: &CrcParams,
    ops: &T,
) -> W::Value
where
    T::Vector: Copy,
{
    let len = bytes.len();
    debug_assert!(len < 128);
    if len == 0 {
        return state;
    }

    let reflector = if params.refin {
        Reflector::NoReflector
    } else {
        let smask = ops.load_aligned(&W::load_constants(params.refin)[0] as *const [u64; 2]);
        Reflector::ForwardReflector { smask }
    };
    let mut crc_state = W::create_state(state, params.refin, ops);
    let keys = extract_keys_array(params);

    process_by_strategy::<T, W>(
        DataChunkProcessor::for_length(len),
        bytes,
        &mut crc_state,
        reflector,
        keys,
        ops,
    )
}

/// [`update`] for a buffer of 128 bytes or more that [`process_unaligned`] is to fold whole,
/// with nothing else in the function: a tier keeps it apart from its entry, whose short arms
/// then stay in line, and from the fold for long buffers, whose set-up it does not pay.
///
/// # Safety
///
/// `bytes` must be at least 128 bytes long.
#[inline(always)]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub(crate) unsafe fn update_mid<T: ArchOps, W: EnhancedCrcWidth>(
    state: W::Value,
    bytes: &[u8],
    params: &CrcParams,
    ops: &T,
) -> W::Value
where
    T::Vector: Copy,
{
    debug_assert!(bytes.len() >= 128);
    let reflector = if params.refin {
        Reflector::NoReflector
    } else {
        let smask = ops.load_aligned(&W::load_constants(params.refin)[0] as *const [u64; 2]);
        Reflector::ForwardReflector { smask }
    };
    let crc_state = W::create_state(state, params.refin, ops);
    let keys = extract_keys_array(params);

    process_unaligned::<T, W>(bytes, &crc_state, &reflector, keys, ops)
}

/// [`update_short`] laid out for the fewest taken branches: 32 bytes and up are looked at
/// first and their block count is picked by three bit tests, each arm folding, reducing and
/// returning on its own, where [`update_short`] compares its way past the shorter lengths, jumps
/// through a table on the block count and jumps again to a shared reduction. The Linux
/// kernel's CRC routines keep their short paths free of taken branches the same way, the
/// arm64 one with conditional selects.
///
/// # Safety
///
/// `bytes` must be shorter than 128 bytes.
#[inline(always)]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub(crate) unsafe fn update_short_ladder<T: ArchOps, W: EnhancedCrcWidth>(
    state: W::Value,
    bytes: &[u8],
    params: &CrcParams,
    ops: &T,
) -> W::Value
where
    T::Vector: Copy,
{
    let len = bytes.len();
    debug_assert!(len < 128);

    let reflector = if params.refin {
        Reflector::NoReflector
    } else {
        let smask = ops.load_aligned(&W::load_constants(params.refin)[0] as *const [u64; 2]);
        Reflector::ForwardReflector { smask }
    };
    let mut crc_state = W::create_state(state, params.refin, ops);
    let keys = extract_keys_array(params);

    // The shortest buffers first, so that they fall through to their fold.
    if len < 2 * CRC_CHUNK_SIZE {
        if len == 0 {
            return state;
        }
        return process_by_strategy::<T, W>(
            DataChunkProcessor::for_length(len),
            bytes,
            &mut crc_state,
            reflector,
            keys,
            ops,
        );
    }

    // One block count, folded, reduced and returned.
    macro_rules! arm {
        ($blocks:literal) => {{
            let wide = if $blocks >= 4 {
                ops.fold_short_wide(
                    crc_state.value,
                    bytes.as_ptr(),
                    $blocks,
                    true,
                    keys,
                    crc_state.reflected,
                )
            } else {
                None
            };
            let mut folded = match wide {
                Some(folded) => folded,
                None => fold_blocks_onto_last::<T, W>(
                    $blocks,
                    bytes.as_ptr(),
                    &crc_state,
                    &reflector,
                    keys,
                    ops,
                ),
            };
            let remaining = len - $blocks * CRC_CHUNK_SIZE;
            if remaining > 0 {
                folded = get_last_two_xmms::<T, W>(
                    DataRegion {
                        full_data: bytes,
                        offset: $blocks * CRC_CHUNK_SIZE,
                        remaining,
                    },
                    folded,
                    keys,
                    &reflector,
                    crc_state.reflected,
                    ops,
                );
            }
            return W::perform_final_reduction(folded, crc_state.reflected, keys, ops);
        }};
    }
    // Bit tests rather than a match on the count, which compiles to a jump table.
    if len & 64 == 0 {
        if len & 16 == 0 {
            arm!(2)
        } else {
            arm!(3)
        }
    } else if len & 32 == 0 {
        if len & 16 == 0 {
            arm!(4)
        } else {
            arm!(5)
        }
    } else if len & 16 == 0 {
        arm!(6)
    } else {
        arm!(7)
    }
}

/// Buffers from 128 bytes up to this many are folded by [`process_unaligned`]. Longer ones are
/// split at a 128-byte boundary: the bytes before it and after the last whole block are each
/// folded and reduced on their own, up to two reductions more than the unaligned fold makes.
/// On an Apple M5 Max the unaligned fold was 1.7x to 5x faster from 128 bytes to 1 KiB, 1.01x
/// at 256 KiB, and 0.97x to 0.99x at 1 MiB, where the aligned loads win.
///
/// x86 has its own cutoff, [`X86_UNALIGNED_FOLD_END`].
#[cfg(target_arch = "aarch64")]
const UNALIGNED_FOLD_END: usize = 256 << 10;

/// On x86, buffers from 128 bytes up to this many are folded by [`process_unaligned`], aligned
/// or not. For a buffer that does not start on a 16-byte boundary, against a 4 KiB cutoff,
/// folding them up to 1 MiB was 1.23x to 1.33x faster at 4 KiB and 1.06x to 1.09x at 16 KiB on
/// a Xeon Platinum 8375C, 1.07x to 1.10x at 4 KiB on an EPYC 7R13, and 1.03x to 1.08x from
/// 4 KiB to 64 KiB on an EPYC 7R32 on the SSE tier. An aligned one split at a 128-byte boundary
/// reduces twice under 256 bytes and once more for the bytes after the last whole chunk: on the
/// EPYC 7R13 folding it whole was 1.39x faster at 320 bytes and the same from 1 KiB.
#[cfg(not(target_arch = "aarch64"))]
const X86_UNALIGNED_FOLD_END: usize = 1 << 20;

/// On x86, a buffer this long or longer that does not start on a 64-byte boundary has the bytes
/// before the boundary folded and reduced on their own first, one reduction more, so that no
/// 64-byte load of the fold crosses a cache line. On a Xeon Platinum 8488C a buffer three bytes
/// off a boundary ran at 0.85x the speed of an aligned one at 64 KiB and 0.80x at 1 MiB; at
/// 32 KiB and below it was no slower, and the reduction is a larger share of a shorter buffer.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
const X86_ALIGN_MIN: usize = 64 << 10;

/// Folds a buffer of 128 bytes or more from wherever it starts: eight 16-byte lanes over each
/// 128-byte chunk, those eight folded into one, then the remaining 16-byte blocks and an
/// overlapping read of the last few bytes, and one reduction. Every load is unaligned. A tier
/// with wider registers folds the whole 128-byte chunks itself, when there are two or more.
///
/// # Safety
///
/// `data` must be at least 128 bytes long.
#[inline(always)]
unsafe fn process_unaligned<T: ArchOps, W: EnhancedCrcWidth>(
    data: &[u8],
    state: &CrcState<T::Vector>,
    reflector: &Reflector<T::Vector>,
    keys: &[u64; 23],
    ops: &T,
) -> W::Value
where
    T::Vector: Copy,
{
    debug_assert!(data.len() >= 128);

    let ptr = data.as_ptr();
    let len = data.len();
    let reflected = state.reflected;

    // Under 256 bytes a tier with wide registers folds every whole block in them at once.
    let whole = len / CRC_CHUNK_SIZE;
    let short = if whole < 16 {
        ops.fold_short_wide(state.value, ptr, whole, true, keys, reflected)
    } else {
        None
    };

    let (mut res, pos) = match short {
        Some(folded) => (folded, whole * CRC_CHUNK_SIZE),
        None => {
            let chunks = len / 128;
            let (res, pos) = match ops.fold_simd_blocks_unaligned(state, ptr, chunks, keys) {
                Some(folded) => (folded, chunks * 128),
                None => fold_lanes::<T, W>(data, state, reflector, keys, ops),
            };
            // The whole blocks left, fewer than eight; from three, two to a multiply where the
            // tier has the registers.
            let blocks = (len - pos) / CRC_CHUNK_SIZE;
            let wide = if blocks >= 3 {
                ops.fold_short_wide(res, ptr.add(pos), blocks + 1, false, keys, reflected)
            } else {
                None
            };
            let res = match wide {
                Some(folded) => folded,
                None => fold_tail_blocks::<T, W>(
                    res,
                    ptr.add(pos),
                    blocks,
                    reflector,
                    keys,
                    reflected,
                    ops,
                ),
            };
            (res, pos + blocks * CRC_CHUNK_SIZE)
        }
    };

    if len > pos {
        res = get_last_two_xmms::<T, W>(
            DataRegion {
                full_data: data,
                offset: pos,
                remaining: len - pos,
            },
            res,
            keys,
            reflector,
            reflected,
            ops,
        );
    }

    W::perform_final_reduction(res, reflected, keys, ops)
}

/// Folds `res`, the folded bytes before `ptr`, and the `blocks` whole 16-byte blocks from `ptr`,
/// fewer than eight, into one. `res` and each block but the last are folded straight onto the
/// last block across the distance to it, so that the products are independent rather than a
/// chain of one multiply per block. The Linux kernel's x86 CRC template and libdeflate break
/// the chain the same way, by halving several vectors.
///
/// # Safety
///
/// `ptr` must be valid for reads of `blocks * 16` bytes.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
unsafe fn fold_tail_blocks<T: ArchOps, W: EnhancedCrcWidth>(
    res: T::Vector,
    ptr: *const u8,
    blocks: usize,
    reflector: &Reflector<T::Vector>,
    keys: &[u64; 23],
    reflected: bool,
    ops: &T,
) -> T::Vector
where
    T::Vector: Copy,
{
    // The generated keys for folding across 16, 32, ... 112 bytes.
    const DISTANCE_KEYS: [(usize, usize); 7] = [
        (2, 1),
        (20, 19),
        (18, 17),
        (16, 15),
        (14, 13),
        (12, 11),
        (10, 9),
    ];
    macro_rules! load {
        ($i:expr) => {
            reflect_bytes(reflector, ops.load_bytes(ptr.add($i * CRC_CHUNK_SIZE)), ops)
        };
    }
    macro_rules! fold_onto {
        ($v:expr, $blocks:expr, $acc:expr) => {{
            let (hi, lo) = DISTANCE_KEYS[$blocks - 1];
            let coeff = W::create_coefficient(keys[hi], keys[lo], reflected, ops);
            fold_and_xor::<T, W>($v, coeff, $acc, reflected, ops)
        }};
    }
    // Written once and instantiated for each count, so that each block's keys are at a fixed
    // index.
    macro_rules! tail {
        ($blocks:expr) => {{
            let blocks: usize = $blocks;
            let mut acc = fold_onto!(res, blocks, load!(blocks - 1));
            for i in 0..blocks - 1 {
                acc = fold_onto!(load!(i), blocks - 1 - i, acc);
            }
            acc
        }};
    }
    match blocks {
        0 => res,
        1 => tail!(1),
        2 => tail!(2),
        3 => tail!(3),
        4 => tail!(4),
        5 => tail!(5),
        6 => tail!(6),
        7 => tail!(7),
        _ => unreachable!("fewer than eight blocks follow a 128-byte fold"),
    }
}

/// [`process_unaligned`]'s 128-bit fold: eight 16-byte lanes over each whole 128-byte chunk of
/// `data`, folded into one. Returns that and the bytes it covered.
#[inline(always)]
unsafe fn fold_lanes<T: ArchOps, W: EnhancedCrcWidth>(
    data: &[u8],
    state: &CrcState<T::Vector>,
    reflector: &Reflector<T::Vector>,
    keys: &[u64; 23],
    ops: &T,
) -> (T::Vector, usize)
where
    T::Vector: Copy,
{
    let ptr = data.as_ptr();
    let len = data.len();
    let reflected = state.reflected;
    // A macro and an array literal rather than a closure and `array::from_fn`: those are
    // functions of their own, which LLVM left out of line for some tiers and widths, without
    // the tier's target features and with the lanes passed back through memory.
    macro_rules! load {
        ($offset:expr) => {
            reflect_bytes(reflector, ops.load_bytes(ptr.add($offset)), ops)
        };
    }

    let mut x: [T::Vector; 8] = [
        ops.xor_vectors(load!(0), state.value),
        load!(16),
        load!(32),
        load!(48),
        load!(64),
        load!(80),
        load!(96),
        load!(112),
    ];
    let mut pos = 128;

    let coeff = W::create_coefficient(keys[4], keys[3], reflected, ops);
    while len - pos >= 128 {
        for (i, xi) in x.iter_mut().enumerate() {
            *xi = fold_and_xor::<T, W>(*xi, coeff, load!(pos + 16 * i), reflected, ops);
        }
        pos += 128;
    }

    // Fold the eight lanes into the last, each across the distance from it to the end
    let fold_coefficients = [
        W::create_coefficient(keys[10], keys[9], reflected, ops), // 112 bytes
        W::create_coefficient(keys[12], keys[11], reflected, ops), // 96 bytes
        W::create_coefficient(keys[14], keys[13], reflected, ops), // 80 bytes
        W::create_coefficient(keys[16], keys[15], reflected, ops), // 64 bytes
        W::create_coefficient(keys[18], keys[17], reflected, ops), // 48 bytes
        W::create_coefficient(keys[20], keys[19], reflected, ops), // 32 bytes
        W::create_coefficient(keys[2], keys[1], reflected, ops),  // 16 bytes
    ];
    let mut res = x[7];
    for (i, &coeff) in fold_coefficients.iter().enumerate() {
        res = fold_and_xor::<T, W>(x[i], coeff, res, reflected, ops);
    }

    (res, pos)
}

/// Process data with the selected strategy
#[inline(always)]
unsafe fn process_by_strategy<T: ArchOps, W: EnhancedCrcWidth>(
    strategy: DataChunkProcessor,
    data: &[u8],
    state: &mut CrcState<T::Vector>,
    reflector: Reflector<T::Vector>,
    keys: &[u64; 23],
    ops: &T,
) -> W::Value
where
    T::Vector: Copy,
{
    match strategy {
        DataChunkProcessor::From0To15 => match W::WIDTH {
            16 => crc16::algorithm::process_0_to_15::<T, W>(data, state, &reflector, keys, ops),
            32 => crc32::algorithm::process_0_to_15::<T, W>(data, state, &reflector, keys, ops),
            64 => crc64::algorithm::process_0_to_15::<T, W>(data, state, &reflector, keys, ops),
            _ => panic!("Unsupported CRC width"),
        },
        DataChunkProcessor::From16 => {
            process_exactly_16::<T, W>(data, state, &reflector, keys, ops)
        }
        DataChunkProcessor::From17To31 => {
            process_17_to_31::<T, W>(data, state, &reflector, keys, ops)
        }
        DataChunkProcessor::From32To255 => {
            process_32_to_255::<T, W>(data, state, &reflector, keys, ops)
        }
    }
}

/// Process large inputs with proper SIMD alignment
#[inline(always)]
unsafe fn process_large_aligned<T: ArchOps, W: EnhancedCrcWidth>(
    bytes: &[u8],
    state: &mut CrcState<T::Vector>,
    reflector: Reflector<T::Vector>,
    keys: &[u64; 23],
    ops: &T,
) -> W::Value
where
    T::Vector: Copy,
{
    // Align data for SIMD processing
    let (left, middle, right) = bytes.align_to::<[T::Vector; 8]>();

    if let Some((first, rest)) = middle.split_first() {
        if !left.is_empty() {
            let processor = DataChunkProcessor::for_length(left.len());
            // Process unaligned bytes at the start
            let left_crc =
                process_by_strategy::<T, W>(processor, left, state, reflector, keys, ops);
            // Update state with the result from processing left bytes
            *state = W::create_state(left_crc, state.reflected, ops);
        }

        // try to use the enhanced SIMD implementation first, fall back to non-enhanced if necessary
        if rest.is_empty()
            || !ops.process_enhanced_simd_blocks::<W>(state, first, rest, &reflector, keys)
        {
            process_simd_chunks::<T, W>(state, first, rest, &reflector, keys, ops);
        }

        // Process any unaligned bytes at the end
        if !right.is_empty() {
            let processor = DataChunkProcessor::for_length(right.len());
            // Use the current state to process the right bytes
            return process_by_strategy::<T, W>(processor, right, state, reflector, keys, ops);
        }

        // Extract the final result
        return W::extract_result(state.value, state.reflected, ops);
    }

    // Fall back to existing implementation if proper alignment isn't possible
    let processor = DataChunkProcessor::for_length(bytes.len());
    process_by_strategy::<T, W>(processor, bytes, state, reflector, keys, ops)
}

/// Process SIMD-aligned chunks of 128 bytes
#[inline(always)]
unsafe fn process_simd_chunks<T: ArchOps, W: EnhancedCrcWidth>(
    state: &mut CrcState<T::Vector>,
    first: &[T::Vector; 8],
    rest: &[[T::Vector; 8]],
    reflector: &Reflector<T::Vector>,
    keys: &[u64; 23],
    ops: &T,
) where
    T::Vector: Copy,
{
    // Create a copy of the first 128 bytes
    let mut x = *first;

    // Apply initial reflection if needed
    for item in &mut x {
        *item = reflect_bytes(reflector, *item, ops);
    }

    // XOR initial CRC with the first 16 bytes
    x[0] = ops.xor_vectors(x[0], state.value);

    // Load the coefficient pair for folding
    let coeff = W::create_coefficient(keys[4], keys[3], state.reflected, ops);

    // Process remaining 128-byte chunks
    for chunk in rest {
        for (xi, yi) in x.iter_mut().zip(chunk.iter()) {
            // Load and reflect the new data if needed
            let yi = reflect_bytes(reflector, *yi, ops);

            // Create a temporary state for folding
            let mut temp_state = CrcState {
                value: *xi,
                reflected: state.reflected,
            };

            // Fold 16 bytes
            W::fold_16(&mut temp_state, coeff, yi, ops);

            // XOR with new data
            *xi = temp_state.value;
        }
    }

    // Fold the 8 xmm registers to 1 xmm register with different constants
    let mut res = x[7];

    // Create fold coefficients for different distances
    let fold_coefficients = [
        W::create_coefficient(keys[10], keys[9], state.reflected, ops), // 112 bytes
        W::create_coefficient(keys[12], keys[11], state.reflected, ops), // 96 bytes
        W::create_coefficient(keys[14], keys[13], state.reflected, ops), // 80 bytes
        W::create_coefficient(keys[16], keys[15], state.reflected, ops), // 64 bytes
        W::create_coefficient(keys[18], keys[17], state.reflected, ops), // 48 bytes
        W::create_coefficient(keys[20], keys[19], state.reflected, ops), // 32 bytes
        W::create_coefficient(keys[2], keys[1], state.reflected, ops),  // 16 bytes
    ];

    for (i, &coeff) in fold_coefficients.iter().enumerate() {
        let mut temp_state = CrcState {
            value: x[i],
            reflected: state.reflected,
        };
        W::fold_16(&mut temp_state, coeff, res, ops);

        res = temp_state.value
    }

    // Perform final reduction and update state
    let final_value = W::perform_final_reduction(res, state.reflected, keys, ops);
    *state = W::create_state(final_value, state.reflected, ops);
}

/// Process exactly 16 bytes
#[inline(always)]
unsafe fn process_exactly_16<T: ArchOps, W: EnhancedCrcWidth>(
    data: &[u8],
    state: &mut CrcState<T::Vector>,
    reflector: &Reflector<T::Vector>,
    keys: &[u64; 23],
    ops: &T,
) -> W::Value
where
    T::Vector: Copy,
{
    // Process 16 bytes and fold to width-specific size
    W::perform_final_reduction(
        process_16_byte_block(data.as_ptr(), state.value, reflector, ops),
        state.reflected,
        keys,
        ops,
    )
}

/// Process a 16-byte block of data
#[inline(always)]
unsafe fn process_16_byte_block<T: ArchOps>(
    data_ptr: *const u8,
    initial_crc: T::Vector,
    reflector: &Reflector<T::Vector>,
    ops: &T,
) -> T::Vector
where
    T::Vector: Copy,
{
    // Load data, apply reflection, and XOR with initial CRC
    ops.xor_vectors(
        reflect_bytes(reflector, ops.load_bytes(data_ptr), ops),
        initial_crc,
    )
}

/// Reflect bytes according to the reflector strategy
#[inline(always)]
pub(crate) unsafe fn reflect_bytes<T: ArchOps>(
    reflector: &Reflector<T::Vector>,
    data: T::Vector,
    ops: &T,
) -> T::Vector
where
    T::Vector: Copy,
{
    match reflector {
        Reflector::NoReflector => data,
        Reflector::ForwardReflector { smask } => ops.shuffle_bytes(data, *smask),
    }
}

/// Fold and XOR generic implementation
#[inline(always)]
unsafe fn fold_and_xor<T: ArchOps, W: EnhancedCrcWidth>(
    current: T::Vector,
    coefficient: T::Vector,
    data_to_xor: T::Vector,
    reflected: bool,
    ops: &T,
) -> T::Vector
where
    T::Vector: Copy,
{
    // Create a temporary state for folding
    let mut temp_state = CrcState {
        value: current,
        reflected,
    };

    // Fold 16 bytes using width-specific method
    W::fold_16(&mut temp_state, coefficient, data_to_xor, ops);

    temp_state.value
}

/// Process inputs between 17 and 31 bytes
/// This implementation works for both CRC-32 and CRC-64 using the width-specific traits
#[inline(always)]
unsafe fn process_17_to_31<T: ArchOps, W: EnhancedCrcWidth>(
    data: &[u8],
    state: &mut CrcState<T::Vector>,
    reflector: &Reflector<T::Vector>,
    keys: &[u64; 23],
    ops: &T,
) -> W::Value
where
    T::Vector: Copy,
{
    // Process the first 16 bytes
    let xmm7 = process_16_byte_block(data.as_ptr(), state.value, reflector, ops);

    // Process the remaining bytes (1-15)
    let remaining_len = data.len() - CRC_CHUNK_SIZE;

    // Use the shared function to handle the last two chunks
    let final_xmm7 = get_last_two_xmms::<T, W>(
        DataRegion {
            full_data: data,
            offset: CRC_CHUNK_SIZE,
            remaining: remaining_len,
        },
        xmm7,
        keys,
        reflector,
        state.reflected,
        ops,
    );

    // Perform final reduction
    W::perform_final_reduction(final_xmm7, state.reflected, keys, ops)
}

/// Folds `blocks` whole 16-byte blocks at `data`, two or more, into one vector.
///
/// The last seven whole blocks but the last are each folded straight onto the last, across
/// the distance between them, rather than each into the next: the products are independent,
/// so the chain is one carry-less multiply and the XORs, where folding block by block was one
/// multiply per block, the multiply's latency each time. The Linux kernel's x86 CRC template
/// breaks the chain the same way for short buffers, by folding several vectors and halving
/// them. The keys are the generated ones the 128-byte fold collapses its lanes with, which
/// reach 112 bytes; blocks before those seven, from the remainders of longer buffers on x86,
/// are folded one by one into the first of them.
///
/// # Safety
///
/// `data` must be readable for `blocks * 16` bytes, and `blocks` at least 2.
#[inline(always)]
unsafe fn fold_blocks_onto_last<T: ArchOps, W: EnhancedCrcWidth>(
    blocks: usize,
    data: *const u8,
    state: &CrcState<T::Vector>,
    reflector: &Reflector<T::Vector>,
    keys: &[u64; 23],
    ops: &T,
) -> T::Vector
where
    T::Vector: Copy,
{
    const DISTANCE_KEYS: [(usize, usize); 6] =
        [(2, 1), (20, 19), (18, 17), (16, 15), (14, 13), (12, 11)];

    // Macros rather than closures: a closure is a function of its own, which LLVM may leave out
    // of line, where the tier's target features do not reach.
    macro_rules! load {
        ($i:expr) => {
            reflect_bytes(
                reflector,
                ops.load_bytes(data.add($i * CRC_CHUNK_SIZE)),
                ops,
            )
        };
    }
    macro_rules! fold_onto_last {
        ($block:expr, $i:expr, $acc:expr) => {{
            let (hi, lo) = DISTANCE_KEYS[blocks - 2 - $i];
            let coeff = W::create_coefficient(keys[hi], keys[lo], state.reflected, ops);
            fold_and_xor::<T, W>($block, coeff, $acc, state.reflected, ops)
        }};
    }

    let first = blocks.saturating_sub(DISTANCE_KEYS.len() + 1);
    let mut head = ops.xor_vectors(load!(0), state.value);
    if first > 0 {
        let rk01rk02 = W::create_coefficient(keys[2], keys[1], state.reflected, ops);
        for i in 1..=first {
            head = fold_and_xor::<T, W>(head, rk01rk02, load!(i), state.reflected, ops);
        }
    }
    let mut acc = fold_onto_last!(head, first, load!(blocks - 1));
    for i in first + 1..blocks - 1 {
        acc = fold_onto_last!(load!(i), i, acc);
    }
    acc
}

// Process inputs between 32 and 255 bytes
/// This implementation works for both CRC-32 and CRC-64 using the width-specific traits
#[inline(always)]
unsafe fn process_32_to_255<T: ArchOps, W: EnhancedCrcWidth>(
    data: &[u8],
    state: &mut CrcState<T::Vector>,
    reflector: &Reflector<T::Vector>,
    keys: &[u64; 23],
    ops: &T,
) -> W::Value
where
    T::Vector: Copy,
{
    let len = data.len();
    debug_assert!((32..256).contains(&len));
    let blocks = len / CRC_CHUNK_SIZE;
    // The fold for a given block count, instantiated below for each count a buffer under 128
    // bytes has: with the count a constant, the loops unroll and each block's keys are at a
    // fixed index, where a run-time count looked both indices up in `DISTANCE_KEYS` and then
    // loaded the keys through them, block by block.
    macro_rules! fold_blocks {
        ($blocks:expr) => {
            fold_blocks_onto_last::<T, W>($blocks, data.as_ptr(), state, reflector, keys, ops)
        };
    }
    // From four blocks, two to a multiply where the tier has the registers; below that the
    // 128-bit fold takes no more multiplies and a shorter chain.
    let wide = if blocks >= 4 {
        ops.fold_short_wide(
            state.value,
            data.as_ptr(),
            blocks,
            true,
            keys,
            state.reflected,
        )
    } else {
        None
    };
    let mut xmm7 = match (wide, blocks) {
        (Some(folded), _) => folded,
        (None, 2) => fold_blocks!(2),
        (None, 3) => fold_blocks!(3),
        (None, 4) => fold_blocks!(4),
        (None, 5) => fold_blocks!(5),
        (None, 6) => fold_blocks!(6),
        (None, 7) => fold_blocks!(7),
        (None, _) => fold_blocks!(blocks),
    };

    let current_pos = blocks * CRC_CHUNK_SIZE;
    let remaining_len = len - current_pos;
    if remaining_len > 0 {
        // Use the shared get_last_two_xmms function to handle the remaining bytes
        xmm7 = get_last_two_xmms::<T, W>(
            DataRegion {
                full_data: data,
                offset: current_pos,
                remaining: remaining_len,
            },
            xmm7,
            keys,
            reflector,
            state.reflected,
            ops,
        );
    }

    // Perform final reduction
    W::perform_final_reduction(xmm7, state.reflected, keys, ops)
}

/// Data region descriptor for overlapping SIMD reads in CRC processing
///
/// # Safety Invariants
///
/// When this struct is used with overlapping SIMD reads:
/// - `offset` must be >= `CRC_CHUNK_SIZE` (16 bytes)
/// - `remaining` must be in range 1..=15
/// - `full_data` must contain at least `offset + remaining` bytes
struct DataRegion<'a> {
    full_data: &'a [u8],
    offset: usize,
    remaining: usize,
}

/// Handle the last two chunks of data using an overlapping SIMD read
///
/// # Safety
///
/// - `region.full_data` must contain at least `region.offset + region.remaining` bytes
/// - `region.offset` must be >= `CRC_CHUNK_SIZE` (16 bytes)
/// - `region.remaining` must be in range 1..=15
/// - Caller must ensure appropriate SIMD features are available
#[inline(always)]
unsafe fn get_last_two_xmms<T: ArchOps, W: EnhancedCrcWidth>(
    region: DataRegion,
    current_state: T::Vector,
    keys: &[u64; 23],
    reflector: &Reflector<T::Vector>,
    reflected: bool,
    ops: &T,
) -> T::Vector
where
    T::Vector: Copy,
{
    debug_assert!(region.offset >= CRC_CHUNK_SIZE);
    debug_assert!(region.remaining > 0 && region.remaining < CRC_CHUNK_SIZE);
    debug_assert!(region.offset + region.remaining <= region.full_data.len());

    let coefficient = W::create_coefficient(keys[2], keys[1], reflected, ops);
    let const_mask = ops.set_all_bytes(0x80);
    let (table_ptr, offset) = W::get_last_bytes_table_ptr(reflected, region.remaining);

    if reflected {
        // Overlapping read: loads tail of previous chunk + remaining data
        let read_offset = region.offset - CRC_CHUNK_SIZE + region.remaining;
        let xmm1 = ops.load_bytes(region.full_data.as_ptr().add(read_offset));
        let mut xmm0 = ops.load_bytes(table_ptr.add(offset));
        let shuffled = ops.shuffle_bytes(current_state, xmm0);
        xmm0 = ops.xor_vectors(xmm0, const_mask);

        let shuffled_masked = ops.shuffle_bytes(current_state, xmm0);

        // CRC-16 and CRC-32 use the same logic (both operate in 32-bit space)
        let (xmm2_blended, mut temp_state) = if W::WIDTH <= 32 {
            let compare_mask = ops.create_compare_mask(xmm0);

            let xmm2_blended = ops.blend_vectors(xmm1, shuffled, compare_mask);

            let temp_state = CrcState {
                value: shuffled_masked,
                reflected,
            };

            (xmm2_blended, temp_state)
        } else {
            let xmm2_blended = ops.blend_vectors(shuffled_masked, xmm1, xmm0);

            let temp_state = CrcState {
                value: shuffled,
                reflected,
            };

            (xmm2_blended, temp_state)
        };

        W::fold_16(&mut temp_state, coefficient, xmm2_blended, ops);

        temp_state.value
    } else {
        let read_offset = region.offset - CRC_CHUNK_SIZE + region.remaining;
        let mut xmm1 = ops.load_bytes(region.full_data.as_ptr().add(read_offset));

        if let Reflector::ForwardReflector { smask } = reflector {
            xmm1 = ops.shuffle_bytes(xmm1, *smask);
        }

        let xmm0 = ops.load_bytes(table_ptr.add(offset));
        let shuffled = ops.shuffle_bytes(current_state, xmm0);
        let xmm0_masked = ops.xor_vectors(xmm0, const_mask);
        let shuffled_masked = ops.shuffle_bytes(current_state, xmm0_masked);
        let xmm2_blended = ops.blend_vectors(xmm1, shuffled, xmm0_masked);

        // Create a temporary state for folding
        let mut temp_state = CrcState {
            value: shuffled_masked,
            reflected,
        };

        W::fold_16(&mut temp_state, coefficient, xmm2_blended, ops);

        temp_state.value
    }
}
