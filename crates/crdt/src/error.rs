/// Why a doc could not be loaded or a batch imported. p2pcore maps it into its own `Error`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// Loro refused the bytes (corrupt snapshot or update); carries Loro's message.
    #[error("{0}")]
    Io(String),
    /// An update whose operations depend on ones this doc does not have; nothing was applied.
    #[error("update with missing dependencies")]
    MissingDeps,
}
