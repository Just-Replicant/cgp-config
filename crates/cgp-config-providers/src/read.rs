//! The IO seam: [`CanReadConfigFile`], with [`ReadStdFile`] (the filesystem) and
//! [`ReadFromMap`] (in-memory files for tests and sandboxes).

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use cgp_config_core::{CanReportWarning, ConfigError, HasLoadLimits, LoadWarning, Origin};
use cgp_fork::prelude::*;
use zeroize::Zeroizing;

/// The text of one config file.
///
/// The text is `Zeroizing`: it may hold secrets until it is parsed, and is wiped when
/// dropped.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FileText {
    /// The file's origin (`Origin::File` with the path as given).
    pub origin: Origin,
    /// The UTF-8 text, without a byte-order mark.
    pub text: Zeroizing<String>,
    /// Group or others may read the file (Unix), for the `SecretWorldReadable` check
    /// after parsing.
    pub readable_by_others: bool,
}

impl FileText {
    /// A file's text as read.
    #[must_use]
    pub fn new(path: &Path, text: String) -> Self {
        Self {
            origin: origin_of(path),
            text: Zeroizing::new(text),
            readable_by_others: false,
        }
    }
}

/// The origin of a file path.
#[must_use]
pub fn origin_of(path: &Path) -> Origin {
    Origin::file(path.to_string_lossy().as_ref())
}

/// Reads one config file.
///
/// `Ok(None)` when the file does not exist. Every reader enforces
/// `LoadLimits::max_bytes`, requires UTF-8 (a UTF-16 byte-order mark is
/// `ConfigError::Encoding`), and strips a leading UTF-8 byte-order mark.
#[cgp_component {
    name: ConfigFileReaderComponent,
    provider: ConfigFileReader,
}]
pub trait CanReadConfigFile: HasErrorType {
    /// The file's text, or `None` when it does not exist.
    ///
    /// # Errors
    ///
    /// IO failures other than "not found", `LimitExceeded`, and `Encoding`.
    fn read_config_file(&self, path: &Path) -> Result<Option<FileText>, Self::Error>;

    /// The entries of a directory (file paths, sorted), or `None` when it does not exist.
    ///
    /// # Errors
    ///
    /// IO failures other than "not found".
    fn list_config_dir(&self, dir: &Path) -> Result<Option<Vec<PathBuf>>, Self::Error>;
}

/// Decodes raw bytes: UTF-16 BOMs are `Encoding`, a UTF-8 BOM is stripped, invalid UTF-8
/// is `Encoding`.
///
/// # Errors
///
/// `ConfigError::Encoding` (never quoting the bytes).
pub fn decode(origin: &Origin, bytes: Vec<u8>) -> Result<String, ConfigError> {
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return Err(ConfigError::Encoding {
            origin: origin.clone(),
        });
    }
    let bytes = match bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        Some(rest) => rest.to_vec(),
        None => bytes,
    };
    String::from_utf8(bytes).map_err(|error| {
        // The rejected buffer may hold secrets; wipe it.
        drop(Zeroizing::new(error.into_bytes()));
        ConfigError::Encoding {
            origin: origin.clone(),
        }
    })
}

fn io_error(origin: &Origin, error: &std::io::Error) -> ConfigError {
    ConfigError::io(origin.clone(), error.to_string())
}

/// Reads files with `std::fs`.
///
/// * at most `max_bytes` (`LimitExceeded` past it, checked on the open file, so a growing
///   file cannot exceed it either);
/// * a torn read (the length or modification time changed while reading) is retried once,
///   then is `ConfigError::Io`;
/// * on Unix, a group- or world-writable file records `LoadWarning::WorldWritable`, and
///   [`FileText::readable_by_others`] is set from the mode.
pub struct ReadStdFile;

#[cgp_impl(ReadStdFile: ConfigFileReaderComponent)]
impl ConfigFileReader
where
    Self: CanRaiseError<ConfigError> + HasLoadLimits + CanReportWarning,
{
    fn read_config_file(&self, path: &Path) -> Result<Option<FileText>, Self::Error> {
        let origin = origin_of(path);
        let max_bytes = self.load_limits().max_bytes;
        let mut attempt = 0;
        loop {
            match read_once(path, &origin, max_bytes).map_err(Self::raise_error)? {
                Read::Missing => return Ok(None),
                Read::Torn if attempt == 0 => attempt += 1,
                Read::Torn => {
                    return Err(Self::raise_error(ConfigError::io(
                        origin,
                        "the file changed while it was being read",
                    )));
                }
                Read::Done { text, mode } => {
                    let (writable, readable) = mode_flags(mode);
                    if writable {
                        self.report_warning(LoadWarning::WorldWritable {
                            origin: origin.clone(),
                        });
                    }
                    let text = decode(&origin, text).map_err(Self::raise_error)?;
                    let mut file = FileText::new(path, text);
                    file.readable_by_others = readable;
                    return Ok(Some(file));
                }
            }
        }
    }

    fn list_config_dir(&self, dir: &Path) -> Result<Option<Vec<PathBuf>>, Self::Error> {
        let origin = origin_of(dir);
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(Self::raise_error(io_error(&origin, &error))),
        };
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| Self::raise_error(io_error(&origin, &error)))?;
            let is_file = entry
                .file_type()
                .map_err(|error| Self::raise_error(io_error(&origin, &error)))?
                .is_file()
                || std::fs::metadata(entry.path()).is_ok_and(|meta| meta.is_file());
            if is_file {
                files.push(entry.path());
            }
        }
        files.sort();
        Ok(Some(files))
    }
}

enum Read {
    Missing,
    Torn,
    Done { text: Vec<u8>, mode: u32 },
}

fn read_once(path: &Path, origin: &Origin, max_bytes: usize) -> Result<Read, ConfigError> {
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Read::Missing),
        Err(error) => return Err(io_error(origin, &error)),
    };
    let before = file.metadata().map_err(|error| io_error(origin, &error))?;
    if before.is_dir() {
        return Err(ConfigError::io(
            origin.clone(),
            "is a directory, not a file",
        ));
    }
    let limit = u64::try_from(max_bytes).unwrap_or(u64::MAX);
    if before.len() > limit {
        return Err(ConfigError::limit_exceeded("max_bytes", max_bytes));
    }
    let mut text = Vec::with_capacity(usize::try_from(before.len()).unwrap_or(0).min(max_bytes));
    (&mut file)
        .take(limit.saturating_add(1))
        .read_to_end(&mut text)
        .map_err(|error| io_error(origin, &error))?;
    if text.len() > max_bytes {
        drop(Zeroizing::new(text));
        return Err(ConfigError::limit_exceeded("max_bytes", max_bytes));
    }
    let after = std::fs::metadata(path).map_err(|error| io_error(origin, &error))?;
    let changed = after.len() != before.len()
        || u64::try_from(text.len()).ok() != Some(after.len())
        || after.modified().ok() != before.modified().ok();
    if changed {
        drop(Zeroizing::new(text));
        return Ok(Read::Torn);
    }
    Ok(Read::Done {
        text,
        mode: file_mode(&before),
    })
}

#[cfg(unix)]
fn file_mode(meta: &std::fs::Metadata) -> u32 {
    std::os::unix::fs::PermissionsExt::mode(&meta.permissions())
}

#[cfg(not(unix))]
fn file_mode(_meta: &std::fs::Metadata) -> u32 {
    0
}

/// `(group/world writable, group/world readable)` from a Unix mode.
#[must_use]
pub fn mode_flags(mode: u32) -> (bool, bool) {
    (mode & 0o022 != 0, mode & 0o044 != 0)
}

/// In-memory files, by path: the input of [`ReadFromMap`].
#[derive(Debug, Clone, Default)]
pub struct VirtualFiles {
    files: BTreeMap<PathBuf, String>,
}

impl VirtualFiles {
    /// No files.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a file.
    #[must_use]
    pub fn with(mut self, path: impl Into<PathBuf>, text: impl Into<String>) -> Self {
        self.insert(path, text);
        self
    }

    /// Adds or replaces a file.
    pub fn insert(&mut self, path: impl Into<PathBuf>, text: impl Into<String>) {
        self.files.insert(path.into(), text.into());
    }

    /// Whether a file exists.
    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        self.files.contains_key(path)
    }
}

/// Serves files from a `virtual_files: VirtualFiles` field and never touches the disk.
/// Applies the same limits and encoding rules as [`ReadStdFile`].
pub struct ReadFromMap;

#[cgp_impl(ReadFromMap: ConfigFileReaderComponent)]
impl ConfigFileReader
where
    Self: CanRaiseError<ConfigError>
        + HasLoadLimits
        + HasField<Symbol!("virtual_files"), Value = VirtualFiles>,
{
    fn read_config_file(&self, path: &Path) -> Result<Option<FileText>, Self::Error> {
        let files = self.get_field(PhantomData::<Symbol!("virtual_files")>);
        let Some(text) = files.files.get(path) else {
            return Ok(None);
        };
        let max_bytes = self.load_limits().max_bytes;
        if text.len() > max_bytes {
            return Err(Self::raise_error(ConfigError::limit_exceeded(
                "max_bytes",
                max_bytes,
            )));
        }
        let origin = origin_of(path);
        let text = decode(&origin, text.as_bytes().to_vec()).map_err(Self::raise_error)?;
        Ok(Some(FileText::new(path, text)))
    }

    fn list_config_dir(&self, dir: &Path) -> Result<Option<Vec<PathBuf>>, Self::Error> {
        let files = self.get_field(PhantomData::<Symbol!("virtual_files")>);
        let listed: Vec<PathBuf> = files
            .files
            .keys()
            .filter(|path| path.parent() == Some(dir))
            .cloned()
            .collect();
        Ok((!listed.is_empty()).then_some(listed))
    }
}

#[cfg(test)]
mod tests {
    use cgp_config_core::Origin;

    use super::{decode, mode_flags};

    #[test]
    fn decoding_rules() {
        let origin = Origin::file("a.toml");
        assert_eq!(
            decode(&origin, b"\xEF\xBB\xBFa = 1".to_vec()).unwrap(),
            "a = 1"
        );
        assert_eq!(
            decode(&origin, b"\xFF\xFEa".to_vec()).unwrap_err().code(),
            "config::encoding"
        );
        assert_eq!(
            decode(&origin, b"\xFE\xFFa".to_vec()).unwrap_err().code(),
            "config::encoding"
        );
        let error = decode(&origin, b"a = \"\xC3\x28\"".to_vec()).unwrap_err();
        assert_eq!(error.code(), "config::encoding");
    }

    #[test]
    fn modes() {
        assert_eq!(mode_flags(0o600), (false, false));
        assert_eq!(mode_flags(0o644), (false, true));
        assert_eq!(mode_flags(0o664), (true, true));
        assert_eq!(mode_flags(0o602), (true, false));
    }
}
