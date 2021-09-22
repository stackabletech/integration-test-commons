use crate::test::prelude::{Node, Pod, TestKubeClient};

use anyhow::{anyhow, Result};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use kube::Resource;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// A wrapper to avoid passing in client or cluster everywhere.
pub struct TestCluster<T: Clone + Debug + DeserializeOwned + Resource<DynamicType = ()> + Serialize>
{
    pub client: TestKubeClient,
    pub cluster: Option<T>,
    pub options: TestClusterOptions,
    pub labels: TestClusterLabels,
    pub timeouts: TestClusterTimeouts,
}

/// Some reoccurring common test cluster options.
pub struct TestClusterOptions {
    app_name: String,
    instance_name: String,
}

impl TestClusterOptions {
    pub fn new(app_name: &str, instance_name: &str) -> Self {
        TestClusterOptions {
            app_name: app_name.to_string(),
            /// Append UUID to the cluster name. The full UUID is too long when combined in the pod
            /// names (63 characters). So we just use a slice here to avoid conflicts with the names.
            instance_name: format!("{}-{}", instance_name, Uuid::new_v4().as_fields().0).as_str()
                [0..62]
                .to_string(),
        }
    }
}

/// Some reoccurring common test cluster timeouts.
pub struct TestClusterTimeouts {
    pub cluster_ready: Duration,
    pub pods_terminated: Duration,
}

/// Some reoccurring common labels.
pub struct TestClusterLabels {
    pub app: String,
    pub instance: String,
    pub version: String,
}

impl TestClusterLabels {
    pub fn new(app: &str, instance: &str, version: &str) -> Self {
        TestClusterLabels {
            app: app.to_string(),
            instance: instance.to_string(),
            version: version.to_string(),
        }
    }
}

impl<T> TestCluster<T>
where
    T: Clone + Debug + DeserializeOwned + Resource<DynamicType = ()> + Serialize,
{
    /// This creates a kube client and should be executed at the start of every test.
    pub fn new(
        options: TestClusterOptions,
        labels: TestClusterLabels,
        timeouts: TestClusterTimeouts,
    ) -> Self {
        TestCluster {
            client: TestKubeClient::new(),
            cluster: None,
            options,
            labels,
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

    /// Check if the creation timestamps of all pods are older than the provided timestamp.
    /// Maybe used with testing commands like Restart etc.
    pub fn check_pod_creation_timestamp(&self, creation_timestamp: &Option<Time>) -> Result<()> {
        for pod in &self.list::<Pod>(None) {
            let pod_creation_timestamp = &pod.metadata.creation_timestamp;

            if pod_creation_timestamp < creation_timestamp {
                return Err(anyhow!(self.log(
                &format!("Pod [{}] has an older timestamp [{:?}] than the provided timestamp [{:?}]. This should not be happening!",
                pod.metadata.name.as_ref().unwrap(),
                pod_creation_timestamp,
                creation_timestamp,
            ))));
            }
        }

        Ok(())
    }
    /// Check if all pods have the provided version parameter in the `APP_VERSION_LABEL` label.
    /// May be used to check the for the correct version after cluster updates.
    pub fn check_pod_version(&self, version: &str) -> Result<()> {
        for pod in &self.list::<Pod>(None) {
            if let Some(pod_version) = pod
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get(&self.labels.version))
            {
                if version != pod_version {
                    return Err(anyhow!(self.log(&format!(
                        "Pod [{}] has version [{}] but should have version [{}]. This should not happen!",
                        pod.metadata.name.as_ref().unwrap(),
                        pod_version,
                        version.to_string()
                    ))));
                }
            } else {
                return Err(anyhow!(
                    "Pod [{}] has no version label [{}]. Expected version [{}]. This should not happen!",
                    pod.metadata.name.as_ref().unwrap(),
                    &self.labels.version,
                    version.to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Creates or updates a custom resource and waits for the cluster to be up and running
    /// within the provided timeout. Depending on the cluster definition we hand in the number
    /// of created pods we expect manually.
    pub fn create_or_update(&mut self, cluster: &T, expected_pod_count: usize) -> Result<()> {
        self.apply(cluster)?;
        self.wait_ready(expected_pod_count)?;
        Ok(())
    }

    /// List resources belonging to the cluster. Additional labels to filter or limit the
    /// selector may be passed via `additional_labels`.
    pub fn list<R>(&self, additional_labels: Option<BTreeMap<String, String>>) -> Vec<R>
    where
        R: Clone + Debug + DeserializeOwned + Resource<DynamicType = ()> + Serialize,
    {
        let mut labels = additional_labels.unwrap_or_default();

        labels.insert(self.labels.app.clone(), self.options.app_name.clone());
        labels.insert(
            self.labels.instance.clone(),
            self.options.instance_name.clone(),
        );

        let transformed_labels = labels
            .iter()
            .map(|(key, value)| format!("{}={}", key, value))
            .collect::<Vec<String>>();

        self.client
            .list_labeled::<R>(&transformed_labels.join(","))
            .items
    }

    /// List all nodes registered in the api server that have an agent running (or default to
    /// `kubernetes.io/arch=stackable-linux` label).
    /// May be used to determine the expected pods for tests (depending on the custom resource).
    pub fn list_nodes(&self, selector: Option<&str>) -> Vec<Node> {
        self.client
            .list_labeled::<Node>(selector.unwrap_or("kubernetes.io/arch=stackable-linux"))
            .items
    }

    /// Write a formatted message with cluster kind and cluster name in the beginning to the console.
    fn log(&self, message: &str) -> String {
        format!("[{}/{}] {}", T::kind(&()), self.name(), message)
    }

    /// Return the cluster / instance name
    pub fn name(&self) -> &str {
        self.options.instance_name.as_str()
    }

    /// A "busy" wait for all pods to be terminated and cleaned up.
    pub fn wait_for_pods_terminated(&self) -> Result<()> {
        let now = Instant::now();

        while now.elapsed().as_secs() < self.timeouts.pods_terminated.as_secs() {
            let pods = &self.list::<Pod>(None);

            if pods.is_empty() {
                return Ok(());
            }

            println!(
                "{}",
                self.log(&format!("Waiting for {} Pod(s) to terminate", pods.len()))
            );
            thread::sleep(Duration::from_secs(1));
        }

        Err(anyhow!(self.log(&format!(
            "Pods did not terminate within the specified timeout of {} second(s)",
            self.timeouts.pods_terminated.as_secs()
        ))))
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

        while now.elapsed().as_secs() < self.timeouts.cluster_ready.as_secs() {
            let created_pods = &self.list::<Pod>(None);
            println!(
                "{}",
                self.log(&format!(
                    "Waiting for [{}/{}] pod(s) to be ready...",
                    created_pods.len(),
                    expected_pod_count
                )),
            );

            if created_pods.len() != expected_pod_count {
                thread::sleep(Duration::from_secs(2));
                continue;
            } else {
                for pod in created_pods {
                    self.client.verify_pod_condition(pod, "Ready");
                }
                println!("{}", self.log("Installation finished"));
                return Ok(());
            }
        }

        Err(anyhow!(self.log(&format!(
            "Cluster did not startup within the specified timeout of {} second(s)",
            self.timeouts.cluster_ready.as_secs()
        ))))
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
                self.log(&err.to_string());
            }
        }
    }
}
