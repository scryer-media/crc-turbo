// Copyright 2026 the crc-turbo authors. Licensed under MIT or Apache-2.0.

//! The error the fallible entry points return.

use core::fmt;

use crate::CrcAlgorithm;

/// Why a CRC could not be set up.
///
/// Returned by [`CrcParams::try_new`](crate::CrcParams::try_new), [`try_checksum`](crate::try_checksum),
/// [`Digest::try_new`](crate::Digest::try_new) and
/// [`CrcCombine::try_new`](crate::CrcCombine::try_new), which are the non-panicking forms of
/// the functions that take an algorithm or build parameters. Checksumming itself cannot fail:
/// once parameters exist or an algorithm has been accepted, every call returns a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CrcError {
    /// The width is not 16, 32 or 64.
    UnsupportedWidth(u8),
    /// `refin` and `refout` differ, which no catalogued CRC of a supported width does and the
    /// combine arithmetic does not model.
    MismatchedReflection,
    /// A `Custom` algorithm was named where parameters are needed; those variants are
    /// placeholders that [`CrcParams`](crate::CrcParams) carries, and only the `*_with_params`
    /// functions accept them.
    MissingParams(CrcAlgorithm),
}

impl fmt::Display for CrcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedWidth(width) => {
                write!(f, "unsupported CRC width {width}: must be 16, 32 or 64")
            }
            Self::MismatchedReflection => {
                f.write_str("unsupported CRC configuration: refin and refout differ")
            }
            Self::MissingParams(algorithm) => write!(
                f,
                "{algorithm:?} is a placeholder for custom parameters: build a CrcParams and use the *_with_params functions"
            ),
        }
    }
}

impl core::error::Error for CrcError {}

#[cfg(test)]
mod tests {
    use super::CrcError;
    use crate::CrcAlgorithm;

    #[test]
    fn every_variant_displays_its_cause() {
        assert_eq!(
            CrcError::UnsupportedWidth(8).to_string(),
            "unsupported CRC width 8: must be 16, 32 or 64"
        );
        assert_eq!(
            CrcError::MismatchedReflection.to_string(),
            "unsupported CRC configuration: refin and refout differ"
        );
        assert!(CrcError::MissingParams(CrcAlgorithm::CrcCustom)
            .to_string()
            .starts_with("CrcCustom is a placeholder"));
    }

    #[test]
    fn it_is_a_std_error() {
        fn takes_error<E: std::error::Error + Send + Sync + 'static>(_: E) {}
        takes_error(CrcError::MismatchedReflection);
    }
}
