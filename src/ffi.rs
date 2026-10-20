// Copyright 2025 Don MacAskill. Licensed under MIT or Apache-2.0.

//! FFI bindings for the Rust library
//!
//! This module provides a C-compatible interface for the Rust library, allowing
//! C programs to use the library's functionality.

#![cfg(all(
    feature = "ffi",
    any(target_arch = "aarch64", target_arch = "x86_64", target_arch = "x86")
))]

use crate::CrcAlgorithm;
use crate::CrcParams;
use crate::{get_calculator_target, Digest};
use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::slice;
use std::sync::{Mutex, OnceLock};

static STRING_CACHE: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();

/// Error codes for FFI operations
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrcTurboError {
    /// Operation completed successfully
    Success = 0,
    /// Lock was poisoned (thread panicked while holding lock)
    LockPoisoned = 1,
    /// Null pointer was passed where non-null required
    NullPointer = 2,
    /// Invalid key count for CRC parameters
    InvalidKeyCount = 3,
    /// Unsupported CRC width (must be 16, 32 or 64)
    UnsupportedWidth = 4,
    /// Invalid UTF-8 string
    InvalidUtf8 = 5,
    /// File I/O error
    IoError = 6,
    /// Internal string conversion error
    StringConversionError = 7,
    /// The algorithm is a custom variant, which carries its parameters in a CrcTurboParams:
    /// use the `_with_params` function
    UnsupportedAlgorithm = 8,
    /// `refin` and `refout` differ, which the library does not support
    MismatchedReflection = 9,
}

impl CrcTurboError {
    /// Returns a static string describing the error
    fn message(&self) -> &'static CStr {
        match self {
            CrcTurboError::Success => c"Operation completed successfully",
            CrcTurboError::LockPoisoned => {
                c"Lock was poisoned (thread panicked while holding lock)"
            }
            CrcTurboError::NullPointer => c"Null pointer was passed where non-null required",
            CrcTurboError::InvalidKeyCount => c"Invalid key count for CRC parameters",
            CrcTurboError::UnsupportedWidth => c"Unsupported CRC width (must be 16, 32 or 64)",
            CrcTurboError::InvalidUtf8 => c"Invalid UTF-8 string",
            CrcTurboError::IoError => c"File I/O error",
            CrcTurboError::StringConversionError => c"Internal string conversion error",
            CrcTurboError::UnsupportedAlgorithm => {
                c"Custom algorithm without parameters (use the _with_params function)"
            }
            CrcTurboError::MismatchedReflection => {
                c"refin and refout differ (both must be true or both false)"
            }
        }
    }
}

// Thread-local storage for the last error that occurred
thread_local! {
    static LAST_ERROR: std::cell::Cell<CrcTurboError> = const { std::cell::Cell::new(CrcTurboError::Success) };
}

/// Sets the thread-local last error
fn set_last_error(error: CrcTurboError) {
    LAST_ERROR.with(|e| e.set(error));
}

/// Clears the thread-local last error (sets it to Success)
fn clear_last_error() {
    LAST_ERROR.with(|e| e.set(CrcTurboError::Success));
}

// Global storage for stable key pointers to ensure they remain valid across FFI boundary
static STABLE_KEY_STORAGE: OnceLock<Mutex<HashMap<u64, Box<[u64]>>>> = OnceLock::new();

/// Creates a stable pointer to the keys for FFI usage.
/// The keys are stored in global memory to ensure the pointer remains valid.
/// Returns (pointer, count) on success, or (null, 0) on error.
/// Sets CrcTurboError::LockPoisoned on lock failure.
fn create_stable_key_pointer(keys: &crate::CrcKeysStorage) -> (*const u64, u32) {
    let storage = STABLE_KEY_STORAGE.get_or_init(|| Mutex::new(HashMap::new()));

    // Create a unique hash for this key set to avoid duplicates
    let key_hash = match keys {
        crate::CrcKeysStorage::KeysFold256(keys) => {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            use std::hash::{Hash, Hasher};
            keys.hash(&mut hasher);
            hasher.finish()
        }
        crate::CrcKeysStorage::KeysFutureTest(keys) => {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            use std::hash::{Hash, Hasher};
            keys.hash(&mut hasher);
            hasher.finish()
        }
    };

    let mut storage_map = match storage.lock() {
        Ok(guard) => guard,
        Err(_) => {
            set_last_error(CrcTurboError::LockPoisoned);
            return (std::ptr::null(), 0);
        }
    };

    // Check if we already have this key set stored
    if let Some(stored_keys) = storage_map.get(&key_hash) {
        return (stored_keys.as_ptr(), stored_keys.len() as u32);
    }

    // Store the keys in stable memory
    let key_vec: Vec<u64> = match keys {
        crate::CrcKeysStorage::KeysFold256(keys) => keys.to_vec(),
        crate::CrcKeysStorage::KeysFutureTest(keys) => keys.to_vec(),
    };

    let boxed_keys = key_vec.into_boxed_slice();
    let count = boxed_keys.len() as u32;

    storage_map.insert(key_hash, boxed_keys);

    let ptr = storage_map.get(&key_hash).expect("just inserted").as_ptr();

    (ptr, count)
}

/// A handle to the Digest object
#[repr(C)]
pub struct CrcTurboDigestHandle(*mut Digest);

/// The supported CRC algorithms
///
/// As with any C enum passed by value, a value outside this list is undefined behaviour. The
/// custom variants carry no parameters of their own: they are valid only inside a
/// CrcTurboParams, and the functions that take an algorithm alone report UnsupportedAlgorithm
/// for them.
#[repr(C)]
#[derive(Clone, Copy)]
pub enum CrcTurboAlgorithm {
    // CrcCustom works with any supported widths (16, 32, 64)
    CrcCustom,
    Crc16Arc,
    Crc16Cdma2000,
    Crc16Cms,
    Crc16Dds110,
    Crc16DectR,
    Crc16DectX,
    Crc16Dnp,
    Crc16En13757,
    Crc16Genibus,
    Crc16Gsm,
    Crc16Ibm3740,
    Crc16IbmSdlc,
    Crc16IsoIec144433A,
    Crc16Kermit,
    Crc16Lj1200,
    Crc16M17,
    Crc16MaximDow,
    Crc16Mcrf4xx,
    Crc16Modbus,
    Crc16Nrsc5,
    Crc16OpensafetyA,
    Crc16OpensafetyB,
    Crc16Profibus,
    Crc16Riello,
    Crc16SpiFujitsu,
    Crc16T10Dif,
    Crc16Teledisk,
    Crc16Tms37157,
    Crc16Umts,
    Crc16Usb,
    Crc16Xmodem,
    Crc32Aixm,
    Crc32Autosar,
    Crc32Base91D,
    Crc32Bzip2,
    Crc32CdRomEdc,
    Crc32Cksum,
    Crc32Custom,
    Crc32Iscsi,
    Crc32IsoHdlc,
    Crc32Jamcrc,
    Crc32Mef,
    Crc32Mpeg2,
    Crc32Xfer,
    Crc64Custom,
    Crc64Ecma182,
    Crc64GoIso,
    Crc64Ms,
    Crc64Nvme,
    Crc64Redis,
    Crc64We,
    Crc64Xz,
}

// Convert from FFI enum to internal enum
#[allow(deprecated)]
impl From<CrcTurboAlgorithm> for CrcAlgorithm {
    fn from(value: CrcTurboAlgorithm) -> Self {
        match value {
            CrcTurboAlgorithm::Crc16Arc => CrcAlgorithm::Crc16Arc,
            CrcTurboAlgorithm::Crc16Cdma2000 => CrcAlgorithm::Crc16Cdma2000,
            CrcTurboAlgorithm::Crc16Cms => CrcAlgorithm::Crc16Cms,
            CrcTurboAlgorithm::Crc16Dds110 => CrcAlgorithm::Crc16Dds110,
            CrcTurboAlgorithm::Crc16DectR => CrcAlgorithm::Crc16DectR,
            CrcTurboAlgorithm::Crc16DectX => CrcAlgorithm::Crc16DectX,
            CrcTurboAlgorithm::Crc16Dnp => CrcAlgorithm::Crc16Dnp,
            CrcTurboAlgorithm::Crc16En13757 => CrcAlgorithm::Crc16En13757,
            CrcTurboAlgorithm::Crc16Genibus => CrcAlgorithm::Crc16Genibus,
            CrcTurboAlgorithm::Crc16Gsm => CrcAlgorithm::Crc16Gsm,
            CrcTurboAlgorithm::Crc16Ibm3740 => CrcAlgorithm::Crc16Ibm3740,
            CrcTurboAlgorithm::Crc16IbmSdlc => CrcAlgorithm::Crc16IbmSdlc,
            CrcTurboAlgorithm::Crc16IsoIec144433A => CrcAlgorithm::Crc16IsoIec144433A,
            CrcTurboAlgorithm::Crc16Kermit => CrcAlgorithm::Crc16Kermit,
            CrcTurboAlgorithm::Crc16Lj1200 => CrcAlgorithm::Crc16Lj1200,
            CrcTurboAlgorithm::Crc16M17 => CrcAlgorithm::Crc16M17,
            CrcTurboAlgorithm::Crc16MaximDow => CrcAlgorithm::Crc16MaximDow,
            CrcTurboAlgorithm::Crc16Mcrf4xx => CrcAlgorithm::Crc16Mcrf4xx,
            CrcTurboAlgorithm::Crc16Modbus => CrcAlgorithm::Crc16Modbus,
            CrcTurboAlgorithm::Crc16Nrsc5 => CrcAlgorithm::Crc16Nrsc5,
            CrcTurboAlgorithm::Crc16OpensafetyA => CrcAlgorithm::Crc16OpensafetyA,
            CrcTurboAlgorithm::Crc16OpensafetyB => CrcAlgorithm::Crc16OpensafetyB,
            CrcTurboAlgorithm::Crc16Profibus => CrcAlgorithm::Crc16Profibus,
            CrcTurboAlgorithm::Crc16Riello => CrcAlgorithm::Crc16Riello,
            CrcTurboAlgorithm::Crc16SpiFujitsu => CrcAlgorithm::Crc16SpiFujitsu,
            CrcTurboAlgorithm::Crc16T10Dif => CrcAlgorithm::Crc16T10Dif,
            CrcTurboAlgorithm::Crc16Teledisk => CrcAlgorithm::Crc16Teledisk,
            CrcTurboAlgorithm::Crc16Tms37157 => CrcAlgorithm::Crc16Tms37157,
            CrcTurboAlgorithm::Crc16Umts => CrcAlgorithm::Crc16Umts,
            CrcTurboAlgorithm::Crc16Usb => CrcAlgorithm::Crc16Usb,
            CrcTurboAlgorithm::Crc16Xmodem => CrcAlgorithm::Crc16Xmodem,
            CrcTurboAlgorithm::Crc32Aixm => CrcAlgorithm::Crc32Aixm,
            CrcTurboAlgorithm::Crc32Autosar => CrcAlgorithm::Crc32Autosar,
            CrcTurboAlgorithm::Crc32Base91D => CrcAlgorithm::Crc32Base91D,
            CrcTurboAlgorithm::Crc32Bzip2 => CrcAlgorithm::Crc32Bzip2,
            CrcTurboAlgorithm::Crc32CdRomEdc => CrcAlgorithm::Crc32CdRomEdc,
            CrcTurboAlgorithm::Crc32Cksum => CrcAlgorithm::Crc32Cksum,
            CrcTurboAlgorithm::Crc32Custom => CrcAlgorithm::Crc32Custom,
            CrcTurboAlgorithm::Crc32Iscsi => CrcAlgorithm::Crc32Iscsi,
            CrcTurboAlgorithm::Crc32IsoHdlc => CrcAlgorithm::Crc32IsoHdlc,
            CrcTurboAlgorithm::Crc32Jamcrc => CrcAlgorithm::Crc32Jamcrc,
            CrcTurboAlgorithm::Crc32Mef => CrcAlgorithm::Crc32Mef,
            CrcTurboAlgorithm::Crc32Mpeg2 => CrcAlgorithm::Crc32Mpeg2,
            CrcTurboAlgorithm::Crc32Xfer => CrcAlgorithm::Crc32Xfer,
            CrcTurboAlgorithm::CrcCustom => CrcAlgorithm::CrcCustom,
            CrcTurboAlgorithm::Crc64Custom => CrcAlgorithm::Crc64Custom,
            CrcTurboAlgorithm::Crc64Ecma182 => CrcAlgorithm::Crc64Ecma182,
            CrcTurboAlgorithm::Crc64GoIso => CrcAlgorithm::Crc64GoIso,
            CrcTurboAlgorithm::Crc64Ms => CrcAlgorithm::Crc64Ms,
            CrcTurboAlgorithm::Crc64Nvme => CrcAlgorithm::Crc64Nvme,
            CrcTurboAlgorithm::Crc64Redis => CrcAlgorithm::Crc64Redis,
            CrcTurboAlgorithm::Crc64We => CrcAlgorithm::Crc64We,
            CrcTurboAlgorithm::Crc64Xz => CrcAlgorithm::Crc64Xz,
        }
    }
}

/// Gets the last error that occurred in the current thread
/// Returns CrcTurboError::Success if no error has occurred
#[no_mangle]
pub extern "C" fn crc_turbo_get_last_error() -> CrcTurboError {
    LAST_ERROR.with(|e| e.get())
}

/// Clears the last error for the current thread
#[no_mangle]
pub extern "C" fn crc_turbo_clear_error() {
    clear_last_error();
}

/// Gets a human-readable error message for the given error code
/// Returns a pointer to a static string (do not free)
#[no_mangle]
pub extern "C" fn crc_turbo_error_message(error: CrcTurboError) -> *const c_char {
    error.message().as_ptr()
}

/// Custom CRC parameters
#[repr(C)]
pub struct CrcTurboParams {
    pub algorithm: CrcTurboAlgorithm,
    pub width: u8,
    pub poly: u64,
    pub init: u64,
    pub refin: bool,
    pub refout: bool,
    pub xorout: u64,
    pub check: u64,
    pub key_count: u32,
    pub keys: *const u64,
}

/// Fallible conversion from FFI struct to internal struct
///
/// Rejects a null or misaligned key pointer (`NullPointer`), a key count other than 23 or 25
/// (`InvalidKeyCount`), a width other than 16, 32 or 64 (`UnsupportedWidth`) and `refin`
/// differing from `refout` (`MismatchedReflection`), so that no caller-supplied value reaches a
/// panic inside the library.
fn try_params_from_ffi(value: &CrcTurboParams) -> Result<CrcParams, CrcTurboError> {
    if value.keys.is_null() || !value.keys.is_aligned() {
        return Err(CrcTurboError::NullPointer);
    }
    if !matches!(value.width, 16 | 32 | 64) {
        return Err(CrcTurboError::UnsupportedWidth);
    }
    if value.refin != value.refout {
        return Err(CrcTurboError::MismatchedReflection);
    }

    // The keys are read only once the count names one of the two supported array sizes, so the
    // read never runs past the array a caller passes with that count.
    let storage = match value.key_count {
        23 => {
            let keys = unsafe { *(value.keys as *const [u64; 23]) };
            crate::CrcKeysStorage::from_keys_fold_256(keys)
        }
        25 => {
            let keys = unsafe { *(value.keys as *const [u64; 25]) };
            crate::CrcKeysStorage::from_keys_fold_future_test(keys)
        }
        _ => return Err(CrcTurboError::InvalidKeyCount),
    };

    let init_algorithm = crate::structs::initial_state(value.width, value.init, value.refin);

    Ok(CrcParams {
        algorithm: value.algorithm.into(),
        name: "custom", // C interface doesn't need the name field
        width: value.width,
        poly: value.poly,
        init: value.init,
        init_algorithm,
        refin: value.refin,
        refout: value.refout,
        xorout: value.xorout,
        check: value.check,
        keys: storage,
    })
}

// Convert from FFI struct to internal struct (legacy, may panic)
// For backwards compatibility, but prefer try_params_from_ffi
impl From<CrcTurboParams> for CrcParams {
    fn from(value: CrcTurboParams) -> Self {
        try_params_from_ffi(&value)
            .expect("Invalid CRC parameters: unsupported key count or null pointer")
    }
}

// Convert from internal struct to FFI struct
#[allow(deprecated)]
impl From<CrcParams> for CrcTurboParams {
    fn from(params: CrcParams) -> Self {
        // Create stable key pointer for FFI usage
        let (keys_ptr, key_count) = create_stable_key_pointer(&params.keys);

        CrcTurboParams {
            algorithm: match params.algorithm {
                CrcAlgorithm::Crc16Arc => CrcTurboAlgorithm::Crc16Arc,
                CrcAlgorithm::Crc16Cdma2000 => CrcTurboAlgorithm::Crc16Cdma2000,
                CrcAlgorithm::Crc16Cms => CrcTurboAlgorithm::Crc16Cms,
                CrcAlgorithm::Crc16Dds110 => CrcTurboAlgorithm::Crc16Dds110,
                CrcAlgorithm::Crc16DectR => CrcTurboAlgorithm::Crc16DectR,
                CrcAlgorithm::Crc16DectX => CrcTurboAlgorithm::Crc16DectX,
                CrcAlgorithm::Crc16Dnp => CrcTurboAlgorithm::Crc16Dnp,
                CrcAlgorithm::Crc16En13757 => CrcTurboAlgorithm::Crc16En13757,
                CrcAlgorithm::Crc16Genibus => CrcTurboAlgorithm::Crc16Genibus,
                CrcAlgorithm::Crc16Gsm => CrcTurboAlgorithm::Crc16Gsm,
                CrcAlgorithm::Crc16Ibm3740 => CrcTurboAlgorithm::Crc16Ibm3740,
                CrcAlgorithm::Crc16IbmSdlc => CrcTurboAlgorithm::Crc16IbmSdlc,
                CrcAlgorithm::Crc16IsoIec144433A => CrcTurboAlgorithm::Crc16IsoIec144433A,
                CrcAlgorithm::Crc16Kermit => CrcTurboAlgorithm::Crc16Kermit,
                CrcAlgorithm::Crc16Lj1200 => CrcTurboAlgorithm::Crc16Lj1200,
                CrcAlgorithm::Crc16M17 => CrcTurboAlgorithm::Crc16M17,
                CrcAlgorithm::Crc16MaximDow => CrcTurboAlgorithm::Crc16MaximDow,
                CrcAlgorithm::Crc16Mcrf4xx => CrcTurboAlgorithm::Crc16Mcrf4xx,
                CrcAlgorithm::Crc16Modbus => CrcTurboAlgorithm::Crc16Modbus,
                CrcAlgorithm::Crc16Nrsc5 => CrcTurboAlgorithm::Crc16Nrsc5,
                CrcAlgorithm::Crc16OpensafetyA => CrcTurboAlgorithm::Crc16OpensafetyA,
                CrcAlgorithm::Crc16OpensafetyB => CrcTurboAlgorithm::Crc16OpensafetyB,
                CrcAlgorithm::Crc16Profibus => CrcTurboAlgorithm::Crc16Profibus,
                CrcAlgorithm::Crc16Riello => CrcTurboAlgorithm::Crc16Riello,
                CrcAlgorithm::Crc16SpiFujitsu => CrcTurboAlgorithm::Crc16SpiFujitsu,
                CrcAlgorithm::Crc16T10Dif => CrcTurboAlgorithm::Crc16T10Dif,
                CrcAlgorithm::Crc16Teledisk => CrcTurboAlgorithm::Crc16Teledisk,
                CrcAlgorithm::Crc16Tms37157 => CrcTurboAlgorithm::Crc16Tms37157,
                CrcAlgorithm::Crc16Umts => CrcTurboAlgorithm::Crc16Umts,
                CrcAlgorithm::Crc16Usb => CrcTurboAlgorithm::Crc16Usb,
                CrcAlgorithm::Crc16Xmodem => CrcTurboAlgorithm::Crc16Xmodem,
                CrcAlgorithm::Crc32Aixm => CrcTurboAlgorithm::Crc32Aixm,
                CrcAlgorithm::Crc32Autosar => CrcTurboAlgorithm::Crc32Autosar,
                CrcAlgorithm::Crc32Base91D => CrcTurboAlgorithm::Crc32Base91D,
                CrcAlgorithm::Crc32Bzip2 => CrcTurboAlgorithm::Crc32Bzip2,
                CrcAlgorithm::Crc32CdRomEdc => CrcTurboAlgorithm::Crc32CdRomEdc,
                CrcAlgorithm::Crc32Cksum => CrcTurboAlgorithm::Crc32Cksum,
                CrcAlgorithm::Crc32Custom => CrcTurboAlgorithm::Crc32Custom,
                CrcAlgorithm::Crc32Iscsi => CrcTurboAlgorithm::Crc32Iscsi,
                CrcAlgorithm::Crc32IsoHdlc => CrcTurboAlgorithm::Crc32IsoHdlc,
                CrcAlgorithm::Crc32Jamcrc => CrcTurboAlgorithm::Crc32Jamcrc,
                CrcAlgorithm::Crc32Mef => CrcTurboAlgorithm::Crc32Mef,
                CrcAlgorithm::Crc32Mpeg2 => CrcTurboAlgorithm::Crc32Mpeg2,
                CrcAlgorithm::Crc32Xfer => CrcTurboAlgorithm::Crc32Xfer,
                CrcAlgorithm::CrcCustom => CrcTurboAlgorithm::CrcCustom,
                CrcAlgorithm::Crc64Custom => CrcTurboAlgorithm::Crc64Custom,
                CrcAlgorithm::Crc64Ecma182 => CrcTurboAlgorithm::Crc64Ecma182,
                CrcAlgorithm::Crc64GoIso => CrcTurboAlgorithm::Crc64GoIso,
                CrcAlgorithm::Crc64Ms => CrcTurboAlgorithm::Crc64Ms,
                CrcAlgorithm::Crc64Nvme => CrcTurboAlgorithm::Crc64Nvme,
                CrcAlgorithm::Crc64Redis => CrcTurboAlgorithm::Crc64Redis,
                CrcAlgorithm::Crc64We => CrcTurboAlgorithm::Crc64We,
                CrcAlgorithm::Crc64Xz => CrcTurboAlgorithm::Crc64Xz,
            },
            width: params.width,
            poly: params.poly,
            init: params.init,
            refin: params.refin,
            refout: params.refout,
            xorout: params.xorout,
            check: params.check,
            key_count,
            keys: keys_ptr,
        }
    }
}

/// The library's algorithm for a C enum value, or `UnsupportedAlgorithm` for the custom
/// variants, whose parameters the library would otherwise panic looking for.
fn builtin_algorithm(algorithm: CrcTurboAlgorithm) -> Result<CrcAlgorithm, CrcTurboError> {
    match algorithm {
        CrcTurboAlgorithm::CrcCustom
        | CrcTurboAlgorithm::Crc32Custom
        | CrcTurboAlgorithm::Crc64Custom => Err(CrcTurboError::UnsupportedAlgorithm),
        builtin => Ok(builtin.into()),
    }
}

/// Creates a new Digest to compute CRC checksums using algorithm
/// Returns NULL for a custom algorithm variant (use crc_turbo_digest_new_with_params)
#[no_mangle]
pub extern "C" fn crc_turbo_digest_new(algorithm: CrcTurboAlgorithm) -> *mut CrcTurboDigestHandle {
    let algorithm = match builtin_algorithm(algorithm) {
        Ok(algorithm) => algorithm,
        Err(error) => {
            set_last_error(error);
            return std::ptr::null_mut();
        }
    };
    clear_last_error();
    let digest = Box::new(Digest::new(algorithm));
    let handle = Box::new(CrcTurboDigestHandle(Box::into_raw(digest)));
    Box::into_raw(handle)
}

/// Creates a new Digest with a custom initial state
/// Returns NULL for a custom algorithm variant
#[no_mangle]
pub extern "C" fn crc_turbo_digest_new_with_init_state(
    algorithm: CrcTurboAlgorithm,
    init_state: u64,
) -> *mut CrcTurboDigestHandle {
    let algorithm = match builtin_algorithm(algorithm) {
        Ok(algorithm) => algorithm,
        Err(error) => {
            set_last_error(error);
            return std::ptr::null_mut();
        }
    };
    clear_last_error();
    let digest = Box::new(Digest::new_with_init_state(algorithm, init_state));
    let handle = Box::new(CrcTurboDigestHandle(Box::into_raw(digest)));
    Box::into_raw(handle)
}

/// Creates a new Digest to compute CRC checksums using custom parameters
/// Returns NULL if parameters are invalid (invalid key count or null pointer)
/// Call crc_turbo_get_last_error() to get the specific error code
#[no_mangle]
pub extern "C" fn crc_turbo_digest_new_with_params(
    params: CrcTurboParams,
) -> *mut CrcTurboDigestHandle {
    clear_last_error();
    match try_params_from_ffi(&params) {
        Ok(crc_params) => {
            let digest = Box::new(Digest::new_with_params(crc_params));
            let handle = Box::new(CrcTurboDigestHandle(Box::into_raw(digest)));
            Box::into_raw(handle)
        }
        Err(error) => {
            set_last_error(error);
            std::ptr::null_mut()
        }
    }
}

/// Updates the Digest with data
#[no_mangle]
pub extern "C" fn crc_turbo_digest_update(
    handle: *mut CrcTurboDigestHandle,
    data: *const c_char,
    len: usize,
) {
    if handle.is_null() {
        set_last_error(CrcTurboError::NullPointer);
        return;
    }
    if data.is_null() {
        set_last_error(CrcTurboError::NullPointer);
        return;
    }

    clear_last_error();
    unsafe {
        let digest = &mut *(*handle).0;

        #[allow(clippy::unnecessary_cast)]
        let bytes = slice::from_raw_parts(data as *const u8, len);
        digest.update(bytes);
    }
}

/// Calculates the CRC checksum for data that's been written to the Digest
/// Returns 0 on error (e.g. null handle)
#[no_mangle]
pub extern "C" fn crc_turbo_digest_finalize(handle: *mut CrcTurboDigestHandle) -> u64 {
    if handle.is_null() {
        set_last_error(CrcTurboError::NullPointer);
        return 0;
    }

    clear_last_error();
    unsafe {
        let digest = &*(*handle).0;
        digest.finalize()
    }
}

/// Free the Digest resources without finalizing
#[no_mangle]
pub extern "C" fn crc_turbo_digest_free(handle: *mut CrcTurboDigestHandle) {
    if handle.is_null() {
        set_last_error(CrcTurboError::NullPointer);
        return;
    }

    clear_last_error();
    unsafe {
        let handle = Box::from_raw(handle);
        let _ = Box::from_raw(handle.0); // This drops the digest
    }
}

/// Reset the Digest state
#[no_mangle]
pub extern "C" fn crc_turbo_digest_reset(handle: *mut CrcTurboDigestHandle) {
    if handle.is_null() {
        set_last_error(CrcTurboError::NullPointer);
        return;
    }

    clear_last_error();
    unsafe {
        let digest = &mut *(*handle).0;

        digest.reset();
    }
}

/// Finalize and reset the Digest in one operation
/// Returns 0 on error (e.g. null handle)
#[no_mangle]
pub extern "C" fn crc_turbo_digest_finalize_reset(handle: *mut CrcTurboDigestHandle) -> u64 {
    if handle.is_null() {
        set_last_error(CrcTurboError::NullPointer);
        return 0;
    }

    clear_last_error();
    unsafe {
        let digest = &mut *(*handle).0;

        digest.finalize_reset()
    }
}

/// Combine two Digest checksums
#[no_mangle]
pub extern "C" fn crc_turbo_digest_combine(
    handle1: *mut CrcTurboDigestHandle,
    handle2: *mut CrcTurboDigestHandle,
) {
    if handle1.is_null() || handle2.is_null() {
        set_last_error(CrcTurboError::NullPointer);
        return;
    }

    clear_last_error();
    // The second digest is copied out first, so the same handle passed twice never holds a
    // mutable and a shared reference to one object at once.
    unsafe {
        let digest2 = *(*handle2).0;
        let digest1 = &mut *(*handle1).0;
        digest1.combine(&digest2);
    }
}

/// Gets the amount of data processed by the Digest so far
/// Returns 0 on error (e.g. null handle)
#[no_mangle]
pub extern "C" fn crc_turbo_digest_get_amount(handle: *mut CrcTurboDigestHandle) -> u64 {
    if handle.is_null() {
        set_last_error(CrcTurboError::NullPointer);
        return 0;
    }

    clear_last_error();
    unsafe {
        let digest = &*(*handle).0;
        digest.get_amount()
    }
}

/// Gets the current state of the Digest
/// Returns 0 on error (e.g. null handle)
#[no_mangle]
pub extern "C" fn crc_turbo_digest_get_state(handle: *mut CrcTurboDigestHandle) -> u64 {
    if handle.is_null() {
        set_last_error(CrcTurboError::NullPointer);
        return 0;
    }
    clear_last_error();
    unsafe {
        let digest = &*(*handle).0;
        digest.get_state()
    }
}

/// Helper method to calculate a CRC checksum directly for a string using algorithm
/// Returns 0 on error (e.g. null data pointer)
#[no_mangle]
pub extern "C" fn crc_turbo_checksum(
    algorithm: CrcTurboAlgorithm,
    data: *const c_char,
    len: usize,
) -> u64 {
    if data.is_null() {
        set_last_error(CrcTurboError::NullPointer);
        return 0;
    }
    let algorithm = match builtin_algorithm(algorithm) {
        Ok(algorithm) => algorithm,
        Err(error) => {
            set_last_error(error);
            return 0;
        }
    };
    clear_last_error();
    unsafe {
        #[allow(clippy::unnecessary_cast)]
        let bytes = slice::from_raw_parts(data as *const u8, len);
        crate::checksum(algorithm, bytes)
    }
}

/// Helper method to calculate a CRC checksum directly for data using custom parameters
/// Returns 0 if parameters are invalid or data is null
/// Call crc_turbo_get_last_error() to get the specific error code
#[no_mangle]
pub extern "C" fn crc_turbo_checksum_with_params(
    params: CrcTurboParams,
    data: *const c_char,
    len: usize,
) -> u64 {
    if data.is_null() {
        set_last_error(CrcTurboError::NullPointer);
        return 0;
    }
    match try_params_from_ffi(&params) {
        Ok(crc_params) => {
            clear_last_error();
            unsafe {
                #[allow(clippy::unnecessary_cast)]
                let bytes = slice::from_raw_parts(data as *const u8, len);
                crate::checksum_with_params(crc_params, bytes)
            }
        }
        Err(error) => {
            set_last_error(error);
            0
        }
    }
}

/// Helper method to just calculate a CRC checksum directly for a file using algorithm
/// Returns 0 if path is null or file I/O fails
/// Call crc_turbo_get_last_error() to get the specific error code
#[no_mangle]
pub extern "C" fn crc_turbo_checksum_file(
    algorithm: CrcTurboAlgorithm,
    path_ptr: *const u8,
    path_len: usize,
) -> u64 {
    if path_ptr.is_null() {
        set_last_error(CrcTurboError::NullPointer);
        return 0;
    }

    let algorithm = match builtin_algorithm(algorithm) {
        Ok(algorithm) => algorithm,
        Err(error) => {
            set_last_error(error);
            return 0;
        }
    };
    unsafe {
        match crate::checksum_file(algorithm, &convert_to_string(path_ptr, path_len), None) {
            Ok(result) => {
                clear_last_error();
                result
            }
            Err(_) => {
                set_last_error(CrcTurboError::IoError);
                0
            }
        }
    }
}

/// Helper method to calculate a CRC checksum directly for a file using custom parameters
/// Returns 0 if parameters are invalid, path is null, or file I/O fails
/// Call crc_turbo_get_last_error() to get the specific error code
#[no_mangle]
pub extern "C" fn crc_turbo_checksum_file_with_params(
    params: CrcTurboParams,
    path_ptr: *const u8,
    path_len: usize,
) -> u64 {
    if path_ptr.is_null() {
        set_last_error(CrcTurboError::NullPointer);
        return 0;
    }

    match try_params_from_ffi(&params) {
        Ok(crc_params) => unsafe {
            match crate::checksum_file_with_params(
                crc_params,
                &convert_to_string(path_ptr, path_len),
                None,
            ) {
                Ok(result) => {
                    clear_last_error();
                    result
                }
                Err(_) => {
                    set_last_error(CrcTurboError::IoError);
                    0
                }
            }
        },
        Err(error) => {
            set_last_error(error);
            0
        }
    }
}

/// Combine two CRC checksums using algorithm
#[no_mangle]
pub extern "C" fn crc_turbo_checksum_combine(
    algorithm: CrcTurboAlgorithm,
    checksum1: u64,
    checksum2: u64,
    checksum2_len: u64,
) -> u64 {
    let algorithm = match builtin_algorithm(algorithm) {
        Ok(algorithm) => algorithm,
        Err(error) => {
            set_last_error(error);
            return 0;
        }
    };
    clear_last_error();
    crate::checksum_combine(algorithm, checksum1, checksum2, checksum2_len)
}

/// Combine two CRC checksums using custom parameters
/// Returns 0 if parameters are invalid
/// Call crc_turbo_get_last_error() to get the specific error code
#[no_mangle]
pub extern "C" fn crc_turbo_checksum_combine_with_params(
    params: CrcTurboParams,
    checksum1: u64,
    checksum2: u64,
    checksum2_len: u64,
) -> u64 {
    match try_params_from_ffi(&params) {
        Ok(crc_params) => {
            clear_last_error();
            crate::checksum_combine_with_params(crc_params, checksum1, checksum2, checksum2_len)
        }
        Err(error) => {
            set_last_error(error);
            0
        }
    }
}

/// Returns the custom CRC parameters for a given set of Rocksoft CRC parameters
/// If width is not 16, 32 or 64, sets error to UnsupportedWidth and returns parameters without
/// keys, which every _with_params function rejects
#[no_mangle]
pub extern "C" fn crc_turbo_get_custom_params(
    name_ptr: *const c_char,
    width: u8,
    poly: u64,
    init: u64,
    reflected: bool,
    xorout: u64,
    check: u64,
) -> CrcTurboParams {
    // An unsupported width would panic inside CrcParams::new; hand back parameters with no keys
    // instead, which every _with_params function rejects with NullPointer.
    if !matches!(width, 16 | 32 | 64) {
        set_last_error(CrcTurboError::UnsupportedWidth);
        return CrcTurboParams {
            algorithm: CrcTurboAlgorithm::CrcCustom,
            width,
            poly,
            init,
            refin: reflected,
            refout: reflected,
            xorout,
            check,
            key_count: 0,
            keys: std::ptr::null(),
        };
    }
    clear_last_error();

    let name = if name_ptr.is_null() {
        "custom"
    } else {
        unsafe {
            match CStr::from_ptr(name_ptr).to_str() {
                Ok(s) => s,
                Err(_) => {
                    set_last_error(CrcTurboError::InvalidUtf8);
                    "custom"
                }
            }
        }
    };

    // Get the custom params from the library
    let params = CrcParams::new(
        get_or_leak_string(name), // ✅ Use cached leak
        width,
        poly,
        init,
        reflected,
        xorout,
        check,
    );

    // Create stable key pointer for FFI usage
    let (keys_ptr, key_count) = create_stable_key_pointer(&params.keys);

    // Convert to FFI struct - use CrcCustom for all widths since CrcParams::new now uses it
    CrcTurboParams {
        algorithm: CrcTurboAlgorithm::CrcCustom,
        width: params.width,
        poly: params.poly,
        init: params.init,
        refin: params.refin,
        refout: params.refout,
        xorout: params.xorout,
        check: params.check,
        key_count,
        keys: keys_ptr,
    }
}

/// Gets the target build properties (CPU architecture and fine-tuning parameters) for this algorithm
/// Returns NULL if string conversion fails
/// Call crc_turbo_get_last_error() to get the specific error code
/// The string is allocated for the caller: release it with crc_turbo_free_string()
#[no_mangle]
pub extern "C" fn crc_turbo_get_calculator_target(algorithm: CrcTurboAlgorithm) -> *const c_char {
    let target = get_calculator_target(algorithm.into());

    match std::ffi::CString::new(target) {
        Ok(s) => {
            clear_last_error();
            s.into_raw()
        }
        Err(_) => {
            set_last_error(CrcTurboError::StringConversionError);
            std::ptr::null_mut()
        }
    }
}

/// Frees a string returned by crc_turbo_get_calculator_target()
/// A NULL pointer is ignored
#[no_mangle]
pub extern "C" fn crc_turbo_free_string(string: *mut c_char) {
    if !string.is_null() {
        unsafe { drop(std::ffi::CString::from_raw(string)) };
    }
}

/// Gets the version of this library
/// Returns a pointer to "unknown" if version string is invalid
#[no_mangle]
pub extern "C" fn crc_turbo_get_version() -> *const c_char {
    const VERSION: &CStr =
        match CStr::from_bytes_with_nul(concat!(env!("CARGO_PKG_VERSION"), "\0").as_bytes()) {
            Ok(version) => version,
            // Fallback to "unknown" if version string is malformed
            Err(_) => c"unknown",
        };

    VERSION.as_ptr()
}

/// Calculates the CRC-32/ISCSI checksum (commonly called "crc32c" in many, but not all,
/// implementations).
///
/// https://reveng.sourceforge.io/crc-catalogue/all.htm#crc.cat.crc-32-iscsi
///
/// Returns 0 on error (e.g. null data pointer)
#[no_mangle]
pub extern "C" fn crc_turbo_crc32_iscsi(data: *const c_char, len: usize) -> u32 {
    if data.is_null() {
        set_last_error(CrcTurboError::NullPointer);
        return 0;
    }
    clear_last_error();
    unsafe {
        #[allow(clippy::unnecessary_cast)]
        let bytes = slice::from_raw_parts(data as *const u8, len);
        crate::crc32_iscsi(bytes)
    }
}

/// Calculates the CRC-32/ISO-HDLC checksum (commonly called "crc32" in many, but not all,
/// implementations).
///
/// https://reveng.sourceforge.io/crc-catalogue/all.htm#crc.cat.crc-32-iso-hdlc
///
/// Returns 0 on error (e.g. null data pointer)
#[no_mangle]
pub extern "C" fn crc_turbo_crc32_iso_hdlc(data: *const c_char, len: usize) -> u32 {
    if data.is_null() {
        set_last_error(CrcTurboError::NullPointer);
        return 0;
    }
    clear_last_error();
    unsafe {
        #[allow(clippy::unnecessary_cast)]
        let bytes = slice::from_raw_parts(data as *const u8, len);
        crate::crc32_iso_hdlc(bytes)
    }
}

/// Calculates the CRC-64/NVME checksum.
///
/// https://reveng.sourceforge.io/crc-catalogue/all.htm#crc.cat.crc-64-nvme
///
/// Returns 0 on error (e.g. null data pointer)
#[no_mangle]
pub extern "C" fn crc_turbo_crc64_nvme(data: *const c_char, len: usize) -> u64 {
    if data.is_null() {
        set_last_error(CrcTurboError::NullPointer);
        return 0;
    }
    clear_last_error();
    unsafe {
        #[allow(clippy::unnecessary_cast)]
        let bytes = slice::from_raw_parts(data as *const u8, len);
        crate::crc64_nvme(bytes)
    }
}

unsafe fn convert_to_string(data: *const u8, len: usize) -> String {
    if data.is_null() {
        return String::new();
    }

    // Safely construct string slice from raw parts
    match std::str::from_utf8(slice::from_raw_parts(data, len)) {
        Ok(s) => s.to_string(),
        Err(_) => String::new(), // Return empty string for invalid UTF-8
    }
}

fn get_or_leak_string(s: &str) -> &'static str {
    let cache = STRING_CACHE.get_or_init(|| Mutex::new(HashSet::new()));
    let mut cache = cache.lock().unwrap();

    // Check if we already have this string
    if let Some(&cached) = cache.get(s) {
        return cached;
    }

    // Leak it and cache the result
    let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
    cache.insert(leaked);
    leaked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(width: u8, init: u64, refin: bool, refout: bool, keys: &[u64; 23]) -> CrcTurboParams {
        CrcTurboParams {
            algorithm: CrcTurboAlgorithm::Crc32Custom,
            width,
            poly: 0x04c1_1db7,
            init,
            refin,
            refout,
            xorout: 0,
            check: 0,
            key_count: 23,
            keys: keys.as_ptr(),
        }
    }

    #[test]
    fn mismatched_reflection_is_rejected_at_the_boundary() {
        let keys = [0u64; 23];
        assert!(matches!(
            try_params_from_ffi(&params(32, 0, true, false, &keys)),
            Err(CrcTurboError::MismatchedReflection)
        ));
        assert!(matches!(
            try_params_from_ffi(&params(32, 0, false, true, &keys)),
            Err(CrcTurboError::MismatchedReflection)
        ));
        assert!(try_params_from_ffi(&params(32, 0, true, true, &keys)).is_ok());
        assert!(try_params_from_ffi(&params(32, 0, false, false, &keys)).is_ok());
    }

    #[test]
    fn a_reflected_init_is_reversed_within_its_width() {
        let keys = [0u64; 23];
        let reflected = try_params_from_ffi(&params(32, 0x1234_5678, true, true, &keys))
            .expect("valid parameters");
        assert_eq!(reflected.init, 0x1234_5678);
        assert_eq!(reflected.init_algorithm, 0x1e6a_2c48);
        let forward = try_params_from_ffi(&params(32, 0x1234_5678, false, false, &keys))
            .expect("valid parameters");
        assert_eq!(forward.init_algorithm, 0x1234_5678);
    }
}
