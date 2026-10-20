# Contributors

`crc-turbo` is a fork. Most of the code in this repository was written by
other people, and their copyright and the MIT OR Apache-2.0 licence of the
upstream project carry over unchanged.

## Upstream: crc-fast

<https://github.com/awesomized/crc-fast-rust> - written and maintained by
**Don MacAskill**, with contributions from the people in its history.

`src/combine.rs` keeps Mark Adler's `crccomb.c`, the generalised form of
zlib's CRC combination, as the reference its tests check against (Zlib
licence; the notice is kept in that file). Its x^(2^k) table for one-shot
combines follows zlib's `x2n_table`.

## Changes taken from crc-fast's open pull requests and issues

These were proposed or reported to crc-fast and had not been merged or
answered when the fork was taken. Each commit that carries one names its
author.

- **netsbot**: `core::arch` in the AVX-512 tier, for `no_std` builds
  (crc-fast#42).
- **MuntasirSZN**: the report of that `no_std` build failure (crc-fast#57).
- **onur-ozkan**: embedded `no_std` builds that do not force a panic
  handler on the consumer (crc-fast#49).
- **MggMuggins**: `rand` 0.10 and `ffi` requiring `std`, from the Debian
  packaging work (crc-fast#48).
- **musicinmybrain**: the SPDX expression for the Zlib-licensed code in
  `src/combine.rs` (crc-fast#41).
- **xtqqczze**: the `spin`, `rand`, `crossbeam-epoch` and `digest` updates
  (crc-fast#50, #51, #55, #46), and the advisories behind them (crc-fast#52,
  #53, #54).
- **jacobloveless**: the lookup-table size measurement and request behind
  `generated-tables` (crc-fast#61).
- **Nukesor**: the request for `digest` 0.11 (crc-fast#44).
- **dcbtsla**: an AVX2 + VPCLMULQDQ tier for CPUs without AVX-512
  (crc-fast#56), written independently of and before the one this crate
  ships, whose feature-detection model this crate adopts; and the field
  arithmetic behind `checksum_combine` and `CrcCombine` (crc-fast#60).
- **GiorgioRegni**: the review of CRC-32/ISCSI on 1.10.0 that found the
  undefined upper bits in the AVX-512 kernel, the aarch64 `crc` and
  AVX512BW detection gaps and the missing test coverage (crc-fast#64).
- **dengyt2018**: the request for error values in place of panics for
  `no_std` callers (crc-fast#37).

## This fork

- NZB Man (maintainer)
