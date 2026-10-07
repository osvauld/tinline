use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error(transparent)]
    Database(#[from] redb::DatabaseError),
    #[error(transparent)]
    Transaction(#[from] redb::TransactionError),
    #[error(transparent)]
    Table(#[from] redb::TableError),
    #[error(transparent)]
    Storage(#[from] redb::StorageError),
    #[error(transparent)]
    Commit(#[from] redb::CommitError),
    /// A sealed record did not open: wrong key, tampered bytes, or a record moved to another path.
    #[error("sealed record at {0} does not open")]
    Unseal(String),
}
