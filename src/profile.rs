use crate::workspace::{self, EnvVar, IpcResponse, LaunchSpec, WorkspaceStartOptions};
use anyhow::{bail, Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceProfile {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<EnvVar>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mounts: Vec<ProfileMount>,
    #[serde(default)]
    pub network: NetworkPolicy,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub setup_commands: Vec<ProfileSetupCommand>,
}

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
pub struct ProfileSetupCommand {
    pub command: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<EnvVar>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ProfileStore {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<WorkspaceProfile>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProfileList {
    pub path: PathBuf,
    pub profiles: Vec<WorkspaceProfile>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProfilePath {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProfileDeleteResult {
    pub id: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProfileSetupRun {
    pub workspace_id: String,
    pub profile_id: String,
    pub launched: Vec<IpcResponse>,
}

pub fn profiles_path() -> PathBuf {
    config_dir().join("profiles.json")
}

pub fn profile_path() -> ProfilePath {
    ProfilePath {
        path: profiles_path(),
    }
}

pub fn list_profiles() -> Result<ProfileList> {
    let path = profiles_path();
    let store = read_store(&path)?;
    Ok(ProfileList {
        path,
        profiles: store.profiles,
    })
}

pub fn get_profile(id: &str) -> Result<WorkspaceProfile> {
    let id = sanitize_profile_id(id)?;
    read_store(&profiles_path())?
        .profiles
        .into_iter()
        .find(|profile| profile.id == id)
        .ok_or_else(|| anyhow::anyhow!("profile {id:?} was not found"))
}

pub fn put_profile(profile: WorkspaceProfile) -> Result<WorkspaceProfile> {
    validate_profile(&profile)?;
    let path = profiles_path();
    let mut store = read_store(&path)?;
    if let Some(existing) = store
        .profiles
        .iter_mut()
        .find(|existing| existing.id == profile.id)
    {
        *existing = profile.clone();
    } else {
        store.profiles.push(profile.clone());
    }
    store.profiles.sort_by(|left, right| left.id.cmp(&right.id));
    write_store(&path, &store)?;
    Ok(profile)
}

pub fn delete_profile(id: &str) -> Result<ProfileDeleteResult> {
    let id = sanitize_profile_id(id)?;
    let path = profiles_path();
    let mut store = read_store(&path)?;
    let original_len = store.profiles.len();
    store.profiles.retain(|profile| profile.id != id);
    let deleted = store.profiles.len() != original_len;
    if deleted {
        write_store(&path, &store)?;
    }
    Ok(ProfileDeleteResult { id, deleted })
}

pub fn validate_profile(profile: &WorkspaceProfile) -> Result<()> {
    sanitize_profile_id(&profile.id)?;
    if matches!(profile.width, Some(0)) {
        bail!("profile width must be greater than zero");
    }
    if matches!(profile.height, Some(0)) {
        bail!("profile height must be greater than zero");
    }
    for env_var in &profile.env {
        validate_env_var(env_var)?;
    }
    for mount in &profile.mounts {
        if mount.host_path.as_os_str().is_empty() {
            bail!("profile mount host_path cannot be empty");
        }
        if mount.workspace_path.as_os_str().is_empty() {
            bail!("profile mount workspace_path cannot be empty");
        }
    }
    if matches!(profile.network.mode, NetworkMode::Allowlist)
        && profile.network.allow_hosts.is_empty()
    {
        bail!("allowlist network profiles require at least one host");
    }
    for setup in &profile.setup_commands {
        if setup.command.is_empty() {
            bail!("profile setup command cannot be empty");
        }
        for env_var in &setup.env {
            validate_env_var(env_var)?;
        }
    }
    Ok(())
}

pub fn apply_profile_to_start_options(
    profile_id: &str,
    options: &mut WorkspaceStartOptions,
    width_explicit: bool,
    height_explicit: bool,
) -> Result<WorkspaceProfile> {
    let profile = get_profile(profile_id)?;
    if !width_explicit {
        if let Some(width) = profile.width {
            options.width = width;
        }
    }
    if !height_explicit {
        if let Some(height) = profile.height {
            options.height = height;
        }
    }
    Ok(profile)
}

pub fn apply_profile_to_launch_spec(
    profile_id: &str,
    spec: &mut LaunchSpec,
    cwd_explicit: bool,
) -> Result<WorkspaceProfile> {
    let profile = get_profile(profile_id)?;
    if !cwd_explicit {
        if let Some(cwd) = profile.cwd.clone() {
            spec.cwd = Some(cwd);
        }
    }
    let explicit_env = std::mem::take(&mut spec.env);
    spec.env = merged_env(profile.env.clone(), explicit_env);
    Ok(profile)
}

pub fn setup_launch_specs(profile_id: &str) -> Result<Vec<LaunchSpec>> {
    let profile = get_profile(profile_id)?;
    profile
        .setup_commands
        .iter()
        .map(|setup| {
            if setup.command.is_empty() {
                bail!("profile setup command cannot be empty");
            }
            Ok(LaunchSpec {
                command: setup.command.clone(),
                cwd: setup.cwd.clone().or_else(|| profile.cwd.clone()),
                env: merged_env(profile.env.clone(), setup.env.clone()),
            })
        })
        .collect()
}

pub fn launch_profile_setup(workspace_id: &str, profile_id: &str) -> Result<ProfileSetupRun> {
    let specs = setup_launch_specs(profile_id)?;
    let mut launched = Vec::new();
    for spec in specs {
        launched.push(workspace::launch_app_with_spec(workspace_id, spec)?);
    }
    Ok(ProfileSetupRun {
        workspace_id: workspace_id.to_string(),
        profile_id: profile_id.to_string(),
        launched,
    })
}

fn merged_env(mut base: Vec<EnvVar>, overrides: Vec<EnvVar>) -> Vec<EnvVar> {
    for override_var in overrides {
        if let Some(existing) = base
            .iter_mut()
            .find(|base_var| base_var.name == override_var.name)
        {
            *existing = override_var;
        } else {
            base.push(override_var);
        }
    }
    base
}

fn read_store(path: &Path) -> Result<ProfileStore> {
    if !path.exists() {
        return Ok(ProfileStore::default());
    }
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(ProfileStore::default());
    }
    serde_json::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))
}

fn write_store(path: &Path, store: &ProfileStore) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let temp_path = path.with_extension("json.tmp");
    let content = serde_json::to_string_pretty(store).context("failed to serialize profiles")?;
    fs::write(&temp_path, format!("{content}\n"))
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    fs::rename(&temp_path, path).with_context(|| {
        format!(
            "failed to move {} to {}",
            temp_path.display(),
            path.display()
        )
    })?;
    Ok(())
}

fn config_dir() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("agent-workspace-linux")
}

fn sanitize_profile_id(id: &str) -> Result<String> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        bail!("profile id cannot be empty");
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        bail!("profile id may only contain ASCII letters, numbers, '-' and '_'");
    }
    Ok(trimmed.to_string())
}

fn validate_env_var(env_var: &EnvVar) -> Result<()> {
    if env_var.name.is_empty() {
        bail!("environment variable name cannot be empty");
    }
    if env_var.name.contains('=') {
        bail!("environment variable name cannot contain '='");
    }
    if env_var.name.contains('\0') || env_var.value.contains('\0') {
        bail!("environment variable cannot contain NUL bytes");
    }
    Ok(())
}
