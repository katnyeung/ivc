# IVC - Intention Version Control

> Version control for the **why**, not just the what.

IVC is a Rust CLI tool that wraps Git. It captures the **intentions** behind code changes -- the reasoning, alternatives considered, uncertainties, assumptions -- and stores them in a SurrealDB graph database alongside the Git commit history.

Git tracks what changed. IVC tracks why it changed.

## How It Works

IVC wraps common git commands. Use `ivc` instead of `git` for your daily workflow:

```bash
ivc init              # initialise IVC in an existing git repo
ivc add .             # wraps git add
ivc commit -m "..."   # wraps git commit + captures metadata
ivc push              # wraps git push + syncs metadata
ivc pr                # extracts intention tree via one LLM call
ivc log               # displays the intention tree
```

All git flags pass through. If IVC is removed, the git repo remains perfectly intact.

### The Key Insight

- **Commits are fast.** `ivc commit` captures lightweight metadata (SHA, message, files, diff stats) in embedded SurrealDB. No LLM call. Zero noticeable latency.
- **PRs are smart.** `ivc pr` sends all commits on the branch to Claude in a single API call and receives a structured intention tree.
- **One LLM call per PR**, not per commit. Cost-efficient and avoids wasted processing on unstable commits that may be amended or squashed.

### Example Output

```
ivc pr

Intention tree for feature/content-automation (4 commits)

+-  Intention 1: Enable Spring scheduling infrastructure
|   Type: FEATURE
|   Files: Application.java, ScheduleConfig.java
|   Reasoning: Added @EnableScheduling and ThreadPoolTaskScheduler
|   Assumptions: Pool size 2 sufficient for current load
|
+-  Intention 2: Create content automation service with daily cron
|   Type: FEATURE
|   Files: ContentAutomationService.java
|   Reasoning: Orchestrates content generation and Instagram posting
|   Uncertainties:
|     - No retry logic if Instagram API fails
|     - Cron expression hardcoded
|
+-  Intention 3: Add manual trigger and status endpoints
|   Type: FEATURE
|   Files: ScheduleController.java
|   Depends on: Intention 2
|
+-- Intention 4: Add integration tests
    Type: FEATURE
    Files: ContentAutomationServiceTest.java
    Depends on: Intention 2
```

## Installation

### Prerequisites

- Rust toolchain (1.75+)
- Git
- `ANTHROPIC_API_KEY` environment variable (for `ivc pr` and `ivc backfill`)

### Build from source

```bash
git clone https://github.com/user/ivc.git
cd ivc
cargo build --release
# Binary is at target/release/ivc
```

## Commands

### Core Commands

| Command | Description | LLM Call? |
|---------|-------------|-----------|
| `ivc init` | Initialise IVC in an existing git repo | No |
| `ivc commit` | Wrap git commit + capture metadata in SurrealDB | No |
| `ivc push` | Wrap git push + sync metadata, clean stale captures | No |
| `ivc pr` | Extract intention tree from branch commits | Yes (one call) |
| `ivc pr --base develop` | Use a different base branch | Yes (one call) |
| `ivc log` | Display intention tree for current branch | No |
| `ivc log <filepath>` | Show all intentions that touched a file | No |
| `ivc backfill --pr 38` | Reconstruct intentions for a historical PR | Yes (one call) |
| `ivc backfill --since 2025-01-01` | Backfill all PRs merged since a date | Yes (one per PR) |
| `ivc backfill --file src/main.rs` | Backfill all PRs that touched a file | Yes (one per PR) |
| `ivc backfill ... --dry-run` | Preview what would be processed | No |

### Git Passthroughs

All common git commands are wrapped as pure passthroughs with no extra overhead:

```
ivc add, ivc status, ivc diff, ivc pull, ivc checkout, ivc branch,
ivc merge, ivc rebase, ivc reset, ivc stash, ivc fetch, ivc remote,
ivc tag, ivc cherry-pick, ivc restore, ivc switch, ivc show, ivc clean
```

`ivc git-log` wraps `git log` (since `ivc log` is the intention tree viewer).

## Configuration

IVC stores its configuration in `.ivc/config.toml`:

```toml
[database]
mode = "embedded"
path = ".ivc/data"

[ai]
provider = "anthropic"
model = "claude-sonnet-4-20250514"
# API key read from ANTHROPIC_API_KEY env var

[git]
default_base = "main"
```

## Data Model

IVC uses embedded SurrealDB with a graph data model:

- **commit_capture** -- lightweight metadata captured at commit time
- **intention** -- structured intention extracted by LLM at PR time
- **event** -- append-only event log for auditability
- **decomposed_into** -- relation: root intention decomposes into sub-intentions
- **depends_on** -- relation: dependencies between sibling intentions
- **derived_from_commit** -- relation: links intentions to their source commits

## Backfill

IVC can reconstruct intentions for historical PRs that were merged before IVC was adopted:

```bash
# Single PR
ivc backfill --pr 38
ivc backfill --pr 38 --dry-run

# Date range (processes merge commits in range, one LLM call per PR)
ivc backfill --since 2025-01-01
ivc backfill --since 2025-06-01 --until 2025-12-31 --limit 20

# File history (all PRs that touched a specific file)
ivc backfill --file src/service/ContentAutomationService.java

# Common flags
#   --dry-run          Preview without calling LLM
#   --limit <n>        Max PRs to process (default: 10)
#   --skip-existing    Skip PRs that already have intentions (default: true)
```

Backfilled intentions are stored with `source_type = BACKFILLED` and `source_confidence = 0.35` (lower than live-captured intentions at 0.70). The `created_at` timestamp uses the original merge date to maintain correct chronological ordering.

## Phases

### Phase 1: Core Intention Capture (current)

Wraps git commit/push. Captures lightweight metadata at commit time. Processes into structured intention tree at PR time via one LLM call. Backfill for historical PRs.

### Phase 2: GitHub + Ticket Integration

- `ivc pr` creates a real GitHub PR with structured description
- `.ivc.json` committed to the PR
- Optional Jira integration enriches intentions with ticket context
- Intention validation against acceptance criteria

### Phase 3: Intention Chain Across PRs

- Vector embeddings for semantic search across intentions
- Cross-PR chaining: BUILDS_ON, FIXES, EXTENDS, DEFERS_FROM
- `ivc chain <keyword>` traces feature history across all PRs
- SurrealDB remote mode for team sharing

### Phase 4: Review Engine (Separate Tool)

A separate tool (not IVC) consumes the intention graph and produces review verdicts:

- Skills system: markdown files defining review dimensions
- Per-intention scoring across multiple dimensions
- Three scopes: intention-scoped, proximity-scoped, system-scoped
- Progressive autonomy based on confidence scores

### Phase 5: Karpathy Loop

- Review outcomes feed back into the knowledge base
- Confidence scores evolve from accumulated decisions
- Skills auto-update based on review patterns
- Trust maturity progression: HITL -> HOTL -> HOOTL

## Design Principles

1. **IVC is a wrapper, not a replacement.** Every `ivc` command performs the real git operation. Remove IVC and the git repo is intact.
2. **Fast at commit time.** No LLM call at commit. Just metadata capture. Must not slow the developer down.
3. **Smart at PR time.** One LLM call processes all commits into a structured intention tree.
4. **The intention tree is the product.** Not scores, not reviews, not checks. Just the structured "why" behind the code.
5. **Jira is optional.** Works without it. Better with it. Same structure either way.

## Architecture

Single Rust binary with everything embedded:

- **git2** (libgit2 bindings) for git operations
- **SurrealDB** (embedded, in-process) for intention graph storage
- **reqwest** + Claude API for intention extraction
- **clap** for CLI parsing
- **tokio** for async runtime

## License

MIT
