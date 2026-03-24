#![allow(dead_code)] // Used by bin targets, not the lib

use serde::{Deserialize, Serialize};

/// Detected CI environment information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiEnvironment {
    pub provider: String,
    pub run_id: Option<String>,
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub pr_number: Option<String>,
}

impl CiEnvironment {
    /// Detect the current CI environment from environment variables.
    pub fn detect() -> Option<Self> {
        if std::env::var("GITHUB_ACTIONS").is_ok() {
            return Some(Self {
                provider: "github-actions".into(),
                run_id: std::env::var("GITHUB_RUN_ID").ok(),
                branch: std::env::var("GITHUB_REF_NAME").ok(),
                commit: std::env::var("GITHUB_SHA").ok(),
                pr_number: std::env::var("GITHUB_EVENT_NUMBER").ok(),
            });
        }

        if std::env::var("GITLAB_CI").is_ok() {
            return Some(Self {
                provider: "gitlab-ci".into(),
                run_id: std::env::var("CI_PIPELINE_ID").ok(),
                branch: std::env::var("CI_COMMIT_REF_NAME").ok(),
                commit: std::env::var("CI_COMMIT_SHA").ok(),
                pr_number: std::env::var("CI_MERGE_REQUEST_IID").ok(),
            });
        }

        if std::env::var("CIRCLECI").is_ok() {
            return Some(Self {
                provider: "circleci".into(),
                run_id: std::env::var("CIRCLE_BUILD_NUM").ok(),
                branch: std::env::var("CIRCLE_BRANCH").ok(),
                commit: std::env::var("CIRCLE_SHA1").ok(),
                pr_number: std::env::var("CIRCLE_PR_NUMBER").ok(),
            });
        }

        if std::env::var("BUILDKITE").is_ok() {
            return Some(Self {
                provider: "buildkite".into(),
                run_id: std::env::var("BUILDKITE_BUILD_NUMBER").ok(),
                branch: std::env::var("BUILDKITE_BRANCH").ok(),
                commit: std::env::var("BUILDKITE_COMMIT").ok(),
                pr_number: std::env::var("BUILDKITE_PULL_REQUEST").ok(),
            });
        }

        if std::env::var("TF_BUILD").is_ok() {
            return Some(Self {
                provider: "azure-pipelines".into(),
                run_id: std::env::var("BUILD_BUILDID").ok(),
                branch: std::env::var("BUILD_SOURCEBRANCH").ok(),
                commit: std::env::var("BUILD_SOURCEVERSION").ok(),
                pr_number: std::env::var("SYSTEM_PULLREQUEST_PULLREQUESTNUMBER").ok(),
            });
        }

        if std::env::var("CI").is_ok() {
            return Some(Self {
                provider: "unknown".into(),
                run_id: None,
                branch: None,
                commit: None,
                pr_number: None,
            });
        }

        None
    }
}
