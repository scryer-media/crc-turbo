# crc-turbo

**Placeholder. Nothing is released here yet.**

`crc-turbo` is a planned, actively maintained fork of
[crc-fast](https://github.com/awesomized/crc-fast-rust), the SIMD CRC-16,
CRC-32 and CRC-64 library by Don MacAskill, taken at its 1.10.0 release.

crc-fast has had no release, merge or maintainer reply since 2025-12-31,
while security dependency bumps, `no_std` fixes, `digest` 0.11 support and
a 256-bit x86 tier wait in its open pull requests. We asked in
[awesomized/crc-fast-rust#62](https://github.com/awesomized/crc-fast-rust/issues/62)
to help maintain crc-fast itself and offered the maintainer until
**2026-10-20** to respond. This repository exists so the fork path stays open
in the meantime. If crc-fast resumes maintenance, this fork is not needed and
this repository will say so.

## What will land here after 2026-10-20

The code is written and gated locally. It keeps crc-fast 1.10.0's public API
(migration is the dependency line plus `crc_fast::` → `crc_turbo::`) and adds,
with every change credited to the author of the crc-fast pull request or
issue it came from:

- an AVX2 + VPCLMULQDQ 256-bit folding tier for x86 CPUs without AVX-512,
  for every algorithm in the table and both bit orders
- checksum combination in a handful of field multiplies instead of a
  per-bit loop, plus a reusable `CrcCombine`
- the dependency updates for the open RUSTSEC advisories and the yanked
  `spin` release
- `no_std` and embedded build fixes, `ffi` implying `std`, an ordinary
  `rlib` with the C library built separately
- `digest` 0.11 support behind an opt-in feature
- optional generated lookup tables for smaller binaries
- a corrected licence expression for the Zlib-licensed combine reference

## Licence

MIT OR Apache-2.0, with Zlib for the code inherited from Mark Adler's
`crccomb.c`, the same as crc-fast. The licence files arrive with the code.
