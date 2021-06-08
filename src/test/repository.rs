use super::prelude::{KubeClient, TestKubeClient};
use anyhow::Result;
use kube_derive::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const REPO_SPEC: &str = "
    apiVersion: stable.stackable.de/v1
    kind: Repository
    metadata:
        name: integration-test-repository
        namespace: default
    spec:
        repo_type: StackableRepo
        properties:
            url: https://raw.githubusercontent.com/stackabletech/integration-test-repo/main/
";

/// Specification of a Stackable repository
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[kube(
    kind = "Repository",
    group = "stable.stackable.de",
    version = "v1",
    namespaced
)]
pub struct RepositorySpec {
    pub repo_type: RepoType,
    pub properties: HashMap<String, String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub enum RepoType {
    StackableRepo,
}

pub fn setup_repository(client: &TestKubeClient) {
    client.apply_crd(&Repository::crd());
    client.apply::<Repository>(REPO_SPEC);
}

pub async fn setup_repository_async(client: &KubeClient) -> Result<()> {
    client.apply_crd(&Repository::crd()).await?;
    client.apply::<Repository>(REPO_SPEC).await?;
    Ok(())
}
