//! `env:`, `file:`, and `docker:` resolution. Providers and the session reader share it.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use cgp_config_core::{
    ConfigError, EnvSnapshot, HasEnvSnapshot, HasLoadLimits, Origin, SecretReader, SecretRef,
    SecretText,
};
use cgp_fork::prelude::*;
use zeroize::{Zeroize, Zeroizing};

use crate::component::{
    DockerScheme, EnvScheme, FileScheme, HasSecretRoots, SecretResolver, SecretResolverComponent,
};
use crate::{Secret, SecretRoots};

/// `env:`, `file:`, and `docker:` as one delegate table.
pub struct StdSecretSchemes;

delegate_components! {
    #[unchecked]
    StdSecretSchemes {
        EnvScheme: EnvSecret,
        FileScheme: FileSecret,
        DockerScheme: DockerSecret,
    }
}

/// Reads `env:{name}` from an [`EnvSnapshot`]. An empty value is a secret; a missing name
/// is [`ConfigError::SecretDenied`].
pub struct EnvSecret;

/// Reads `file:{path}` when the canonical path sits under [`HasSecretRoots`].
pub struct FileSecret;

/// Reads `docker:{name}` from `/run/secrets/{name}` after the same root check. `name` is
/// one safe path segment.
pub struct DockerSecret;

/// The reader a resolving session holds: an env snapshot, the roots, and `max_bytes`.
pub struct StdSecretReader {
    env: EnvSnapshot,
    roots: SecretRoots,
    max_bytes: usize,
}

impl StdSecretReader {
    /// A reader. `max_bytes` is [`LoadLimits::max_bytes`](cgp_config_core::LoadLimits::max_bytes).
    #[must_use]
    pub fn new(env: EnvSnapshot, roots: SecretRoots, max_bytes: usize) -> Self {
        Self {
            env,
            roots,
            max_bytes,
        }
    }
}

impl SecretReader for StdSecretReader {
    fn read_secret(&self, reference: &SecretRef) -> Result<SecretText, ConfigError> {
        match reference.scheme() {
            "env" => resolve_env(&self.env, reference),
            "file" => resolve_file(&self.roots, reference, self.max_bytes),
            "docker" => resolve_docker(&self.roots, reference, self.max_bytes),
            _ => Err(denied(reference)),
        }
    }
}

#[cgp_impl(EnvSecret: SecretResolverComponent)]
impl SecretResolver<EnvScheme>
where
    Self: HasEnvSnapshot + CanRaiseError<ConfigError>,
{
    fn resolve_secret(&self, reference: &SecretRef) -> Result<Secret<String>, Self::Error> {
        resolve_env(self.env_snapshot(), reference)
            .map(Secret::from_text)
            .map_err(Self::raise_error)
    }
}

#[cgp_impl(FileSecret: SecretResolverComponent)]
impl SecretResolver<FileScheme>
where
    Self: HasSecretRoots + HasLoadLimits + CanRaiseError<ConfigError>,
{
    fn resolve_secret(&self, reference: &SecretRef) -> Result<Secret<String>, Self::Error> {
        resolve_file(
            &self.secret_roots(),
            reference,
            self.load_limits().max_bytes,
        )
        .map(Secret::from_text)
        .map_err(Self::raise_error)
    }
}

#[cgp_impl(DockerSecret: SecretResolverComponent)]
impl SecretResolver<DockerScheme>
where
    Self: HasSecretRoots + HasLoadLimits + CanRaiseError<ConfigError>,
{
    fn resolve_secret(&self, reference: &SecretRef) -> Result<Secret<String>, Self::Error> {
        resolve_docker(
            &self.secret_roots(),
            reference,
            self.load_limits().max_bytes,
        )
        .map(Secret::from_text)
        .map_err(Self::raise_error)
    }
}

/// Resolves an `env:` reference from `env`.
///
/// # Errors
///
/// [`ConfigError::SecretDenied`] when the scheme is not `env` or the name is absent.
pub fn resolve_env(env: &EnvSnapshot, reference: &SecretRef) -> Result<SecretText, ConfigError> {
    if reference.scheme() != "env" {
        return Err(denied(reference));
    }
    match env.get(reference.rest()) {
        Some(value) => Ok(SecretText::new(value)),
        None => Err(denied(reference)),
    }
}

/// Resolves a `file:` reference. Relative names are joined to each root; the first that
/// canonicalizes to a file inside that root wins. Absolute paths must still sit under a
/// root. The bytes are not trimmed.
///
/// # Errors
///
/// [`ConfigError::SecretDenied`] for a path outside the roots or a missing file,
/// [`ConfigError::Encoding`] when the file is not UTF-8, [`ConfigError::LimitExceeded`]
/// past `max_bytes`, and [`ConfigError::Io`] for other read failures. None of those carry
/// the file's bytes.
pub fn resolve_file(
    roots: &SecretRoots,
    reference: &SecretRef,
    max_bytes: usize,
) -> Result<SecretText, ConfigError> {
    if reference.scheme() != "file" {
        return Err(denied(reference));
    }
    let path = Path::new(reference.rest());
    if path.is_absolute() {
        return read_within(roots, path, reference, max_bytes);
    }
    for root in roots.iter() {
        match locate(roots, &root.join(path))? {
            Some(canonical) => return read_file(&canonical, reference, max_bytes),
            None => continue,
        }
    }
    Err(denied(reference))
}

/// Resolves `docker:{name}` as `/run/secrets/{name}`.
///
/// # Errors
///
/// [`ConfigError::SecretDenied`] when `name` is not one safe segment, or the path is
/// outside the roots or missing. Other failures match [`resolve_file`].
pub fn resolve_docker(
    roots: &SecretRoots,
    reference: &SecretRef,
    max_bytes: usize,
) -> Result<SecretText, ConfigError> {
    if reference.scheme() != "docker" || !reference.is_single_name() {
        return Err(denied(reference));
    }
    let path = Path::new("/run/secrets").join(reference.rest());
    read_within(roots, &path, reference, max_bytes)
}

/// Canonicalizes `candidate`, requires it to sit under `roots`, and reads it.
///
/// # Errors
///
/// As [`resolve_file`].
pub fn read_within(
    roots: &SecretRoots,
    candidate: &Path,
    reference: &SecretRef,
    max_bytes: usize,
) -> Result<SecretText, ConfigError> {
    match locate(roots, candidate)? {
        Some(canonical) => read_file(&canonical, reference, max_bytes),
        None => Err(denied(reference)),
    }
}

/// `Ok(None)` when the path is missing or outside the roots. `Err` is a read failure of
/// the check itself that is still safe to show.
fn locate(roots: &SecretRoots, candidate: &Path) -> Result<Option<PathBuf>, ConfigError> {
    match candidate.canonicalize() {
        Ok(canonical) if roots.contains(&canonical) => Ok(Some(canonical)),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Ok(None),
    }
}

fn read_file(
    path: &Path,
    reference: &SecretRef,
    max_bytes: usize,
) -> Result<SecretText, ConfigError> {
    let origin = Origin::custom(reference.redacted().to_string());
    let file = File::open(path).map_err(|error| io_error(&origin, &error, reference))?;
    let mut bytes = Zeroizing::new(Vec::new());
    let cap = u64::try_from(max_bytes).unwrap_or(u64::MAX);
    let mut limited = file.take(cap.saturating_add(1));
    if let Err(error) = limited.read_to_end(&mut bytes) {
        return Err(io_error(&origin, &error, reference));
    }
    if bytes.len() > max_bytes {
        return Err(ConfigError::limit_exceeded("max_bytes", max_bytes));
    }
    let raw = core::mem::take(&mut *bytes);
    drop(bytes);
    match String::from_utf8(raw) {
        Ok(mut text) => {
            let secret = SecretText::new(text.as_str());
            text.zeroize();
            Ok(secret)
        }
        Err(error) => {
            drop(Zeroizing::new(error.into_bytes()));
            Err(ConfigError::Encoding { origin })
        }
    }
}

fn io_error(origin: &Origin, error: &std::io::Error, reference: &SecretRef) -> ConfigError {
    if error.kind() == std::io::ErrorKind::NotFound {
        return denied(reference);
    }
    let message = match error.kind() {
        std::io::ErrorKind::PermissionDenied => "permission denied",
        _ => "read failed",
    };
    ConfigError::io(origin.clone(), message)
}

fn denied(reference: &SecretRef) -> ConfigError {
    ConfigError::SecretDenied {
        reference: reference.redacted(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use cgp_config_core::{EnvSnapshot, SecretRef};

    use super::{read_within, resolve_docker, resolve_env, resolve_file};
    use crate::SecretRoots;

    const SENTINEL: &str = "s3cr3t-SENTINEL";

    fn reference(text: &str) -> SecretRef {
        SecretRef::parse(text).unwrap()
    }

    #[test]
    fn env_reads_the_snapshot_and_keeps_an_empty_value() {
        let env = EnvSnapshot::from_iter([("TOKEN", SENTINEL), ("EMPTY", "")]);
        let text = resolve_env(&env, &reference("env:TOKEN")).unwrap();
        assert_eq!(text.expose(), SENTINEL);
        assert_eq!(alloc::format!("{text:?}"), "[REDACTED]");
        assert_eq!(
            resolve_env(&env, &reference("env:EMPTY")).unwrap().expose(),
            ""
        );
        let missing = resolve_env(&env, &reference("env:NOPE")).unwrap_err();
        assert_eq!(missing.code(), "config::secret_denied");
        assert!(!missing.to_string().contains(SENTINEL));
    }

    #[test]
    fn file_stays_inside_the_roots_and_does_not_trim() {
        let root = tempfile::tempdir().unwrap();
        let mut file = std::fs::File::create(root.path().join("db")).unwrap();
        writeln!(file, "{SENTINEL}").unwrap();
        let roots = SecretRoots::discover(Some(root.path()), &EnvSnapshot::new());
        let text = resolve_file(&roots, &reference("file:db"), 1024).unwrap();
        assert_eq!(text.expose(), "s3cr3t-SENTINEL\n");
        let absolute = root.path().join("db");
        let text = read_within(
            &roots,
            &absolute,
            &reference(&format!("file:{}", absolute.display())),
            1024,
        )
        .unwrap();
        assert_eq!(text.expose(), "s3cr3t-SENTINEL\n");
        let outside = reference("file:/etc/passwd");
        assert_eq!(
            resolve_file(&roots, &outside, 1024).unwrap_err().code(),
            "config::secret_denied"
        );
        let climbed = reference("file:../secret");
        let error = resolve_file(&roots, &climbed, 1024).unwrap_err();
        assert_eq!(error.code(), "config::secret_denied");
        assert!(!error.to_string().contains(".."));
        let capped = resolve_file(&roots, &reference("file:db"), 4).unwrap_err();
        assert_eq!(capped.code(), "config::limit_exceeded");
    }

    #[test]
    fn docker_rejects_a_path_and_a_root_escape() {
        let roots = SecretRoots::discover(None, &EnvSnapshot::new());
        for name in ["docker:a/b", "docker:..", "docker:."] {
            let error = resolve_docker(&roots, &reference(name), 1024).unwrap_err();
            assert_eq!(error.code(), "config::secret_denied", "{name}");
            assert!(!error.to_string().contains('/'), "{error}");
        }
    }

    #[test]
    fn credentials_directory_is_a_root_and_the_first_root_wins() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        std::fs::write(first.path().join("db"), "one").unwrap();
        std::fs::write(second.path().join("db"), "two").unwrap();
        let env =
            EnvSnapshot::from_iter([("CREDENTIALS_DIRECTORY", second.path().to_str().unwrap())]);
        let roots = SecretRoots::discover(Some(first.path()), &env);
        assert_eq!(
            resolve_file(&roots, &reference("file:db"), 1024)
                .unwrap()
                .expose(),
            "one"
        );
        let only_credentials = SecretRoots::discover(None, &env);
        assert_eq!(
            resolve_file(&only_credentials, &reference("file:db"), 1024)
                .unwrap()
                .expose(),
            "two"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_that_leaves_the_root_is_denied() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(outside.path(), SENTINEL).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();
        let roots = SecretRoots::discover(Some(root.path()), &EnvSnapshot::new());
        let error = resolve_file(&roots, &reference("file:link"), 1024).unwrap_err();
        assert_eq!(error.code(), "config::secret_denied");
        assert!(!error.to_string().contains(SENTINEL));
    }
}
