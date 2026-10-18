use std::sync::Mutex;

use async_trait::async_trait;
use omp_core::release::DownloadProtocol;

use crate::client::{AddRequest, ClientError, ClientItem, DownloadClient};

/// A client that records what it is given and answers from a script. For
/// tests of everything that talks to a client.
pub struct FakeClient {
    protocol: DownloadProtocol,
    added: Mutex<Vec<String>>,
    failure: Mutex<Option<ClientError>>,
    items: Mutex<Vec<ClientItem>>,
    items_failure: Mutex<Option<ClientError>>,
    removed: Mutex<Vec<(String, bool)>>,
}

impl FakeClient {
    pub fn new(protocol: DownloadProtocol) -> Self {
        Self {
            protocol,
            added: Mutex::new(Vec::new()),
            failure: Mutex::new(None),
            items: Mutex::new(Vec::new()),
            items_failure: Mutex::new(None),
            removed: Mutex::new(Vec::new()),
        }
    }

    /// Make every `add` fail with this error until cleared.
    pub fn fail_with(&self, error: Option<ClientError>) {
        *self.failure.lock().unwrap() = error;
    }

    /// Titles added so far.
    pub fn added(&self) -> Vec<String> {
        self.added.lock().unwrap().clone()
    }

    /// Make `items` fail until cleared.
    pub fn fail_items(&self, error: Option<ClientError>) {
        *self.items_failure.lock().unwrap() = error;
    }

    pub fn set_items(&self, items: Vec<ClientItem>) {
        *self.items.lock().unwrap() = items;
    }

    pub fn removed(&self) -> Vec<(String, bool)> {
        self.removed.lock().unwrap().clone()
    }
}

#[async_trait]
impl DownloadClient for FakeClient {
    fn protocol(&self) -> DownloadProtocol {
        self.protocol
    }

    async fn add(&self, request: &AddRequest<'_>) -> Result<String, ClientError> {
        if let Some(e) = self.failure.lock().unwrap().clone() {
            return Err(e);
        }
        let mut added = self.added.lock().unwrap();
        added.push(request.release.title.clone());
        Ok(format!("fake-{}", added.len()))
    }

    async fn items(&self) -> Result<Vec<ClientItem>, ClientError> {
        if let Some(e) = self.items_failure.lock().unwrap().clone() {
            return Err(e);
        }
        Ok(self.items.lock().unwrap().clone())
    }

    async fn remove(&self, download_id: &str, delete_data: bool) -> Result<(), ClientError> {
        self.removed
            .lock()
            .unwrap()
            .push((download_id.to_owned(), delete_data));
        Ok(())
    }
}
