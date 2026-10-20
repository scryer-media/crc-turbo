/* Links libcrc_turbo through libcrc_turbo.h and checks a few known values.
 * Built and run by the `capi` lane of ci.yml and by `make capi-test`. */
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "libcrc_turbo.h"

static int check(const char *what, uint64_t got, uint64_t want) {
  if (got != want) {
    fprintf(stderr, "%s: got %#llx, want %#llx\n", what, (unsigned long long)got,
            (unsigned long long)want);
    return 1;
  }
  return 0;
}

int main(void) {
  const char *data = "123456789";
  size_t len = strlen(data);
  int failed = 0;

  failed |= check("CRC-32/ISO-HDLC", crc_turbo_checksum(Crc32IsoHdlc, data, len), 0xcbf43926);
  failed |= check("CRC-32/ISCSI", crc_turbo_crc32_iscsi(data, len), 0xe3069283);
  failed |= check("CRC-64/NVME", crc_turbo_crc64_nvme(data, len), 0xae8b14860a799888ULL);
  failed |= check("CRC-16/T10-DIF", crc_turbo_checksum(Crc16T10Dif, data, len), 0xd0db);

  struct CrcTurboDigestHandle *digest = crc_turbo_digest_new(Crc64Xz);
  crc_turbo_digest_update(digest, data, 4);
  crc_turbo_digest_update(digest, data + 4, len - 4);
  failed |= check("CRC-64/XZ digest", crc_turbo_digest_finalize(digest), 0x995dc9bbdf1939faULL);
  crc_turbo_digest_free(digest);

  uint64_t head = crc_turbo_checksum(Crc32Bzip2, data, 4);
  uint64_t tail = crc_turbo_checksum(Crc32Bzip2, data + 4, len - 4);
  failed |= check("CRC-32/BZIP2 combine", crc_turbo_checksum_combine(Crc32Bzip2, head, tail, len - 4),
                  0xfc891918);

  failed |= check("no error", crc_turbo_get_last_error(), Success);

  /* The custom variants carry no parameters: an error, never an abort. */
  failed |= check("custom without params", crc_turbo_checksum(CrcCustom, "1", 1), 0);
  failed |= check("custom error", crc_turbo_get_last_error(), UnsupportedAlgorithm);
  crc_turbo_clear_error();

  /* Mismatched reflection is refused at the boundary, never an abort. */
  struct CrcTurboParams mixed =
      crc_turbo_get_custom_params("mixed", 32, 0x04c11db7, 0xffffffff, true, 0xffffffff, 0xcbf43926);
  mixed.refout = false;
  failed |= check("mixed reflection", crc_turbo_checksum_with_params(mixed, data, len), 0);
  failed |= check("mixed reflection error", crc_turbo_get_last_error(), MismatchedReflection);
  crc_turbo_clear_error();

  if (!failed) {
    char *target = (char *)crc_turbo_get_calculator_target(Crc32IsoHdlc);
    printf("libcrc_turbo %s: ok (%s)\n", crc_turbo_get_version(), target);
    crc_turbo_free_string(target);
  }
  return failed;
}
