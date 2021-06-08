use super::prelude::TestKubeClient;
use kube::api::Meta;
use serde::{de::DeserializeOwned, Serialize};
use std::{mem, ops::Deref};

/// Trait combo which must be satisfied for a resource to be deletable
pub trait DeletableResource: Clone + Default + DeserializeOwned + Meta {}
impl<T: Clone + Default + DeserializeOwned + Meta> DeletableResource for T {}

/// A temporary resource which is deleted when it goes out of scope
pub struct TemporaryResource<'a, T: DeletableResource> {
    client: &'a TestKubeClient,
    resource: T,
}

impl<'a, T: DeletableResource> TemporaryResource<'a, T> {
    /// Creates a new temporary resource according to the given specification.
    pub fn new(client: &'a TestKubeClient, spec: &str) -> Self
    where
        T: Serialize,
    {
        let resource = client.create(spec);
        TemporaryResource { client, resource }
    }
}

impl<'a, T: DeletableResource> Drop for TemporaryResource<'a, T> {
    fn drop(&mut self) {
        let resource = mem::take(&mut self.resource);
        self.client.delete(resource);
    }
}

impl<'a, T: DeletableResource> Deref for TemporaryResource<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.resource
    }
}
