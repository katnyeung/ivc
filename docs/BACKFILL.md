# IVC Backfill Design

## Purpose

When IVC is initialised in an existing repository with years of history, it starts with an empty graph. The backfill command allows developers to selectively reconstruct intention trees for historical PRs without reprocessing the entire repository.

## Principle: Start From Now, Backfill On Demand

IVC does NOT automatically process historical commits on init. The graph starts empty and grows in two ways:

1. **Forward capture:** New commits via `ivc commit` and new PRs via `ivc pr` or webhook.
2. **On-demand backfill:** Developer explicitly requests intention reconstruction for specific historical PRs or files.

The graph fills organically around the code people actually care about. Hot paths get full history because developers naturally backfill what they need to understand. Cold corners never get processed because nobody is looking at them.

## Command

```
ivc backfill [options]

Options:
    --pr <number>          Backfill a specific merged PR by number
    --file <path>          Backfill all merged PRs that touched this file
    --since <date>         Backfill all PRs merged since this date (ISO format)
    --until <date>         Backfill all PRs merged until this date (ISO format)
    --branch <name>        Backfill all PRs merged into this branch (default: main)
    --limit <n>            Maximum number of PRs to process (cost control, default: 10)
    --dry-run              Show which PRs would be processed, estimated tokens, no LLM call
    --skip-existing        Skip PRs that already have intentions in SurrealDB
```

## Backfill Modes

### Mode 1: Single PR

```bash
ivc backfill --pr 38
```

Backfill one specific PR. The most common use case: a developer is investigating code and wants to understand why it was written.

### Mode 2: File History

```bash
ivc backfill --file src/main/java/com/graphee/service/ContentAutomationService.java
```

Backfill all PRs that touched a specific file. Useful when investigating a bug or understanding the evolution of a component. Uses `git log --follow <file>` to find all commits that touched the file, then groups them by PR.

### Mode 3: Date Range

```bash
ivc backfill --since 2025-01-01 --limit 50
```

Backfill recent history. Useful when a team first adopts IVC and wants a baseline of recent intentions.

### Mode 4: Dry Run (Always Recommend First)

```bash
ivc backfill --file ContentAutomationService.java --dry-run
```

Output:
```
Would backfill 4 PRs that touched ContentAutomationService.java:

  PR #38  (2026-03-15) "Add content automation"
    4 commits | 5 files | ~320 lines changed
    Estimated tokens: ~2,400

  PR #44  (2026-04-02) "Make cron configurable"
    2 commits | 2 files | ~85 lines changed
    Estimated tokens: ~1,200

  PR #67  (2026-05-18) "Add retry logic for Instagram API"
    3 commits | 3 files | ~150 lines changed
    Estimated tokens: ~1,800

  PR #91  (2026-06-30) "Fix timezone handling in scheduler"
    1 commit | 1 file | ~25 lines changed
    Estimated tokens: ~600

Total: 4 PRs, 10 commits, estimated ~6,000 tokens
Proceed? (y/n)
```

## How Backfill Works Internally

### Step 1: Identify PRs

For `--pr <number>`:
- Search git log for the merge commit that references the PR number.
- Merge commits typically have messages like "Merge pull request #38 from feature/content-automation" or "Merge branch 'feature/content-automation' (#38)".
- If GitHub API is configured (Phase 2), fetch PR details directly.

For `--file <path>`:
- Run `git log --follow --all -- <path>` via git2 to find all commits touching the file.
- Group commits by their merge commit (identify which PR each commit belongs to).
- If a commit is not part of any merge (direct push to main), treat it as its own single-commit PR.

For `--since <date>`:
- Run `git log --merges --since=<date>` to find all merge commits in the date range.
- Each merge commit represents a PR.

### Step 2: Extract Commits Per PR

For each identified PR:

```rust
// Find the merge commit
let merge_commit = find_merge_commit(pr_number)?;

// Find the first parent (the base branch before merge)
let base = merge_commit.parent(0)?;

// Find the second parent (the branch that was merged)
let branch_tip = merge_commit.parent(1)?;

// Find the merge base (where the branch diverged)
let merge_base = repo.merge_base(base.id(), branch_tip.id())?;

// Walk commits from merge_base to branch_tip
let commits = repo.revwalk()
    .push(branch_tip.id())
    .hide(merge_base)
    .collect();
```

### Step 3: Extract Diffs

For each commit in the PR, extract the diff via git2:

```rust
let diff = repo.diff_tree_to_tree(
    parent_tree,
    commit_tree,
    None
)?;
```

Collect:
- Files changed with their paths
- Diff content (additions and deletions)
- Diff stats (lines added, removed, files modified)

### Step 4: Build Context for LLM

Assemble the context for one LLM call per PR:

```
You are reconstructing the intentions behind a historical pull request.
This PR has already been merged. You are working from the commit history
and diffs only.

PR merge commit: <sha>
Merged on: <date>
Branch: <branch_name> (if available from merge commit message)

Commits (in chronological order):

Commit 1: <sha>
Message: "add Spring scheduled task config"
Files changed: Application.java, ScheduleConfig.java
Diff:
<diff content>

Commit 2: <sha>
Message: "create ContentAutomationService with cron trigger"
Files changed: ContentAutomationService.java
Diff:
<diff content>

... (all commits)

Decompose this into an intention tree. For each intention identify:
- title: what was done
- reasoning: why (infer from the code patterns and commit messages)
- type: FEATURE, BUG_FIX, SECURITY_PATCH, TECH_DEBT, REFACTOR, UNKNOWN
- files_changed: which files belong to this intention
- uncertainties: anything that looks unresolved or potentially problematic
- alternatives_considered: if the code suggests alternatives were weighed
- assumptions: implicit assumptions in the code
- dependencies: which intentions depend on others

Respond in JSON matching this schema:
{
  "root_intention": {
    "title": "...",
    "reasoning": "...",
    "type": "...",
    "sub_intentions": [
      {
        "title": "...",
        "reasoning": "...",
        "type": "...",
        "files_changed": ["..."],
        "uncertainties": ["..."],
        "alternatives_considered": [{"approach": "...", "rejected_because": "..."}],
        "assumptions": ["..."],
        "commit_shas": ["..."],
        "depends_on_index": null | <index of sibling>
      }
    ]
  }
}
```

### Step 5: Store in SurrealDB

Store the intention tree with backfill-specific metadata:

```surql
CREATE intention SET
    title = $title,
    reasoning = $reasoning,
    type = $type,
    files_changed = $files,
    uncertainties = $uncertainties,
    alternatives_considered = $alternatives,
    assumptions = $assumptions,
    commit_shas = $commit_shas,
    branch = $branch,
    repo = $repo,
    source_type = "BACKFILLED",
    source_confidence = 0.35,
    backfill_metadata = {
        backfilled_at: time::now(),
        merge_commit: $merge_sha,
        merge_date: $merge_date,
        pr_number: $pr_number
    },
    created_at = $merge_date;
    -- Use the original merge date, not the backfill date
    -- This keeps chronological ordering correct in ivc log
```

Create the graph relations:
```surql
-- Root to sub-intentions
RELATE $root_intention -> decomposed_into -> $sub_intention
    SET order = $index;

-- Dependencies between sub-intentions
RELATE $sub_intention -> depends_on -> $dependency
    SET reason = $reason;

-- Link intentions to their source commits
RELATE $intention -> derived_from_commit -> $commit_capture;
```

Create events:
```surql
CREATE event SET
    event_type = "INTENTIONS_BACKFILLED",
    source = "CLI",
    intention = $root_intention,
    payload = {
        pr_number: $pr_number,
        commits_processed: $commit_count,
        tokens_used: $tokens
    },
    created_at = time::now();
```

### Step 6: Output

After processing, display the intention tree (same format as `ivc pr` output):

```
Backfilled PR #38 (merged 2026-03-15)

├── Intention 1: Enable Spring scheduling infrastructure
│   Type: FEATURE | Source: BACKFILLED (confidence: 0.35)
│   Files: Application.java, ScheduleConfig.java
│   Reasoning: Added @EnableScheduling and ThreadPoolTaskScheduler
│
├── Intention 2: Create content automation service with daily cron
│   Type: FEATURE | Source: BACKFILLED (confidence: 0.35)
│   Files: ContentAutomationService.java
│   Reasoning: Orchestrates content generation and Instagram posting
│   Uncertainties:
│     - No retry logic if Instagram API fails
│     - Cron expression hardcoded
│
├── Intention 3: Add manual trigger and status endpoints
│   Type: FEATURE | Source: BACKFILLED (confidence: 0.35)
│   Files: ScheduleController.java
│   Depends on: Intention 2
│
└── Intention 4: Add integration tests
    Type: FEATURE | Source: BACKFILLED (confidence: 0.35)
    Files: ContentAutomationServiceTest.java
    Depends on: Intention 2

Stored in SurrealDB. Run ivc log to view.
```

## Handling Edge Cases

### Squash Merges

Some repos use squash merging. The merge commit contains ALL changes as a single commit. There are no individual branch commits to walk.

Detection: merge commit has only one parent (squash merge) instead of two (regular merge).

Handling: treat the single squash commit as the entire PR. The LLM has less granularity but can still decompose the diff into logical intentions based on the code changes.

```rust
if merge_commit.parent_count() == 1 {
    // Squash merge: the merge commit IS the PR
    // Extract diff between merge commit and its parent
    let diff = repo.diff_tree_to_tree(parent_tree, merge_tree, None)?;
    // Process as a single-commit PR
}
```

### Rebase Merges (Fast-Forward)

Some repos use rebase and fast-forward merging. There is no merge commit at all. The branch commits are replayed directly onto main.

Detection: no merge commits in git log for the date range.

Handling: this is harder. Without merge commits, there is no clear boundary between PRs. Options:
1. If GitHub API is configured, fetch PR data to identify commit ranges.
2. If not, fall back to processing individual commits or groups of commits by author and time proximity.

For Phase 1, document this limitation and require GitHub API for repos that use rebase merging.

### Very Large PRs

Some PRs have hundreds of files changed and thousands of lines. The diff may exceed the LLM context window.

Handling:
1. Estimate token count before sending.
2. If the diff is too large, send only the file list, diff stats, and commit messages (without full diff content).
3. The intention extraction will be less detailed but still produces a useful tree structure.
4. Mark these intentions with lower source_confidence (0.2-0.3).

```rust
let estimated_tokens = estimate_tokens(&diff_content);
let context = if estimated_tokens > MAX_CONTEXT_TOKENS {
    // Send summary only
    build_summary_context(&commits, &diff_stats)
} else {
    // Send full diffs
    build_full_context(&commits, &diffs)
};
```

### PRs With No Clear Merge Commit Message

Some merge commits have generic messages like "Merge branch 'feature/xyz'" without a PR number. 

Handling: match by branch name pattern. If `--pr <number>` was specified, search for merge commits whose message contains the number or whose second parent branch name contains it. If matching fails, report that the PR could not be found and suggest using `--file` mode instead.

### Already Backfilled PRs

Running backfill twice on the same PR should not create duplicate intentions.

Handling: `--skip-existing` flag (default: true). Before processing a PR, check if intentions already exist in SurrealDB for those commit SHAs.

```surql
SELECT count() FROM intention
WHERE commit_shas CONTAINSANY $pr_commit_shas
AND repo = $repo;
```

If found, skip and report: "PR #38 already has intentions. Use --force to reprocess."

## Cost Estimation

Rough token estimates for backfill:
- Average PR: 5 commits, 200 lines changed = ~2,000-3,000 tokens input
- LLM response: ~500-1,000 tokens output
- Total per PR: ~3,000-4,000 tokens

At current Claude API pricing, backfilling 100 PRs costs roughly a few dollars. The `--dry-run` flag ensures developers know the cost before committing.

## Phase Placement

- **Phase 1:** `ivc backfill --pr <number>` with local git history only. The simplest form.
- **Phase 2:** `ivc backfill --file` and `--since` modes. GitHub API integration for repos using squash/rebase merging. Jira ticket enrichment for backfilled PRs.
- **Phase 3:** Backfilled intentions get vector embeddings and chain into the cross-PR graph.

## Implementation Order for Phase 1

1. PR identification from git merge commits
2. Commit extraction per PR via git2 revwalk
3. Diff extraction per commit
4. LLM prompt construction and API call
5. Response parsing into Intention model
6. SurrealDB storage with BACKFILLED source type
7. Duplicate detection (skip existing)
8. Dry run mode
9. Token estimation
10. Console output (intention tree display)
