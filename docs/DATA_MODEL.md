# IVC Data Model (SurrealDB)

## Phase 1 Schema (Current)

### commit_capture

Lightweight metadata captured at `ivc commit` time. No LLM processing yet. Fast.

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
    -- extracted via regex, e.g. "GRAPHEE-42"
DEFINE FIELD processed ON commit_capture TYPE bool DEFAULT false;
    -- true after ivc pr processes this into intentions
DEFINE FIELD created_at ON commit_capture TYPE datetime DEFAULT time::now();

DEFINE INDEX commit_sha_idx ON commit_capture FIELDS commit_sha UNIQUE;
DEFINE INDEX commit_branch_idx ON commit_capture FIELDS repo, branch;
```

### intention

Created by `ivc pr` after LLM processing. The structured "why" behind the code.

```surql
DEFINE TABLE intention SCHEMAFULL;
DEFINE FIELD title ON intention TYPE string;
DEFINE FIELD reasoning ON intention TYPE string;
DEFINE FIELD type ON intention TYPE string;
    -- FEATURE, BUG_FIX, SECURITY_PATCH, TECH_DEBT, REFACTOR, UNKNOWN
DEFINE FIELD files_changed ON intention TYPE array<string>;
DEFINE FIELD uncertainties ON intention TYPE array<string>;
DEFINE FIELD alternatives_considered ON intention TYPE array<object>;
    -- [{approach: "Querydsl", rejected_because: "not in dependencies"}]
DEFINE FIELD assumptions ON intention TYPE array<string>;
DEFINE FIELD commit_shas ON intention TYPE array<string>;
    -- can map to multiple commits
DEFINE FIELD branch ON intention TYPE string;
DEFINE FIELD repo ON intention TYPE string;
DEFINE FIELD source_type ON intention TYPE string;
    -- RECONSTRUCTED_FROM_COMMITS, RECONSTRUCTED_WITH_TICKET, HUMAN_PROVIDED
DEFINE FIELD source_confidence ON intention TYPE float;
    -- 0.4-0.5 without ticket, 0.6-0.7 with ticket, 0.9+ with rich context
DEFINE FIELD created_at ON intention TYPE datetime DEFAULT time::now();

DEFINE INDEX intention_branch_idx ON intention FIELDS repo, branch;
```

### Graph Relations

```surql
-- Root intention decomposes into sub-intentions
DEFINE TABLE decomposed_into SCHEMAFULL TYPE RELATION IN intention OUT intention;
DEFINE FIELD order ON decomposed_into TYPE int;

-- Dependencies between sibling intentions
DEFINE TABLE depends_on SCHEMAFULL TYPE RELATION IN intention OUT intention;
DEFINE FIELD reason ON depends_on TYPE string;

-- Link intentions to the commits they were derived from
DEFINE TABLE derived_from_commit SCHEMAFULL TYPE RELATION IN intention OUT commit_capture;
```

### event (append-only)

Every state change is an event. Immutable log.

```surql
DEFINE TABLE event SCHEMAFULL;
DEFINE FIELD event_type ON event TYPE string;
    -- COMMIT_CAPTURED, PUSH_SYNCED, INTENTIONS_EXTRACTED, PR_CREATED
DEFINE FIELD source ON event TYPE string;
    -- CLI, GITHUB_WEBHOOK
DEFINE FIELD intention ON event TYPE option<record<intention>>;
DEFINE FIELD payload ON event TYPE object;
DEFINE FIELD created_at ON event TYPE datetime DEFAULT time::now();
```

---

## Phase 2 Additions

### intention_source (ticket integration)

```surql
DEFINE TABLE intention_source SCHEMAFULL;
DEFINE FIELD source_type ON intention_source TYPE string;
    -- SPRINT_TICKET, INCIDENT, SECURITY_ADVISORY, DEVELOPER_INITIATIVE
DEFINE FIELD source_url ON intention_source TYPE option<string>;
DEFINE FIELD acceptance_criteria ON intention_source TYPE array<string>;

DEFINE TABLE derived_from_source SCHEMAFULL TYPE RELATION IN intention OUT intention_source;
DEFINE FIELD coverage ON derived_from_source TYPE float;
DEFINE FIELD missing_criteria ON derived_from_source TYPE array<string>;
```

---

## Phase 3 Additions

### Vector embeddings and cross-PR chains

```surql
-- Add embedding to intention
DEFINE FIELD embedding ON intention TYPE option<array>;
DEFINE INDEX intention_embedding_idx ON intention
    FIELDS embedding MTREE DIMENSION 1536 DIST COSINE;

-- Cross-PR chaining
DEFINE TABLE chains_from SCHEMAFULL TYPE RELATION IN intention OUT intention;
DEFINE FIELD relationship ON chains_from TYPE string;
    -- BUILDS_ON, FIXES, EXTENDS, REPLACES, DEFERS_FROM
```

---

## Phase 4+ Additions (Review Engine, Separate Tool)

See PHASES.md. Review verdicts, system findings, skills, and reviewer decisions are managed by the review tool, not IVC. They may share the same SurrealDB instance but are separate schemas owned by the review tool.
