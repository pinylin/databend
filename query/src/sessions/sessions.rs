// Copyright 2021 Datafuse Labs.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::collections::hash_map::Entry::Occupied;
use std::collections::hash_map::Entry::Vacant;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use common_base::tokio;
use common_base::SignalStream;
use common_exception::ErrorCode;
use common_exception::Result;
use common_infallible::RwLock;
use common_metrics::label_counter;
use common_tracing::tracing;
use futures::future::Either;
use futures::StreamExt;

use crate::catalogs::DatabaseCatalog;
use crate::clusters::ClusterDiscovery;
use crate::configs::Config;
use crate::servers::http::v1::HttpQueryManager;
use crate::sessions::session::Session;
use crate::sessions::session_ref::SessionRef;
use crate::users::UserApiProvider;

pub struct SessionManager {
    pub(in crate::sessions) conf: Config,
    pub(in crate::sessions) discovery: Arc<ClusterDiscovery>,
    pub(in crate::sessions) catalog: Arc<DatabaseCatalog>,
    pub(in crate::sessions) user: Arc<UserApiProvider>,
    pub(in crate::sessions) http_query_manager: Arc<HttpQueryManager>,

    pub(in crate::sessions) max_sessions: usize,
    pub(in crate::sessions) active_sessions: Arc<RwLock<HashMap<String, Arc<Session>>>>,
}

impl SessionManager {
    pub async fn from_conf(conf: Config) -> Result<Arc<SessionManager>> {
        let catalog = Arc::new(DatabaseCatalog::try_create_with_config(conf.clone()).await?);

        // Cluster discovery.
        let discovery = ClusterDiscovery::create_global(conf.clone()).await?;

        // User manager and init the default users.
        let user = UserApiProvider::create_global(conf.clone()).await?;

        let http_query_manager = HttpQueryManager::create_global(conf.clone()).await?;

        let max_active_sessions = conf.query.max_active_sessions as usize;
        Ok(Arc::new(SessionManager {
            catalog,
            conf,
            discovery,
            user,
            http_query_manager,
            max_sessions: max_active_sessions,
            active_sessions: Arc::new(RwLock::new(HashMap::with_capacity(max_active_sessions))),
        }))
    }

    pub fn get_conf(&self) -> &Config {
        &self.conf
    }

    pub fn get_cluster_discovery(self: &Arc<Self>) -> Arc<ClusterDiscovery> {
        self.discovery.clone()
    }

    pub fn get_http_query_manager(self: &Arc<Self>) -> Arc<HttpQueryManager> {
        self.http_query_manager.clone()
    }

    // Get the user api provider.
    pub fn get_user_manager(self: &Arc<Self>) -> Arc<UserApiProvider> {
        self.user.clone()
    }

    pub fn get_catalog(self: &Arc<Self>) -> Arc<DatabaseCatalog> {
        self.catalog.clone()
    }

    pub fn create_session(self: &Arc<Self>, typ: impl Into<String>) -> Result<SessionRef> {
        let mut sessions = self.active_sessions.write();
        match sessions.len() == self.max_sessions {
            true => Err(ErrorCode::TooManyUserConnections(
                "The current accept connection has exceeded mysql_handler_thread_num config",
            )),
            false => {
                let session = Session::try_create(
                    self.conf.clone(),
                    uuid::Uuid::new_v4().to_string(),
                    typ.into(),
                    self.clone(),
                )?;

                label_counter(
                    super::metrics::METRIC_SESSION_CONNECT_NUMBERS,
                    &self.conf.query.tenant_id,
                    &self.conf.query.cluster_id,
                );

                sessions.insert(session.get_id(), session.clone());
                Ok(SessionRef::create(session))
            }
        }
    }

    pub fn create_rpc_session(self: &Arc<Self>, id: String, aborted: bool) -> Result<SessionRef> {
        let mut sessions = self.active_sessions.write();

        let session = match sessions.entry(id) {
            Occupied(entry) => entry.get().clone(),
            Vacant(_) if aborted => return Err(ErrorCode::AbortedSession("Aborting server.")),
            Vacant(entry) => {
                let session = Session::try_create(
                    self.conf.clone(),
                    entry.key().clone(),
                    String::from("RPCSession"),
                    self.clone(),
                )?;

                label_counter(
                    super::metrics::METRIC_SESSION_CONNECT_NUMBERS,
                    &self.conf.query.tenant_id,
                    &self.conf.query.cluster_id,
                );

                entry.insert(session).clone()
            }
        };

        Ok(SessionRef::create(session))
    }

    #[allow(clippy::ptr_arg)]
    pub fn get_session(self: &Arc<Self>, id: &String) -> Option<SessionRef> {
        let sessions = self.active_sessions.read();
        sessions
            .get(id)
            .map(|session| SessionRef::create(session.clone()))
    }

    #[allow(clippy::ptr_arg)]
    pub fn destroy_session(self: &Arc<Self>, session_id: &String) {
        label_counter(
            super::metrics::METRIC_SESSION_CLOSE_NUMBERS,
            &self.conf.query.tenant_id,
            &self.conf.query.cluster_id,
        );

        self.active_sessions.write().remove(session_id);
    }

    pub fn graceful_shutdown(
        self: &Arc<Self>,
        mut signal: SignalStream,
        timeout_secs: i32,
    ) -> impl Future<Output = ()> {
        let active_sessions = self.active_sessions.clone();
        async move {
            tracing::info!(
                "Waiting {} secs for connections to close. You can press Ctrl + C again to force shutdown.",
                timeout_secs);
            let mut signal = Box::pin(signal.next());

            for _index in 0..timeout_secs {
                if SessionManager::destroy_idle_sessions(&active_sessions) {
                    return;
                }

                let interval = Duration::from_secs(1);
                let sleep = Box::pin(tokio::time::sleep(interval));
                match futures::future::select(sleep, signal).await {
                    Either::Right((_, _)) => break,
                    Either::Left((_, reserve_signal)) => signal = reserve_signal,
                };
            }

            tracing::info!("Will shutdown forcefully.");
            active_sessions
                .read()
                .values()
                .for_each(Session::force_kill_session);
        }
    }

    fn destroy_idle_sessions(sessions: &Arc<RwLock<HashMap<String, Arc<Session>>>>) -> bool {
        // Read lock does not support reentrant
        // https://github.com/Amanieu/parking_lot/blob/lock_api-0.4.4/lock_api/src/rwlock.rs#L422
        let active_sessions_read_guard = sessions.read();

        // First try to kill the idle session
        active_sessions_read_guard.values().for_each(Session::kill);
        let active_sessions = active_sessions_read_guard.len();

        match active_sessions {
            0 => true,
            _ => {
                tracing::info!("Waiting for {} connections to close.", active_sessions);
                false
            }
        }
    }
}
