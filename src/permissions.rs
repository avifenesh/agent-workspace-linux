use crate::policy::{AppliedWorkspacePolicy, MountMode, NetworkMode, NetworkPolicy, ProfileMount};
use crate::profile::WorkspaceProfile;
use crate::workspace::{LaunchSpec, WorkspaceStartOptions};
use anyhow::{bail, Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct McpPermissionCeiling {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<NetworkPolicy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mounts: Vec<ProfileMount>,
    #[serde(default)]
    pub apps: AppPermissionCeiling,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct AppPermissionCeiling {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct McpPermissionState {
    pub configured: bool,
    pub restricted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PathBuf>,
    #[serde(default)]
    pub ceiling: McpPermissionCeiling,
    pub message: String,
}

impl Default for McpPermissionState {
    fn default() -> Self {
        Self::from_ceiling(None, McpPermissionCeiling::default())
    }
}

impl McpPermissionState {
    pub fn from_ceiling(source: Option<PathBuf>, ceiling: McpPermissionCeiling) -> Self {
        let configured = source.is_some();
        let ceiling = if configured {
            ceiling
        } else {
            McpPermissionCeiling::default()
        };
        let restricted = ceiling.is_restricted();
        let message = match (configured, restricted) {
            (false, _) => {
                "No MCP permission ceiling is configured; the host/client session controls workspace permissions, including full-access sessions after hidden-workspace approval."
                    .to_string()
            }
            (true, false) => {
                "MCP permission ceiling file is configured but does not restrict network, mounts, or app launches; the host/client session controls open dimensions."
                    .to_string()
            }
            (true, true) => {
                "MCP permission ceiling is active; clients may narrow these permissions but cannot broaden them."
                    .to_string()
            }
        };
        Self {
            configured,
            restricted,
            source,
            ceiling,
            message,
        }
    }

    pub fn validate_profile(&self, profile: &WorkspaceProfile) -> Result<()> {
        self.ceiling.validate_profile(profile)
    }

    pub fn validate_start_options(&self, options: &WorkspaceStartOptions) -> Result<()> {
        if let Some(policy) = &options.applied_policy {
            self.ceiling
                .validate_applied_policy(policy, "workspace start profile")?;
        }
        Ok(())
    }

    pub fn validate_launch_spec(&self, spec: &LaunchSpec) -> Result<()> {
        self.ceiling
            .validate_command(&spec.command, "workspace launch command")?;
        if let Some(policy) = &spec.applied_policy {
            self.ceiling
                .validate_applied_policy(policy, "workspace launch profile")?;
        } else {
            self.ceiling
                .validate_mounts(&[], "workspace launch without profile")?;
            self.ceiling.validate_network(
                &NetworkPolicy::default(),
                "workspace launch without profile",
            )?;
        }
        Ok(())
    }
}

impl McpPermissionCeiling {
    fn is_restricted(&self) -> bool {
        self.network
            .as_ref()
            .is_some_and(|network| !matches!(network.mode, NetworkMode::InheritHost))
            || !self.mounts.is_empty()
            || !self.apps.allow.is_empty()
    }

    fn validate_config(&self) -> Result<()> {
        if let Some(network) = &self.network {
            validate_network_hosts(network, "MCP permission network")?;
        }
        for mount in &self.mounts {
            validate_absolute_path(&mount.host_path, "MCP permission mount host_path")?;
            validate_absolute_path(&mount.workspace_path, "MCP permission mount workspace_path")?;
        }
        for command in &self.apps.allow {
            validate_allowed_command(command)?;
        }
        Ok(())
    }

    fn validate_profile(&self, profile: &WorkspaceProfile) -> Result<()> {
        self.validate_network(&profile.network, "profile network")?;
        self.validate_mounts(&profile.mounts, "profile mounts")?;
        for setup in &profile.setup_commands {
            self.validate_command(&setup.command, "profile setup command")?;
        }
        for app in &profile.startup_apps {
            self.validate_command(&app.command, "profile startup app")?;
        }
        Ok(())
    }

    fn validate_applied_policy(
        &self,
        policy: &AppliedWorkspacePolicy,
        context: &str,
    ) -> Result<()> {
        self.validate_network(&policy.network, context)?;
        self.validate_mounts(&policy.mounts, context)?;
        Ok(())
    }

    fn validate_network(&self, requested: &NetworkPolicy, context: &str) -> Result<()> {
        let Some(ceiling) = &self.network else {
            return Ok(());
        };
        validate_network_hosts(requested, context)?;
        if network_within_ceiling(ceiling, requested) {
            return Ok(());
        }
        bail!(
            "{context} requests network.mode={} with allow_hosts=[{}], which exceeds MCP permission ceiling network.mode={} with allow_hosts=[{}]",
            network_mode_name(&requested.mode),
            requested.allow_hosts.join(", "),
            network_mode_name(&ceiling.mode),
            ceiling.allow_hosts.join(", ")
        );
    }

    fn validate_mounts(&self, requested: &[ProfileMount], context: &str) -> Result<()> {
        if self.mounts.is_empty() {
            return Ok(());
        }
        if requested.is_empty() {
            bail!(
                "{context} does not request a mount policy, but the MCP permission ceiling limits file access"
            );
        }
        for mount in requested {
            validate_absolute_path(&mount.host_path, "requested mount host_path")?;
            validate_absolute_path(&mount.workspace_path, "requested mount workspace_path")?;
            if self
                .mounts
                .iter()
                .any(|allowed| mount_within(allowed, mount))
            {
                continue;
            }
            bail!(
                "{context} requests mount {} -> {} ({}) outside the MCP permission ceiling",
                mount.host_path.display(),
                mount.workspace_path.display(),
                mount_mode_name(&mount.mode)
            );
        }
        Ok(())
    }

    fn validate_command(&self, command: &[String], context: &str) -> Result<()> {
        if self.apps.allow.is_empty() {
            return Ok(());
        }
        let Some(program) = command.first().filter(|program| !program.trim().is_empty()) else {
            bail!("{context} is empty and cannot be matched against MCP app allowlist");
        };
        if self
            .apps
            .allow
            .iter()
            .any(|allowed| command_matches_allow_entry(program, allowed))
        {
            return Ok(());
        }
        let allowed = self
            .apps
            .allow
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        bail!("{context} program {program:?} is not allowed by the MCP app allowlist [{allowed}]");
    }
}

pub fn load_mcp_permission_state(path: PathBuf) -> Result<McpPermissionState> {
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read MCP permissions from {}", path.display()))?;
    let ceiling: McpPermissionCeiling = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse MCP permissions from {}", path.display()))?;
    ceiling.validate_config()?;
    Ok(McpPermissionState::from_ceiling(Some(path), ceiling))
}

pub fn template_permission_ceiling(
    kind: &str,
    allow_hosts: Vec<String>,
    mounts: Vec<ProfileMount>,
    apps: Vec<PathBuf>,
) -> Result<McpPermissionCeiling> {
    let network = match kind {
        "open" => {
            if !allow_hosts.is_empty() {
                bail!("permissions template open does not accept --allow-host");
            }
            None
        }
        "closed" | "disabled" => {
            if !allow_hosts.is_empty() {
                bail!("permissions template closed does not accept --allow-host");
            }
            Some(NetworkPolicy {
                mode: NetworkMode::Disabled,
                allow_hosts: Vec::new(),
            })
        }
        "local" | "local-only" | "local_only" => Some(NetworkPolicy {
            mode: NetworkMode::LocalOnly,
            allow_hosts,
        }),
        unknown => {
            bail!("unknown permissions template {unknown:?}. Expected: open, closed, or local")
        }
    };
    let ceiling = McpPermissionCeiling {
        network,
        mounts,
        apps: AppPermissionCeiling { allow: apps },
    };
    ceiling.validate_config()?;
    Ok(ceiling)
}

fn validate_allowed_command(command: &Path) -> Result<()> {
    if command.as_os_str().is_empty() {
        bail!("MCP app allowlist entries cannot be empty");
    }
    if command.is_absolute() {
        validate_absolute_path(command, "MCP app allowlist entry")?;
        return Ok(());
    }
    let mut components = command.components();
    if matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none() {
        return Ok(());
    }
    bail!(
        "MCP app allowlist entry {} must be an absolute path or a single command name",
        command.display()
    );
}

fn validate_absolute_path(path: &Path, field: &str) -> Result<()> {
    if path.as_os_str().is_empty() {
        bail!("{field} cannot be empty");
    }
    if !path.is_absolute() {
        bail!("{field} {} must be absolute", path.display());
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("{field} {} cannot contain '..'", path.display());
    }
    Ok(())
}

fn validate_network_hosts(network: &NetworkPolicy, context: &str) -> Result<()> {
    if matches!(
        network.mode,
        NetworkMode::LocalOnly | NetworkMode::Allowlist
    ) && network
        .allow_hosts
        .iter()
        .any(|host| host.trim().is_empty())
    {
        bail!("{context} contains an empty network allow_hosts entry");
    }
    Ok(())
}

fn network_within_ceiling(ceiling: &NetworkPolicy, requested: &NetworkPolicy) -> bool {
    match ceiling.mode {
        NetworkMode::InheritHost => true,
        NetworkMode::Disabled => matches!(requested.mode, NetworkMode::Disabled),
        NetworkMode::LocalOnly => {
            matches!(requested.mode, NetworkMode::Disabled)
                || (matches!(requested.mode, NetworkMode::LocalOnly)
                    && host_subset(&requested.allow_hosts, &ceiling.allow_hosts))
        }
        NetworkMode::Allowlist => {
            matches!(
                requested.mode,
                NetworkMode::Disabled | NetworkMode::LocalOnly
            ) || (matches!(requested.mode, NetworkMode::Allowlist)
                && host_subset(&requested.allow_hosts, &ceiling.allow_hosts))
        }
    }
}

fn host_subset(requested: &[String], ceiling: &[String]) -> bool {
    let ceiling = normalized_hosts(ceiling);
    normalized_hosts(requested)
        .iter()
        .all(|host| ceiling.contains(host))
}

fn normalized_hosts(hosts: &[String]) -> BTreeSet<String> {
    hosts
        .iter()
        .map(|host| host.trim().to_ascii_lowercase())
        .filter(|host| !host.is_empty())
        .collect()
}

fn mount_within(allowed: &ProfileMount, requested: &ProfileMount) -> bool {
    path_is_same_or_child(&requested.host_path, &allowed.host_path)
        && path_is_same_or_child(&requested.workspace_path, &allowed.workspace_path)
        && mount_mode_within(&allowed.mode, &requested.mode)
}

fn path_is_same_or_child(child: &Path, parent: &Path) -> bool {
    child == parent || child.starts_with(parent)
}

fn mount_mode_within(allowed: &MountMode, requested: &MountMode) -> bool {
    matches!(allowed, MountMode::ReadWrite) || matches!(requested, MountMode::ReadOnly)
}

fn command_matches_allow_entry(program: &str, allowed: &Path) -> bool {
    let program_path = Path::new(program);
    if allowed.is_absolute() {
        if program_path.is_absolute() && paths_equivalent(program_path, allowed) {
            return true;
        }
        if let Some(resolved) = resolve_command_path(program) {
            return paths_equivalent(&resolved, allowed);
        }
        return false;
    }
    program == allowed.to_string_lossy()
}

fn paths_equivalent(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn resolve_command_path(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        return Some(PathBuf::from(program));
    }
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable_file(candidate))
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn network_mode_name(mode: &NetworkMode) -> &'static str {
    match mode {
        NetworkMode::InheritHost => "inherit_host",
        NetworkMode::Disabled => "disabled",
        NetworkMode::LocalOnly => "local_only",
        NetworkMode::Allowlist => "allowlist",
    }
}

fn mount_mode_name(mode: &MountMode) -> &'static str {
    match mode {
        MountMode::ReadOnly => "read_only",
        MountMode::ReadWrite => "read_write",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{AppliedWorkspacePolicy, PolicyRuntimeCapabilities};
    use crate::workspace::EnvVar;

    fn launch_spec(command: &[&str], policy: Option<AppliedWorkspacePolicy>) -> LaunchSpec {
        LaunchSpec {
            command: command.iter().map(|part| part.to_string()).collect(),
            name: None,
            profile_id: policy.as_ref().map(|policy| policy.profile_id.clone()),
            applied_policy: policy,
            user_acknowledged_unenforced_policy: false,
            cwd: None,
            env: Vec::<EnvVar>::new(),
        }
    }

    fn policy(mounts: Vec<ProfileMount>, network: NetworkPolicy) -> AppliedWorkspacePolicy {
        AppliedWorkspacePolicy::new_with_capabilities(
            "test-profile".to_string(),
            mounts,
            network,
            false,
            0,
            PolicyRuntimeCapabilities::default(),
        )
    }

    fn mount(host_path: &str, workspace_path: &str, mode: MountMode) -> ProfileMount {
        ProfileMount {
            host_path: PathBuf::from(host_path),
            workspace_path: PathBuf::from(workspace_path),
            mode,
        }
    }

    fn configured_state(ceiling: McpPermissionCeiling) -> McpPermissionState {
        McpPermissionState::from_ceiling(Some(PathBuf::from("/tmp/mcp-permissions.json")), ceiling)
    }

    #[test]
    fn permission_templates_cover_open_closed_and_local() {
        let open = template_permission_ceiling("open", Vec::new(), Vec::new(), Vec::new()).unwrap();
        assert!(open.network.is_none());
        assert!(!open.is_restricted());

        let closed = template_permission_ceiling(
            "closed",
            Vec::new(),
            Vec::new(),
            vec![PathBuf::from("sh")],
        )
        .unwrap();
        assert!(matches!(
            closed.network.as_ref().map(|network| &network.mode),
            Some(NetworkMode::Disabled)
        ));
        assert_eq!(closed.apps.allow, vec![PathBuf::from("sh")]);
        assert!(closed.is_restricted());

        let local = template_permission_ceiling(
            "local",
            vec!["localhost:3000".to_string()],
            vec![mount(
                "/home/me/project",
                "/workspace/project",
                MountMode::ReadWrite,
            )],
            Vec::new(),
        )
        .unwrap();
        assert!(matches!(
            local.network.as_ref().map(|network| &network.mode),
            Some(NetworkMode::LocalOnly)
        ));
        assert_eq!(
            local.network.unwrap().allow_hosts,
            vec!["localhost:3000".to_string()]
        );
        assert_eq!(local.mounts.len(), 1);
    }

    #[test]
    fn permission_template_rejects_hosts_for_non_local_modes() {
        let error = template_permission_ceiling(
            "closed",
            vec!["localhost:3000".to_string()],
            Vec::new(),
            Vec::new(),
        )
        .expect_err("closed template should reject allow-host")
        .to_string();
        assert!(error.contains("does not accept --allow-host"));
    }

    #[test]
    fn default_ceiling_allows_unprofiled_launch() {
        let state = McpPermissionState::default();
        state
            .validate_launch_spec(&launch_spec(&["sh", "-c", "true"], None))
            .unwrap();
    }

    #[test]
    fn configured_empty_ceiling_reports_open_and_allows_launch() {
        let ceiling: McpPermissionCeiling = serde_json::from_str("{}").unwrap();
        let state = configured_state(ceiling);

        assert!(state.configured);
        assert!(!state.restricted);
        state
            .validate_launch_spec(&launch_spec(&["bash", "-lc", "true"], None))
            .unwrap();
    }

    #[test]
    fn unconfigured_state_drops_accidental_ceiling() {
        let state = McpPermissionState::from_ceiling(
            None,
            McpPermissionCeiling {
                network: Some(NetworkPolicy {
                    mode: NetworkMode::Disabled,
                    allow_hosts: Vec::new(),
                }),
                mounts: vec![mount(
                    "/home/me/project",
                    "/workspace/project",
                    MountMode::ReadOnly,
                )],
                apps: AppPermissionCeiling {
                    allow: vec![PathBuf::from("xterm")],
                },
            },
        );

        assert!(!state.configured);
        assert!(!state.restricted);
        assert!(state.ceiling.network.is_none());
        assert!(state.ceiling.mounts.is_empty());
        assert!(state.ceiling.apps.allow.is_empty());
        state
            .validate_launch_spec(&launch_spec(&["bash", "-lc", "true"], None))
            .unwrap();
    }

    #[test]
    fn disabled_network_ceiling_rejects_unprofiled_launch() {
        let state = configured_state(McpPermissionCeiling {
            network: Some(NetworkPolicy {
                mode: NetworkMode::Disabled,
                allow_hosts: Vec::new(),
            }),
            mounts: Vec::new(),
            apps: AppPermissionCeiling::default(),
        });

        let error = state
            .validate_launch_spec(&launch_spec(&["curl", "https://example.com"], None))
            .unwrap_err()
            .to_string();
        assert!(error.contains("exceeds MCP permission ceiling"));
    }

    #[test]
    fn disabled_network_ceiling_allows_disabled_profile_launch() {
        let state = configured_state(McpPermissionCeiling {
            network: Some(NetworkPolicy {
                mode: NetworkMode::Disabled,
                allow_hosts: Vec::new(),
            }),
            mounts: Vec::new(),
            apps: AppPermissionCeiling::default(),
        });
        let applied = policy(
            Vec::new(),
            NetworkPolicy {
                mode: NetworkMode::Disabled,
                allow_hosts: Vec::new(),
            },
        );

        state
            .validate_launch_spec(&launch_spec(
                &["curl", "https://example.com"],
                Some(applied),
            ))
            .unwrap();
    }

    #[test]
    fn local_only_ceiling_rejects_inherited_network() {
        let state = configured_state(McpPermissionCeiling {
            network: Some(NetworkPolicy {
                mode: NetworkMode::LocalOnly,
                allow_hosts: vec!["localhost:3000".to_string()],
            }),
            mounts: Vec::new(),
            apps: AppPermissionCeiling::default(),
        });

        let error = state
            .validate_launch_spec(&launch_spec(&["curl", "https://example.com"], None))
            .unwrap_err()
            .to_string();
        assert!(error.contains("network.mode=inherit_host"));
    }

    #[test]
    fn allowlist_ceiling_requires_requested_hosts_to_be_subset() {
        let ceiling = McpPermissionCeiling {
            network: Some(NetworkPolicy {
                mode: NetworkMode::Allowlist,
                allow_hosts: vec!["example.com".to_string()],
            }),
            mounts: Vec::new(),
            apps: AppPermissionCeiling::default(),
        };
        let allowed = NetworkPolicy {
            mode: NetworkMode::Allowlist,
            allow_hosts: vec!["EXAMPLE.com".to_string()],
        };
        let blocked = NetworkPolicy {
            mode: NetworkMode::Allowlist,
            allow_hosts: vec!["example.org".to_string()],
        };

        ceiling.validate_network(&allowed, "test").unwrap();
        assert!(ceiling.validate_network(&blocked, "test").is_err());
    }

    #[test]
    fn mount_ceiling_rejects_missing_or_broader_mount_policy() {
        let state = configured_state(McpPermissionCeiling {
            network: None,
            mounts: vec![mount(
                "/home/me/project",
                "/workspace/project",
                MountMode::ReadOnly,
            )],
            apps: AppPermissionCeiling::default(),
        });

        assert!(state
            .validate_launch_spec(&launch_spec(&["ls"], None))
            .unwrap_err()
            .to_string()
            .contains("does not request a mount policy"));

        let broader = policy(
            vec![mount("/home/me", "/workspace", MountMode::ReadOnly)],
            NetworkPolicy::default(),
        );
        assert!(state
            .validate_launch_spec(&launch_spec(&["ls"], Some(broader)))
            .is_err());
    }

    #[test]
    fn mount_ceiling_allows_child_read_only_mount() {
        let state = configured_state(McpPermissionCeiling {
            network: None,
            mounts: vec![mount(
                "/home/me/project",
                "/workspace/project",
                MountMode::ReadOnly,
            )],
            apps: AppPermissionCeiling::default(),
        });
        let narrowed = policy(
            vec![mount(
                "/home/me/project/src",
                "/workspace/project/src",
                MountMode::ReadOnly,
            )],
            NetworkPolicy::default(),
        );

        state
            .validate_launch_spec(&launch_spec(&["ls"], Some(narrowed)))
            .unwrap();
    }

    #[test]
    fn mount_ceiling_rejects_read_write_when_ceiling_is_read_only() {
        let ceiling = McpPermissionCeiling {
            network: None,
            mounts: vec![mount(
                "/home/me/project",
                "/workspace/project",
                MountMode::ReadOnly,
            )],
            apps: AppPermissionCeiling::default(),
        };
        let requested = vec![mount(
            "/home/me/project",
            "/workspace/project",
            MountMode::ReadWrite,
        )];

        assert!(ceiling.validate_mounts(&requested, "test").is_err());
    }

    #[test]
    fn app_allowlist_limits_launch_programs() {
        let state = configured_state(McpPermissionCeiling {
            network: None,
            mounts: Vec::new(),
            apps: AppPermissionCeiling {
                allow: vec![PathBuf::from("xterm")],
            },
        });

        state
            .validate_launch_spec(&launch_spec(&["xterm"], None))
            .unwrap();
        assert!(state
            .validate_launch_spec(&launch_spec(&["bash"], None))
            .is_err());
    }
}
