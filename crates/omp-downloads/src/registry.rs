//! Choosing which configured client gets a release. Ported from Radarr
//! `DownloadClientProvider` (GPL-3.0).

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use omp_core::release::DownloadProtocol;

use crate::client::{ClientError, DownloadClient};

pub struct RegisteredClient {
    /// The provider id, which is also the key for back-off status.
    pub id: i64,
    pub name: String,
    /// Lower number is tried first.
    pub priority: i32,
    /// Empty means the client takes any movie.
    pub tags: Vec<i32>,
    pub client: Arc<dyn DownloadClient>,
}

impl RegisteredClient {
    pub fn protocol(&self) -> DownloadProtocol {
        self.client.protocol()
    }
}

#[derive(Default)]
pub struct ClientRegistry {
    clients: Vec<Arc<RegisteredClient>>,
    last_used: Mutex<HashMap<DownloadProtocol, i64>>,
}

impl ClientRegistry {
    pub fn new(clients: Vec<RegisteredClient>) -> Self {
        Self {
            clients: clients.into_iter().map(Arc::new).collect(),
            last_used: Mutex::new(HashMap::new()),
        }
    }

    pub fn get(&self, id: i64) -> Option<Arc<RegisteredClient>> {
        self.clients.iter().find(|c| c.id == id).cloned()
    }

    pub fn all(&self) -> &[Arc<RegisteredClient>] {
        &self.clients
    }

    /// Picks a client for a release of `protocol` for a movie with `tags`.
    ///
    /// - Clients with tags are only used when one of the movie's tags matches;
    ///   otherwise only untagged clients are eligible.
    /// - Blocked clients (recent failures) are avoided when any other is
    ///   available. With `filter_blocked`, a blocked-only set is an error.
    /// - Among the rest, the lowest priority number wins, and equal-priority
    ///   clients take turns.
    pub fn select(
        &self,
        protocol: DownloadProtocol,
        movie_tags: &[i32],
        filter_blocked: bool,
        blocked: &HashSet<i64>,
    ) -> Result<Arc<RegisteredClient>, ClientError> {
        let available: Vec<&Arc<RegisteredClient>> = self
            .clients
            .iter()
            .filter(|c| c.protocol() == protocol)
            .collect();
        if available.is_empty() {
            return Err(ClientError::Unavailable(format!(
                "{protocol:?} download client isn't configured yet"
            )));
        }

        let tagged: Vec<_> = available
            .iter()
            .copied()
            .filter(|c| c.tags.iter().any(|t| movie_tags.contains(t)))
            .collect();
        let mut available = if tagged.is_empty() {
            available
                .into_iter()
                .filter(|c| c.tags.is_empty())
                .collect()
        } else {
            tagged
        };
        if available.is_empty() {
            return Err(ClientError::Unavailable(
                "No download client was found without tags or a matching movie tag. Please check your settings."
                    .into(),
            ));
        }

        if !blocked.is_empty() {
            let usable: Vec<_> = available
                .iter()
                .copied()
                .filter(|c| !blocked.contains(&c.id))
                .collect();
            if !usable.is_empty() {
                available = usable;
            } else if filter_blocked {
                return Err(ClientError::Unavailable(format!(
                    "All download clients for {protocol:?} are not available"
                )));
            }
        }

        let best = available
            .iter()
            .map(|c| c.priority)
            .min()
            .unwrap_or_default();
        let mut candidates: Vec<_> = available
            .into_iter()
            .filter(|c| c.priority == best)
            .collect();
        candidates.sort_by_key(|c| c.id);

        let mut last_used = self.last_used.lock().unwrap();
        let last = last_used.get(&protocol).copied().unwrap_or(0);
        let chosen = candidates
            .iter()
            .find(|c| c.id > last)
            .or_else(|| candidates.first())
            .copied()
            .expect("candidates is not empty");
        last_used.insert(protocol, chosen.id);
        Ok(Arc::clone(chosen))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeClient;

    fn client(
        id: i64,
        protocol: DownloadProtocol,
        priority: i32,
        tags: &[i32],
    ) -> RegisteredClient {
        RegisteredClient {
            id,
            name: format!("client-{id}"),
            priority,
            tags: tags.to_vec(),
            client: Arc::new(FakeClient::new(protocol)),
        }
    }

    use DownloadProtocol::{Torrent, Usenet};

    fn pick(
        r: &ClientRegistry,
        p: DownloadProtocol,
        tags: &[i32],
        blocked: &[i64],
    ) -> Result<i64, ClientError> {
        r.select(p, tags, false, &blocked.iter().copied().collect())
            .map(|c| c.id)
    }

    #[test]
    fn no_client_for_a_protocol_is_unavailable() {
        let r = ClientRegistry::new(vec![client(1, Usenet, 1, &[])]);
        assert!(matches!(
            pick(&r, Torrent, &[], &[]),
            Err(ClientError::Unavailable(_))
        ));
        assert!(matches!(
            ClientRegistry::default().select(Usenet, &[], false, &HashSet::new()),
            Err(ClientError::Unavailable(_))
        ));
    }

    #[test]
    fn tagged_clients_need_a_matching_tag() {
        let r = ClientRegistry::new(vec![client(1, Usenet, 1, &[7]), client(2, Usenet, 1, &[])]);
        assert_eq!(pick(&r, Usenet, &[7], &[]).unwrap(), 1, "matching tag wins");
        assert_eq!(
            pick(&r, Usenet, &[], &[]).unwrap(),
            2,
            "untagged movie gets the untagged client"
        );
        let only_tagged = ClientRegistry::new(vec![client(1, Usenet, 1, &[7])]);
        assert!(pick(&only_tagged, Usenet, &[], &[]).is_err());
        assert!(pick(&only_tagged, Usenet, &[7], &[]).is_ok());
    }

    #[test]
    fn lowest_priority_number_is_used_and_equals_take_turns() {
        let r = ClientRegistry::new(vec![
            client(1, Usenet, 5, &[]),
            client(2, Usenet, 1, &[]),
            client(3, Usenet, 1, &[]),
        ]);
        assert_eq!(pick(&r, Usenet, &[], &[]).unwrap(), 2);
        assert_eq!(pick(&r, Usenet, &[], &[]).unwrap(), 3);
        assert_eq!(pick(&r, Usenet, &[], &[]).unwrap(), 2, "wraps around");
        // The other protocol keeps its own turn counter.
        let mixed =
            ClientRegistry::new(vec![client(1, Usenet, 1, &[]), client(9, Torrent, 1, &[])]);
        assert_eq!(pick(&mixed, Torrent, &[], &[]).unwrap(), 9);
        assert_eq!(pick(&mixed, Usenet, &[], &[]).unwrap(), 1);
    }

    #[test]
    fn blocked_clients_are_avoided_when_possible() {
        let r = ClientRegistry::new(vec![client(1, Usenet, 1, &[]), client(2, Usenet, 9, &[])]);
        assert_eq!(
            pick(&r, Usenet, &[], &[1]).unwrap(),
            2,
            "falls to the lower-priority healthy client"
        );
        // Everything blocked: use one anyway, unless the caller says not to.
        assert!(pick(&r, Usenet, &[], &[1, 2]).is_ok());
        let strict = r.select(Usenet, &[], true, &[1, 2].into_iter().collect());
        assert!(matches!(strict, Err(ClientError::Unavailable(_))));
    }

    #[test]
    fn lookup_by_id() {
        let r = ClientRegistry::new(vec![client(4, Usenet, 1, &[])]);
        assert!(r.get(4).is_some());
        assert!(r.get(5).is_none());
        assert_eq!(r.all().len(), 1);
    }
}
