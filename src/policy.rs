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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MountMode {
    ReadOnly,
    ReadWrite,
}

impl Default for MountMode {
    fn default() -> Self {
        Self::ReadOnly
    }
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NetworkMode {
    InheritHost,
    Disabled,
    LocalOnly,
    Allowlist,
}

impl Default for NetworkMode {
    fn default() -> Self {
        Self::InheritHost
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
            if let Some(backend) = &runtime_capabilities.preferred_mount_backend {
                format!(
                    "mounts are declared; {backend} is available as a candidate but is not active"
                )
            } else {
                "mounts are declared but no mount namespace backend is available".to_string()
            }
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
            NetworkMode::Allowlist => false,
        };
        let network_detail = match network.mode {
            NetworkMode::InheritHost => "profile inherits the host network by policy".to_string(),
            NetworkMode::Disabled if network_enforced => {
                "network disabled is enforced with bubblewrap --unshare-net for launched apps"
                    .to_string()
            }
            NetworkMode::Disabled => {
                if let Some(backend) = &runtime_capabilities.preferred_network_backend {
                    format!(
                        "network disabled is declared; {backend} is available as a candidate but is not active"
                    )
                } else {
                    "network disabled is declared but no network isolation backend is available"
                        .to_string()
                }
            }
            NetworkMode::LocalOnly if network_enforced => {
                format!(
                    "local-only network is enforced with bubblewrap --unshare-net; sandbox loopback is available for {}",
                    network.allow_hosts.join(", ")
                )
            }
            NetworkMode::LocalOnly => {
                format!(
                    "local-only network is declared for {} but bubblewrap is not available",
                    network.allow_hosts.join(", ")
                )
            }
            NetworkMode::Allowlist => {
                format!(
                    "network allowlist is declared for {} but no allowlist backend is active",
                    network.allow_hosts.join(", ")
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
            NetworkMode::Allowlist => {
                network_status.planned_backend = Some("egress_proxy_allowlist".to_string());
                network_status.backend_requirements.extend(
                    ["network_namespace", "egress_proxy", "domain_filter"].map(str::to_string),
                );
                network_status.limitations.push(
                    "allow_hosts is recorded for UI intent, but traffic is not filtered to those hosts"
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
    pub firejail: PolicyToolCheck,
    pub unshare: PolicyToolCheck,
    pub slirp4netns: PolicyToolCheck,
    pub can_candidate_mounts: bool,
    pub can_candidate_disable_network: bool,
    pub can_candidate_local_network: bool,
    pub can_candidate_network_allowlist: bool,
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
        let preferred_mount_backend = if bubblewrap.ok {
            Some("bubblewrap".to_string())
        } else if firejail.ok {
            Some("firejail".to_string())
        } else if unshare.ok {
            Some("unshare".to_string())
        } else {
            None
        };
        let preferred_network_backend = if bubblewrap.ok {
            Some("bubblewrap".to_string())
        } else if firejail.ok {
            Some("firejail".to_string())
        } else if unshare.ok {
            Some("unshare".to_string())
        } else {
            None
        };
        let can_candidate_mounts = preferred_mount_backend.is_some();
        let can_candidate_disable_network = preferred_network_backend.is_some();
        let can_candidate_local_network = bubblewrap.ok;
        let can_candidate_network_allowlist = firejail.ok || slirp4netns.ok;
        let mut notes = Vec::new();
        if bubblewrap.ok {
            notes.push(
                "bubblewrap is the preferred candidate for mount and network namespace enforcement"
                    .to_string(),
            );
        } else {
            notes.push(
                "install bubblewrap to unlock the preferred unprivileged policy backend candidate"
                    .to_string(),
            );
        }
        if can_candidate_network_allowlist {
            notes.push("network allowlists still need a concrete rules backend before they can be enforced".to_string());
        }
        if can_candidate_local_network {
            notes.push("local-only networking can be enforced with bubblewrap loopback-only network namespaces".to_string());
        }

        Self {
            bubblewrap,
            firejail,
            unshare,
            slirp4netns,
            can_candidate_mounts,
            can_candidate_disable_network,
            can_candidate_local_network,
            can_candidate_network_allowlist,
            preferred_mount_backend,
            preferred_network_backend,
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
    fn allowlist_network_stays_unenforced_even_with_candidates() {
        let policy = AppliedWorkspacePolicy::new_with_capabilities(
            "shopping".to_string(),
            Vec::new(),
            NetworkPolicy {
                mode: NetworkMode::Allowlist,
                allow_hosts: vec!["example.com".to_string()],
            },
            false,
            0,
            capabilities(true, true, true, true),
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
        assert!(policy
            .enforcement
            .network
            .limitations
            .iter()
            .any(|limitation| limitation.contains("allow_hosts")));
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
        assert!(policy.runtime_capabilities.can_candidate_local_network);
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
    fn strict_profiles_block_requested_unenforced_policy() {
        let policy = AppliedWorkspacePolicy::new_with_capabilities(
            "strict-local".to_string(),
            Vec::new(),
            NetworkPolicy {
                mode: NetworkMode::Allowlist,
                allow_hosts: vec!["example.com".to_string()],
            },
            true,
            0,
            capabilities(true, false, false, true),
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
