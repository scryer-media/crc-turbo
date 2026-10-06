# Security policy

## Supported versions

Only the latest published release of `crc-turbo` receives fixes.

## Reporting a vulnerability

Please do not open a public issue for security problems. Report privately
through GitHub at <https://github.com/scryer-media/crc-turbo/security/advisories/new>
(the Security tab, "Report a vulnerability"); private reporting is enabled on
this repository. You will get an acknowledgement within a few days, and a
fix ships as a new release of the crate, credited to you unless you ask
otherwise.

## What counts

A CRC library's attack surface is small but not empty. Reports of any of
these are security reports and are handled as such:

- a wrong checksum from an accelerated tier, for any input, length,
  alignment or carried state, where the table-driven software path is right;
- an out-of-bounds read or write, or any other undefined behaviour, in the
  SIMD tiers or the C library (`libcrc_turbo.h`);
- a panic or unbounded work reachable through the public API or the C
  library from caller-supplied data or parameters.

Include the algorithm or `CrcParams`, the input (or its length and how it
was generated), the CPU and the tier `get_calculator_target` reports.

This crate is a hard fork of
[crc-fast](https://github.com/awesomized/crc-fast-rust) and is not kept in
step with it. A bug that exists there too is still fixed and released here;
we do not hold a fix for it, and we do not disclose on its behalf.
