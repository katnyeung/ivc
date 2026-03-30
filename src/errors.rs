#[derive(thiserror::Error, Debug)]
pub enum IvcError {
    #[error("Not an IVC repository. Run 'ivc init' first.")]
    NotInitialised,

    #[error("Not a Git repository.")]
    NotAGitRepo,

    #[error("Git operation failed: {0}")]
    GitError(#[from] git2::Error),

    #[error("Database error: {0}")]
    DbError(String),

    #[error("AI API error: {0}")]
    AiError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),
}
