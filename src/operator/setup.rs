use crate::test::prelude::{Node, Pod, TestKubeClient};

use anyhow::{anyhow, Result};
use kube::api::ObjectList;
use kube::{Resource, ResourceExt};
use serde::de::DeserializeOwned;
use serde::Serialize;
use stackable_operator::status::Conditions;
use std::fmt::Debug;
use std::thread;
use std::time::{Duration, Instant};

/// A wrapper to avoid passing in client or cluster everywhere.
pub struct TestCluster<T: Clone + Debug + DeserializeOwned + Resource<DynamicType = ()> + Serialize>
{
    pub client: TestKubeClient,
    pub cluster: Option<T>,
    pub options: TestClusterOptions,
    pub timeouts: TestClusterTimeouts,
}

/// Some reoccurring common test cluster options.
pub struct TestClusterOptions {
    pub cluster_ready_condition_type: String,
    pub pod_name_label: String,
}

/// Some reoccurring common test cluster timeouts.
pub struct TestClusterTimeouts {
    pub cluster_ready: Duration,
    pub pods_terminated: Duration,
}

impl<T> TestCluster<T>
where
    T: Clone + Debug + DeserializeOwned + Resource<DynamicType = ()> + Serialize,
{
    /// This creates a kube client and should be executed at the start of every test.
    pub fn new(options: TestClusterOptions, timeouts: TestClusterTimeouts) -> Self {
        TestCluster {
            client: TestKubeClient::new(),
            cluster: None,
            options,
            timeouts,
        }
    }

    /// Applies a custom resource, stores the returned cluster object and sleeps for
    /// two seconds to give the operator time to react on the custom resource.
    /// Without the sleep it can happen that tests run without any pods being created.
    fn apply(&mut self, cluster: &T) -> Result<()> {
        self.cluster = Some(self.client.apply(&serde_yaml::to_string(cluster)?));

        // we wait here to give the operator time to react to the custom resource
        thread::sleep(Duration::from_secs(2));
        Ok(())
    }

    /// Applies a command and waits 2 seconds to let the operator react on in.
    pub fn apply_command<C>(&self, command: &C) -> Result<C>
    where
        C: Clone + Debug + DeserializeOwned + Resource<DynamicType = ()> + Serialize,
    {
        let cmd: C = self.client.apply(&serde_yaml::to_string(command)?);

        // we wait here to give the operator time to react to the command
        thread::sleep(Duration::from_secs(2));
        Ok(cmd)
    }

    /// Returns all available pods in the cluster via the name label.
    pub fn get_current_pods(&self) -> Vec<Pod> {
        let current_pods: ObjectList<Pod> = self.client.list_labeled(&format!(
            "app.kubernetes.io/name={}",
            &self.options.pod_name_label
        ));
        current_pods.items
    }

    /// Returns all available nodes in the cluster. Can be used to determine the expected pods
    /// for tests (depending on the custom resource)
    pub fn get_available_nodes(&self) -> Vec<Node> {
        let available_nodes: ObjectList<Node> = self
            .client
            .list_labeled("kubernetes.io/arch=stackable-linux");
        available_nodes.items
    }

    /// A "busy" wait for all pods to be terminated and cleaned up.
    pub fn wait_for_pods_terminated(&self) -> Result<()> {
        let now = Instant::now();

        while now.elapsed().as_secs() < self.timeouts.pods_terminated.as_secs() {
            let pods = self.get_current_pods();

            if pods.is_empty() {
                return Ok(());
            }

            println!("Waiting for {} Pod(s) to terminate", pods.len());
            thread::sleep(Duration::from_secs(1));
        }

        Err(anyhow!(
            "Pods did not terminate within the specified timeout of {} second(s)",
            self.timeouts.pods_terminated.as_secs()
        ))
    }
}

impl<T> TestCluster<T>
where
    T: Clone + Conditions + Debug + DeserializeOwned + Resource<DynamicType = ()> + Serialize,
{
    /// Creates or updates a custom resource and waits for the cluster to be up and running
    /// within the provided timeout. Depending on the cluster definition we hand in the number
    /// of created pods we expect manually.
    pub fn create_or_update(&mut self, cluster: &T, expected_pod_count: usize) -> Result<()> {
        self.apply(cluster)?;
        self.wait_ready(expected_pod_count)?;
        Ok(())
    }

    /// Wait for the `expected_pod_count` pods to become ready or return an error if they fail to
    /// do so after a certain time. The amount of time it waits is configured by the user in the
    /// `cluster_ready` field of the `TestClusterTimeouts`.
    /// 
    /// # Arguments
    /// 
    /// * `expected_pod_count` - Number of pods to wait for until they become ready.
    /// 
    pub fn wait_ready(&self, expected_pod_count: usize) -> Result<()> {
        let now = Instant::now();

        let name = self.cluster.as_ref().unwrap().name();

        while now.elapsed().as_secs() < self.timeouts.cluster_ready.as_secs() {
            print!("Waiting for [{}/{}] to be ready...", T::kind(&()), name);
            let created_pods = self.get_current_pods();

            if created_pods.len() != expected_pod_count {
                println!(
                    "{} of {} pods created.",
                    created_pods.len(),
                    expected_pod_count
                );
            } else {
                for pod in &created_pods {
                    // TODO: switch to pod condition type enum from operator-rs?
                    self.client.verify_pod_condition(pod, "Ready");
                }

                println!("\nInstallation finished");
                return Ok(());
            }
            thread::sleep(Duration::from_secs(2));
        }

        Err(anyhow!(
            "Cluster did not startup within the specified timeout of {} second(s)",
            self.timeouts.cluster_ready.as_secs()
        ))
    }
}

/// This will clean up the custom resource, pods and commands (via OwnerReference) belonging
/// to the cluster each time a single test is finished.
impl<T> Drop for TestCluster<T>
where
    T: Clone + Debug + DeserializeOwned + Resource<DynamicType = ()> + Serialize,
{
    fn drop(&mut self) {
        if let Some(cluster) = self.cluster.take() {
            self.client.delete(cluster);
            if let Err(err) = self.wait_for_pods_terminated() {
                println!("{}", err.to_string())
            }
        }
    }
}
