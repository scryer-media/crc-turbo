//! One algorithm, CRC-64/NVME, through every entry point a WebAssembly consumer reaches for:
//! `checksum`, the named helper and `Digest`. CI builds this without LTO and fails if the module
//! grows past what one 32 KiB table and its code need, which it did when every call linked all
//! fifty tables (crc-fast pull request 67, by Andres J Ruiz Torres).

#![no_std]

use crc_turbo::{checksum, crc64_nvme, CrcAlgorithm, Digest};

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

/// # Safety
///
/// `ptr` and `len` describe readable memory.
#[no_mangle]
pub unsafe extern "C" fn one_shot(ptr: *const u8, len: usize) -> u64 {
    checksum(CrcAlgorithm::Crc64Nvme, core::slice::from_raw_parts(ptr, len))
}

/// # Safety
///
/// `ptr` and `len` describe readable memory.
#[no_mangle]
pub unsafe extern "C" fn helper(ptr: *const u8, len: usize) -> u64 {
    crc64_nvme(core::slice::from_raw_parts(ptr, len))
}

/// # Safety
///
/// `ptr` and `len` describe readable memory.
#[no_mangle]
pub unsafe extern "C" fn streaming(ptr: *const u8, len: usize) -> u64 {
    let data = core::slice::from_raw_parts(ptr, len);
    let (head, tail) = data.split_at(len / 2);
    let mut digest = Digest::new(CrcAlgorithm::Crc64Nvme);
    digest.update(head);
    digest.update(tail);
    digest.finalize()
}
