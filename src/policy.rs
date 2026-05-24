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
    pub enforcement: PolicyEnforcement,
}

impl AppliedWorkspacePolicy {
    pub fn new(
        profile_id: String,
        mounts: Vec<ProfileMount>,
        network: NetworkPolicy,
        setup_command_count: usize,
    ) -> Self {
        let mount_policy_requested = !mounts.is_empty();
        let restricted_network_requested = !matches!(network.mode, NetworkMode::InheritHost);
        Self {
            profile_id,
            mounts,
            network,
            setup_command_count,
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
                    enforced: !mount_policy_requested,
                    detail: if mount_policy_requested {
                        "mounts are declared but no mount namespace backend is active".to_string()
                    } else {
                        "no mount policy requested".to_string()
                    },
                },
                network: PolicyCapabilityStatus {
                    requested: restricted_network_requested,
                    enforced: !restricted_network_requested,
                    detail: if restricted_network_requested {
                        "network policy is declared but no network isolation backend is active"
                            .to_string()
                    } else {
                        "profile inherits the host network by policy".to_string()
                    },
                },
            },
        }
    }

    pub fn has_requested_unenforced_policy(&self) -> bool {
        requested_but_unenforced(&self.enforcement.mounts)
            || requested_but_unenforced(&self.enforcement.network)
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
