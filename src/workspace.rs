use crate::policy::{AppliedWorkspacePolicy, PolicyRuntimeCapabilities, PolicyToolCheck};
use anyhow::{anyhow, bail, Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const DEFAULT_WORKSPACE_ID: &str = "default";
const DEFAULT_WIDTH: u32 = 1280;
const DEFAULT_HEIGHT: u32 = 720;
const DISPLAY_RANGE: std::ops::Range<u32> = 90..180;
const APPLIED_POLICY_FILE: &str = "applied_policy.json";
const EVENT_LOG_FILE: &str = "events.jsonl";

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DoctorReport {
    pub runtime: RuntimeReport,
    pub ready_for_x11_workspace: bool,
    pub blockers: Vec<String>,
    pub recommended_next_step: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RuntimeReport {
    pub xvfb: Check,
    pub xephyr: Check,
    pub xauth: Check,
    pub xdpyinfo: Check,
    pub window_manager: Check,
    pub xdotool: Check,
    pub screenshot: Check,
    pub policy: PolicyRuntimeCapabilities,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Check {
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct WorkspaceStartOptions {
    pub id: String,
    pub profile_id: Option<String>,
    pub applied_policy: Option<AppliedWorkspacePolicy>,
    pub user_acknowledged_hidden_workspace: bool,
    pub user_acknowledged_unenforced_policy: bool,
    pub width: u32,
    pub height: u32,
}

impl Default for WorkspaceStartOptions {
    fn default() -> Self {
        Self {
            id: DEFAULT_WORKSPACE_ID.to_string(),
            profile_id: None,
            applied_policy: None,
            user_acknowledged_hidden_workspace: false,
            user_acknowledged_unenforced_policy: false,
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DaemonOptions {
    pub id: String,
    pub profile_id: Option<String>,
    pub applied_policy: Option<AppliedWorkspacePolicy>,
    pub user_acknowledged_hidden_workspace: bool,
    pub user_acknowledged_unenforced_policy: bool,
    pub display: String,
    pub width: u32,
    pub height: u32,
    pub runtime_dir: PathBuf,
    pub socket_path: PathBuf,
    pub xauthority_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceStatus {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_policy: Option<AppliedWorkspacePolicy>,
    pub user_acknowledged_hidden_workspace: bool,
    pub user_acknowledged_unenforced_policy: bool,
    pub ready: bool,
    pub display: String,
    pub width: u32,
    pub height: u32,
    pub runtime_dir: PathBuf,
    pub socket_path: PathBuf,
    pub xauthority_path: PathBuf,
    pub x_server_pid: u32,
    pub window_manager_pid: Option<u32>,
    pub apps: Vec<WorkspaceApp>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WorkspaceList {
    pub runtime_base_dir: PathBuf,
    pub workspaces: Vec<WorkspaceListEntry>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WorkspaceListEntry {
    pub id: String,
    pub runtime_dir: PathBuf,
    pub socket_path: PathBuf,
    pub running: bool,
    pub status: Option<WorkspaceStatus>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WorkspaceCleanup {
    pub runtime_base_dir: PathBuf,
    pub removed: Vec<WorkspaceCleanupEntry>,
    pub skipped: Vec<WorkspaceCleanupEntry>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WorkspaceCleanupEntry {
    pub id: String,
    pub runtime_dir: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceApp {
    pub id: String,
    pub pid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub command: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<EnvVar>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr_path: Option<PathBuf>,
    pub started_at_unix: u64,
    pub running: bool,
    pub exit_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LaunchSpec {
    pub command: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<EnvVar>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceWindow {
    pub id: String,
    pub title: String,
    pub pid: Option<u32>,
    pub geometry: WindowGeometry,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub screen: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceScreenshot {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceAppLog {
    pub app_id: String,
    pub stream: String,
    pub path: PathBuf,
    pub content: String,
    pub bytes_read: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceEvent {
    pub sequence: u64,
    pub timestamp_unix: u64,
    pub kind: String,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub detail: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum IpcRequest {
    Status,
    LaunchApp {
        command: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<PathBuf>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        env: Vec<EnvVar>,
    },
    ListWindows,
    Screenshot {
        output_path: Option<PathBuf>,
    },
    FocusWindow {
        window_id: String,
    },
    CloseWindow {
        window_id: String,
    },
    Click {
        x: i32,
        y: i32,
    },
    Key {
        key: String,
    },
    TypeText {
        text: String,
    },
    ReadAppLog {
        app_id: String,
        stream: String,
        tail_bytes: Option<u64>,
    },
    ReadEvents {
        tail: Option<usize>,
    },
    KillApp {
        app_id: String,
    },
    Stop,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IpcResponse {
    pub ok: bool,
    pub message: String,
    pub status: Option<WorkspaceStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<Vec<WorkspaceWindow>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<WorkspaceScreenshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_log: Option<WorkspaceAppLog>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events: Option<Vec<WorkspaceEvent>>,
}

pub fn default_workspace_id() -> String {
    DEFAULT_WORKSPACE_ID.to_string()
}

pub fn doctor_report() -> DoctorReport {
    let runtime = RuntimeReport {
        xvfb: command_path_check("Xvfb"),
        xephyr: command_path_check("Xephyr"),
        xauth: command_path_check("xauth"),
        xdpyinfo: command_path_check("xdpyinfo"),
        window_manager: first_available_command(&["openbox", "i3", "fluxbox"]),
        xdotool: command_path_check("xdotool"),
        screenshot: first_available_command(&["import", "scrot"]),
        policy: policy_runtime_capabilities(),
    };

    let mut blockers = Vec::new();
    if !runtime.xvfb.ok && !runtime.xephyr.ok {
        blockers.push("Install Xvfb or Xephyr to create the isolated X11 display.".to_string());
    }
    if !runtime.xauth.ok {
        blockers.push(
            "Install xauth so workspace displays can use a scoped authority file.".to_string(),
        );
    }
    if !runtime.xdpyinfo.ok {
        blockers.push("Install xdpyinfo so workspace display readiness can be probed.".to_string());
    }
    if !runtime.window_manager.ok {
        blockers.push(
            "Install a lightweight window manager such as openbox, i3, or fluxbox.".to_string(),
        );
    }
    if !runtime.xdotool.ok {
        blockers.push(
            "Install xdotool for scoped input and window control inside the workspace.".to_string(),
        );
    }
    if !runtime.screenshot.ok {
        blockers.push("Install ImageMagick import or scrot for workspace screenshots.".to_string());
    }

    let ready_for_x11_workspace = blockers.is_empty();
    let recommended_next_step = if ready_for_x11_workspace {
        "Run `agent-workspace-linux workspace start`, then launch apps into the workspace."
            .to_string()
    } else {
        "Install the missing X11 workspace dependencies, then rerun `agent-workspace-linux doctor`."
            .to_string()
    };

    DoctorReport {
        runtime,
        ready_for_x11_workspace,
        blockers,
        recommended_next_step,
    }
}

pub fn policy_runtime_capabilities() -> PolicyRuntimeCapabilities {
    PolicyRuntimeCapabilities::from_tools(
        policy_tool_check("bwrap"),
        policy_tool_check("firejail"),
        policy_tool_check("unshare"),
        policy_tool_check("slirp4netns"),
    )
}

pub fn start_workspace(options: WorkspaceStartOptions) -> Result<IpcResponse> {
    match prepare_workspace_start(options)? {
        WorkspaceStartPlan::AlreadyRunning(status) => Ok(IpcResponse {
            ok: true,
            message: format!("workspace {:?} is already running", status.id),
            status: Some(status),
            windows: None,
            screenshot: None,
            app_log: None,
            events: None,
        }),
        WorkspaceStartPlan::Start(daemon_options) => {
            spawn_detached_daemon(&daemon_options)?;
            wait_for_socket(&daemon_options.socket_path)?;
            request(&daemon_options.socket_path, IpcRequest::Status)
        }
    }
}

pub fn start_workspace_foreground(options: WorkspaceStartOptions) -> Result<()> {
    match prepare_workspace_start(options)? {
        WorkspaceStartPlan::AlreadyRunning(status) => {
            bail!(
                "workspace {:?} is already running on {}",
                status.id,
                status.display
            )
        }
        WorkspaceStartPlan::Start(daemon_options) => run_daemon(daemon_options),
    }
}

pub fn status_workspace(id: &str) -> Result<WorkspaceStatus> {
    let id = sanitize_workspace_id(id)?;
    let response = request(&workspace_socket_path(&id), IpcRequest::Status)?;
    response
        .status
        .ok_or_else(|| anyhow!("workspace daemon returned no status"))
}

pub fn list_workspaces() -> Result<WorkspaceList> {
    let runtime_base_dir = runtime_base_dir();
    if !runtime_base_dir.exists() {
        return Ok(WorkspaceList {
            runtime_base_dir,
            workspaces: Vec::new(),
        });
    }

    let mut workspaces = Vec::new();
    for entry in fs::read_dir(&runtime_base_dir)
        .with_context(|| format!("failed to read {}", runtime_base_dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        if sanitize_workspace_id(&id).is_err() {
            continue;
        }
        let runtime_dir = entry.path();
        let socket_path = runtime_dir.join("control.sock");
        let status_result = status_workspace(&id);
        let (running, status, error) = match status_result {
            Ok(status) => (true, Some(status), None),
            Err(error) => (false, None, Some(error.to_string())),
        };
        workspaces.push(WorkspaceListEntry {
            id,
            runtime_dir,
            socket_path,
            running,
            status,
            error,
        });
    }

    workspaces.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(WorkspaceList {
        runtime_base_dir,
        workspaces,
    })
}

pub fn cleanup_stale_workspaces(id: Option<String>) -> Result<WorkspaceCleanup> {
    let target_id = id.map(|id| sanitize_workspace_id(&id)).transpose()?;
    let list = list_workspaces()?;
    let mut removed = Vec::new();
    let mut skipped = Vec::new();

    for workspace in list.workspaces {
        if let Some(target_id) = &target_id {
            if &workspace.id != target_id {
                continue;
            }
        }

        if workspace.running {
            skipped.push(WorkspaceCleanupEntry {
                id: workspace.id,
                runtime_dir: workspace.runtime_dir,
                reason: "workspace is running".to_string(),
            });
            continue;
        }

        match fs::remove_dir_all(&workspace.runtime_dir) {
            Ok(()) => removed.push(WorkspaceCleanupEntry {
                id: workspace.id,
                runtime_dir: workspace.runtime_dir,
                reason: "removed stale workspace runtime".to_string(),
            }),
            Err(error) => skipped.push(WorkspaceCleanupEntry {
                id: workspace.id,
                runtime_dir: workspace.runtime_dir,
                reason: error.to_string(),
            }),
        }
    }

    Ok(WorkspaceCleanup {
        runtime_base_dir: list.runtime_base_dir,
        removed,
        skipped,
    })
}

pub fn launch_app_with_spec(id: &str, spec: LaunchSpec) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    validate_launch_spec(&spec)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::LaunchApp {
            command: spec.command,
            profile_id: spec.profile_id,
            cwd: spec.cwd,
            env: spec.env,
        },
    )
}

fn validate_launch_spec(spec: &LaunchSpec) -> Result<()> {
    if spec.command.is_empty() {
        bail!("launch command cannot be empty");
    }
    if let Some(cwd) = &spec.cwd {
        if !cwd.is_dir() {
            bail!("launch cwd {} is not a directory", cwd.display());
        }
    }
    for env_var in &spec.env {
        validate_env_var(env_var)?;
    }
    Ok(())
}

pub fn list_windows(id: &str) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    request(&workspace_socket_path(&id), IpcRequest::ListWindows)
}

pub fn screenshot(id: &str, output_path: Option<PathBuf>) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::Screenshot { output_path },
    )
}

pub fn focus_window(id: &str, window_id: String) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    let window_id = sanitize_x11_id(&window_id, "window id")?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::FocusWindow { window_id },
    )
}

pub fn close_window(id: &str, window_id: String) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    let window_id = sanitize_x11_id(&window_id, "window id")?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::CloseWindow { window_id },
    )
}

pub fn click(id: &str, x: i32, y: i32) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    request(&workspace_socket_path(&id), IpcRequest::Click { x, y })
}

pub fn key(id: &str, key: String) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    if key.trim().is_empty() {
        bail!("key cannot be empty");
    }
    request(&workspace_socket_path(&id), IpcRequest::Key { key })
}

pub fn type_text(id: &str, text: String) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    if text.is_empty() {
        bail!("text cannot be empty");
    }
    request(&workspace_socket_path(&id), IpcRequest::TypeText { text })
}

pub fn read_app_log(
    id: &str,
    app_id: String,
    stream: String,
    tail_bytes: Option<u64>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    if app_id.trim().is_empty() {
        bail!("app id cannot be empty");
    }
    validate_log_stream(&stream)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::ReadAppLog {
            app_id,
            stream,
            tail_bytes,
        },
    )
}

pub fn read_events(id: &str, tail: Option<usize>) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    request(&workspace_socket_path(&id), IpcRequest::ReadEvents { tail })
}

pub fn kill_app(id: &str, app_id: String) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    if app_id.trim().is_empty() {
        bail!("app id cannot be empty");
    }
    request(&workspace_socket_path(&id), IpcRequest::KillApp { app_id })
}

pub fn stop_workspace(id: &str) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    request(&workspace_socket_path(&id), IpcRequest::Stop)
}

pub fn run_daemon(options: DaemonOptions) -> Result<()> {
    let id = sanitize_workspace_id(&options.id)?;
    fs::create_dir_all(&options.runtime_dir)
        .with_context(|| format!("failed to create {}", options.runtime_dir.display()))?;
    remove_stale_socket(&options.socket_path)?;

    let mut x_server = spawn_xvfb(&options)?;
    wait_for_display(&options.display, &options.xauthority_path)?;
    let mut window_manager = spawn_window_manager(&options)?;

    let listener = UnixListener::bind(&options.socket_path)
        .with_context(|| format!("failed to bind {}", options.socket_path.display()))?;
    let event_path = options.runtime_dir.join(EVENT_LOG_FILE);
    let mut state = DaemonState {
        status: WorkspaceStatus {
            id,
            profile_id: options.profile_id,
            applied_policy: options.applied_policy,
            user_acknowledged_hidden_workspace: options.user_acknowledged_hidden_workspace,
            user_acknowledged_unenforced_policy: options.user_acknowledged_unenforced_policy,
            ready: true,
            display: options.display,
            width: options.width,
            height: options.height,
            runtime_dir: options.runtime_dir,
            socket_path: options.socket_path,
            xauthority_path: options.xauthority_path,
            x_server_pid: x_server.id(),
            window_manager_pid: window_manager.as_ref().map(Child::id),
            apps: Vec::new(),
        },
        apps: Vec::new(),
        event_path,
        next_event_sequence: 1,
    };
    let start_detail = serde_json::json!({
        "display": &state.status.display,
        "width": state.status.width,
        "height": state.status.height,
        "profile_id": state.status.profile_id.as_deref(),
        "user_acknowledged_hidden_workspace": state.status.user_acknowledged_hidden_workspace,
        "user_acknowledged_unenforced_policy": state.status.user_acknowledged_unenforced_policy,
    });
    record_event(&mut state, "workspace_start", start_detail)?;

    eprintln!(
        "agent workspace daemon listening on {} for display {}",
        state.status.socket_path.display(),
        state.status.display
    );
    loop {
        let stream = match listener.accept() {
            Ok((stream, _addr)) => stream,
            Err(error) => {
                eprintln!("workspace IPC accept failed: {error}");
                continue;
            }
        };
        let should_stop = match handle_stream(stream, &mut state) {
            Ok(should_stop) => should_stop,
            Err(error) => {
                eprintln!("workspace IPC request failed: {error:#}");
                false
            }
        };
        if should_stop {
            break;
        }
    }

    eprintln!("agent workspace daemon stopping");
    for app in &mut state.apps {
        let _ = app.child.kill();
        let _ = app.child.wait();
    }
    if let Some(wm) = &mut window_manager {
        let _ = wm.kill();
        let _ = wm.wait();
    }
    let _ = x_server.kill();
    let _ = x_server.wait();
    let _ = fs::remove_file(&state.status.socket_path);
    Ok(())
}

enum WorkspaceStartPlan {
    AlreadyRunning(WorkspaceStatus),
    Start(DaemonOptions),
}

fn prepare_workspace_start(options: WorkspaceStartOptions) -> Result<WorkspaceStartPlan> {
    let id = sanitize_workspace_id(&options.id)?;
    if let Ok(status) = status_workspace(&id) {
        return Ok(WorkspaceStartPlan::AlreadyRunning(status));
    }
    if !options.user_acknowledged_hidden_workspace {
        bail!(
            "starting a hidden agent workspace requires explicit acknowledgement; pass --ack-hidden-workspace or set acknowledge_hidden_workspace=true"
        );
    }
    if options
        .applied_policy
        .as_ref()
        .is_some_and(AppliedWorkspacePolicy::has_requested_unenforced_policy)
        && !options.user_acknowledged_unenforced_policy
    {
        bail!(
            "profile requests mount or network policy that is not enforced by this X11 runtime; pass --ack-unenforced-policy or set acknowledge_unenforced_policy=true"
        );
    }

    let runtime = doctor_report();
    if !runtime.ready_for_x11_workspace {
        bail!(
            "workspace runtime is not ready: {}",
            runtime.blockers.join("; ")
        );
    }

    let runtime_dir = workspace_dir(&id);
    fs::create_dir_all(&runtime_dir)
        .with_context(|| format!("failed to create {}", runtime_dir.display()))?;
    let socket_path = runtime_dir.join("control.sock");
    remove_stale_socket(&socket_path)?;
    let xauthority_path = runtime_dir.join("Xauthority");
    let display = pick_display()?;
    create_xauthority(&display, &xauthority_path)?;

    Ok(WorkspaceStartPlan::Start(DaemonOptions {
        id,
        profile_id: options.profile_id,
        applied_policy: options.applied_policy,
        user_acknowledged_hidden_workspace: options.user_acknowledged_hidden_workspace,
        user_acknowledged_unenforced_policy: options.user_acknowledged_unenforced_policy,
        display,
        width: options.width,
        height: options.height,
        runtime_dir,
        socket_path,
        xauthority_path,
    }))
}

fn spawn_detached_daemon(options: &DaemonOptions) -> Result<()> {
    let stdout_path = options.runtime_dir.join("daemon.out.log");
    let stderr_path = options.runtime_dir.join("daemon.err.log");
    let exe = env::current_exe().context("failed to resolve current executable")?;
    let mut daemon = Command::new("setsid");
    daemon.arg(exe).arg("daemon").arg("--id").arg(&options.id);
    if let Some(profile_id) = &options.profile_id {
        daemon.arg("--profile").arg(profile_id);
    }
    if let Some(policy) = &options.applied_policy {
        let policy_path = write_applied_policy_file(&options.runtime_dir, policy)?;
        daemon.arg("--policy").arg(policy_path);
    }
    if options.user_acknowledged_hidden_workspace {
        daemon.arg("--ack-hidden-workspace");
    }
    if options.user_acknowledged_unenforced_policy {
        daemon.arg("--ack-unenforced-policy");
    }
    daemon
        .arg("--display")
        .arg(&options.display)
        .arg("--width")
        .arg(options.width.to_string())
        .arg("--height")
        .arg(options.height.to_string())
        .arg("--runtime-dir")
        .arg(&options.runtime_dir)
        .arg("--socket")
        .arg(&options.socket_path)
        .arg("--xauthority")
        .arg(&options.xauthority_path)
        .stdin(Stdio::null())
        .stdout(Stdio::from(fs::File::create(&stdout_path).with_context(
            || format!("failed to create {}", stdout_path.display()),
        )?))
        .stderr(Stdio::from(fs::File::create(&stderr_path).with_context(
            || format!("failed to create {}", stderr_path.display()),
        )?));

    daemon
        .spawn()
        .context("failed to spawn agent workspace daemon")?;
    Ok(())
}

fn write_applied_policy_file(
    runtime_dir: &Path,
    policy: &AppliedWorkspacePolicy,
) -> Result<PathBuf> {
    let policy_path = runtime_dir.join(APPLIED_POLICY_FILE);
    let content =
        serde_json::to_string_pretty(policy).context("failed to serialize applied policy")?;
    fs::write(&policy_path, format!("{content}\n"))
        .with_context(|| format!("failed to write {}", policy_path.display()))?;
    Ok(policy_path)
}

fn record_event(
    state: &mut DaemonState,
    kind: &str,
    detail: serde_json::Value,
) -> Result<WorkspaceEvent> {
    let event = WorkspaceEvent {
        sequence: state.next_event_sequence,
        timestamp_unix: unix_now(),
        kind: kind.to_string(),
        detail,
    };
    state.next_event_sequence += 1;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&state.event_path)
        .with_context(|| format!("failed to open {}", state.event_path.display()))?;
    serde_json::to_writer(&mut file, &event).context("failed to serialize workspace event")?;
    file.write_all(b"\n")
        .with_context(|| format!("failed to write {}", state.event_path.display()))?;
    Ok(event)
}

fn read_event_log(path: &Path, tail: Option<usize>) -> Result<Vec<WorkspaceEvent>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut events = Vec::new();
    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        events.push(
            serde_json::from_str(line)
                .with_context(|| format!("failed to parse event in {}", path.display()))?,
        );
    }
    if let Some(tail) = tail {
        let start = events.len().saturating_sub(tail);
        Ok(events.split_off(start))
    } else {
        Ok(events)
    }
}

fn response_with_status(
    ok: bool,
    message: impl Into<String>,
    status: &WorkspaceStatus,
) -> IpcResponse {
    IpcResponse {
        ok,
        message: message.into(),
        status: Some(status.clone()),
        windows: None,
        screenshot: None,
        app_log: None,
        events: None,
    }
}

struct DaemonState {
    status: WorkspaceStatus,
    apps: Vec<AppProcess>,
    event_path: PathBuf,
    next_event_sequence: u64,
}

struct AppProcess {
    info: WorkspaceApp,
    child: Child,
}

fn handle_stream(mut stream: UnixStream, state: &mut DaemonState) -> Result<bool> {
    let mut line = String::new();
    {
        let mut reader = BufReader::new(&stream);
        reader.read_line(&mut line)?;
    }
    let request: IpcRequest =
        serde_json::from_str(line.trim()).context("failed to parse workspace IPC request")?;
    refresh_apps(state);

    let (response, should_stop) = match request {
        IpcRequest::Status => (
            response_with_status(true, "workspace is running", &state.status),
            false,
        ),
        IpcRequest::LaunchApp {
            command,
            profile_id,
            cwd,
            env,
        } => match spawn_app(
            state,
            LaunchSpec {
                command,
                profile_id,
                cwd,
                env,
            },
        ) {
            Ok(app) => {
                record_event(
                    state,
                    "app_launch",
                    serde_json::json!({
                        "app_id": &app.id,
                        "pid": app.pid,
                        "command": &app.command,
                        "profile_id": app.profile_id.as_deref(),
                        "cwd": app.cwd.as_ref().map(|path| path.display().to_string()),
                    }),
                )?;
                (
                    response_with_status(true, "app launched in workspace", &state.status),
                    false,
                )
            }
            Err(error) => (
                response_with_status(false, error.to_string(), &state.status),
                false,
            ),
        },
        IpcRequest::ListWindows => match list_workspace_windows(&state.status) {
            Ok(windows) => {
                record_event(
                    state,
                    "list_windows",
                    serde_json::json!({ "count": windows.len() }),
                )?;
                let mut response =
                    response_with_status(true, "workspace windows listed", &state.status);
                response.windows = Some(windows);
                (response, false)
            }
            Err(error) => (
                response_with_status(false, error.to_string(), &state.status),
                false,
            ),
        },
        IpcRequest::Screenshot { output_path } => {
            match capture_workspace_screenshot(&state.status, output_path) {
                Ok(screenshot) => {
                    record_event(
                        state,
                        "screenshot",
                        serde_json::json!({ "path": screenshot.path.display().to_string() }),
                    )?;
                    let mut response =
                        response_with_status(true, "workspace screenshot captured", &state.status);
                    response.screenshot = Some(screenshot);
                    (response, false)
                }
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
            }
        }
        IpcRequest::FocusWindow { window_id } => {
            match focus_workspace_window(&state.status, &window_id) {
                Ok(()) => {
                    record_event(
                        state,
                        "focus_window",
                        serde_json::json!({ "window_id": &window_id }),
                    )?;
                    (
                        response_with_status(true, "workspace window focused", &state.status),
                        false,
                    )
                }
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
            }
        }
        IpcRequest::CloseWindow { window_id } => {
            match close_workspace_window(&state.status, &window_id) {
                Ok(()) => {
                    record_event(
                        state,
                        "close_window",
                        serde_json::json!({ "window_id": &window_id }),
                    )?;
                    (
                        response_with_status(
                            true,
                            "workspace window close requested",
                            &state.status,
                        ),
                        false,
                    )
                }
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
            }
        }
        IpcRequest::Click { x, y } => match click_workspace(&state.status, x, y) {
            Ok(()) => {
                record_event(state, "click", serde_json::json!({ "x": x, "y": y }))?;
                (
                    response_with_status(true, "workspace click sent", &state.status),
                    false,
                )
            }
            Err(error) => (
                response_with_status(false, error.to_string(), &state.status),
                false,
            ),
        },
        IpcRequest::Key { key } => {
            let logged_key = key.trim().to_string();
            match key_workspace(&state.status, key) {
                Ok(()) => {
                    record_event(state, "key", serde_json::json!({ "key": logged_key }))?;
                    (
                        response_with_status(true, "workspace key sent", &state.status),
                        false,
                    )
                }
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
            }
        }
        IpcRequest::TypeText { text } => {
            let char_count = text.chars().count();
            match type_workspace_text(&state.status, text) {
                Ok(()) => {
                    record_event(
                        state,
                        "type_text",
                        serde_json::json!({ "char_count": char_count }),
                    )?;
                    (
                        response_with_status(true, "workspace text typed", &state.status),
                        false,
                    )
                }
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
            }
        }
        IpcRequest::ReadAppLog {
            app_id,
            stream,
            tail_bytes,
        } => match read_workspace_app_log(state, &app_id, &stream, tail_bytes) {
            Ok(app_log) => {
                record_event(
                    state,
                    "read_app_log",
                    serde_json::json!({
                        "app_id": &app_log.app_id,
                        "stream": &app_log.stream,
                        "tail_bytes": tail_bytes,
                        "bytes_read": app_log.bytes_read,
                        "truncated": app_log.truncated,
                    }),
                )?;
                let mut response =
                    response_with_status(true, "workspace app log read", &state.status);
                response.app_log = Some(app_log);
                (response, false)
            }
            Err(error) => (
                response_with_status(false, error.to_string(), &state.status),
                false,
            ),
        },
        IpcRequest::ReadEvents { tail } => match read_event_log(&state.event_path, tail) {
            Ok(events) => {
                let mut response =
                    response_with_status(true, "workspace events returned", &state.status);
                response.events = Some(events);
                (response, false)
            }
            Err(error) => (
                response_with_status(false, error.to_string(), &state.status),
                false,
            ),
        },
        IpcRequest::KillApp { app_id } => match kill_workspace_app(state, &app_id) {
            Ok(message) => {
                record_event(
                    state,
                    "kill_app",
                    serde_json::json!({ "target": &app_id, "message": &message }),
                )?;
                (response_with_status(true, message, &state.status), false)
            }
            Err(error) => (
                response_with_status(false, error.to_string(), &state.status),
                false,
            ),
        },
        IpcRequest::Stop => {
            record_event(state, "workspace_stop", serde_json::json!({}))?;
            (
                response_with_status(true, "workspace stopping", &state.status),
                true,
            )
        }
    };

    serde_json::to_writer(&mut stream, &response)?;
    stream.write_all(b"\n")?;
    Ok(should_stop)
}

fn spawn_app(state: &mut DaemonState, spec: LaunchSpec) -> Result<WorkspaceApp> {
    validate_launch_spec(&spec)?;
    let log_paths = prepare_app_log_paths(&state.status.runtime_dir)?;
    let mut child_command = Command::new(&spec.command[0]);
    child_command.args(&spec.command[1..]);
    child_command
        .env("DISPLAY", &state.status.display)
        .env("XAUTHORITY", &state.status.xauthority_path)
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            fs::File::create(&log_paths.stdout)
                .with_context(|| format!("failed to create {}", log_paths.stdout.display()))?,
        ))
        .stderr(Stdio::from(
            fs::File::create(&log_paths.stderr)
                .with_context(|| format!("failed to create {}", log_paths.stderr.display()))?,
        ));
    if let Some(cwd) = &spec.cwd {
        child_command.current_dir(cwd);
    }
    for env_var in &spec.env {
        child_command.env(&env_var.name, &env_var.value);
    }
    let child = child_command
        .spawn()
        .with_context(|| format!("failed to launch {}", spec.command.join(" ")))?;
    let pid = child.id();
    let stdout_path = rename_app_log(&log_paths.stdout, pid, "stdout")?;
    let stderr_path = rename_app_log(&log_paths.stderr, pid, "stderr")?;
    let info = WorkspaceApp {
        id: format!("app-{pid}"),
        pid,
        profile_id: spec.profile_id,
        command: spec.command,
        cwd: spec.cwd,
        env: spec.env,
        stdout_path: Some(stdout_path),
        stderr_path: Some(stderr_path),
        started_at_unix: unix_now(),
        running: true,
        exit_status: None,
    };
    state.status.apps.push(info.clone());
    state.apps.push(AppProcess {
        info: info.clone(),
        child,
    });
    Ok(info)
}

struct AppLogPaths {
    stdout: PathBuf,
    stderr: PathBuf,
}

fn prepare_app_log_paths(runtime_dir: &Path) -> Result<AppLogPaths> {
    let log_dir = runtime_dir.join("apps");
    fs::create_dir_all(&log_dir)
        .with_context(|| format!("failed to create {}", log_dir.display()))?;
    let prefix = format!("launch-{}", unix_now_millis());
    Ok(AppLogPaths {
        stdout: log_dir.join(format!("{prefix}.stdout.log")),
        stderr: log_dir.join(format!("{prefix}.stderr.log")),
    })
}

fn rename_app_log(path: &Path, pid: u32, stream: &str) -> Result<PathBuf> {
    let target = path
        .parent()
        .ok_or_else(|| anyhow!("app log path has no parent: {}", path.display()))?
        .join(format!("app-{pid}.{stream}.log"));
    fs::rename(path, &target).with_context(|| {
        format!(
            "failed to move app log {} to {}",
            path.display(),
            target.display()
        )
    })?;
    Ok(target)
}

fn list_workspace_windows(status: &WorkspaceStatus) -> Result<Vec<WorkspaceWindow>> {
    let output = workspace_command(status, "xdotool")
        .args(["search", "--onlyvisible", "--name", "."])
        .output()
        .context("failed to run xdotool window search")?;
    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| {
            let id = line.trim();
            (!id.is_empty()).then(|| window_info(status, id))
        })
        .collect()
}

fn window_info(status: &WorkspaceStatus, id: &str) -> Result<WorkspaceWindow> {
    let title = workspace_command(status, "xdotool")
        .args(["getwindowname", id])
        .output()
        .with_context(|| format!("failed to read window name for {id}"))
        .and_then(|output| output_text(output, "xdotool getwindowname"))
        .unwrap_or_default()
        .trim()
        .to_string();
    let pid = workspace_command(status, "xdotool")
        .args(["getwindowpid", id])
        .output()
        .ok()
        .and_then(|output| output.status.success().then_some(output.stdout))
        .and_then(|stdout| String::from_utf8(stdout).ok())
        .and_then(|text| text.trim().parse::<u32>().ok());
    let geometry_output = workspace_command(status, "xdotool")
        .args(["getwindowgeometry", "--shell", id])
        .output()
        .with_context(|| format!("failed to read window geometry for {id}"))?;
    let geometry_text = output_text(geometry_output, "xdotool getwindowgeometry")?;

    Ok(WorkspaceWindow {
        id: id.to_string(),
        title,
        pid,
        geometry: parse_window_geometry(&geometry_text)?,
    })
}

fn parse_window_geometry(text: &str) -> Result<WindowGeometry> {
    let mut x = None;
    let mut y = None;
    let mut width = None;
    let mut height = None;
    let mut screen = None;

    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "X" => x = value.parse::<i32>().ok(),
            "Y" => y = value.parse::<i32>().ok(),
            "WIDTH" => width = value.parse::<u32>().ok(),
            "HEIGHT" => height = value.parse::<u32>().ok(),
            "SCREEN" => screen = value.parse::<i32>().ok(),
            _ => {}
        }
    }

    Ok(WindowGeometry {
        x: x.context("window geometry missing X")?,
        y: y.context("window geometry missing Y")?,
        width: width.context("window geometry missing WIDTH")?,
        height: height.context("window geometry missing HEIGHT")?,
        screen,
    })
}

fn capture_workspace_screenshot(
    status: &WorkspaceStatus,
    output_path: Option<PathBuf>,
) -> Result<WorkspaceScreenshot> {
    let path = resolve_screenshot_path(status, output_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    if command_path_check("import").ok {
        let output = workspace_command(status, "import")
            .args(["-window", "root"])
            .arg(&path)
            .output()
            .context("failed to run import for workspace screenshot")?;
        output_text(output, "import -window root")?;
    } else if command_path_check("scrot").ok {
        let output = workspace_command(status, "scrot")
            .arg(&path)
            .output()
            .context("failed to run scrot for workspace screenshot")?;
        output_text(output, "scrot")?;
    } else {
        bail!("missing screenshot command: install ImageMagick import or scrot");
    }

    Ok(WorkspaceScreenshot {
        path,
        width: status.width,
        height: status.height,
        format: "png".to_string(),
    })
}

fn resolve_screenshot_path(status: &WorkspaceStatus, output_path: Option<PathBuf>) -> PathBuf {
    match output_path {
        Some(path) if path.is_absolute() => path,
        Some(path) => status.runtime_dir.join(path),
        None => status
            .runtime_dir
            .join(format!("screenshot-{}.png", unix_now())),
    }
}

fn focus_workspace_window(status: &WorkspaceStatus, window_id: &str) -> Result<()> {
    let window_id = sanitize_x11_id(window_id, "window id")?;
    let output = workspace_command(status, "xdotool")
        .args(["windowactivate", "--sync", &window_id])
        .output()
        .context("failed to run xdotool windowactivate")?;
    output_text(output, "xdotool windowactivate")?;
    Ok(())
}

fn close_workspace_window(status: &WorkspaceStatus, window_id: &str) -> Result<()> {
    let window_id = sanitize_x11_id(window_id, "window id")?;
    let output = workspace_command(status, "xdotool")
        .args(["windowclose", &window_id])
        .output()
        .context("failed to run xdotool windowclose")?;
    output_text(output, "xdotool windowclose")?;
    Ok(())
}

fn click_workspace(status: &WorkspaceStatus, x: i32, y: i32) -> Result<()> {
    if x < 0 || y < 0 || x as u32 >= status.width || y as u32 >= status.height {
        bail!(
            "click coordinates {x},{y} are outside workspace bounds {}x{}",
            status.width,
            status.height
        );
    }
    let output = workspace_command(status, "xdotool")
        .args(["mousemove", "--sync", &x.to_string(), &y.to_string()])
        .args(["click", "1"])
        .output()
        .context("failed to run xdotool click")?;
    output_text(output, "xdotool click")?;
    Ok(())
}

fn key_workspace(status: &WorkspaceStatus, key: String) -> Result<()> {
    if key.trim().is_empty() {
        bail!("key cannot be empty");
    }
    let output = workspace_command(status, "xdotool")
        .args(["key", "--clearmodifiers", key.trim()])
        .output()
        .context("failed to run xdotool key")?;
    output_text(output, "xdotool key")?;
    Ok(())
}

fn type_workspace_text(status: &WorkspaceStatus, text: String) -> Result<()> {
    if text.is_empty() {
        bail!("text cannot be empty");
    }
    let output = workspace_command(status, "xdotool")
        .args(["type", "--clearmodifiers", "--delay", "1", &text])
        .output()
        .context("failed to run xdotool type")?;
    output_text(output, "xdotool type")?;
    Ok(())
}

fn read_workspace_app_log(
    state: &mut DaemonState,
    app_id: &str,
    stream: &str,
    tail_bytes: Option<u64>,
) -> Result<WorkspaceAppLog> {
    refresh_apps(state);
    let stream = validate_log_stream(stream)?;
    let app = state
        .status
        .apps
        .iter()
        .find(|app| matches_app_id(app, app_id))
        .ok_or_else(|| anyhow!("workspace app {app_id:?} was not found"))?;
    let path = match stream.as_str() {
        "stdout" => app.stdout_path.as_ref(),
        "stderr" => app.stderr_path.as_ref(),
        _ => None,
    }
    .ok_or_else(|| anyhow!("workspace app {} has no {stream} log path", app.id))?;
    let (content, bytes_read, truncated) = read_log_content(path, tail_bytes)?;

    Ok(WorkspaceAppLog {
        app_id: app.id.clone(),
        stream,
        path: path.clone(),
        content,
        bytes_read,
        truncated,
    })
}

fn read_log_content(path: &Path, tail_bytes: Option<u64>) -> Result<(String, u64, bool)> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let total = bytes.len();
    let limit = tail_bytes
        .map(|value| value.min(usize::MAX as u64) as usize)
        .unwrap_or(total);
    let start = total.saturating_sub(limit);
    let content = String::from_utf8_lossy(&bytes[start..]).to_string();
    Ok((content, (total - start) as u64, start > 0))
}

fn kill_workspace_app(state: &mut DaemonState, app_id: &str) -> Result<String> {
    let app_id = app_id.trim();
    if app_id.is_empty() {
        bail!("app id cannot be empty");
    }

    let message = {
        let app = state
            .apps
            .iter_mut()
            .find(|app| matches_app_id(&app.info, app_id))
            .ok_or_else(|| anyhow!("workspace app {app_id:?} was not found"))?;

        if !app.info.running {
            format!("workspace app {} is already stopped", app.info.id)
        } else if let Some(status) = app
            .child
            .try_wait()
            .context("failed to check app process status")?
        {
            app.info.running = false;
            app.info.exit_status = Some(status.to_string());
            format!("workspace app {} is already stopped", app.info.id)
        } else {
            app.child
                .kill()
                .with_context(|| format!("failed to kill workspace app {}", app.info.id))?;
            let status = app
                .child
                .wait()
                .with_context(|| format!("failed to wait for workspace app {}", app.info.id))?;
            app.info.running = false;
            app.info.exit_status = Some(status.to_string());
            format!("workspace app {} killed", app.info.id)
        }
    };

    state.status.apps = state.apps.iter().map(|app| app.info.clone()).collect();
    Ok(message)
}

fn matches_app_id(app: &WorkspaceApp, app_id: &str) -> bool {
    app.id == app_id || app.pid.to_string() == app_id
}

fn workspace_command(status: &WorkspaceStatus, program: &str) -> Command {
    let mut command = Command::new(program);
    command
        .env("DISPLAY", &status.display)
        .env("XAUTHORITY", &status.xauthority_path)
        .stdin(Stdio::null());
    command
}

fn output_text(output: std::process::Output, description: &str) -> Result<String> {
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        bail!("{description} failed: {detail}");
    }
}

fn refresh_apps(state: &mut DaemonState) {
    for app in &mut state.apps {
        if app.info.running {
            match app.child.try_wait() {
                Ok(Some(status)) => {
                    app.info.running = false;
                    app.info.exit_status = Some(status.to_string());
                }
                Ok(None) => {}
                Err(error) => {
                    app.info.running = false;
                    app.info.exit_status = Some(error.to_string());
                }
            }
        }
    }
    state.status.apps = state.apps.iter().map(|app| app.info.clone()).collect();
}

fn request(socket_path: &Path, request: IpcRequest) -> Result<IpcResponse> {
    let mut stream = UnixStream::connect(socket_path)
        .with_context(|| format!("failed to connect to {}", socket_path.display()))?;
    serde_json::to_writer(&mut stream, &request)?;
    stream.write_all(b"\n")?;

    let mut line = String::new();
    let mut reader = BufReader::new(stream);
    reader.read_line(&mut line)?;
    let response: IpcResponse =
        serde_json::from_str(line.trim()).context("failed to parse workspace IPC response")?;
    Ok(response)
}

fn spawn_xvfb(options: &DaemonOptions) -> Result<Child> {
    Command::new("Xvfb")
        .arg(&options.display)
        .args(["-screen", "0"])
        .arg(format!("{}x{}x24", options.width, options.height))
        .args(["-nolisten", "tcp"])
        .arg("-auth")
        .arg(&options.xauthority_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to start Xvfb")
}

fn spawn_window_manager(options: &DaemonOptions) -> Result<Option<Child>> {
    let Some(command) = first_available_command_name(&["openbox", "i3", "fluxbox"]) else {
        bail!("missing window manager: install openbox, i3, or fluxbox");
    };
    let child = Command::new(command)
        .env("DISPLAY", &options.display)
        .env("XAUTHORITY", &options.xauthority_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to start {command}"))?;
    Ok(Some(child))
}

fn wait_for_display(display: &str, xauthority: &Path) -> Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        let ok = Command::new("xdpyinfo")
            .arg("-display")
            .arg(display)
            .env("XAUTHORITY", xauthority)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if ok {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    bail!("timed out waiting for X display {display}");
}

fn wait_for_socket(socket_path: &Path) -> Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if socket_path.exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    bail!("timed out waiting for workspace IPC socket");
}

fn pick_display() -> Result<String> {
    for number in DISPLAY_RANGE {
        let display = format!(":{number}");
        let socket = PathBuf::from(format!("/tmp/.X11-unix/X{number}"));
        if socket.exists() {
            continue;
        }
        let in_use = Command::new("xdpyinfo")
            .arg("-display")
            .arg(&display)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !in_use {
            return Ok(display);
        }
    }
    bail!("no free X11 display found in range :90..:179");
}

fn create_xauthority(display: &str, path: &Path) -> Result<()> {
    let cookie = random_cookie()?;
    let _ = fs::remove_file(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    let output = Command::new("xauth")
        .arg("-f")
        .arg(path)
        .arg("add")
        .arg(display)
        .arg(".")
        .arg(cookie)
        .output()
        .context("failed to run xauth")?;
    if output.status.success() {
        Ok(())
    } else {
        bail!(
            "xauth failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
}

fn random_cookie() -> Result<String> {
    let mut file = fs::File::open("/dev/urandom").context("failed to open /dev/urandom")?;
    let mut bytes = [0_u8; 16];
    file.read_exact(&mut bytes)
        .context("failed to read random X authority cookie")?;
    Ok(bytes
        .into_iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>())
}

fn remove_stale_socket(socket_path: &Path) -> Result<()> {
    if socket_path.exists() {
        fs::remove_file(socket_path)
            .with_context(|| format!("failed to remove {}", socket_path.display()))?;
    }
    Ok(())
}

fn workspace_socket_path(id: &str) -> PathBuf {
    workspace_dir(id).join("control.sock")
}

fn workspace_dir(id: &str) -> PathBuf {
    runtime_base_dir().join(id)
}

fn runtime_base_dir() -> PathBuf {
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let user = env::var("USER").unwrap_or_else(|_| "user".to_string());
            PathBuf::from(format!("/tmp/agent-workspace-linux-{user}"))
        })
        .join("agent-workspace-linux")
}

fn sanitize_workspace_id(id: &str) -> Result<String> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        bail!("workspace id cannot be empty");
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        bail!("workspace id may only contain ASCII letters, numbers, '-' and '_'");
    }
    Ok(trimmed.to_string())
}

fn sanitize_x11_id(id: &str, label: &str) -> Result<String> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        bail!("{label} cannot be empty");
    }
    if !trimmed.chars().all(|ch| ch.is_ascii_digit()) {
        bail!("{label} must be a decimal X11 id");
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

fn validate_log_stream(stream: &str) -> Result<String> {
    match stream.trim() {
        "stdout" => Ok("stdout".to_string()),
        "stderr" => Ok("stderr".to_string()),
        _ => bail!("log stream must be 'stdout' or 'stderr'"),
    }
}

fn first_available_command(commands: &[&str]) -> Check {
    for command in commands {
        let check = command_path_check(command);
        if check.ok {
            return check;
        }
    }

    Check {
        ok: false,
        detail: format!("missing all of: {}", commands.join(", ")),
    }
}

fn policy_tool_check(command: &str) -> PolicyToolCheck {
    let check = command_path_check(command);
    PolicyToolCheck {
        ok: check.ok,
        detail: check.detail,
    }
}

fn first_available_command_name<'a>(commands: &'a [&str]) -> Option<&'a str> {
    commands
        .iter()
        .find(|command| command_path_check(command).ok)
        .copied()
}

fn command_path_check(command: &str) -> Check {
    match Command::new("sh")
        .args(["-c", &format!("command -v {command}")])
        .output()
    {
        Ok(output) if output.status.success() => {
            let detail = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Check {
                ok: true,
                detail: if detail.is_empty() {
                    "ok".to_string()
                } else {
                    detail
                },
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let detail = if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                format!("missing: {command}")
            };
            Check { ok: false, detail }
        }
        Err(error) => Check {
            ok: false,
            detail: error.to_string(),
        },
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn unix_now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}
