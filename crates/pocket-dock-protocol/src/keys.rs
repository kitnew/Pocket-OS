//! Static X25519 identities and their files.
//!
//! An identity file holds one private key and its public key as hex:
//!
//! ```text
//! # Pocket Dock identity. Private: keep it mode 0600, never commit it.
//! private=<64 hex>
//! public=<64 hex>
//! ```
//!
//! Files are created with mode 0600 (directories 0700) and refused when group- or
//! world-accessible, like OpenSSH does with private keys. Public keys are exchanged as the
//! 64-character hex string; [`PublicKey::short_id`] is a shorter display form.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::str::FromStr;

use crate::NOISE_PARAMS;

/// A 32-byte X25519 public key.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublicKey(pub [u8; 32]);

impl PublicKey {
    pub fn from_slice(b: &[u8]) -> Option<Self> {
        b.try_into().ok().map(PublicKey)
    }

    /// First 12 hex digits, for display and logs (not for trust decisions).
    pub fn short_id(&self) -> String {
        hex(&self.0[..6])
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex(&self.0))
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PublicKey({})", self.short_id())
    }
}

impl FromStr for PublicKey {
    type Err = KeyError;
    fn from_str(s: &str) -> Result<Self, KeyError> {
        let bytes = unhex(s.trim()).ok_or(KeyError::Malformed("public key"))?;
        PublicKey::from_slice(&bytes).ok_or(KeyError::Malformed("public key"))
    }
}

/// A static keypair (the private half never leaves this process or its file).
#[derive(Clone)]
pub struct Identity {
    private: [u8; 32],
    public: PublicKey,
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Identity({:?})", self.public)
    }
}

#[derive(Debug)]
pub enum KeyError {
    Io(io::Error),
    /// The file is readable or writable by group/others.
    Permissions(u32),
    Malformed(&'static str),
    Generate(String),
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyError::Io(e) => write!(f, "{e}"),
            KeyError::Permissions(mode) => write!(
                f,
                "permissions {mode:o} are too open (private key files must be 0600)"
            ),
            KeyError::Malformed(what) => write!(f, "malformed {what}"),
            KeyError::Generate(e) => write!(f, "cannot generate a key: {e}"),
        }
    }
}

impl std::error::Error for KeyError {}

impl From<io::Error> for KeyError {
    fn from(e: io::Error) -> Self {
        KeyError::Io(e)
    }
}

impl Identity {
    pub fn generate() -> Result<Self, KeyError> {
        let params = NOISE_PARAMS
            .parse()
            .map_err(|e| KeyError::Generate(format!("{e:?}")))?;
        let kp = snow::Builder::new(params)
            .generate_keypair()
            .map_err(|e| KeyError::Generate(e.to_string()))?;
        Ok(Identity {
            private: kp
                .private
                .as_slice()
                .try_into()
                .map_err(|_| KeyError::Malformed("generated key"))?,
            public: PublicKey::from_slice(&kp.public)
                .ok_or(KeyError::Malformed("generated key"))?,
        })
    }

    pub fn public(&self) -> PublicKey {
        self.public
    }

    pub(crate) fn private_bytes(&self) -> &[u8; 32] {
        &self.private
    }

    /// Reads an identity file, refusing group/world-accessible files.
    pub fn load(path: &Path) -> Result<Self, KeyError> {
        check_private_file(path)?;
        let text = fs::read_to_string(path)?;
        let mut private = None;
        let mut public = None;
        for line in text.lines().map(str::trim) {
            if let Some(v) = line.strip_prefix("private=") {
                private = unhex(v);
            } else if let Some(v) = line.strip_prefix("public=") {
                public = Some(v.parse::<PublicKey>()?);
            }
        }
        let private: [u8; 32] = private
            .and_then(|p| p.try_into().ok())
            .ok_or(KeyError::Malformed("private key"))?;
        Ok(Identity {
            private,
            public: public.ok_or(KeyError::Malformed("public key"))?,
        })
    }

    /// Writes the identity with mode 0600 (atomically; never replaces an existing file).
    pub fn save_new(&self, path: &Path) -> Result<(), KeyError> {
        let text = format!(
            "# Pocket Dock identity. Private: keep it mode 0600, never commit it.\nprivate={}\npublic={}\n",
            hex(&self.private),
            self.public
        );
        write_private_file(path, text.as_bytes(), false)
    }
}

/// Refuses a file that group or others can access (mode & 0o077).
pub fn check_private_file(path: &Path) -> Result<(), KeyError> {
    let mode = fs::metadata(path)?.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(KeyError::Permissions(mode));
    }
    Ok(())
}

/// Writes `data` to `path` with mode 0600 through a temporary file and a rename, creating
/// the parent directory with mode 0700. With `replace = false` an existing file is an
/// error (`AlreadyExists`).
pub fn write_private_file(path: &Path, data: &[u8], replace: bool) -> Result<(), KeyError> {
    if let Some(dir) = path.parent() {
        if !dir.exists() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
        }
    }
    if !replace && path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", path.display()),
        )
        .into());
    }
    let tmp = path.with_extension("tmp");
    let _ = fs::remove_file(&tmp);
    {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 || !s.is_ascii() {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("pocket-keys-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn identity_round_trip_with_private_permissions() {
        let dir = tmpdir("rt");
        let path = dir.join("sub/identity.key");
        let id = Identity::generate().unwrap();
        id.save_new(&path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let back = Identity::load(&path).unwrap();
        assert_eq!(back.public(), id.public());
        assert_eq!(back.private_bytes(), id.private_bytes());
        // Never overwritten.
        assert!(Identity::generate().unwrap().save_new(&path).is_err());
        // Refused once readable by others.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            Identity::load(&path),
            Err(KeyError::Permissions(0o644))
        ));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn public_key_parsing() {
        let id = Identity::generate().unwrap();
        let s = id.public().to_string();
        assert_eq!(s.len(), 64);
        assert_eq!(s.parse::<PublicKey>().unwrap(), id.public());
        assert_eq!(id.public().short_id(), s[..12]);
        for bad in ["", "zz", &s[..62], &format!("{s}00")] {
            assert!(bad.parse::<PublicKey>().is_err());
        }
    }
}
