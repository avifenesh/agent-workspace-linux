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
    Allowlist,
}

impl Default for NetworkMode {
    fn default() -> Self {
        Self::InheritHost
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AppliedWorkspacePolicy {
    pub profile_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mounts: Vec<ProfileMount>,
    #[serde(default)]
    pub network: NetworkPolicy,
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
        let network_enforced = match network.mode {
            NetworkMode::InheritHost => true,
            NetworkMode::Disabled => runtime_capabilities.bubblewrap.ok,
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
            NetworkMode::Allowlist => {
                "network allowlist is declared but no allowlist backend is active".to_string()
            }
        };
        Self {
            profile_id,
            mounts,
            network,
            setup_command_count,
            runtime_capabilities,
            enforcement: PolicyEnforcement {
                display_isolation: PolicyCapabilityStatus {
                    requested: true,
                    enforced: true,
                    detail: "workspace apps run on a private X11 DISPLAY with a scoped XAUTHORITY"
                        .to_string(),
                },
                input_scope: PolicyCapabilityStatus {
                    requested: true,
                    enforced: true,
                    detail: "input commands are sent to the workspace display only".to_string(),
                },
                mounts: PolicyCapabilityStatus {
                    requested: mount_policy_requested,
                    enforced: !mount_policy_requested || mounts_enforced,
                    detail: mount_detail,
                },
                network: PolicyCapabilityStatus {
                    requested: restricted_network_requested,
                    enforced: network_enforced,
                    detail: network_detail,
                },
            },
        }
    }

    pub fn has_requested_unenforced_policy(&self) -> bool {
        requested_but_unenforced(&self.enforcement.mounts)
            || requested_but_unenforced(&self.enforcement.network)
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

        Self {
            bubblewrap,
            firejail,
            unshare,
            slirp4netns,
            can_candidate_mounts,
            can_candidate_disable_network,
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
    pub detail: String,
}

fn requested_but_unenforced(status: &PolicyCapabilityStatus) -> bool {
    status.requested && !status.enforced
}
