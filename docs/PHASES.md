# IVC Implementation Phases

## Phase 1: Core Intention Capture ← CURRENT

**Goal:** IVC wraps git commit and push. Captures lightweight metadata at commit time. Processes into structured intention tree at PR time via one LLM call.

### Commands
| Command | What It Does | LLM Call? |
|---------|-------------|-----------|
| `ivc init` | Create .ivc/ directory, init embedded SurrealDB | No |
| `ivc commit` | Run git commit + capture metadata in SurrealDB | No |
| `ivc push` | Run git push + sync metadata | No |
| `ivc pr` | Collect all commits, one LLM call, build intention tree | Yes (one call) |
| `ivc log` | Display intention tree from SurrealDB | No |

### Out of Scope
- No Jira/ticket integration
- No GitHub PR creation (ivc pr just builds tree locally)
- No cross-PR chaining
- No review, scoring, or checks
- No vector embeddings

### Success Criteria
Given a branch with 5 commits, `ivc pr` produces a correct intention tree and `ivc log` displays it. The tree accurately describes why the code was written. Metadata captured with ivc commit adds zero noticeable latency.

---

## Phase 2: GitHub + Ticket Integration

**Goal:** `ivc pr` creates a real GitHub PR with structured description. Optional Jira integration enriches intentions with ticket context.

### New Capabilities
- `ivc pr` creates GitHub PR via octocrab with intention tree as description
- .ivc.json committed to the PR
- Jira integration: fetch ticket details, map acceptance criteria to intentions
- Ticket reference detection in commit messages and branch names
- Intention validation: flag if intention does not cover all acceptance criteria

### Success Criteria
A PR created via `ivc pr` has a structured description. With Jira configured, the reviewer sees acceptance criteria coverage (e.g. "4 of 5 criteria mapped").

---

## Phase 3: Intention Chain Across PRs

**Goal:** Intentions chain across PRs. Institutional memory.

### New Capabilities
- Vector embeddings for intentions (Claude API)
- Semantic search: find similar past intentions
- Cross-PR chaining: BUILDS_ON, FIXES, EXTENDS, DEFERS_FROM
- `ivc chain <keyword>` traces feature history across PRs
- Deferred decisions tracked as obligations
- SurrealDB remote mode for team sharing

### Success Criteria
`ivc chain bean-filter` shows the complete history of a feature across all PRs from first commit to current state.

---

## Phase 4: Review Engine (Separate Tool, Not IVC)

**Goal:** A separate tool consumes the IVC intention graph and produces review verdicts.

This is NOT part of IVC. IVC is the intention capture wrapper. The review tool is a separate binary/service that reads from IVC's SurrealDB.

### Capabilities (separate tool)
- Reads intention tree from SurrealDB
- Skills system: markdown files defining review dimensions
- Per-intention scoring (simple: one number per dimension, skill defines direction and threshold)
- Three scopes: intention-scoped, proximity-scoped, system-scoped
- Multiple review agents (parallel, each with focused skill)
- Sequential stages as cost optimisation
- Review agent never commits to the PR (only produces verdicts)

---

## Phase 5: Karpathy Loop

**Goal:** Review outcomes feed back into the knowledge base. Confidence evolves. Skills auto-update.

### Capabilities
- Record reviewer decisions as events
- Derive confidence from accumulated outcomes
- Skill evolution based on review patterns
- Progressive autonomy: auto-approve high-confidence intentions
- Trust maturity tracking (HITL → HOTL → HOOTL progression)
