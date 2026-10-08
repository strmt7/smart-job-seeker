//! Sealing candidate data at rest.
//!
//! The store keeps job postings (public data) in plain SQLite on purpose, but
//! the candidate's *own* material — the facts they assert and the documents
//! built from them — must not sit in plaintext in a file anyone with the disk
//! can read.
//!
//! On Windows this uses DPAPI (`CryptProtectData`), which derives the key from
//! the logged-in user's credentials and the machine: sealed bytes can only be
//! unsealed by the same user on the same machine, with no password for the
//! candidate to manage or for us to store. The FFI is written by hand rather
//! than pulling in a large bindings crate — it is three functions, stable since
//! Windows XP, and auditable in one screen.
//!
//! Every sealed value is wrapped in a small self-describing envelope
//! (`magic | version | sealer kind | payload`) so that:
//! - a value sealed by one backend can never be silently read as another;
//! - a future format change is detectable rather than producing garbage;
//! - tampering is rejected (DPAPI verifies integrity of its own payload).
//!
//! There is deliberately **no** backend that pretends to encrypt: the only
//! alternatives are DPAPI and an explicitly-named pass-through used by tests
//! and by platforms where no equivalent exists. The store records which mode
//! produced the data, so the app can tell the candidate the truth.

use serde::{Deserialize, Serialize};

pub const MAGIC: &[u8; 4] = b"WPSL";
pub const VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SealError {
    /// The platform backend refused (bad credentials, corrupted blob, ...).
    Backend(String),
    /// The envelope is not ours, or not a version we understand.
    MalformedEnvelope(String),
    /// The envelope was sealed by a different backend than the one in use.
    WrongSealer {
        found: SealerKind,
        active: SealerKind,
    },
    /// Integrity check failed: the bytes were altered.
    Tampered,
    /// No sealing backend is available on this platform.
    Unavailable,
}

impl std::fmt::Display for SealError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SealError::Backend(m) => write!(f, "sealing backend failed: {m}"),
            SealError::MalformedEnvelope(m) => write!(f, "sealed value is malformed: {m}"),
            SealError::WrongSealer { found, active } => write!(
                f,
                "value was sealed with {found:?} but this machine uses {active:?}"
            ),
            SealError::Tampered => write!(f, "sealed value failed its integrity check"),
            SealError::Unavailable => write!(f, "no sealing backend is available here"),
        }
    }
}

impl std::error::Error for SealError {}

/// Which backend produced a sealed value. Recorded inside every envelope *and*
/// in the store's metadata, so the two can be compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SealerKind {
    /// Windows DPAPI, user + machine scoped.
    Dpapi,
    /// No encryption. Named plainly so nothing can claim otherwise.
    PlaintextForTestsOrOtherPlatforms,
}

impl SealerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SealerKind::Dpapi => "dpapi",
            SealerKind::PlaintextForTestsOrOtherPlatforms => "plaintext",
        }
    }

    /// How to describe this honestly in the UI.
    pub fn label(self) -> &'static str {
        match self {
            SealerKind::Dpapi => "sealed with your Windows account (DPAPI)",
            SealerKind::PlaintextForTestsOrOtherPlatforms => {
                "NOT encrypted: no sealing backend is active"
            }
        }
    }
}

pub trait Sealer: Send + Sync {
    fn kind(&self) -> SealerKind;
    /// Backend call: turns plaintext into opaque bytes.
    fn seal_raw(&self, plaintext: &[u8]) -> Result<Vec<u8>, SealError>;
    /// Backend call: recovers plaintext, verifying integrity.
    fn unseal_raw(&self, sealed: &[u8]) -> Result<Vec<u8>, SealError>;

    /// Wrap in the self-describing envelope. Callers use this, not `seal_raw`.
    fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, SealError> {
        let body = self.seal_raw(plaintext)?;
        let mut out = Vec::with_capacity(body.len() + 6);
        out.extend_from_slice(MAGIC);
        out.push(VERSION);
        out.push(self.kind() as u8);
        out.extend_from_slice(&body);
        Ok(out)
    }

    /// Unwrap and unseal, refusing envelopes from a different backend.
    fn unseal(&self, envelope: &[u8]) -> Result<Vec<u8>, SealError> {
        if envelope.len() < 6 || &envelope[..4] != MAGIC {
            return Err(SealError::MalformedEnvelope("not a sealed value".into()));
        }
        if envelope[4] != VERSION {
            return Err(SealError::MalformedEnvelope(format!(
                "unsupported sealed-value version {}",
                envelope[4]
            )));
        }
        let found = match envelope[5] {
            0 => SealerKind::Dpapi,
            1 => SealerKind::PlaintextForTestsOrOtherPlatforms,
            other => {
                return Err(SealError::MalformedEnvelope(format!(
                    "unknown sealer id {other}"
                )))
            }
        };
        if found != self.kind() {
            return Err(SealError::WrongSealer {
                found,
                active: self.kind(),
            });
        }
        self.unseal_raw(&envelope[6..])
    }
}

pub fn is_sealed(value: &[u8]) -> bool {
    value.len() >= 6 && &value[..4] == MAGIC
}

/* ------------------------------ Windows DPAPI ------------------------------ */

// The only `unsafe` in the workspace, and it is confined to this module: three
// documented Win32 calls with their buffer lifetimes handled immediately. Every
// other crate keeps the workspace's deny(unsafe_code) in force.
#[cfg(windows)]
#[allow(unsafe_code)]
mod dpapi {
    use super::{SealError, Sealer, SealerKind};

    #[repr(C)]
    struct DataBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }

    const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

    #[link(name = "crypt32")]
    extern "system" {
        fn CryptProtectData(
            data_in: *const DataBlob,
            description: *const u16,
            entropy: *const DataBlob,
            reserved: *mut core::ffi::c_void,
            prompt: *mut core::ffi::c_void,
            flags: u32,
            data_out: *mut DataBlob,
        ) -> i32;

        fn CryptUnprotectData(
            data_in: *const DataBlob,
            description: *mut *mut u16,
            entropy: *const DataBlob,
            reserved: *mut core::ffi::c_void,
            prompt: *mut core::ffi::c_void,
            flags: u32,
            data_out: *mut DataBlob,
        ) -> i32;

        fn LocalFree(mem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    }

    /// Optional entropy: a fixed application string mixed into the key, so
    /// another program running as the same user cannot trivially unseal our
    /// blobs even if it obtains them.
    const ENTROPY: &[u8] = b"waypoint.seal.v1";

    fn blob_for(bytes: &[u8]) -> DataBlob {
        DataBlob {
            cb_data: bytes.len() as u32,
            // The API only reads from this pointer.
            pb_data: bytes.as_ptr() as *mut u8,
        }
    }

    /// Windows DPAPI, scoped to the current user and machine.
    pub struct DpapiSealer;

    impl Sealer for DpapiSealer {
        fn kind(&self) -> SealerKind {
            SealerKind::Dpapi
        }

        fn seal_raw(&self, plaintext: &[u8]) -> Result<Vec<u8>, SealError> {
            let input = blob_for(plaintext);
            let mut entropy_bytes = ENTROPY.to_vec();
            let entropy = blob_for(&entropy_bytes);
            let mut out = DataBlob {
                cb_data: 0,
                pb_data: core::ptr::null_mut(),
            };
            // SAFETY: both input blobs describe live slices for the duration of
            // the call; `out` is written by the API and freed immediately after
            // being copied.
            let ok = unsafe {
                CryptProtectData(
                    &input,
                    core::ptr::null(),
                    &entropy,
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut out,
                )
            };
            entropy_bytes.clear();
            if ok == 0 {
                return Err(SealError::Backend(format!(
                    "CryptProtectData failed (error {})",
                    std::io::Error::last_os_error()
                )));
            }
            // SAFETY: on success the API allocated `out.pb_data` with `cb_data`
            // readable bytes; we copy them out and then free the buffer.
            let bytes =
                unsafe { std::slice::from_raw_parts(out.pb_data, out.cb_data as usize).to_vec() };
            unsafe {
                LocalFree(out.pb_data as *mut core::ffi::c_void);
            }
            Ok(bytes)
        }

        fn unseal_raw(&self, sealed: &[u8]) -> Result<Vec<u8>, SealError> {
            let input = blob_for(sealed);
            let mut entropy_bytes = ENTROPY.to_vec();
            let entropy = blob_for(&entropy_bytes);
            let mut out = DataBlob {
                cb_data: 0,
                pb_data: core::ptr::null_mut(),
            };
            // SAFETY: as above; `description` is an out-parameter we free.
            let ok = unsafe {
                CryptUnprotectData(
                    &input,
                    core::ptr::null_mut(),
                    &entropy,
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut out,
                )
            };
            entropy_bytes.clear();
            if ok == 0 {
                // A failed integrity check and a wrong user look identical to
                // the API; both mean "these bytes are not usable as ours".
                return Err(SealError::Tampered);
            }
            // SAFETY: on success the API allocated `out.pb_data`.
            let bytes =
                unsafe { std::slice::from_raw_parts(out.pb_data, out.cb_data as usize).to_vec() };
            unsafe {
                LocalFree(out.pb_data as *mut core::ffi::c_void);
            }
            Ok(bytes)
        }
    }
}

#[cfg(windows)]
pub use dpapi::DpapiSealer;

/* ------------------------------ pass-through ------------------------------- */

/// No encryption at all. Exists so tests are deterministic and so platforms
/// without an equivalent backend still run — and it is named so that nobody
/// can mistake it for protection. The store records which one is active.
pub struct PlaintextSealer;

impl Sealer for PlaintextSealer {
    fn kind(&self) -> SealerKind {
        SealerKind::PlaintextForTestsOrOtherPlatforms
    }

    fn seal_raw(&self, plaintext: &[u8]) -> Result<Vec<u8>, SealError> {
        Ok(plaintext.to_vec())
    }

    fn unseal_raw(&self, sealed: &[u8]) -> Result<Vec<u8>, SealError> {
        Ok(sealed.to_vec())
    }
}

/// The best sealer this platform can offer, or `None` if there is none.
pub fn platform_sealer() -> Option<std::sync::Arc<dyn Sealer>> {
    #[cfg(windows)]
    {
        Some(std::sync::Arc::new(DpapiSealer))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sealer() -> std::sync::Arc<dyn Sealer> {
        platform_sealer().unwrap_or_else(|| std::sync::Arc::new(PlaintextSealer))
    }

    #[test]
    fn round_trip_through_the_platform_backend() {
        let s = sealer();
        let secret = b"Nine years building distributed systems, Zurich";
        let sealed = s.seal(secret).unwrap();
        assert!(is_sealed(&sealed));
        assert!(matches!(
            s.kind(),
            SealerKind::Dpapi | SealerKind::PlaintextForTestsOrOtherPlatforms
        ));
        let back = s.unseal(&sealed).unwrap();
        assert_eq!(back, secret);
    }

    #[test]
    fn dpapi_output_does_not_contain_the_plaintext() {
        let Some(s) = platform_sealer() else {
            eprintln!("no platform sealer on this OS; skipping");
            return;
        };
        let secret = b"Candidate facts that must not sit in plaintext";
        let sealed = s.seal(secret).unwrap();
        assert!(
            !sealed.windows(secret.len()).any(|w| w == secret.as_slice()),
            "the sealed payload must not embed its plaintext"
        );
        // Nor any recognisable fragment of it.
        assert!(!sealed.windows(12).any(|w| w == &secret[..12]));
    }

    #[test]
    fn sealing_the_same_value_twice_gives_different_ciphertext() {
        let Some(s) = platform_sealer() else {
            return;
        };
        let secret = b"same input";
        let a = s.seal(secret).unwrap();
        let b = s.seal(secret).unwrap();
        assert_ne!(a, b, "a deterministic ciphertext would leak equality");
        assert_eq!(s.unseal(&a).unwrap(), s.unseal(&b).unwrap());
    }

    #[test]
    fn tampered_bytes_are_rejected() {
        let Some(s) = platform_sealer() else {
            return;
        };
        let mut sealed = s.seal(b"a fact worth protecting").unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01;
        match s.unseal(&sealed) {
            Err(SealError::Tampered) | Err(SealError::Backend(_)) => {}
            other => panic!("tampering must be detected, got {other:?}"),
        }
    }

    #[test]
    fn an_envelope_from_another_backend_is_refused_not_garbled() {
        let plain = PlaintextSealer;
        let sealed = plain.seal(b"written by the pass-through").unwrap();
        let s = platform_sealer();
        if let Some(s) = s {
            match s.unseal(&sealed) {
                Err(SealError::WrongSealer { found, active }) => {
                    assert_eq!(found, SealerKind::PlaintextForTestsOrOtherPlatforms);
                    assert_eq!(active, SealerKind::Dpapi);
                }
                other => panic!("a foreign envelope must be refused, got {other:?}"),
            }
        }
    }

    #[test]
    fn junk_and_truncated_values_are_refused() {
        let s = sealer();
        assert!(matches!(
            s.unseal(b"not sealed at all"),
            Err(SealError::MalformedEnvelope(_))
        ));
        assert!(matches!(
            s.unseal(b"WPSL"),
            Err(SealError::MalformedEnvelope(_))
        ));
        let mut wrong_version = b"WPSL".to_vec();
        wrong_version.push(9);
        wrong_version.push(0);
        wrong_version.extend_from_slice(b"payload");
        assert!(matches!(
            s.unseal(&wrong_version),
            Err(SealError::MalformedEnvelope(_))
        ));
    }

    #[test]
    fn empty_and_large_values_round_trip() {
        let s = sealer();
        for size in [0usize, 1, 4096, 256 * 1024] {
            let payload = vec![b'x'; size];
            let sealed = s.seal(&payload).unwrap();
            assert_eq!(s.unseal(&sealed).unwrap(), payload, "size {size}");
        }
    }

    #[test]
    fn the_pass_through_is_honestly_labelled() {
        assert_eq!(
            PlaintextSealer.kind().label(),
            "NOT encrypted: no sealing backend is active"
        );
        assert!(SealerKind::Dpapi.label().contains("Windows account"));
    }
}
