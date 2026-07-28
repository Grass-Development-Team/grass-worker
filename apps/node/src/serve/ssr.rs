//! SSR service lifecycle.
//!
//! SSR deployments run their server bundle inside a service container
//! (never raw on the host). Services start lazily on the first request for
//! their deployment, are adopted across node restarts through their
//! deterministic container name, and stop again after the configured idle
//! period. The serve path reaches a service by its container IP on the
//! shared runtime network.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use grass_node_protocol::ServeResources;
use tokio::sync::{Mutex, mpsc};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    config::NodeConfig,
    output::manifest::ServerSection,
    runtime::{BuildRuntime, ContainerRuntime, PrepareImageInput, RunServiceInput},
};

/// Fixed port SSR servers listen on inside their container; the manager
/// injects it through the manifest's `port_env` plus the common variables.
pub const SSR_CONTAINER_PORT: u16 = 8321;
const REAPER_INTERVAL: Duration = Duration::from_secs(60);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(300);
const SSR_CONTAINER_PREFIX: &str = "grass-ssr-";
const SSR_NODE_LABEL: &str = "dev.cxcs.grass.node-id";
const SSR_DEPLOYMENT_LABEL: &str = "dev.cxcs.grass.deployment-id";

struct ServiceEntry {
    upstream: String,
    container_name: String,
    last_used: Instant,
}

pub struct SsrManager {
    runtime: Option<Arc<BuildRuntime>>,
    node_id: Uuid,
    image: String,
    network: String,
    idle_stop: Duration,
    startup_timeout: Duration,
    services: Mutex<HashMap<Uuid, ServiceEntry>>,
    /// Per-deployment start locks so concurrent first requests start one
    /// container, not many.
    start_locks: Mutex<HashMap<Uuid, Arc<Mutex<()>>>>,
}

fn container_name(node_id: Uuid, deployment_id: Uuid) -> String {
    format!(
        "{SSR_CONTAINER_PREFIX}{}-{}",
        node_id.simple(),
        deployment_id.simple()
    )
}

#[cfg(test)]
fn legacy_container_name(deployment_id: Uuid) -> String {
    format!("{SSR_CONTAINER_PREFIX}{}", deployment_id.simple())
}

fn container_deployment_id(name: &str) -> Option<Uuid> {
    let id = name
        .strip_prefix(SSR_CONTAINER_PREFIX)?
        .rsplit('-')
        .next()?;
    (id.len() == 32).then(|| Uuid::parse_str(id).ok()).flatten()
}

/// Environment for an SSR server: the manifest-declared variables plus the
/// common names the supported frameworks read (PORT/HOST/HOSTNAME/NITRO_*).
fn service_env(server: &ServerSection, port: u16) -> Vec<(String, String)> {
    let port_value = port.to_string();
    let host_value = "0.0.0.0".to_owned();
    let mut env: Vec<(String, String)> = vec![
        ("NODE_ENV".to_owned(), "production".to_owned()),
        ("PORT".to_owned(), port_value.clone()),
        ("HOST".to_owned(), host_value.clone()),
        ("HOSTNAME".to_owned(), host_value.clone()),
        ("NITRO_PORT".to_owned(), port_value.clone()),
        ("NITRO_HOST".to_owned(), host_value.clone()),
    ];
    for (name, value) in [
        (server.port_env.trim(), port_value),
        (server.host_env.trim(), host_value),
    ] {
        if !name.is_empty() && !env.iter().any(|(existing, _)| existing == name) {
            env.push((name.to_owned(), value));
        }
    }
    env
}

impl SsrManager {
    pub fn new(runtime: Option<Arc<BuildRuntime>>, node_id: Uuid, config: &NodeConfig) -> Self {
        Self {
            runtime,
            node_id,
            image: config.runtime.default_serve_image.clone(),
            network: config.runtime.network.clone(),
            idle_stop: Duration::from_secs(config.serve.ssr.idle_stop_seconds),
            startup_timeout: Duration::from_secs(config.serve.ssr.startup_timeout_seconds.max(5)),
            services: Mutex::new(HashMap::new()),
            start_locks: Mutex::new(HashMap::new()),
        }
    }

    fn service_input(
        &self,
        deployment_id: Uuid,
        app_dir: std::path::PathBuf,
        start_command: String,
        server: &ServerSection,
        resources: ServeResources,
    ) -> RunServiceInput {
        RunServiceInput {
            name: container_name(self.node_id, deployment_id),
            image: self.image.clone(),
            app_dir,
            start_command,
            env: service_env(server, SSR_CONTAINER_PORT),
            container_port: SSR_CONTAINER_PORT,
            cpu_millicores: resources.cpu_millicores,
            memory_mb: resources.memory_mb,
            network: self.network.clone(),
            labels: HashMap::from([
                (SSR_NODE_LABEL.to_owned(), self.node_id.to_string()),
                (SSR_DEPLOYMENT_LABEL.to_owned(), deployment_id.to_string()),
            ]),
        }
    }

    /// Returns the upstream address for a deployment's SSR service,
    /// starting the service container when necessary.
    pub async fn upstream_for(
        &self,
        deployment_id: Uuid,
        deployment_dir: &Path,
        server: &ServerSection,
        resources: ServeResources,
    ) -> anyhow::Result<String> {
        if let Some(upstream) = self.known_upstream(deployment_id).await {
            return Ok(upstream);
        }

        let start_lock = {
            let mut locks = self.start_locks.lock().await;
            locks
                .entry(deployment_id)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _guard = start_lock.lock().await;

        // Another request may have started the service while we waited.
        if let Some(upstream) = self.known_upstream(deployment_id).await {
            return Ok(upstream);
        }

        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("container runtime is unavailable on this node"))?;

        let app_dir = deployment_dir.to_path_buf();
        if !app_dir.join(&server.entry).is_file() {
            anyhow::bail!("server entry {} is missing from the artifact", server.entry);
        }
        let start_command = if server.start_command.trim().is_empty() {
            format!("node {}", server.entry)
        } else {
            server.start_command.clone()
        };

        // Pull the serve image on first use; discard pull progress lines.
        let (pull_tx, mut pull_rx) = mpsc::channel::<String>(8);
        let drain = tokio::spawn(async move { while pull_rx.recv().await.is_some() {} });
        runtime
            .prepare_image(PrepareImageInput { image: &self.image }, pull_tx)
            .await
            .map_err(|error| anyhow::anyhow!("serve image unavailable: {error}"))?;
        let _ = drain.await;

        let name = container_name(self.node_id, deployment_id);
        let running = runtime
            .run_service(self.service_input(
                deployment_id,
                app_dir,
                start_command,
                server,
                resources,
            ))
            .await
            .map_err(|error| anyhow::anyhow!("ssr service start failed: {error}"))?;

        if let Err(error) = wait_ready(&running.upstream, self.startup_timeout).await {
            // Remove the container so the next request starts fresh instead
            // of proxying into a wedged process.
            let _ = runtime.stop_service(&name).await;
            return Err(error);
        }

        info!(
            operation = "node.ssr.started",
            deployment_id = %deployment_id,
            upstream = %running.upstream,
            "ssr service ready"
        );
        self.services.lock().await.insert(
            deployment_id,
            ServiceEntry {
                upstream: running.upstream.clone(),
                container_name: name,
                last_used: Instant::now(),
            },
        );
        Ok(running.upstream)
    }

    async fn known_upstream(&self, deployment_id: Uuid) -> Option<String> {
        let mut services = self.services.lock().await;
        let entry = services.get_mut(&deployment_id)?;
        entry.last_used = Instant::now();
        Some(entry.upstream.clone())
    }

    /// Drops a service from the registry (and stops its container) after a
    /// proxy connection failure, so the next request restarts it.
    pub async fn invalidate(&self, deployment_id: Uuid) {
        let removed = self.services.lock().await.remove(&deployment_id);
        if let (Some(entry), Some(runtime)) = (removed, self.runtime.as_ref()) {
            warn!(
                operation = "node.ssr.invalidated",
                deployment_id = %deployment_id,
                "ssr service unreachable; stopping its container"
            );
            let _ = runtime.stop_service(&entry.container_name).await;
        }
    }

    /// Reconciles both services known to this process and deterministic
    /// containers left behind while the Node was offline.
    pub async fn reconcile_routes(
        &self,
        routed_here: &HashSet<Uuid>,
        routed_anywhere: &HashSet<Uuid>,
    ) -> anyhow::Result<()> {
        let Some(runtime) = self.runtime.as_ref() else {
            return Ok(());
        };
        let stale_start_locks = {
            let locks = self.start_locks.lock().await;
            locks
                .iter()
                .filter(|(id, _)| !routed_here.contains(id))
                .map(|(id, lock)| (*id, lock.clone()))
                .collect::<Vec<_>>()
        };
        let mut start_guards = Vec::with_capacity(stale_start_locks.len());
        for (_, lock) in &stale_start_locks {
            start_guards.push(lock.clone().lock_owned().await);
        }

        {
            let mut services = self.services.lock().await;
            let ids = services
                .keys()
                .filter(|id| !routed_here.contains(id))
                .copied()
                .collect::<Vec<_>>();
            for id in ids {
                services.remove(&id);
            }
        }

        let node_id = self.node_id.to_string();
        let mut stale = HashSet::new();
        for service in runtime.list_services(SSR_CONTAINER_PREFIX).await? {
            let Some(deployment_id) = container_deployment_id(&service.name) else {
                continue;
            };
            match service.labels.get(SSR_NODE_LABEL) {
                Some(owner) if owner == &node_id && !routed_here.contains(&deployment_id) => {
                    stale.insert(service.name);
                }
                None if !routed_anywhere.contains(&deployment_id) => {
                    stale.insert(service.name);
                }
                _ => {}
            }
        }
        for name in stale {
            runtime.stop_service(&name).await?;
        }
        let mut locks = self.start_locks.lock().await;
        for (id, original) in stale_start_locks {
            if locks
                .get(&id)
                .is_some_and(|current| Arc::ptr_eq(current, &original))
            {
                locks.remove(&id);
            }
        }
        drop(locks);
        drop(start_guards);
        Ok(())
    }

    /// Periodically stops services that have been idle for the configured
    /// period. A zero idle timeout disables the reaper.
    pub fn spawn_reaper(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            if self.idle_stop.is_zero() {
                return;
            }
            let mut interval = tokio::time::interval(REAPER_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                let idle: Vec<(Uuid, String)> = {
                    let mut services = self.services.lock().await;
                    let expired: Vec<Uuid> = services
                        .iter()
                        .filter(|(_, entry)| entry.last_used.elapsed() >= self.idle_stop)
                        .map(|(id, _)| *id)
                        .collect();
                    expired
                        .into_iter()
                        .filter_map(|id| {
                            services.remove(&id).map(|entry| (id, entry.container_name))
                        })
                        .collect()
                };
                let Some(runtime) = self.runtime.as_ref() else {
                    continue;
                };
                for (deployment_id, name) in idle {
                    info!(
                        operation = "node.ssr.idle_stop",
                        deployment_id = %deployment_id,
                        "stopping idle ssr service"
                    );
                    let _ = runtime.stop_service(&name).await;
                }
            }
        })
    }
}

/// Waits until the upstream accepts TCP connections.
async fn wait_ready(upstream: &str, timeout: Duration) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        match tokio::net::TcpStream::connect(upstream).await {
            Ok(_) => return Ok(()),
            Err(error) => {
                if Instant::now() >= deadline {
                    anyhow::bail!(
                        "ssr service did not accept connections within {}s: {error}",
                        timeout.as_secs()
                    );
                }
                tokio::time::sleep(READY_POLL_INTERVAL).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grass_node_protocol::ServeResources;

    fn server_section(port_env: &str, host_env: &str) -> ServerSection {
        ServerSection {
            entry: "server/index.mjs".to_owned(),
            start_command: String::new(),
            port_env: port_env.to_owned(),
            host_env: host_env.to_owned(),
        }
    }

    async fn runtime_service_names(runtime: &BuildRuntime) -> Vec<String> {
        runtime
            .list_services(SSR_CONTAINER_PREFIX)
            .await
            .unwrap()
            .into_iter()
            .map(|service| service.name)
            .collect()
    }

    #[test]
    fn service_env_covers_manifest_and_common_variables() {
        let env = service_env(&server_section("NITRO_PORT", "NITRO_HOST"), 8321);
        let get = |name: &str| {
            env.iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(get("PORT"), Some("8321"));
        assert_eq!(get("HOSTNAME"), Some("0.0.0.0"));
        assert_eq!(get("NITRO_PORT"), Some("8321"));
        assert_eq!(get("NODE_ENV"), Some("production"));
        // No duplicate keys even when the manifest names a common variable.
        let ports = env.iter().filter(|(key, _)| key == "NITRO_PORT").count();
        assert_eq!(ports, 1);
    }

    #[test]
    fn container_names_are_deterministic_per_node_and_deployment() {
        let node_id = Uuid::now_v7();
        let deployment_id = Uuid::now_v7();
        assert_eq!(
            container_name(node_id, deployment_id),
            container_name(node_id, deployment_id)
        );
        assert!(container_name(node_id, deployment_id).starts_with("grass-ssr-"));
        assert_eq!(
            container_deployment_id(&container_name(node_id, deployment_id)),
            Some(deployment_id)
        );
        assert_eq!(
            container_deployment_id(&legacy_container_name(deployment_id)),
            Some(deployment_id)
        );
    }

    #[test]
    fn service_input_uses_assigned_serve_resources() {
        let mut config = NodeConfig::default();
        config.runtime.resources.cpu_limit = 4;
        config.runtime.resources.memory_mb = 4_096;
        let node_id = Uuid::now_v7();
        let deployment_id = Uuid::now_v7();
        let manager = SsrManager::new(None, node_id, &config);

        let input = manager.service_input(
            deployment_id,
            Path::new("/tmp/app").to_path_buf(),
            "node server.mjs".to_owned(),
            &server_section("PORT", "HOST"),
            ServeResources {
                cpu_millicores: 200,
                memory_mb: 256,
                disk_mb: 512,
            },
        );

        assert_eq!(input.cpu_millicores, 200);
        assert_eq!(input.memory_mb, 256);
        assert_eq!(input.labels[SSR_NODE_LABEL], node_id.to_string());
        assert_eq!(
            input.labels[SSR_DEPLOYMENT_LABEL],
            deployment_id.to_string()
        );
    }

    #[test]
    fn service_container_names_do_not_collide_between_nodes() {
        let config = NodeConfig::default();
        let deployment_id = Uuid::now_v7();
        let manager_a = SsrManager::new(None, Uuid::now_v7(), &config);
        let manager_b = SsrManager::new(None, Uuid::now_v7(), &config);
        let resources = ServeResources {
            cpu_millicores: 200,
            memory_mb: 256,
            disk_mb: 512,
        };
        let input_a = manager_a.service_input(
            deployment_id,
            Path::new("/tmp/app").to_path_buf(),
            "node server.mjs".to_owned(),
            &server_section("PORT", "HOST"),
            resources,
        );
        let input_b = manager_b.service_input(
            deployment_id,
            Path::new("/tmp/app").to_path_buf(),
            "node server.mjs".to_owned(),
            &server_section("PORT", "HOST"),
            resources,
        );

        assert_ne!(input_a.name, input_b.name);
    }

    #[tokio::test]
    async fn wait_ready_times_out_against_a_closed_port() {
        // Port 9 (discard) is almost certainly closed; use an unroutable
        // loopback port instead of relying on external state.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let result = wait_ready(&addr.to_string(), Duration::from_millis(400)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn wait_ready_succeeds_once_the_port_listens() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let accept = tokio::spawn(async move {
            let _ = listener.accept().await;
        });
        wait_ready(&addr.to_string(), Duration::from_secs(5))
            .await
            .unwrap();
        accept.abort();
    }

    #[tokio::test]
    async fn route_reconciliation_stops_services_retired_while_node_was_offline() {
        let runtime = Arc::new(BuildRuntime::Fake(crate::runtime::FakeRuntime::default()));
        let config = NodeConfig::default();
        let node_id = Uuid::now_v7();
        let manager = SsrManager::new(Some(runtime.clone()), node_id, &config);
        let retained = Uuid::now_v7();
        let retired = Uuid::now_v7();
        let resources = ServeResources {
            cpu_millicores: 200,
            memory_mb: 256,
            disk_mb: 512,
        };

        for deployment_id in [retained, retired] {
            runtime
                .run_service(manager.service_input(
                    deployment_id,
                    Path::new("/tmp/app").to_path_buf(),
                    "node server.mjs".to_owned(),
                    &server_section("PORT", "HOST"),
                    resources,
                ))
                .await
                .unwrap();
        }

        manager
            .reconcile_routes(
                &std::collections::HashSet::from([retained]),
                &std::collections::HashSet::from([retained]),
            )
            .await
            .unwrap();

        assert_eq!(
            runtime_service_names(&runtime).await,
            vec![container_name(node_id, retained)]
        );
    }

    #[tokio::test]
    async fn reconciliation_does_not_stop_another_node_service_on_a_shared_runtime() {
        let runtime = Arc::new(BuildRuntime::Fake(crate::runtime::FakeRuntime::default()));
        let config = NodeConfig::default();
        let node_a = Uuid::now_v7();
        let node_b = Uuid::now_v7();
        let manager_a = SsrManager::new(Some(runtime.clone()), node_a, &config);
        let manager_b = SsrManager::new(Some(runtime.clone()), node_b, &config);
        let deployment_a = Uuid::now_v7();
        let deployment_b = Uuid::now_v7();
        let resources = ServeResources {
            cpu_millicores: 200,
            memory_mb: 256,
            disk_mb: 512,
        };

        for (manager, deployment_id) in [(&manager_a, deployment_a), (&manager_b, deployment_b)] {
            runtime
                .run_service(manager.service_input(
                    deployment_id,
                    Path::new("/tmp/app").to_path_buf(),
                    "node server.mjs".to_owned(),
                    &server_section("PORT", "HOST"),
                    resources,
                ))
                .await
                .unwrap();
        }

        manager_a
            .reconcile_routes(
                &std::collections::HashSet::from([deployment_a]),
                &std::collections::HashSet::from([deployment_a, deployment_b]),
            )
            .await
            .unwrap();

        assert_eq!(
            runtime_service_names(&runtime).await,
            vec![
                container_name(node_a, deployment_a),
                container_name(node_b, deployment_b)
            ]
        );
    }

    #[tokio::test]
    async fn legacy_unowned_service_is_removed_only_after_leaving_all_routes() {
        let runtime = Arc::new(BuildRuntime::Fake(crate::runtime::FakeRuntime::default()));
        let config = NodeConfig::default();
        let manager = SsrManager::new(Some(runtime.clone()), Uuid::now_v7(), &config);
        let deployment_id = Uuid::now_v7();
        let mut input = manager.service_input(
            deployment_id,
            Path::new("/tmp/app").to_path_buf(),
            "node server.mjs".to_owned(),
            &server_section("PORT", "HOST"),
            ServeResources {
                cpu_millicores: 200,
                memory_mb: 256,
                disk_mb: 512,
            },
        );
        input.name = legacy_container_name(deployment_id);
        input.labels.clear();
        runtime.run_service(input).await.unwrap();

        manager
            .reconcile_routes(
                &std::collections::HashSet::new(),
                &std::collections::HashSet::from([deployment_id]),
            )
            .await
            .unwrap();
        assert_eq!(
            runtime_service_names(&runtime).await,
            vec![legacy_container_name(deployment_id)]
        );

        manager
            .reconcile_routes(
                &std::collections::HashSet::new(),
                &std::collections::HashSet::new(),
            )
            .await
            .unwrap();
        assert!(runtime_service_names(&runtime).await.is_empty());
    }

    #[tokio::test]
    async fn reconciliation_waits_for_an_in_progress_service_start() {
        let runtime = Arc::new(BuildRuntime::Fake(crate::runtime::FakeRuntime::default()));
        let config = NodeConfig::default();
        let manager = Arc::new(SsrManager::new(
            Some(runtime.clone()),
            Uuid::now_v7(),
            &config,
        ));
        let deployment_id = Uuid::now_v7();
        let start_lock = Arc::new(Mutex::new(()));
        manager
            .start_locks
            .lock()
            .await
            .insert(deployment_id, start_lock.clone());
        let guard = start_lock.lock().await;

        let mut reconcile = {
            let manager = manager.clone();
            tokio::spawn(async move {
                manager
                    .reconcile_routes(
                        &std::collections::HashSet::new(),
                        &std::collections::HashSet::new(),
                    )
                    .await
            })
        };
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut reconcile)
                .await
                .is_err()
        );

        drop(guard);
        reconcile.await.unwrap().unwrap();
        assert!(
            !manager
                .start_locks
                .lock()
                .await
                .contains_key(&deployment_id)
        );
    }
}
