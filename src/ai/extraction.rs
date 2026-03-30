use anyhow::{Context, Result};
use serde::Deserialize;

use crate::models::commit_capture::CommitCapture;
use crate::models::intention::{Alternative, IntentionType, SourceType};

/// The structured result from LLM intention extraction.
#[derive(Debug, Deserialize)]
pub struct ExtractionResult {
    pub root_intention: IntentionData,
    #[serde(default)]
    pub sub_intentions: Vec<SubIntentionData>,
}

#[derive(Debug, Deserialize)]
pub struct IntentionData {
    pub title: String,
    pub reasoning: String,
    #[serde(rename = "type")]
    pub intention_type: IntentionType,
    #[serde(default)]
    pub files_changed: Vec<String>,
    #[serde(default)]
    pub uncertainties: Vec<String>,
    #[serde(default)]
    pub alternatives_considered: Vec<Alternative>,
    #[serde(default)]
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub commit_shas: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct SubIntentionData {
    #[serde(flatten)]
    pub intention: IntentionData,
    pub depends_on_index: Option<usize>,
}

/// Build the prompt for intention extraction from commits and diff.
pub fn build_extraction_prompt(
    commits: &[CommitCapture],
    diff: &str,
    ticket_ref: Option<&str>,
) -> String {
    let mut prompt = String::new();

    prompt.push_str(
        "You are an expert at analyzing code changes and extracting the developer's intentions.\n\n",
    );
    prompt.push_str("Analyze the following commits and their combined diff. Produce a structured intention tree that captures WHY these changes were made, not just WHAT changed.\n\n");

    // Add commit messages
    prompt.push_str("## Commits\n\n");
    for capture in commits {
        prompt.push_str(&format!(
            "- {} (SHA: {})\n  Files: {}\n  Stats: +{} -{}\n\n",
            capture.message.trim(),
            &capture.commit_sha[..8.min(capture.commit_sha.len())],
            capture.files_changed.join(", "),
            capture.diff_stats.additions,
            capture.diff_stats.deletions,
        ));
    }

    // Add ticket reference if available
    if let Some(ticket) = ticket_ref {
        prompt.push_str(&format!(
            "## Ticket Reference\n\nTicket: {ticket} (details not fetched, ticket integration not configured)\n\n"
        ));
    }

    // Add diff
    prompt.push_str("## Combined Diff\n\n```\n");
    // Truncate diff if too large (roughly 100k chars ~ 25k tokens)
    if diff.len() > 100_000 {
        prompt.push_str(&diff[..100_000]);
        prompt.push_str("\n... (diff truncated due to size)\n");
    } else {
        prompt.push_str(diff);
    }
    prompt.push_str("\n```\n\n");

    // Output format specification
    prompt.push_str("## Output Format\n\n");
    prompt.push_str("Respond with ONLY a JSON object (no markdown fences, no explanation) in this exact format:\n\n");
    prompt.push_str(r#"{
  "root_intention": {
    "title": "High-level description of what this branch accomplishes",
    "reasoning": "Why this work was done — the motivation and context",
    "type": "FEATURE | BUG_FIX | SECURITY_PATCH | TECH_DEBT | REFACTOR | UNKNOWN",
    "files_changed": ["list of key files"],
    "uncertainties": ["things the author might not be sure about"],
    "alternatives_considered": [{"approach": "what else could have been done", "rejected_because": "why it was not chosen"}],
    "assumptions": ["assumptions made in the implementation"],
    "commit_shas": ["relevant commit SHAs"]
  },
  "sub_intentions": [
    {
      "title": "Sub-task description",
      "reasoning": "Why this specific part was done this way",
      "type": "FEATURE | BUG_FIX | SECURITY_PATCH | TECH_DEBT | REFACTOR | UNKNOWN",
      "files_changed": ["files for this sub-intention"],
      "uncertainties": [],
      "alternatives_considered": [],
      "assumptions": [],
      "commit_shas": ["relevant SHAs"],
      "depends_on_index": null
    }
  ]
}
"#);
    prompt.push_str("\nNotes:\n");
    prompt.push_str("- depends_on_index is the 0-based index of another sub_intention this one depends on, or null if independent\n");
    prompt.push_str("- Type must be one of: FEATURE, BUG_FIX, SECURITY_PATCH, TECH_DEBT, REFACTOR, UNKNOWN\n");
    prompt.push_str("- Decompose into sub-intentions when the branch contains multiple logical changes\n");
    prompt.push_str("- Focus on the WHY, not just the WHAT\n");

    prompt
}

/// Parse the LLM response into a structured ExtractionResult.
pub fn parse_extraction_response(response: &str) -> Result<ExtractionResult> {
    // Strip markdown code fences if present
    let json_str = strip_code_fences(response);

    serde_json::from_str::<ExtractionResult>(json_str.trim())
        .context("Failed to parse intention extraction response as JSON. The LLM response may not be in the expected format.")
}

/// Strip markdown code fences from a string.
fn strip_code_fences(s: &str) -> &str {
    let trimmed = s.trim();

    // Handle ```json ... ``` or ``` ... ```
    if let Some(rest) = trimmed.strip_prefix("```") {
        // Skip the language identifier line (e.g., "json")
        let rest = if let Some(newline_pos) = rest.find('\n') {
            &rest[newline_pos + 1..]
        } else {
            rest
        };
        // Strip trailing ```
        if let Some(content) = rest.strip_suffix("```") {
            return content.trim();
        }
        return rest.trim();
    }

    trimmed
}

/// Convert ExtractionResult into Intention models with branch/repo context.
pub fn to_intentions(
    result: &ExtractionResult,
    branch: &str,
    repo: &str,
) -> (
    crate::models::intention::Intention,
    Vec<(crate::models::intention::Intention, Option<usize>)>,
) {
    let root = crate::models::intention::Intention {
        id: None,
        title: result.root_intention.title.clone(),
        reasoning: result.root_intention.reasoning.clone(),
        intention_type: result.root_intention.intention_type.clone(),
        files_changed: result.root_intention.files_changed.clone(),
        uncertainties: result.root_intention.uncertainties.clone(),
        alternatives_considered: result.root_intention.alternatives_considered.clone(),
        assumptions: result.root_intention.assumptions.clone(),
        commit_shas: result.root_intention.commit_shas.clone(),
        branch: branch.to_string(),
        repo: repo.to_string(),
        source_type: SourceType::ReconstructedFromCommits,
        source_confidence: 0.7,
        backfill_metadata: None,
        created_at: None,
    };

    let children: Vec<_> = result
        .sub_intentions
        .iter()
        .map(|sub| {
            let intention = crate::models::intention::Intention {
                id: None,
                title: sub.intention.title.clone(),
                reasoning: sub.intention.reasoning.clone(),
                intention_type: sub.intention.intention_type.clone(),
                files_changed: sub.intention.files_changed.clone(),
                uncertainties: sub.intention.uncertainties.clone(),
                alternatives_considered: sub.intention.alternatives_considered.clone(),
                assumptions: sub.intention.assumptions.clone(),
                commit_shas: sub.intention.commit_shas.clone(),
                branch: branch.to_string(),
                repo: repo.to_string(),
                source_type: SourceType::ReconstructedFromCommits,
                source_confidence: 0.7,
                backfill_metadata: None,
                created_at: None,
            };
            (intention, sub.depends_on_index)
        })
        .collect();

    (root, children)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_code_fences_json() {
        let input = "```json\n{\"key\": \"value\"}\n```";
        assert_eq!(strip_code_fences(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn test_strip_code_fences_plain() {
        let input = "```\n{\"key\": \"value\"}\n```";
        assert_eq!(strip_code_fences(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn test_strip_code_fences_none() {
        let input = "{\"key\": \"value\"}";
        assert_eq!(strip_code_fences(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn test_parse_valid_response() {
        let json = r#"{
            "root_intention": {
                "title": "Add user authentication",
                "reasoning": "Users need to log in",
                "type": "FEATURE",
                "files_changed": ["auth.rs"],
                "uncertainties": [],
                "alternatives_considered": [],
                "assumptions": [],
                "commit_shas": ["abc123"]
            },
            "sub_intentions": []
        }"#;
        let result = parse_extraction_response(json);
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.root_intention.title, "Add user authentication");
    }
}
