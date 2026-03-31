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

    /// Find an existing open PR for the given head branch. Returns (PR number, URL) if found.
    pub async fn find_existing_pr(&self, head: &str) -> Result<Option<(u64, String)>> {
        let pulls = self
            .octocrab
            .pulls(&self.owner, &self.repo)
            .list()
            .state(octocrab::params::State::Open)
            .head(format!("{}:{}", self.owner, head))
            .send()
            .await
            .context("Failed to list pull requests")?;

        if let Some(pr) = pulls.items.first() {
            let url = pr
                .html_url
                .as_ref()
                .map(|u| u.to_string())
                .unwrap_or_else(|| format!(
                    "https://github.com/{}/{}/pull/{}",
                    self.owner, self.repo, pr.number
                ));
            Ok(Some((pr.number, url)))
        } else {
            Ok(None)
        }
    }

    /// Update an existing PR's title and body.
    pub async fn update_pr(
        &self,
        pr_number: u64,
        title: &str,
        body: &str,
    ) -> Result<String> {
        let pr = self
            .octocrab
            .pulls(&self.owner, &self.repo)
            .update(pr_number)
            .title(title)
            .body(body)
            .send()
            .await
            .context("Failed to update GitHub pull request")?;

        let url = pr
            .html_url
            .map(|u| u.to_string())
            .unwrap_or_else(|| format!(
                "https://github.com/{}/{}/pull/{}",
                self.owner, self.repo, pr.number
            ));

        Ok(url)
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
