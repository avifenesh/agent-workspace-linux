use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProfileMount {
    pub host_path: PathBuf,
    pub workspace_path: PathBuf,
    #[serde(default)]
    pub mode: MountMode,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MountMode {
    #[default]
    ReadOnly,
    ReadWrite,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NetworkPolicy {
    #[serde(default)]
    pub mode: NetworkMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow_hosts: Vec<String>,
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        Self {
            mode: NetworkMode::InheritHost,
            allow_hosts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NetworkMode {
    #[default]
    InheritHost,
    Disabled,
    LocalOnly,
}

fn local_only_network_label(network: &NetworkPolicy) -> String {
    if network.allow_hosts.is_empty() {
        "sandbox loopback".to_string()
    } else {
        format!("sandbox loopback ({})", network.allow_hosts.join(", "))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PolicyCapabilityState {
    NotRequested,
    Enforced,
    Unenforced,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AppliedWorkspacePolicy {
    pub profile_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mounts: Vec<ProfileMount>,
    #[serde(default)]
    pub network: NetworkPolicy,
    #[serde(default)]
    pub require_full_enforcement: bool,
    pub setup_command_count: usize,
    #[serde(default)]
    pub runtime_capabilities: PolicyRuntimeCapabilities,
    pub enforcement: PolicyEnforcement,
}

impl AppliedWorkspacePolicy {
    pub fn new_with_capabilities(
        profile_id: String,
        mounts: Vec<ProfileMount>,
        network: NetworkPolicy,
        require_full_enforcement: bool,
        setup_command_count: usize,
        runtime_capabilities: PolicyRuntimeCapabilities,
    ) -> Self {
        let mount_policy_requested = !mounts.is_empty();
        let restricted_network_requested = !matches!(network.mode, NetworkMode::InheritHost);
        let mounts_enforced = mount_policy_requested && runtime_capabilities.bubblewrap.ok;
        let mount_detail = if mounts_enforced {
            "mounts are enforced with a bubblewrap mount namespace for launched apps".to_string()
        } else if mount_policy_requested {
            "mounts are declared but bubblewrap is not available to enforce a mount namespace"
                .to_string()
        } else {
            "no mount policy requested".to_string()
        };
        let mut mount_status = PolicyCapabilityStatus::new(
            mount_policy_requested,
            !mount_policy_requested || mounts_enforced,
            mount_detail,
        );
        if mounts_enforced {
            mount_status.backend = Some("bubblewrap_mount_namespace".to_string());
        } else if mount_policy_requested {
            mount_status.planned_backend = Some("bubblewrap_mount_namespace".to_string());
            mount_status
                .backend_requirements
                .push("bubblewrap".to_string());
            mount_status.limitations.push(
                "mount requests are recorded in the profile but launched apps keep the host filesystem view"
                    .to_string(),
            );
        }
        let network_enforced = match network.mode {
            NetworkMode::InheritHost => true,
            NetworkMode::Disabled => runtime_capabilities.bubblewrap.ok,
            NetworkMode::LocalOnly => runtime_capabilities.bubblewrap.ok,
        };
        let network_detail = match network.mode {
            NetworkMode::InheritHost => "profile inherits the host network by policy".to_string(),
            NetworkMode::Disabled if network_enforced => {
                "network disabled is enforced with bubblewrap --unshare-net for launched apps"
                    .to_string()
            }
            NetworkMode::Disabled => {
                "network disabled is declared but bubblewrap is not available to enforce network isolation"
                    .to_string()
            }
            NetworkMode::LocalOnly if network_enforced => {
                format!(
                    "local-only network is enforced with bubblewrap --unshare-net; {} is available",
                    local_only_network_label(&network)
                )
            }
            NetworkMode::LocalOnly => {
                format!(
                    "local-only network is declared for {} but bubblewrap is not available",
                    local_only_network_label(&network)
                )
            }
        };
        let mut network_status = PolicyCapabilityStatus::new(
            restricted_network_requested,
            network_enforced,
            network_detail,
        );
        match network.mode {
            NetworkMode::InheritHost => {
                network_status.limitations.push(
                    "no network isolation requested; launched apps use the host network"
                        .to_string(),
                );
            }
            NetworkMode::Disabled if network_enforced => {
                network_status.backend = Some("bubblewrap_unshare_net".to_string());
            }
            NetworkMode::Disabled => {
                network_status.planned_backend = Some("bubblewrap_unshare_net".to_string());
                network_status
                    .backend_requirements
                    .push("bubblewrap".to_string());
                network_status.limitations.push(
                    "network disabled is recorded in the profile but launched apps keep host network access"
                        .to_string(),
                );
            }
            NetworkMode::LocalOnly if network_enforced => {
                network_status.backend = Some("bubblewrap_loopback_only".to_string());
                network_status.limitations.push(
                    "host loopback services are not bridged into the sandbox; start needed local services inside the workspace or use inherit_host networking"
                        .to_string(),
                );
            }
            NetworkMode::LocalOnly => {
                network_status.planned_backend = Some("bubblewrap_loopback_only".to_string());
                network_status
                    .backend_requirements
                    .push("bubblewrap".to_string());
                network_status.limitations.push(
                    "local-only networking is recorded in the profile but launched apps keep host network access"
                        .to_string(),
                );
                network_status.limitations.push(
                    "launched apps keep host network access when this profile is acknowledged"
                        .to_string(),
                );
            }
        }
        if require_full_enforcement {
            mark_blocked_by_required_enforcement(&mut mount_status);
            mark_blocked_by_required_enforcement(&mut network_status);
        }
        Self {
            profile_id,
            mounts,
            network,
            require_full_enforcement,
            setup_command_count,
            runtime_capabilities,
            enforcement: PolicyEnforcement {
                display_isolation: PolicyCapabilityStatus {
                    requested: true,
                    enforced: true,
                    state: Some(PolicyCapabilityState::Enforced),
                    backend: Some("xvfb_xauth_display".to_string()),
                    planned_backend: None,
                    backend_requirements: Vec::new(),
                    requires_acknowledgement: Some(false),
                    required_acknowledgement: None,
                    limitations: Vec::new(),
                    detail: "workspace apps run on a private X11 DISPLAY with a scoped XAUTHORITY"
                        .to_string(),
                },
                input_scope: PolicyCapabilityStatus {
                    requested: true,
                    enforced: true,
                    state: Some(PolicyCapabilityState::Enforced),
                    backend: Some("workspace_ipc".to_string()),
                    planned_backend: None,
                    backend_requirements: Vec::new(),
                    requires_acknowledgement: Some(false),
                    required_acknowledgement: None,
                    limitations: Vec::new(),
                    detail: "input commands are sent to the workspace display only".to_string(),
                },
                mounts: mount_status,
                network: network_status,
            },
        }
    }

    pub fn has_requested_unenforced_policy(&self) -> bool {
        requested_but_unenforced(&self.enforcement.mounts)
            || requested_but_unenforced(&self.enforcement.network)
    }

    pub fn blocks_requested_unenforced_policy(&self) -> bool {
        self.require_full_enforcement && self.has_requested_unenforced_policy()
    }

    pub fn can_acknowledge_unenforced_policy(&self) -> bool {
        self.has_requested_unenforced_policy() && !self.require_full_enforcement
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct PolicyRuntimeCapabilities {
    pub bubblewrap: PolicyToolCheck,
    /// firejail / unshare / slirp4netns are reported for diagnostics (doctor)
    /// only. They are detected but NOT wired as enforcement backends: bubblewrap
    /// is the single backend that actually enforces mount and network policy, so
    /// these are not advertised as usable policy backends.
    pub firejail: PolicyToolCheck,
    pub unshare: PolicyToolCheck,
    pub slirp4netns: PolicyToolCheck,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_mount_backend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_network_backend: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl PolicyRuntimeCapabilities {
    pub fn from_tools(
        bubblewrap: PolicyToolCheck,
        firejail: PolicyToolCheck,
        unshare: PolicyToolCheck,
        slirp4netns: PolicyToolCheck,
    ) -> Self {
        // bubblewrap is the only enforcement backend that is actually wired.
        // We do not claim firejail/unshare as fallback "candidates" because they
        // never enforce anything today; advertising them would overstate what
        // the runtime can do.
        let backend = bubblewrap.ok.then(|| "bubblewrap".to_string());
        let notes = vec![if bubblewrap.ok {
            "bubblewrap enforces mount and network namespace isolation for launched apps"
                .to_string()
        } else {
            "install bubblewrap to enforce mount and network isolation; without it, mount and network policy is declared but not enforced"
                .to_string()
        }];

        Self {
            bubblewrap,
            firejail,
            unshare,
            slirp4netns,
            preferred_mount_backend: backend.clone(),
            preferred_network_backend: backend,
            notes,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PolicyToolCheck {
    pub ok: bool,
    pub detail: String,
}

impl Default for PolicyToolCheck {
    fn default() -> Self {
        Self {
            ok: false,
            detail: "not checked".to_string(),
        }
    }
}

/// Enforcement summary for a workspace policy.
///
/// HONESTY NOTE: the `enforced` flags below are derived from runtime
/// capability detected at workspace start time (e.g. whether the bubblewrap
/// binary is present), NOT from per-launch verification of the sandbox that
/// was actually constructed. They describe the start-time expectation. The
/// authoritative per-launch truth is the `mount_isolation` / `network_isolation`
/// labels recorded on each `WorkspaceApp`, which reflect the sandbox that was
/// actually applied when that specific app was spawned.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PolicyEnforcement {
    pub display_isolation: PolicyCapabilityStatus,
    pub input_scope: PolicyCapabilityStatus,
    pub mounts: PolicyCapabilityStatus,
    pub network: PolicyCapabilityStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PolicyCapabilityStatus {
    pub requested: bool,
    /// Whether this capability is expected to be enforced. This reflects
    /// runtime capability detected at workspace start time (the basis is
    /// "runtime_capability_at_start"), not per-launch verification. For the
    /// per-launch truth, read the per-app `mount_isolation` /
    /// `network_isolation` labels on `WorkspaceApp`.
    pub enforced: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<PolicyCapabilityState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planned_backend: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backend_requirements: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_acknowledgement: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_acknowledgement: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limitations: Vec<String>,
    pub detail: String,
}

impl PolicyCapabilityStatus {
    fn new(requested: bool, enforced: bool, detail: String) -> Self {
        let state = if requested {
            if enforced {
                PolicyCapabilityState::Enforced
            } else {
                PolicyCapabilityState::Unenforced
            }
        } else {
            PolicyCapabilityState::NotRequested
        };
        let requires_acknowledgement = requested && !enforced;
        Self {
            requested,
            enforced,
            state: Some(state),
            backend: None,
            planned_backend: None,
            backend_requirements: Vec::new(),
            requires_acknowledgement: Some(requires_acknowledgement),
            required_acknowledgement: requires_acknowledgement
                .then(|| "ack_unenforced_policy".to_string()),
            limitations: Vec::new(),
            detail,
        }
    }
}

fn requested_but_unenforced(status: &PolicyCapabilityStatus) -> bool {
    status.requested && !status.enforced
}

fn mark_blocked_by_required_enforcement(status: &mut PolicyCapabilityStatus) {
    if requested_but_unenforced(status) {
        status.requires_acknowledgement = Some(false);
        status.required_acknowledgement = None;
        status
            .limitations
            .retain(|limitation| !limitation.contains("when this profile is acknowledged"));
        status.limitations.push(
            "require_enforced_policy=true blocks launch until this policy is enforced".to_string(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(ok: bool, name: &str) -> PolicyToolCheck {
        PolicyToolCheck {
            ok,
            detail: if ok {
                format!("{name} available")
            } else {
                format!("{name} missing")
            },
        }
    }

    fn capabilities(
        bubblewrap: bool,
        firejail: bool,
        unshare: bool,
        slirp4netns: bool,
    ) -> PolicyRuntimeCapabilities {
        PolicyRuntimeCapabilities::from_tools(
            tool(bubblewrap, "bubblewrap"),
            tool(firejail, "firejail"),
            tool(unshare, "unshare"),
            tool(slirp4netns, "slirp4netns"),
        )
    }

    #[test]
    fn disabled_network_is_enforced_by_bubblewrap_without_extra_ack() {
        let policy = AppliedWorkspacePolicy::new_with_capabilities(
            "qa".to_string(),
            Vec::new(),
            NetworkPolicy {
                mode: NetworkMode::Disabled,
                allow_hosts: Vec::new(),
            },
            true,
            0,
            capabilities(true, false, false, false),
        );

        assert!(policy.enforcement.network.requested);
        assert!(policy.enforcement.network.enforced);
        assert_eq!(
            policy.enforcement.network.state,
            Some(PolicyCapabilityState::Enforced)
        );
        assert_eq!(
            policy.enforcement.network.backend.as_deref(),
            Some("bubblewrap_unshare_net")
        );
        assert_eq!(
            policy.enforcement.network.requires_acknowledgement,
            Some(false)
        );
        assert!(!policy.has_requested_unenforced_policy());
    }

    #[test]
    fn disabled_network_without_bubblewrap_stays_unenforced() {
        let policy = AppliedWorkspacePolicy::new_with_capabilities(
            "qa".to_string(),
            Vec::new(),
            NetworkPolicy {
                mode: NetworkMode::Disabled,
                allow_hosts: Vec::new(),
            },
            false,
            0,
            capabilities(false, false, false, false),
        );

        assert!(policy.enforcement.network.requested);
        assert!(!policy.enforcement.network.enforced);
        assert_eq!(
            policy.enforcement.network.state,
            Some(PolicyCapabilityState::Unenforced)
        );
        assert_eq!(
            policy.enforcement.network.requires_acknowledgement,
            Some(true)
        );
        assert_eq!(
            policy
                .enforcement
                .network
                .required_acknowledgement
                .as_deref(),
            Some("ack_unenforced_policy")
        );
        assert!(policy.has_requested_unenforced_policy());
    }

    #[test]
    fn local_only_network_is_enforced_with_bubblewrap_loopback_only() {
        let policy = AppliedWorkspacePolicy::new_with_capabilities(
            "qa-local".to_string(),
            Vec::new(),
            NetworkPolicy {
                mode: NetworkMode::LocalOnly,
                allow_hosts: vec!["localhost:3000".to_string(), "127.0.0.1:5173".to_string()],
            },
            false,
            0,
            capabilities(true, false, false, false),
        );

        assert!(policy.enforcement.network.requested);
        assert!(policy.enforcement.network.enforced);
        assert_eq!(
            policy.enforcement.network.state,
            Some(PolicyCapabilityState::Enforced)
        );
        assert_eq!(
            policy.enforcement.network.requires_acknowledgement,
            Some(false)
        );
        assert_eq!(policy.enforcement.network.required_acknowledgement, None);
        assert!(!policy.has_requested_unenforced_policy());
        assert_eq!(
            policy.enforcement.network.backend.as_deref(),
            Some("bubblewrap_loopback_only")
        );
        assert!(policy
            .enforcement
            .network
            .limitations
            .iter()
            .any(|limitation| limitation.contains("host loopback services")));
    }

    #[test]
    fn local_only_network_without_hosts_reports_sandbox_loopback() {
        let policy = AppliedWorkspacePolicy::new_with_capabilities(
            "qa-local".to_string(),
            Vec::new(),
            NetworkPolicy {
                mode: NetworkMode::LocalOnly,
                allow_hosts: Vec::new(),
            },
            false,
            0,
            capabilities(true, false, false, false),
        );

        assert_eq!(
            policy.enforcement.network.backend.as_deref(),
            Some("bubblewrap_loopback_only")
        );
        assert!(policy
            .enforcement
            .network
            .detail
            .contains("sandbox loopback is available"));
        assert!(!policy.enforcement.network.detail.ends_with("for "));
    }

    #[test]
    fn strict_profiles_block_requested_unenforced_policy() {
        let policy = AppliedWorkspacePolicy::new_with_capabilities(
            "strict-local".to_string(),
            Vec::new(),
            NetworkPolicy {
                mode: NetworkMode::Disabled,
                allow_hosts: Vec::new(),
            },
            true,
            0,
            capabilities(false, false, false, false),
        );

        assert!(policy.has_requested_unenforced_policy());
        assert!(policy.blocks_requested_unenforced_policy());
        assert!(!policy.can_acknowledge_unenforced_policy());
        assert_eq!(
            policy.enforcement.network.requires_acknowledgement,
            Some(false)
        );
        assert_eq!(policy.enforcement.network.required_acknowledgement, None);
        assert!(policy
            .enforcement
            .network
            .limitations
            .iter()
            .any(|limitation| limitation.contains("blocks launch")));
    }

    #[test]
    fn legacy_capability_status_deserializes_without_structured_metadata() {
        let status: PolicyCapabilityStatus = serde_json::from_value(serde_json::json!({
            "requested": true,
            "enforced": false,
            "detail": "legacy policy status"
        }))
        .expect("legacy policy status should deserialize");

        assert!(status.requested);
        assert!(!status.enforced);
        assert_eq!(status.state, None);
        assert_eq!(status.planned_backend, None);
        assert!(status.backend_requirements.is_empty());
        assert_eq!(status.requires_acknowledgement, None);
        assert_eq!(status.required_acknowledgement, None);
        assert!(status.limitations.is_empty());
    }
}
