//! [`SecretRoots`]: the directories a `file:` secret may live in.

use std::path::{Path, PathBuf};

use cgp_config_core::EnvSnapshot;

/// Canonical directories a file secret must sit inside.
///
/// [`discover`](Self::discover) uses, in order: the config file's directory, then
/// `CREDENTIALS_DIRECTORY` from the env snapshot, then `/run/secrets` when that directory
/// exists. The first root that contains a relative `file:` name wins.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecretRoots {
    roots: Vec<PathBuf>,
}

impl SecretRoots {
    /// Roots from the config directory and the env snapshot.
    ///
    /// An empty `config_dir` is the working directory. A path that is not a directory
    /// is skipped.
    #[must_use]
    pub fn discover(config_dir: Option<&Path>, env: &EnvSnapshot) -> Self {
        let mut roots = Self::default();
        if let Some(dir) = config_dir {
            let dir = if dir.as_os_str().is_empty() {
                Path::new(".")
            } else {
                dir
            };
            roots.push_dir(dir);
        }
        if let Some(dir) = env.get("CREDENTIALS_DIRECTORY") {
            roots.push_dir(Path::new(dir));
        }
        roots.push_dir(Path::new("/run/secrets"));
        roots
    }

    /// The canonical directories, in discovery order.
    pub fn iter(&self) -> impl Iterator<Item = &Path> {
        self.roots.iter().map(PathBuf::as_path)
    }

    /// Whether `canonical` is `root` or a file inside one.
    #[must_use]
    pub fn contains(&self, canonical: &Path) -> bool {
        self.roots.iter().any(|root| canonical.starts_with(root))
    }

    fn push_dir(&mut self, path: &Path) {
        if !path.is_dir() {
            return;
        }
        let Ok(canonical) = path.canonicalize() else {
            return;
        };
        if !self.roots.iter().any(|root| root == &canonical) {
            self.roots.push(canonical);
        }
    }
}
