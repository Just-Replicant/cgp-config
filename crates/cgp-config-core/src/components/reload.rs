//! Reload traits. Specified in 0.1 so 0.2's watcher and reload loop land without
//! breaking 0.1 contexts; 0.1 ships no provider for them.

use cgp_fork::prelude::*;

/// Runs the full pipeline again for a reload (0.2).
///
/// The future is `Send` and borrows `&self`, so it is not `'static`; a reload loop owns an
/// `Arc` of the context and awaits this inside its task.
#[cgp_component(ConfigReloader)]
#[cgp_async(Send)]
pub trait CanReloadConfig<Config>: HasErrorType {
    /// Loads a fresh config.
    ///
    /// # Errors
    ///
    /// The first failing stage's error. The caller keeps the live config.
    async fn reload_config(&self) -> Result<Config, Self::Error>;
}

/// Waits for the next (debounced) reason to reload (0.2).
#[cgp_component(ChangeWatcher)]
#[cgp_async(Send)]
pub trait CanWatchChanges: HasErrorType {
    /// Resolves when a reload should run.
    ///
    /// # Errors
    ///
    /// When the watcher itself fails.
    async fn next_change(&self) -> Result<ChangeEvent, Self::Error>;
}

/// Why a reload was triggered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ChangeEvent {
    /// A watched source changed.
    SourceChanged,
    /// A reload signal arrived (`SIGHUP` on Unix).
    Signal,
    /// The periodic resync tick fired.
    Resync,
}
