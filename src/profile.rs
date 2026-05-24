use crate::policy::{AppliedWorkspacePolicy, NetworkMode};
pub use crate::policy::{MountMode, NetworkPolicy, ProfileMount};
use crate::workspace::{self, EnvVar, IpcResponse, LaunchSpec, WorkspaceStartOptions};
use anyhow::{bail, Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Component, Path, PathBuf},
};

const WORKSPACE_MOUNT_ROOT: &str = "/workspace";

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub startup_apps: Vec<ProfileStartupApp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProfileSetupCommand {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub command: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<EnvVar>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProfileStartupApp {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
    pub would_delete: bool,
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<WorkspaceProfile>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProfilePutResult {
    pub ok: bool,
    pub message: String,
    pub id: String,
    pub action: String,
    pub saved: bool,
    pub created: bool,
    pub replaced: bool,
    pub would_create: bool,
    pub would_replace: bool,
    pub replace: bool,
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<WorkspaceProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing_profile: Option<WorkspaceProfile>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProfileExportResult {
    pub id: String,
    pub wrote: bool,
    pub would_write: bool,
    pub replace: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_path: Option<PathBuf>,
    pub profile: WorkspaceProfile,
}

impl ProfilePutResult {
    pub fn error(profile: WorkspaceProfile, replace: bool, dry_run: bool, message: String) -> Self {
        Self {
            ok: false,
            message,
            id: profile.id.clone(),
            action: "error".to_string(),
            saved: false,
            created: false,
            replaced: false,
            would_create: false,
            would_replace: false,
            replace,
            dry_run,
            profile: Some(profile),
            existing_profile: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProfileCheck {
    pub profile: WorkspaceProfile,
    pub applied_policy: AppliedWorkspacePolicy,
    pub requires_hidden_workspace_ack: bool,
    pub requires_unenforced_policy_ack: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProfileSetupRun {
    pub workspace_id: String,
    pub profile_id: String,
    pub wait: bool,
    pub kill_on_timeout: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    pub launched: Vec<IpcResponse>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waited: Vec<IpcResponse>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub killed: Vec<IpcResponse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub succeeded: Option<bool>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProfileStartupRun {
    pub workspace_id: String,
    pub profile_id: String,
    pub wait_window: bool,
    pub screenshot_window: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_timeout_ms: Option<u64>,
    pub launched: Vec<IpcResponse>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProfileWorkspaceOpen {
    pub workspace_id: String,
    pub profile_id: String,
    pub ready: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_succeeded: Option<bool>,
    pub startup_launched: bool,
    pub start: IpcResponse,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<ProfileSetupRun>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup: Option<ProfileStartupRun>,
}

#[derive(Debug, Clone, Default)]
pub struct ProfileWorkspaceOpenOptions {
    pub run_setup: bool,
    pub setup: ProfileSetupOptions,
    pub startup: ProfileStartupOptions,
}

#[derive(Debug, Clone, Default)]
pub struct ProfileSetupOptions {
    pub wait: bool,
    pub timeout_ms: Option<u64>,
    pub kill_on_timeout: bool,
    pub acknowledge_unenforced_policy: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ProfileStartupOptions {
    pub acknowledge_unenforced_policy: bool,
    pub wait_window: bool,
    pub window_timeout_ms: Option<u64>,
    pub screenshot_window: bool,
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

pub fn export_profile(
    id: &str,
    output_path: Option<PathBuf>,
    replace: bool,
) -> Result<ProfileExportResult> {
    let profile = get_profile(id)?;
    let would_write = output_path.is_some();
    if let Some(path) = &output_path {
        if path.exists() && !replace {
            bail!(
                "profile export output {} already exists; pass --replace or set replace=true to overwrite it",
                path.display()
            );
        }
        let content = serde_json::to_string_pretty(&profile)
            .context("failed to serialize profile for export")?;
        fs::write(path, format!("{content}\n"))
            .with_context(|| format!("failed to write {}", path.display()))?;
    }
    Ok(ProfileExportResult {
        id: profile.id.clone(),
        wrote: output_path.is_some(),
        would_write,
        replace,
        output_path,
        profile,
    })
}

pub fn template_profile(
    kind: &str,
    id: Option<String>,
    host_path: Option<PathBuf>,
) -> Result<WorkspaceProfile> {
    let kind = kind.trim();
    let profile = match kind {
        "project-dev" | "project_dev" => {
            let id = id.unwrap_or_else(|| "project-dev".to_string());
            let host_path = match host_path {
                Some(path) => path,
                None => env::current_dir().context("failed to resolve template host path")?,
            };
            WorkspaceProfile {
                id,
                description: Some(
                    "Project QA profile with the selected project mounted read-write.".to_string(),
                ),
                width: None,
                height: None,
                cwd: Some(PathBuf::from("/workspace/project")),
                env: Vec::new(),
                mounts: vec![ProfileMount {
                    host_path,
                    workspace_path: PathBuf::from("/workspace/project"),
                    mode: MountMode::ReadWrite,
                }],
                network: NetworkPolicy::default(),
                setup_commands: Vec::new(),
                startup_apps: Vec::new(),
            }
        }
        _ => bail!("unknown profile template {kind:?}. Expected: project-dev"),
    };
    validate_profile(&profile)?;
    Ok(profile)
}

pub fn check_profile(id: &str) -> Result<ProfileCheck> {
    let profile = get_profile(id)?;
    let applied_policy = applied_policy(&profile);
    let requires_unenforced_policy_ack = applied_policy.has_requested_unenforced_policy();
    let warnings = policy_warnings(&applied_policy);
    Ok(ProfileCheck {
        profile,
        applied_policy,
        requires_hidden_workspace_ack: true,
        requires_unenforced_policy_ack,
        warnings,
    })
}

pub fn put_profile(
    profile: WorkspaceProfile,
    replace: bool,
    dry_run: bool,
) -> Result<ProfilePutResult> {
    validate_profile(&profile)?;
    let path = profiles_path();
    let mut store = read_store(&path)?;
    let existing_index = store
        .profiles
        .iter()
        .position(|existing| existing.id == profile.id);
    let existing_profile = existing_index.map(|index| store.profiles[index].clone());
    let action = match (&existing_profile, replace) {
        (None, _) => "create",
        (Some(_), true) => "replace",
        (Some(_), false) => "reject_existing",
    };
    let would_create = existing_profile.is_none();
    let would_replace = existing_profile.is_some() && replace;

    if !dry_run {
        if let Some(index) = existing_index {
            if !replace {
                bail!(
                    "profile {:?} already exists; pass --replace or set replace=true to overwrite it",
                    profile.id
                );
            }
            store.profiles[index] = profile.clone();
        } else {
            store.profiles.push(profile.clone());
        }
        store.profiles.sort_by(|left, right| left.id.cmp(&right.id));
        write_store(&path, &store)?;
    }

    let saved = !dry_run && action != "reject_existing";
    let created = saved && action == "create";
    let replaced = saved && action == "replace";
    let message = match (dry_run, action) {
        (true, "create") => "profile put dry run: profile would be created",
        (true, "replace") => "profile put dry run: profile would be replaced",
        (true, "reject_existing") => {
            "profile put dry run: profile exists and requires replace=true to overwrite"
        }
        (false, "create") => "profile created",
        (false, "replace") => "profile replaced",
        _ => {
            bail!(
                "profile {:?} already exists; pass --replace or set replace=true to overwrite it",
                profile.id
            );
        }
    };

    Ok(ProfilePutResult {
        ok: true,
        message: message.to_string(),
        id: profile.id.clone(),
        action: action.to_string(),
        saved,
        created,
        replaced,
        would_create,
        would_replace,
        replace,
        dry_run,
        profile: Some(profile),
        existing_profile,
    })
}

pub fn delete_profile(id: &str, dry_run: bool) -> Result<ProfileDeleteResult> {
    let id = sanitize_profile_id(id)?;
    let path = profiles_path();
    let mut store = read_store(&path)?;
    let profile_index = store.profiles.iter().position(|profile| profile.id == id);
    let profile = profile_index.map(|index| store.profiles[index].clone());
    let would_delete = profile.is_some();
    let deleted = would_delete && !dry_run;
    if let Some(index) = profile_index.filter(|_| !dry_run) {
        store.profiles.remove(index);
        write_store(&path, &store)?;
    }
    Ok(ProfileDeleteResult {
        id,
        deleted,
        would_delete,
        dry_run,
        profile,
    })
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
    validate_mounts(&profile.mounts)?;
    validate_network_policy(&profile.network)?;
    for setup in &profile.setup_commands {
        workspace::validate_optional_app_name(&setup.name)?;
        if setup.command.is_empty() {
            bail!("profile setup command cannot be empty");
        }
        for env_var in &setup.env {
            validate_env_var(env_var)?;
        }
    }
    for app in &profile.startup_apps {
        workspace::validate_optional_app_name(&app.name)?;
        if app.command.is_empty() {
            bail!("profile startup app command cannot be empty");
        }
        for env_var in &app.env {
            validate_env_var(env_var)?;
        }
    }
    Ok(())
}

fn validate_network_policy(network: &NetworkPolicy) -> Result<()> {
    if matches!(
        network.mode,
        NetworkMode::Allowlist | NetworkMode::LocalOnly
    ) && network.allow_hosts.is_empty()
    {
        bail!(
            "{} network profiles require at least one host",
            network_mode_name(&network.mode)
        );
    }
    if matches!(network.mode, NetworkMode::LocalOnly) {
        for host in &network.allow_hosts {
            if !is_local_network_target(host) {
                bail!(
                    "local_only network profiles may only allow localhost or loopback targets, got {host:?}"
                );
            }
        }
    }
    Ok(())
}

fn network_mode_name(mode: &NetworkMode) -> &'static str {
    match mode {
        NetworkMode::InheritHost => "inherit_host",
        NetworkMode::Disabled => "disabled",
        NetworkMode::LocalOnly => "local_only",
        NetworkMode::Allowlist => "allowlist",
    }
}

fn is_local_network_target(target: &str) -> bool {
    let Some(host) = network_target_host(target) else {
        return false;
    };
    host == "localhost" || host == "::1" || host.starts_with("127.")
}

fn network_target_host(target: &str) -> Option<String> {
    let trimmed = target.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return None;
    }
    let without_scheme = trimmed
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(trimmed);
    let authority = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(without_scheme)
        .trim();
    if authority.is_empty() {
        return None;
    }
    let host = if let Some(rest) = authority.strip_prefix('[') {
        rest.split_once(']').map(|(host, _)| host).unwrap_or(rest)
    } else if authority.matches(':').count() == 1 {
        authority
            .split_once(':')
            .map(|(host, _)| host)
            .unwrap_or(authority)
    } else {
        authority
    };
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

fn validate_mounts(mounts: &[ProfileMount]) -> Result<()> {
    let mut workspace_paths: BTreeSet<PathBuf> = BTreeSet::new();
    for mount in mounts {
        validate_mount_path(&mount.host_path, "host_path")?;
        validate_mount_path(&mount.workspace_path, "workspace_path")?;
        if !mount.host_path.is_absolute() {
            bail!(
                "profile mount host_path {} must be absolute",
                mount.host_path.display()
            );
        }
        if !mount.workspace_path.starts_with(WORKSPACE_MOUNT_ROOT)
            || mount.workspace_path == Path::new(WORKSPACE_MOUNT_ROOT)
        {
            bail!(
                "profile mount workspace_path {} must be under {WORKSPACE_MOUNT_ROOT}/",
                mount.workspace_path.display()
            );
        }
        for existing in &workspace_paths {
            if mount.workspace_path.starts_with(existing)
                || existing.starts_with(&mount.workspace_path)
            {
                bail!(
                    "profile mount workspace_path {} overlaps {}",
                    mount.workspace_path.display(),
                    existing.display()
                );
            }
        }
        workspace_paths.insert(mount.workspace_path.clone());
    }
    Ok(())
}

fn validate_mount_path(path: &Path, field: &str) -> Result<()> {
    if path.as_os_str().is_empty() {
        bail!("profile mount {field} cannot be empty");
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!(
            "profile mount {field} {} cannot contain '..'",
            path.display()
        );
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
    options.profile_id = Some(profile.id.clone());
    options.applied_policy = Some(applied_policy(&profile));
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

pub fn applied_policy(profile: &WorkspaceProfile) -> AppliedWorkspacePolicy {
    AppliedWorkspacePolicy::new_with_capabilities(
        profile.id.clone(),
        profile.mounts.clone(),
        profile.network.clone(),
        profile.setup_commands.len(),
        workspace::policy_runtime_capabilities(),
    )
}

fn policy_warnings(policy: &AppliedWorkspacePolicy) -> Vec<String> {
    let mut warnings = Vec::new();
    if policy.enforcement.mounts.requested && !policy.enforcement.mounts.enforced {
        warnings.push(policy.enforcement.mounts.detail.clone());
    }
    if policy.enforcement.network.requested && !policy.enforcement.network.enforced {
        warnings.push(policy.enforcement.network.detail.clone());
    }
    warnings
}

pub fn apply_profile_to_launch_spec(
    profile_id: &str,
    spec: &mut LaunchSpec,
    cwd_explicit: bool,
) -> Result<WorkspaceProfile> {
    let profile = get_profile(profile_id)?;
    spec.profile_id = Some(profile.id.clone());
    spec.applied_policy = Some(applied_policy(&profile));
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
                name: setup.name.clone(),
                profile_id: Some(profile.id.clone()),
                applied_policy: Some(applied_policy(&profile)),
                user_acknowledged_unenforced_policy: false,
                cwd: setup.cwd.clone().or_else(|| profile.cwd.clone()),
                env: merged_env(profile.env.clone(), setup.env.clone()),
            })
        })
        .collect()
}

pub fn startup_app_launch_specs(profile_id: &str) -> Result<Vec<LaunchSpec>> {
    let profile = get_profile(profile_id)?;
    profile
        .startup_apps
        .iter()
        .map(|app| {
            if app.command.is_empty() {
                bail!("profile startup app command cannot be empty");
            }
            Ok(LaunchSpec {
                command: app.command.clone(),
                name: app.name.clone(),
                profile_id: Some(profile.id.clone()),
                applied_policy: Some(applied_policy(&profile)),
                user_acknowledged_unenforced_policy: false,
                cwd: app.cwd.clone().or_else(|| profile.cwd.clone()),
                env: merged_env(profile.env.clone(), app.env.clone()),
            })
        })
        .collect()
}

pub fn launch_profile_setup(
    workspace_id: &str,
    profile_id: &str,
    options: ProfileSetupOptions,
) -> Result<ProfileSetupRun> {
    let wait = options.wait || options.timeout_ms.is_some() || options.kill_on_timeout;
    let mut specs = setup_launch_specs(profile_id)?;
    let mut launched = Vec::new();
    let mut waited = Vec::new();
    let mut killed = Vec::new();
    let mut app_ids = Vec::new();
    for spec in &mut specs {
        spec.user_acknowledged_unenforced_policy = options.acknowledge_unenforced_policy;
    }
    for spec in specs {
        let launch = workspace::launch_app_with_spec(workspace_id, spec)?;
        if !launch.ok {
            launched.push(launch);
            break;
        }
        if wait {
            let app_id = launched_app_id(&launch)
                .context("profile setup launch did not return an app id")?;
            let wait =
                workspace::wait_app(workspace_id, app_id.clone(), options.timeout_ms, false)?;
            let should_kill =
                options.kill_on_timeout && !wait.ok && response_app_running(&wait, &app_id);
            if should_kill {
                killed.push(workspace::kill_app(workspace_id, app_id.clone(), false)?);
            }
            app_ids.push(app_id);
            waited.push(wait);
        }
        launched.push(launch);
    }
    let completed = wait.then(|| {
        launched.iter().all(|response| response.ok)
            && waited.iter().all(|response| response.ok)
            && waited.len() == launched.len()
    });
    let succeeded = wait.then(|| {
        completed.unwrap_or(false)
            && waited
                .iter()
                .zip(app_ids.iter())
                .all(|(response, app_id)| response_app_exit_code(response, app_id) == Some(0))
    });
    Ok(ProfileSetupRun {
        workspace_id: workspace_id.to_string(),
        profile_id: profile_id.to_string(),
        wait,
        kill_on_timeout: options.kill_on_timeout,
        timeout_ms: options.timeout_ms,
        launched,
        waited,
        killed,
        completed,
        succeeded,
    })
}

pub fn launch_profile_startup_apps(
    workspace_id: &str,
    profile_id: &str,
    options: ProfileStartupOptions,
) -> Result<ProfileStartupRun> {
    let mut specs = startup_app_launch_specs(profile_id)?;
    let mut launched = Vec::new();
    for spec in &mut specs {
        spec.user_acknowledged_unenforced_policy = options.acknowledge_unenforced_policy;
    }
    for spec in specs {
        launched.push(workspace::launch_app_with_options(
            workspace_id,
            spec,
            options.wait_window,
            options.window_timeout_ms,
            options.screenshot_window,
        )?);
    }
    Ok(ProfileStartupRun {
        workspace_id: workspace_id.to_string(),
        profile_id: profile_id.to_string(),
        wait_window: options.wait_window,
        screenshot_window: options.screenshot_window,
        window_timeout_ms: options.window_timeout_ms,
        launched,
    })
}

pub fn open_profile_workspace(
    options: WorkspaceStartOptions,
    profile_id: &str,
    open_options: ProfileWorkspaceOpenOptions,
) -> Result<ProfileWorkspaceOpen> {
    let workspace_id = options.id.clone();
    let start = workspace::start_workspace(options)?;
    let (setup, startup) = if start.ok {
        let setup = if open_options.run_setup {
            Some(launch_profile_setup(
                &workspace_id,
                profile_id,
                open_options.setup,
            )?)
        } else {
            None
        };
        let setup_succeeded = setup
            .as_ref()
            .and_then(|setup| setup.succeeded)
            .unwrap_or(true);
        let startup = if setup_succeeded {
            Some(launch_profile_startup_apps(
                &workspace_id,
                profile_id,
                open_options.startup,
            )?)
        } else {
            None
        };
        (setup, startup)
    } else {
        (None, None)
    };
    let setup_succeeded = setup.as_ref().and_then(|setup| setup.succeeded);
    let startup_launched = startup
        .as_ref()
        .is_some_and(|startup| startup.launched.iter().all(|response| response.ok));
    let ready = start.ok && setup_succeeded.unwrap_or(true) && startup_launched;
    Ok(ProfileWorkspaceOpen {
        workspace_id,
        profile_id: profile_id.to_string(),
        ready,
        setup_succeeded,
        startup_launched,
        start,
        setup,
        startup,
    })
}

fn launched_app_id(response: &IpcResponse) -> Option<String> {
    if let Some(app) = response.apps.as_ref().and_then(|apps| apps.last()) {
        return Some(app.id.clone());
    }
    response
        .status
        .as_ref()?
        .apps
        .last()
        .map(|app| app.id.clone())
}

fn response_app_running(response: &IpcResponse, app_id: &str) -> bool {
    response
        .apps
        .as_ref()
        .and_then(|apps| apps.iter().find(|app| app.id == app_id))
        .or_else(|| {
            response
                .status
                .as_ref()?
                .apps
                .iter()
                .find(|app| app.id == app_id || app.pid.to_string() == app_id)
        })
        .is_some_and(|app| app.running)
}

fn response_app_exit_code(response: &IpcResponse, app_id: &str) -> Option<i32> {
    if let Some(app) = response
        .apps
        .as_ref()
        .and_then(|apps| apps.iter().find(|app| app.id == app_id))
    {
        return app.exit_code;
    }
    response
        .status
        .as_ref()?
        .apps
        .iter()
        .find(|app| app.id == app_id || app.pid.to_string() == app_id)?
        .exit_code
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

#[cfg(test)]
mod tests {
    use super::*;

    fn profile_with_network(network: NetworkPolicy) -> WorkspaceProfile {
        WorkspaceProfile {
            id: "qa-local".to_string(),
            description: None,
            width: None,
            height: None,
            cwd: None,
            env: Vec::new(),
            mounts: Vec::new(),
            network,
            setup_commands: Vec::new(),
            startup_apps: Vec::new(),
        }
    }

    #[test]
    fn local_only_network_accepts_loopback_targets() {
        let profile = profile_with_network(NetworkPolicy {
            mode: NetworkMode::LocalOnly,
            allow_hosts: vec![
                "localhost:3000".to_string(),
                "http://127.0.0.1:5173/path".to_string(),
                "[::1]:9000".to_string(),
            ],
        });

        validate_profile(&profile).expect("local_only loopback targets should validate");
    }

    #[test]
    fn local_only_network_rejects_remote_targets() {
        let profile = profile_with_network(NetworkPolicy {
            mode: NetworkMode::LocalOnly,
            allow_hosts: vec!["example.com".to_string()],
        });

        let error = validate_profile(&profile).expect_err("remote local_only target should fail");
        assert!(error
            .to_string()
            .contains("may only allow localhost or loopback"));
    }
}
