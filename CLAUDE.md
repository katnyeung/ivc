# IVC — Intention Version Control

> Version control for the why, not just the what.

## What Is IVC

IVC is a Rust CLI tool that **wraps Git**. It captures the **intentions** behind code changes (the reasoning, alternatives considered, uncertainties, assumptions) and stores them in a SurrealDB graph database alongside the Git commit history. Git tracks what changed. IVC tracks why it changed.

IVC is purely the **intention capture and storage layer**. It is NOT a code review tool. It is NOT a check tool. It does not produce pass/fail. It does not block PRs. It captures intentions and builds the graph. That is it.

A separate review tool may consume the intention graph in the future, but that is out of scope for IVC itself.

## How IVC Works

IVC wraps `git commit` and `git push`. The developer uses `ivc commit` and `ivc push` instead of the raw git commands. Both perform the real git operation underneath, plus capture metadata.

### The Flow

```
ivc commit -m "GRAPHEE-42: add ContentAutomationService with cron trigger"
    │
    ├── 1. Runs real git commit (identical to git commit)
    ├── 2. Captures lightweight metadata locally:
    │      commit SHA, message, files changed, diff stats
    │      Stored in embedded SurrealDB. No LLM call yet (fast).
    └── 3. If ticket reference found (e.g. GRAPHEE-42),
           stores the reference for later enrichment.

ivc push
    │
    ├── 1. Runs real git push (identical to git push)
    └── 2. Syncs local intention metadata to SurrealDB.
           Still no LLM call. Just metadata capture.

ivc pr
    │
    ├── 1. Collects all commit metadata since branch diverged from base
    ├── 2. Fetches ticket details if available (Jira, optional)
    ├── 3. ONE LLM call: sends all commits + diffs + ticket context
    │      Receives structured intention tree
    ├── 4. Stores intention tree in SurrealDB with graph relations
    ├── 5. Creates GitHub PR with structured description from intention tree
    └── 6. Attaches .ivc.json to the PR
```

### Key Design Decisions

- **LLM call happens at PR time, not at commit time.** Commits are local, unstable (may be amended, squashed, rebased). The PR is when the work is ready for others. One LLM call per PR, not per commit. This is cost-efficient and avoids wasted processing.
- **Jira is optional.** IVC works without any ticket system. If a ticket reference is found in the commit message or branch name, IVC fetches it and enriches the intention. If not, IVC works from commits and diffs alone. Lower richness, same structure.
- **Force push replaces stale captures.** If the developer rebases or squashes and force-pushes, IVC replaces the old commit metadata. No orphaned nodes.

## Architecture

IVC is a single Rust binary with everything embedded:

- **git2** (libgit2 bindings) for all Git operations
- **SurrealDB** (embedded, in-process) for intention graph and event storage
- **reqwest** + Claude API for intention extraction from diffs (only at PR time)
- **octocrab** for GitHub API (PR creation)
- **clap** for CLI parsing
- **tokio** for async runtime
- **serde** for JSON serialisation

IVC never replaces Git. Every `ivc` command performs a real `git` operation underneath. If IVC is removed, the Git repository remains perfectly intact. A developer can always fall back to raw `git` commands with zero consequence.

## Current Phase: Phase 1 — Core Intention Capture

### Commands to Implement

1. **`ivc init`** — Initialise IVC in an existing Git repo. Creates `.ivc/` directory with config and embedded SurrealDB data.

2. **`ivc commit`** — Wraps `git commit`. All git commit flags are passed through. After the real commit succeeds, IVC captures lightweight metadata (commit SHA, message, files changed, diff stats) in embedded SurrealDB. No LLM call. Must be as fast as a normal git commit.

3. **`ivc push`** — Wraps `git push`. All git push flags are passed through. After the real push succeeds, syncs local metadata. No LLM call.

4. **`ivc pr`** — The main command. Collects all commit metadata on the current branch since divergence from the base branch. Makes one LLM call to extract structured intentions. Stores the intention tree in SurrealDB. Outputs the intention tree to console. (GitHub PR creation is Phase 2, for now just build and display the tree.)

5. **`ivc log`** — Display the intention tree for the current branch from SurrealDB. Shows the chain of intentions with their reasoning, uncertainties, assumptions, and file mappings.

### What We Are NOT Building in Phase 1

- No code review or scoring (separate tool, future)
- No confidence scores (future)
- No Jira/ticket integration (Phase 2, config exists but not implemented)
- No GitHub PR creation (Phase 2, `ivc pr` just builds the tree locally)
- No cross-PR chaining (Phase 3)
- No vector embeddings or semantic search (Phase 3)

## Data Model (SurrealDB)

### Core Tables

```surql
DEFINE TABLE intention SCHEMAFULL;
DEFINE FIELD title ON intention TYPE string;
DEFINE FIELD reasoning ON intention TYPE string;
DEFINE FIELD type ON intention TYPE string;
    -- FEATURE, BUG_FIX, SECURITY_PATCH, TECH_DEBT, REFACTOR, UNKNOWN
DEFINE FIELD files_changed ON intention TYPE array<string>;
DEFINE FIELD uncertainties ON intention TYPE array<string>;
DEFINE FIELD alternatives_considered ON intention TYPE array<object>;
DEFINE FIELD assumptions ON intention TYPE array<string>;
DEFINE FIELD commit_sha ON intention TYPE string;
DEFINE FIELD branch ON intention TYPE string;
DEFINE FIELD repo ON intention TYPE string;
DEFINE FIELD source_type ON intention TYPE string;
    -- RECONSTRUCTED_FROM_COMMITS, IVC_FILE, HUMAN_PROVIDED
DEFINE FIELD source_confidence ON intention TYPE float;
DEFINE FIELD created_at ON intention TYPE datetime DEFAULT time::now();

DEFINE INDEX intention_sha_idx ON intention FIELDS commit_sha;
DEFINE INDEX intention_branch_idx ON intention FIELDS repo, branch;
```

### Graph Relations

```surql
-- Parent-child: root intention decomposes into sub-intentions
DEFINE TABLE decomposed_into SCHEMAFULL TYPE RELATION IN intention OUT intention;
DEFINE FIELD order ON decomposed_into TYPE int;

-- Dependencies between sibling intentions
DEFINE TABLE depends_on SCHEMAFULL TYPE RELATION IN intention OUT intention;
DEFINE FIELD reason ON depends_on TYPE string;
```

### Commit Metadata (captured at commit time, before LLM processing)

```surql
DEFINE TABLE commit_capture SCHEMAFULL;
DEFINE FIELD commit_sha ON commit_capture TYPE string;
DEFINE FIELD message ON commit_capture TYPE string;
DEFINE FIELD branch ON commit_capture TYPE string;
DEFINE FIELD repo ON commit_capture TYPE string;
DEFINE FIELD files_changed ON commit_capture TYPE array<string>;
DEFINE FIELD diff_stats ON commit_capture TYPE object;
    -- {additions: 45, deletions: 12, files_modified: 3}
DEFINE FIELD ticket_ref ON commit_capture TYPE option<string>;
    -- extracted from commit message via regex, e.g. "GRAPHEE-42"
DEFINE FIELD processed ON commit_capture TYPE bool DEFAULT false;
    -- true after ivc pr has processed this commit into intentions
DEFINE FIELD created_at ON commit_capture TYPE datetime DEFAULT time::now();

DEFINE INDEX commit_sha_idx ON commit_capture FIELDS commit_sha UNIQUE;
```

### Event Sourcing (append-only)

```surql
DEFINE TABLE event SCHEMAFULL;
DEFINE FIELD event_type ON event TYPE string;
    -- COMMIT_CAPTURED, PUSH_SYNCED, INTENTIONS_EXTRACTED, PR_CREATED
DEFINE FIELD source ON event TYPE string;
    -- CLI, GITHUB_WEBHOOK, AI_AGENT
DEFINE FIELD intention ON event TYPE option<record<intention>>;
DEFINE FIELD payload ON event TYPE object;
DEFINE FIELD created_at ON event TYPE datetime DEFAULT time::now();
```

## Project Structure

```
ivc/
├── Cargo.toml
├── CLAUDE.md                    # This file
├── docs/
│   ├── CONCEPT.md               # Full conceptual background
│   ├── PHASES.md                # All 5 implementation phases
│   └── DATA_MODEL.md            # Complete SurrealDB schema
├── src/
│   ├── main.rs                  # Entry point, clap CLI setup
│   ├── cli/
│   │   ├── mod.rs
│   │   ├── init.rs              # ivc init command
│   │   ├── commit.rs            # ivc commit command (wraps git commit)
│   │   ├── push.rs              # ivc push command (wraps git push)
│   │   ├── pr.rs                # ivc pr command (LLM extraction + tree display)
│   │   └── log.rs               # ivc log command
│   ├── git/
│   │   ├── mod.rs
│   │   ├── repo.rs              # Repository operations (open, read)
│   │   ├── diff.rs              # Diff extraction from commits
│   │   ├── branch.rs            # Branch operations (divergence point)
│   │   └── commit.rs            # Commit and push passthrough
│   ├── db/
│   │   ├── mod.rs
│   │   ├── connection.rs        # SurrealDB embedded connection
│   │   ├── schema.rs            # Schema initialisation
│   │   ├── commit_capture.rs    # Commit metadata CRUD
│   │   └── intention.rs         # Intention CRUD and tree operations
│   ├── ai/
│   │   ├── mod.rs
│   │   ├── client.rs            # Claude API client (reqwest)
│   │   └── extraction.rs        # Intention extraction from commits
│   └── models/
│       ├── mod.rs
│       ├── intention.rs         # Intention struct
│       ├── commit_capture.rs    # CommitCapture struct
│       └── event.rs             # Event struct
├── .ivc/
│   └── config.toml              # IVC configuration
└── tests/
    ├── git_tests.rs
    ├── db_tests.rs
    └── extraction_tests.rs
```

## Coding Conventions

- Use `anyhow` for error handling with context: `.context("Failed to open repository")`
- Use `thiserror` for custom error types in library code
- Prefer `async` with `tokio` for all I/O operations
- Use `tracing` for structured logging, not `println!` (except for CLI output to the user)
- All SurrealDB operations go through the `db` module, never called directly from CLI handlers
- All Git operations go through the `git` module, never called directly from CLI handlers
- Write tests for each module. Use embedded SurrealDB in-memory mode for tests.
- Use full forms in all user-facing text: "I am", "I will", "do not" (no contractions)
- Git passthrough: `ivc commit` and `ivc push` must pass ALL flags through to the real git command. IVC adds behaviour after git succeeds, never before.

## Configuration

```toml
# .ivc/config.toml

[database]
mode = "embedded"       # "embedded" or "remote"
path = ".ivc/data"      # for embedded mode
# url = "wss://..."     # for remote mode

[ai]
provider = "anthropic"
model = "claude-sonnet-4-20250514"
# api_key is read from ANTHROPIC_API_KEY env var, never stored in config

[git]
default_base = "main"   # base branch for divergence calculation

[ticket]
# Optional. If not configured, IVC works without ticket integration.
# provider = "jira"
# url = "https://company.atlassian.net"
# project_keys = ["GRAPHEE", "BEAN"]
# Auth via JIRA_API_TOKEN env var

[ticket.patterns]
# Where to look for ticket references in commit messages
# commit_message = true
# branch_name = true
```

## Claude API Integration

For intention extraction (`ivc pr`), send ALL commits on the branch as a batch to Claude. One LLM call per PR, not per commit.

The prompt should include:
- All commit messages on the branch
- The combined diff (or per-commit diffs if the combined is too large)
- The ticket context if available (fetched from Jira)
- Instruction to decompose into an intention tree

The prompt should ask Claude to identify per intention:
- The title (what was done)
- The reasoning (why it was done this way)
- The type (FEATURE, BUG_FIX, SECURITY_PATCH, TECH_DEBT, REFACTOR)
- Files changed and their purpose
- Uncertainties (what the author was not sure about)
- Alternatives considered and why they were rejected
- Assumptions made
- Dependencies between intentions (which depends on which)
- If ticket provided: which acceptance criteria each intention maps to

Response format should be JSON that deserialises directly into our Intention model.

## Concrete Example

Developer works on Graphee.link content automation:

```bash
ivc commit -m "GRAPHEE-42: add Spring scheduled task config"
ivc commit -m "GRAPHEE-42: create ContentAutomationService with cron trigger"
ivc commit -m "GRAPHEE-42: add ScheduleController for manual trigger and status"
ivc commit -m "GRAPHEE-42: add integration tests for scheduler"
ivc push
ivc pr
```

`ivc pr` output:

```
Intention tree for feature/content-automation (4 commits)
Ticket: GRAPHEE-42 (not fetched, Jira not configured)

├── Intention 1: Enable Spring scheduling infrastructure
│   Type: FEATURE
│   Files: Application.java, ScheduleConfig.java
│   Reasoning: Added @EnableScheduling and ThreadPoolTaskScheduler
│   Assumptions: Pool size 2 sufficient for current load
│
├── Intention 2: Create content automation service with daily cron
│   Type: FEATURE
│   Files: ContentAutomationService.java
│   Reasoning: Orchestrates content generation and Instagram posting
│   Uncertainties:
│     - No retry logic if Instagram API fails
│     - Cron expression hardcoded, not configurable
│
├── Intention 3: Add manual trigger and status endpoints
│   Type: FEATURE
│   Files: ScheduleController.java
│   Depends on: Intention 2
│   Reasoning: REST endpoints for testing without waiting for cron
│
└── Intention 4: Add integration tests
    Type: FEATURE
    Files: ContentAutomationServiceTest.java
    Depends on: Intention 2
    Uncertainties:
      - No controller endpoint tests
      - No cron expression parsing test

Stored in .ivc/data. Run ivc log to view again.
```

## Key Principles

1. **IVC is a wrapper, not a replacement.** Every ivc command performs the real git operation. If IVC is removed, the git repo is intact.
2. **Fast at commit time.** No LLM call at commit. Just metadata capture. Must not slow the developer down.
3. **Smart at PR time.** One LLM call processes all commits into a structured intention tree.
4. **Jira is optional.** Works without it. Better with it. Same structure either way.
5. **The intention tree is the product.** Not scores, not reviews, not checks. Just the structured "why" behind the code. Everything else builds on top of this.
