# Contributing

Thanks for looking. A few things before you open a pull request:

- Read [AGENTS.md](../AGENTS.md); the rules there apply to humans too.
- This is a hard **fork** of [crc-fast](https://github.com/awesomized/crc-fast-rust).
  It is not rebased onto crc-fast, so a fix belongs here directly. If you are
  bringing over a change from a crc-fast pull request, say which one and
  keep its author's credit.
- A change to an accelerated tier comes with the forced-tier test against the
  software implementation over the whole algorithm table, and with numbers
  for the tiers it should not move.
- Run `cargo fmt --all`, `cargo clippy --locked --all-targets --all-features -- -D warnings`
  and `cargo test --locked --all-features` locally.
- Record every divergence from crc-fast in the "Fork" section of
  [CHANGELOG.md](../CHANGELOG.md), in the same change.
- Enable the hooks once per clone: `git config core.hooksPath .githooks`.
