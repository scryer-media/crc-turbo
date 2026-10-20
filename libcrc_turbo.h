/* crc_turbo library C/C++ API - Copyright 2025 Don MacAskill */
/* This header is auto-generated. Do not edit directly. */

#ifndef CRC_TURBO_H
#define CRC_TURBO_H

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * Error codes for FFI operations
 */
typedef enum CrcTurboError {
  /**
   * Operation completed successfully
   */
  Success = 0,
  /**
   * Lock was poisoned (thread panicked while holding lock)
   */
  LockPoisoned = 1,
  /**
   * Null pointer was passed where non-null required
   */
  NullPointer = 2,
  /**
   * Invalid key count for CRC parameters
   */
  InvalidKeyCount = 3,
  /**
   * Unsupported CRC width (must be 16, 32 or 64)
   */
  UnsupportedWidth = 4,
  /**
   * Invalid UTF-8 string
   */
  InvalidUtf8 = 5,
  /**
   * File I/O error
   */
  IoError = 6,
  /**
   * Internal string conversion error
   */
  StringConversionError = 7,
  /**
   * The algorithm is a custom variant, which carries its parameters in a CrcTurboParams:
   * use the `_with_params` function
   */
  UnsupportedAlgorithm = 8,
  /**
   * `refin` and `refout` differ, which the library does not support
   */
  MismatchedReflection = 9,
} CrcTurboError;

/**
 * The supported CRC algorithms
 *
 * As with any C enum passed by value, a value outside this list is undefined behaviour. The
 * custom variants carry no parameters of their own: they are valid only inside a
 * CrcTurboParams, and the functions that take an algorithm alone report UnsupportedAlgorithm
 * for them.
 */
typedef enum CrcTurboAlgorithm {
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
} CrcTurboAlgorithm;

/**
 * Represents a CRC Digest, which is used to compute CRC checksums.
 *
 * The `Digest` struct maintains the state of the CRC computation, including
 * the current state, the amount of data processed, the CRC parameters, and
 * the calculator function used to perform the CRC calculation.
 */
typedef struct CrcTurboDigest CrcTurboDigest;

/**
 * A handle to the Digest object
 */
typedef struct CrcTurboDigestHandle {
  struct CrcTurboDigest *_0;
} CrcTurboDigestHandle;

/**
 * Custom CRC parameters
 */
typedef struct CrcTurboParams {
  enum CrcTurboAlgorithm algorithm;
  uint8_t width;
  uint64_t poly;
  uint64_t init;
  bool refin;
  bool refout;
  uint64_t xorout;
  uint64_t check;
  uint32_t key_count;
  const uint64_t *keys;
} CrcTurboParams;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * Gets the last error that occurred in the current thread
 * Returns CrcTurboError::Success if no error has occurred
 */
enum CrcTurboError crc_turbo_get_last_error(void);

/**
 * Clears the last error for the current thread
 */
void crc_turbo_clear_error(void);

/**
 * Gets a human-readable error message for the given error code
 * Returns a pointer to a static string (do not free)
 */
const char *crc_turbo_error_message(enum CrcTurboError error);

/**
 * Creates a new Digest to compute CRC checksums using algorithm
 * Returns NULL for a custom algorithm variant (use crc_turbo_digest_new_with_params)
 */
struct CrcTurboDigestHandle *crc_turbo_digest_new(enum CrcTurboAlgorithm algorithm);

/**
 * Creates a new Digest with a custom initial state
 * Returns NULL for a custom algorithm variant
 */
struct CrcTurboDigestHandle *crc_turbo_digest_new_with_init_state(enum CrcTurboAlgorithm algorithm,
                                                                  uint64_t init_state);

/**
 * Creates a new Digest to compute CRC checksums using custom parameters
 * Returns NULL if parameters are invalid (invalid key count or null pointer)
 * Call crc_turbo_get_last_error() to get the specific error code
 */
struct CrcTurboDigestHandle *crc_turbo_digest_new_with_params(struct CrcTurboParams params);

/**
 * Updates the Digest with data
 */
void crc_turbo_digest_update(struct CrcTurboDigestHandle *handle, const char *data, uintptr_t len);

/**
 * Calculates the CRC checksum for data that's been written to the Digest
 * Returns 0 on error (e.g. null handle)
 */
uint64_t crc_turbo_digest_finalize(struct CrcTurboDigestHandle *handle);

/**
 * Free the Digest resources without finalizing
 */
void crc_turbo_digest_free(struct CrcTurboDigestHandle *handle);

/**
 * Reset the Digest state
 */
void crc_turbo_digest_reset(struct CrcTurboDigestHandle *handle);

/**
 * Finalize and reset the Digest in one operation
 * Returns 0 on error (e.g. null handle)
 */
uint64_t crc_turbo_digest_finalize_reset(struct CrcTurboDigestHandle *handle);

/**
 * Combine two Digest checksums
 */
void crc_turbo_digest_combine(struct CrcTurboDigestHandle *handle1,
                              struct CrcTurboDigestHandle *handle2);

/**
 * Gets the amount of data processed by the Digest so far
 * Returns 0 on error (e.g. null handle)
 */
uint64_t crc_turbo_digest_get_amount(struct CrcTurboDigestHandle *handle);

/**
 * Gets the current state of the Digest
 * Returns 0 on error (e.g. null handle)
 */
uint64_t crc_turbo_digest_get_state(struct CrcTurboDigestHandle *handle);

/**
 * Helper method to calculate a CRC checksum directly for a string using algorithm
 * Returns 0 on error (e.g. null data pointer)
 */
uint64_t crc_turbo_checksum(enum CrcTurboAlgorithm algorithm, const char *data, uintptr_t len);

/**
 * Helper method to calculate a CRC checksum directly for data using custom parameters
 * Returns 0 if parameters are invalid or data is null
 * Call crc_turbo_get_last_error() to get the specific error code
 */
uint64_t crc_turbo_checksum_with_params(struct CrcTurboParams params,
                                        const char *data,
                                        uintptr_t len);

/**
 * Helper method to just calculate a CRC checksum directly for a file using algorithm
 * Returns 0 if path is null or file I/O fails
 * Call crc_turbo_get_last_error() to get the specific error code
 */
uint64_t crc_turbo_checksum_file(enum CrcTurboAlgorithm algorithm,
                                 const uint8_t *path_ptr,
                                 uintptr_t path_len);

/**
 * Helper method to calculate a CRC checksum directly for a file using custom parameters
 * Returns 0 if parameters are invalid, path is null, or file I/O fails
 * Call crc_turbo_get_last_error() to get the specific error code
 */
uint64_t crc_turbo_checksum_file_with_params(struct CrcTurboParams params,
                                             const uint8_t *path_ptr,
                                             uintptr_t path_len);

/**
 * Combine two CRC checksums using algorithm
 */
uint64_t crc_turbo_checksum_combine(enum CrcTurboAlgorithm algorithm,
                                    uint64_t checksum1,
                                    uint64_t checksum2,
                                    uint64_t checksum2_len);

/**
 * Combine two CRC checksums using custom parameters
 * Returns 0 if parameters are invalid
 * Call crc_turbo_get_last_error() to get the specific error code
 */
uint64_t crc_turbo_checksum_combine_with_params(struct CrcTurboParams params,
                                                uint64_t checksum1,
                                                uint64_t checksum2,
                                                uint64_t checksum2_len);

/**
 * Returns the custom CRC parameters for a given set of Rocksoft CRC parameters
 * If width is not 16, 32 or 64, sets error to UnsupportedWidth and returns parameters without
 * keys, which every _with_params function rejects
 */
struct CrcTurboParams crc_turbo_get_custom_params(const char *name_ptr,
                                                  uint8_t width,
                                                  uint64_t poly,
                                                  uint64_t init,
                                                  bool reflected,
                                                  uint64_t xorout,
                                                  uint64_t check);

/**
 * Gets the target build properties (CPU architecture and fine-tuning parameters) for this algorithm
 * Returns NULL if string conversion fails
 * Call crc_turbo_get_last_error() to get the specific error code
 * The string is allocated for the caller: release it with crc_turbo_free_string()
 */
const char *crc_turbo_get_calculator_target(enum CrcTurboAlgorithm algorithm);

/**
 * Frees a string returned by crc_turbo_get_calculator_target()
 * A NULL pointer is ignored
 */
void crc_turbo_free_string(char *string);

/**
 * Gets the version of this library
 * Returns a pointer to "unknown" if version string is invalid
 */
const char *crc_turbo_get_version(void);

/**
 * Calculates the CRC-32/ISCSI checksum (commonly called "crc32c" in many, but not all,
 * implementations).
 *
 * https://reveng.sourceforge.io/crc-catalogue/all.htm#crc.cat.crc-32-iscsi
 *
 * Returns 0 on error (e.g. null data pointer)
 */
uint32_t crc_turbo_crc32_iscsi(const char *data, uintptr_t len);

/**
 * Calculates the CRC-32/ISO-HDLC checksum (commonly called "crc32" in many, but not all,
 * implementations).
 *
 * https://reveng.sourceforge.io/crc-catalogue/all.htm#crc.cat.crc-32-iso-hdlc
 *
 * Returns 0 on error (e.g. null data pointer)
 */
uint32_t crc_turbo_crc32_iso_hdlc(const char *data, uintptr_t len);

/**
 * Calculates the CRC-64/NVME checksum.
 *
 * https://reveng.sourceforge.io/crc-catalogue/all.htm#crc.cat.crc-64-nvme
 *
 * Returns 0 on error (e.g. null data pointer)
 */
uint64_t crc_turbo_crc64_nvme(const char *data, uintptr_t len);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* CRC_TURBO_H */
