# IVC Conceptual Background

## Origin

IVC emerged from connecting several ideas:

### 1. The HITL Spectrum

From IBM Technology's video on Human-in-the-Loop:

- **Human-in-the-loop (HITL):** System stops and waits for human approval before proceeding.
- **Human-on-the-loop (HOTL):** System operates autonomously while a human monitors with veto power.
- **Human-out-of-the-loop (HOOTL):** Full autonomy with no human intervention.

Human involvement can be injected at three stages:
- Training time: labelling gives the model **knowledge**
- Tuning time: preference feedback gives it **judgment** (RLHF)
- Inference time: runtime oversight gives it **guardrails**

The goal is not to keep humans in the loop forever, but to progress along the maturity curve as the system earns trust.

### 2. The Karpathy Loop (Autoresearch)

Karpathy's autoresearch demonstrated a closed-loop pattern:
- Agent tries something
- Measures the result against a single clear metric
- Keeps it if improved, discards if not
- Repeats

700 experiments over 2 days, 20 optimizations discovered. Git becomes the agent's persistent memory. The commit history records what worked and what failed.

Applied to IVC: the Karpathy loop evolves confidence scores and skills based on review outcomes. Each human decision feeds back into the knowledge base.

### 3. The Quantum Analogy

Code review is not linear. Like Feynman's path integral formulation where a photon takes every possible path simultaneously, a code review explores a massive possibility space. Confidence scores act as probability amplitudes, collapsing the infinite space into actionable outcomes.

Code enhancement (Claude Code writing code) is bounded and linear — HITL works naturally.
Code review is exponential and exploratory — the intention chain and confidence scores make it tractable.

### 4. Team-Level Adoption

A Reddit post described 3x vs 30% productivity boost with AI tools. The 3x team invested in context docs, structured planning, and customer discovery. The 30% team just bought licenses. This maps directly to the HITL maturity curve at the organisational level.

## Core Insight

Git tracks **what** changed. IVC tracks **why** it changed. They are complementary, not competing.

The intention chain is the product. The scores are the stop signal. The graph is the institutional memory. Every PR, every commit, every decision makes the chain richer and the scores more accurate.

## The Intention Node (Atomic Unit)

Every node in the graph carries five facets:

| Facet | What It Captures |
|-------|-----------------|
| History | What happened (the event) |
| Experience | What was learned from similar past nodes |
| Confidence Score | How sure we are (derived from experience) |
| Decision | What action was taken (the collapse) |
| Intention | Why this exists (the reasoning tree) |

## Intention Validation at Source

Every intention traces back to its source (sprint ticket, incident, advisory, developer initiative). IVC validates the intention against its source before code is written:

- Does the intention cover the acceptance criteria?
- Does it address the root cause (for bug fixes)?
- Is it specific enough to be measurable?

If the intention does not cover all criteria, IVC flags the gap immediately — before any code is written.

## Three Review Scopes (Phase 4)

When the review engine is built, findings are separated into:

1. **Intention-scoped:** Directly related to the PR's intentions. Primary output.
2. **Proximity-scoped:** Issues in touched files but outside the intention. Optional suggestions.
3. **System-scoped:** Codebase-wide issues. Separate dashboard, never attached to PRs.

System findings are deduplicated and persistent. No PR is punished for pre-existing debt.

## The Stop Signal

The intention defines "good." The review asks bounded questions per intention:
- Correctness: does it match the intention?
- Completeness: are uncertainties resolved?
- Soundness: are assumptions valid?
- Regression risk: does it break existing things?

Scores >= 0.80: auto-approve. 0.60-0.80: binary human decision. < 0.60: targeted review. < 0.40: blocking.

## Skills System (Phase 4)

Skills are markdown files defining review dimensions (security, performance, domain conventions). They evolve based on review outcomes via the Karpathy loop. Universal, stack-specific, project-specific, and learned (auto-detected) skills.

## Why Not Replace Git?

Failed SVCs asked developers to abandon Git. IVC never competes with Git. It rides on top of it. Every `ivc` command performs a real `git` operation. If IVC is removed, the Git repo is intact. The value proposition is not "use a better VCS" but "understand your code history better."
