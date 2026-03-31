use anyhow::{Context, Result};

pub struct GithubClient {
    octocrab: octocrab::Octocrab,
    owner: String,
    repo: String,
}

impl GithubClient {
    /// Check if GITHUB_TOKEN is available without creating a client.
    pub fn is_available() -> bool {
        std::env::var("GITHUB_TOKEN").is_ok()
    }

    /// Create a new GitHub client. Reads GITHUB_TOKEN from env.
    pub fn new(owner: &str, repo: &str) -> Result<Self> {
        let token = std::env::var("GITHUB_TOKEN").context(
            "GITHUB_TOKEN environment variable is not set. \
             Set it with: export GITHUB_TOKEN=your-token",
        )?;

        let octocrab = octocrab::Octocrab::builder()
            .personal_token(token)
            .build()
            .context("Failed to create GitHub client")?;

        Ok(Self {
            octocrab,
            owner: owner.to_string(),
            repo: repo.to_string(),
        })
    }

    /// Create a pull request. Returns the PR URL.
    pub async fn create_pr(
        &self,
        title: &str,
        body: &str,
        head: &str,
        base: &str,
        draft: bool,
    ) -> Result<String> {
        let pr = self
            .octocrab
            .pulls(&self.owner, &self.repo)
            .create(title, head, base)
            .body(body)
            .draft(Some(draft))
            .send()
            .await
            .context("Failed to create GitHub pull request")?;

        let url = pr
            .html_url
            .map(|u| u.to_string())
            .unwrap_or_else(|| format!(
                "https://github.com/{}/{}/pull/{}",
                self.owner, self.repo, pr.number
            ));

        Ok(url)
    }
}
