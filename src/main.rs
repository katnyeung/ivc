use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod ai;
mod cli;
mod config;
mod db;
mod errors;
mod git;
mod github;
mod ivc_json;
mod models;

#[derive(Parser)]
#[command(
    name = "ivc",
    about = "Intention Version Control — version control for the why, not just the what",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialise IVC in an existing Git repository
    Init,

    /// Stage files (wraps git add)
    Add {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Show working tree status (wraps git status)
    Status {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Show changes (wraps git diff)
    Diff {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Commit changes (wraps git commit, captures intention metadata)
    Commit {
        /// Arguments passed through to git commit
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Push changes (wraps git push, syncs metadata)
    Push {
        /// Arguments passed through to git push
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Pull changes (wraps git pull)
    Pull {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Switch branches or restore files (wraps git checkout)
    Checkout {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// List, create, or delete branches (wraps git branch)
    Branch {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Merge branches (wraps git merge)
    Merge {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Rebase commits (wraps git rebase)
    Rebase {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Reset current HEAD (wraps git reset)
    Reset {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Stash changes (wraps git stash)
    Stash {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Show commit logs (wraps git log)
    #[command(name = "git-log")]
    GitLog {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Fetch from remote (wraps git fetch)
    Fetch {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Manage remotes (wraps git remote)
    Remote {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Manage tags (wraps git tag)
    Tag {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Cherry-pick commits (wraps git cherry-pick)
    CherryPick {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Restore files (wraps git restore)
    Restore {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Switch branches (wraps git switch)
    Switch {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Show various objects (wraps git show)
    Show {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Clean untracked files (wraps git clean)
    Clean {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Extract intention tree from branch commits via LLM
    Pr {
        /// Base branch to compare against (reads from .ivc/config.toml if not provided)
        #[arg(long)]
        base: Option<String>,

        /// Create PR as draft
        #[arg(long)]
        draft: bool,

        /// Skip pushing the branch to remote
        #[arg(long)]
        no_push: bool,

        /// Skip GitHub PR creation (just extract and display)
        #[arg(long)]
        no_pr: bool,
    },

    /// Backfill intention trees for historical PRs
    Backfill {
        /// Backfill a specific PR by number
        #[arg(long)]
        pr: Option<u32>,

        /// Backfill all PRs that touched this file
        #[arg(long)]
        file: Option<String>,

        /// Backfill PRs merged since this date (ISO format: 2025-01-01)
        #[arg(long)]
        since: Option<String>,

        /// Backfill PRs merged until this date (ISO format: 2025-12-31)
        #[arg(long)]
        until: Option<String>,

        /// Maximum number of PRs to process (default: 10)
        #[arg(long, default_value = "10")]
        limit: usize,

        /// Show what would be processed without calling LLM
        #[arg(long)]
        dry_run: bool,

        /// Skip PRs that already have intentions (default: true)
        #[arg(long, default_value = "true")]
        skip_existing: bool,
    },

    /// Display the intention tree for the current branch, or for a specific file
    Log {
        /// Optional file path to show all intentions that touched this file
        #[arg()]
        file: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init => cli::init::run().await,
        Commands::Commit { args } => cli::commit::run(args).await,
        Commands::Push { args } => cli::push::run(args).await,
        Commands::Pr {
            base,
            draft,
            no_push,
            no_pr,
        } => {
            cli::pr::run(cli::pr::PrArgs {
                base: base.unwrap_or_default(),
                draft,
                no_push,
                no_pr,
            })
            .await
        }
        Commands::Backfill {
            pr,
            file,
            since,
            until,
            limit,
            dry_run,
            skip_existing,
        } => {
            cli::backfill::run(cli::backfill::BackfillArgs {
                pr,
                file,
                since,
                until,
                limit,
                dry_run,
                skip_existing,
            })
            .await
        }
        Commands::Log { file } => cli::log::run(file).await,

        // Pure git passthroughs — no IVC metadata capture
        Commands::Add { args } => Ok(git::commit::run_git_command("add", &args)?),
        Commands::Status { args } => Ok(git::commit::run_git_command("status", &args)?),
        Commands::Diff { args } => Ok(git::commit::run_git_command("diff", &args)?),
        Commands::Pull { args } => Ok(git::commit::run_git_command("pull", &args)?),
        Commands::Checkout { args } => Ok(git::commit::run_git_command("checkout", &args)?),
        Commands::Branch { args } => Ok(git::commit::run_git_command("branch", &args)?),
        Commands::Merge { args } => Ok(git::commit::run_git_command("merge", &args)?),
        Commands::Rebase { args } => Ok(git::commit::run_git_command("rebase", &args)?),
        Commands::Reset { args } => Ok(git::commit::run_git_command("reset", &args)?),
        Commands::Stash { args } => Ok(git::commit::run_git_command("stash", &args)?),
        Commands::GitLog { args } => Ok(git::commit::run_git_command("log", &args)?),
        Commands::Fetch { args } => Ok(git::commit::run_git_command("fetch", &args)?),
        Commands::Remote { args } => Ok(git::commit::run_git_command("remote", &args)?),
        Commands::Tag { args } => Ok(git::commit::run_git_command("tag", &args)?),
        Commands::CherryPick { args } => Ok(git::commit::run_git_command("cherry-pick", &args)?),
        Commands::Restore { args } => Ok(git::commit::run_git_command("restore", &args)?),
        Commands::Switch { args } => Ok(git::commit::run_git_command("switch", &args)?),
        Commands::Show { args } => Ok(git::commit::run_git_command("show", &args)?),
        Commands::Clean { args } => Ok(git::commit::run_git_command("clean", &args)?),
    }
}
