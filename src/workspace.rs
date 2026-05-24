use crate::policy::{
    AppliedWorkspacePolicy, NetworkMode, PolicyRuntimeCapabilities, PolicyToolCheck,
};
use anyhow::{anyhow, bail, Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    env, fs,
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    os::unix::process::{CommandExt, ExitStatusExt},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    str::FromStr,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const DEFAULT_WORKSPACE_ID: &str = "default";
const IPC_PROTOCOL_NAME: &str = "agent-workspace-linux.ipc";
const IPC_PROTOCOL_VERSION: u32 = 1;
const DEFAULT_APP_WAIT_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_STOP_WAIT_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_CLICK_BUTTON: u8 = 1;
const DEFAULT_CLICK_COUNT: u8 = 1;
const DEFAULT_SCROLL_AMOUNT: u8 = 1;
const MAX_SCROLL_AMOUNT: u8 = 100;
const DEFAULT_PASTE_KEY: &str = "ctrl+v";
const DEFAULT_WIDTH: u32 = 1280;
const DEFAULT_HEIGHT: u32 = 720;
const DISPLAY_RANGE: std::ops::Range<u32> = 90..180;
const APP_TERMINATE_GRACE_MS: u64 = 1_000;
const SIGTERM: i32 = 15;
const SIGKILL: i32 = 9;
const ESRCH: i32 = 3;
const APPLIED_POLICY_FILE: &str = "applied_policy.json";
const EVENT_LOG_FILE: &str = "events.jsonl";

unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

impl ScrollDirection {
    fn as_str(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
        }
    }

    fn x11_button(self) -> u8 {
        match self {
            Self::Up => 4,
            Self::Down => 5,
            Self::Left => 6,
            Self::Right => 7,
        }
    }
}

impl FromStr for ScrollDirection {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "up" => Ok(Self::Up),
            "down" => Ok(Self::Down),
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            _ => bail!("scroll direction must be up, down, left, or right"),
        }
    }
}

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
    pub xprop: Check,
    pub window_manager: Check,
    pub xdotool: Check,
    pub screenshot: Check,
    pub clipboard: Check,
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
    pub started_at_unix: u64,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub pid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub mount_isolation: String,
    pub network_isolation: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_signal: Option<i32>,
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
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_policy: Option<AppliedWorkspacePolicy>,
    #[serde(default)]
    pub user_acknowledged_unenforced_policy: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<EnvVar>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceWindow {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wm_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wm_instance: Option<String>,
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    pub visible: bool,
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
pub struct WorkspaceIpcInfo {
    pub protocol: String,
    pub protocol_version: u32,
    pub server_version: String,
    pub workspace_id: String,
    pub socket_path: PathBuf,
    pub transport: String,
    pub framing: String,
    pub encoding: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceClipboard {
    pub selection: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WorkspaceRun {
    pub app_id: String,
    pub launch: IpcResponse,
    pub wait: IpcResponse,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kill: Option<IpcResponse>,
    pub stdout: WorkspaceAppLog,
    pub stderr: WorkspaceAppLog,
    pub completed: bool,
    pub succeeded: bool,
    pub timed_out: bool,
    pub killed_on_timeout: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_signal: Option<i32>,
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
    IpcInfo,
    Status,
    LaunchApp {
        command: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        applied_policy: Option<AppliedWorkspacePolicy>,
        #[serde(default)]
        user_acknowledged_unenforced_policy: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<PathBuf>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        env: Vec<EnvVar>,
    },
    ListApps {
        app_id: Option<String>,
        name_contains: Option<String>,
        command_contains: Option<String>,
        profile_id: Option<String>,
        running: Option<bool>,
    },
    ListWindows {
        #[serde(default)]
        include_hidden: bool,
        title_contains: Option<String>,
        class_contains: Option<String>,
        pid: Option<u32>,
        app_id: Option<String>,
    },
    ActiveWindow,
    Observe {
        screenshot: bool,
        #[serde(default)]
        include_hidden: bool,
        output_path: Option<PathBuf>,
    },
    WaitWindow {
        title_contains: Option<String>,
        class_contains: Option<String>,
        pid: Option<u32>,
        app_id: Option<String>,
        timeout_ms: u64,
    },
    Screenshot {
        output_path: Option<PathBuf>,
    },
    ScreenshotWindow {
        window_id: Option<String>,
        title_contains: Option<String>,
        class_contains: Option<String>,
        pid: Option<u32>,
        app_id: Option<String>,
        output_path: Option<PathBuf>,
        timeout_ms: u64,
    },
    FocusWindow {
        window_id: String,
    },
    FocusMatchingWindow {
        title_contains: Option<String>,
        class_contains: Option<String>,
        pid: Option<u32>,
        app_id: Option<String>,
        timeout_ms: u64,
    },
    CloseWindow {
        window_id: String,
    },
    CloseMatchingWindow {
        title_contains: Option<String>,
        class_contains: Option<String>,
        pid: Option<u32>,
        app_id: Option<String>,
        timeout_ms: u64,
    },
    MoveWindow {
        window_id: Option<String>,
        title_contains: Option<String>,
        class_contains: Option<String>,
        pid: Option<u32>,
        app_id: Option<String>,
        x: i32,
        y: i32,
        timeout_ms: u64,
    },
    ResizeWindow {
        window_id: Option<String>,
        title_contains: Option<String>,
        class_contains: Option<String>,
        pid: Option<u32>,
        app_id: Option<String>,
        width: u32,
        height: u32,
        timeout_ms: u64,
    },
    RaiseWindow {
        window_id: Option<String>,
        title_contains: Option<String>,
        class_contains: Option<String>,
        pid: Option<u32>,
        app_id: Option<String>,
        timeout_ms: u64,
    },
    MinimizeWindow {
        window_id: Option<String>,
        title_contains: Option<String>,
        class_contains: Option<String>,
        pid: Option<u32>,
        app_id: Option<String>,
        timeout_ms: u64,
    },
    ShowWindow {
        window_id: Option<String>,
        title_contains: Option<String>,
        class_contains: Option<String>,
        pid: Option<u32>,
        app_id: Option<String>,
        timeout_ms: u64,
    },
    Click {
        x: i32,
        y: i32,
        button: u8,
        count: u8,
    },
    ClickWindow {
        window_id: Option<String>,
        title_contains: Option<String>,
        class_contains: Option<String>,
        pid: Option<u32>,
        app_id: Option<String>,
        x: i32,
        y: i32,
        button: u8,
        count: u8,
        timeout_ms: u64,
    },
    MovePointer {
        x: i32,
        y: i32,
    },
    MovePointerWindow {
        window_id: Option<String>,
        title_contains: Option<String>,
        class_contains: Option<String>,
        pid: Option<u32>,
        app_id: Option<String>,
        x: i32,
        y: i32,
        timeout_ms: u64,
    },
    Drag {
        from_x: i32,
        from_y: i32,
        to_x: i32,
        to_y: i32,
        button: u8,
    },
    DragWindow {
        window_id: Option<String>,
        title_contains: Option<String>,
        class_contains: Option<String>,
        pid: Option<u32>,
        app_id: Option<String>,
        from_x: i32,
        from_y: i32,
        to_x: i32,
        to_y: i32,
        button: u8,
        timeout_ms: u64,
    },
    Scroll {
        x: i32,
        y: i32,
        direction: ScrollDirection,
        amount: u8,
    },
    ScrollWindow {
        window_id: Option<String>,
        title_contains: Option<String>,
        class_contains: Option<String>,
        pid: Option<u32>,
        app_id: Option<String>,
        x: i32,
        y: i32,
        direction: ScrollDirection,
        amount: u8,
        timeout_ms: u64,
    },
    Key {
        key: String,
    },
    KeyWindow {
        window_id: Option<String>,
        title_contains: Option<String>,
        class_contains: Option<String>,
        pid: Option<u32>,
        app_id: Option<String>,
        key: String,
        timeout_ms: u64,
    },
    TypeText {
        text: String,
    },
    TypeWindow {
        window_id: Option<String>,
        title_contains: Option<String>,
        class_contains: Option<String>,
        pid: Option<u32>,
        app_id: Option<String>,
        text: String,
        timeout_ms: u64,
    },
    SetClipboard {
        text: String,
    },
    GetClipboard,
    PasteText {
        text: String,
        key: String,
    },
    PasteWindow {
        window_id: Option<String>,
        title_contains: Option<String>,
        class_contains: Option<String>,
        pid: Option<u32>,
        app_id: Option<String>,
        text: String,
        key: String,
        timeout_ms: u64,
    },
    ReadAppLog {
        app_id: String,
        stream: String,
        tail_bytes: Option<u64>,
    },
    WaitApp {
        app_id: String,
        timeout_ms: u64,
        #[serde(default)]
        kill_on_timeout: bool,
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
    pub ipc: Option<WorkspaceIpcInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apps: Option<Vec<WorkspaceApp>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<Vec<WorkspaceWindow>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_window: Option<WorkspaceWindow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<WorkspaceScreenshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_log: Option<WorkspaceAppLog>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clipboard: Option<WorkspaceClipboard>,
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
        xprop: command_path_check("xprop"),
        window_manager: first_available_command(&["openbox", "i3", "fluxbox"]),
        xdotool: command_path_check("xdotool"),
        screenshot: first_available_command(&["import", "scrot"]),
        clipboard: first_available_command(&["xclip", "xsel"]),
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
    if !runtime.xprop.ok {
        blockers.push(
            "Install xprop so workspace windows can be associated with app process ids."
                .to_string(),
        );
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
            apps: Some(status.apps.clone()),
            status: Some(status),
            ipc: None,
            windows: None,
            active_window: None,
            screenshot: None,
            app_log: None,
            clipboard: None,
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

pub fn ipc_info(id: &str) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    request(&workspace_socket_path(&id), IpcRequest::IpcInfo)
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
    validate_launch_policy_ack(&spec)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::LaunchApp {
            command: spec.command,
            name: spec.name,
            profile_id: spec.profile_id,
            applied_policy: spec.applied_policy,
            user_acknowledged_unenforced_policy: spec.user_acknowledged_unenforced_policy,
            cwd: spec.cwd,
            env: spec.env,
        },
    )
}

fn validate_launch_spec(spec: &LaunchSpec) -> Result<()> {
    if spec.command.is_empty() {
        bail!("launch command cannot be empty");
    }
    validate_optional_app_name(&spec.name)?;
    if let Some(cwd) = &spec.cwd {
        if !cwd_is_provided_by_bubblewrap_mount(cwd, spec.applied_policy.as_ref()) && !cwd.is_dir()
        {
            bail!("launch cwd {} is not a directory", cwd.display());
        }
    }
    for env_var in &spec.env {
        validate_env_var(env_var)?;
    }
    Ok(())
}

fn cwd_is_provided_by_bubblewrap_mount(
    cwd: &Path,
    policy: Option<&AppliedWorkspacePolicy>,
) -> bool {
    let Some(policy) = policy else {
        return false;
    };
    uses_bubblewrap_mount_isolation(Some(policy))
        && policy
            .mounts
            .iter()
            .any(|mount| cwd == mount.workspace_path || cwd.starts_with(&mount.workspace_path))
}

fn validate_launch_policy_ack(spec: &LaunchSpec) -> Result<()> {
    if spec
        .applied_policy
        .as_ref()
        .is_some_and(AppliedWorkspacePolicy::has_requested_unenforced_policy)
        && !spec.user_acknowledged_unenforced_policy
    {
        bail!(
            "launch profile requests mount or network policy that is not enforced by this runtime; pass --ack-unenforced-policy or set acknowledge_unenforced_policy=true"
        );
    }
    Ok(())
}

pub fn list_apps(
    id: &str,
    app_id: Option<String>,
    name_contains: Option<String>,
    command_contains: Option<String>,
    profile_id: Option<String>,
    running: Option<bool>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    validate_app_list_filters(&app_id, &name_contains, &command_contains, &profile_id)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::ListApps {
            app_id,
            name_contains,
            command_contains,
            profile_id,
            running,
        },
    )
}

pub fn list_windows(
    id: &str,
    include_hidden: bool,
    title_contains: Option<String>,
    class_contains: Option<String>,
    pid: Option<u32>,
    app_id: Option<String>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    validate_window_list_filters(&title_contains, &class_contains, pid, &app_id)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::ListWindows {
            include_hidden,
            title_contains,
            class_contains,
            pid,
            app_id,
        },
    )
}

pub fn active_window(id: &str) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    request(&workspace_socket_path(&id), IpcRequest::ActiveWindow)
}

pub fn observe(
    id: &str,
    screenshot: bool,
    include_hidden: bool,
    output_path: Option<PathBuf>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::Observe {
            screenshot,
            include_hidden,
            output_path,
        },
    )
}

pub fn wait_window(
    id: &str,
    title_contains: Option<String>,
    class_contains: Option<String>,
    pid: Option<u32>,
    app_id: Option<String>,
    timeout_ms: Option<u64>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    validate_window_match_options(&title_contains, &class_contains, pid, &app_id, false)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::WaitWindow {
            title_contains,
            class_contains,
            pid,
            app_id,
            timeout_ms: timeout_ms.unwrap_or(DEFAULT_APP_WAIT_TIMEOUT_MS),
        },
    )
}

pub fn screenshot(id: &str, output_path: Option<PathBuf>) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::Screenshot { output_path },
    )
}

pub fn screenshot_window(
    id: &str,
    window_id: Option<String>,
    title_contains: Option<String>,
    class_contains: Option<String>,
    pid: Option<u32>,
    app_id: Option<String>,
    output_path: Option<PathBuf>,
    timeout_ms: Option<u64>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    validate_window_target_options(&window_id, &title_contains, &class_contains, pid, &app_id)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::ScreenshotWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            output_path,
            timeout_ms: timeout_ms.unwrap_or(DEFAULT_APP_WAIT_TIMEOUT_MS),
        },
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

pub fn focus_matching_window(
    id: &str,
    title_contains: Option<String>,
    class_contains: Option<String>,
    pid: Option<u32>,
    app_id: Option<String>,
    timeout_ms: Option<u64>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    validate_window_match_options(&title_contains, &class_contains, pid, &app_id, true)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::FocusMatchingWindow {
            title_contains,
            class_contains,
            pid,
            app_id,
            timeout_ms: timeout_ms.unwrap_or(DEFAULT_APP_WAIT_TIMEOUT_MS),
        },
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

pub fn close_matching_window(
    id: &str,
    title_contains: Option<String>,
    class_contains: Option<String>,
    pid: Option<u32>,
    app_id: Option<String>,
    timeout_ms: Option<u64>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    validate_window_match_options(&title_contains, &class_contains, pid, &app_id, true)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::CloseMatchingWindow {
            title_contains,
            class_contains,
            pid,
            app_id,
            timeout_ms: timeout_ms.unwrap_or(DEFAULT_APP_WAIT_TIMEOUT_MS),
        },
    )
}

pub fn move_window(
    id: &str,
    window_id: Option<String>,
    title_contains: Option<String>,
    class_contains: Option<String>,
    pid: Option<u32>,
    app_id: Option<String>,
    x: i32,
    y: i32,
    timeout_ms: Option<u64>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    validate_window_target_options(&window_id, &title_contains, &class_contains, pid, &app_id)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::MoveWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            x,
            y,
            timeout_ms: timeout_ms.unwrap_or(DEFAULT_APP_WAIT_TIMEOUT_MS),
        },
    )
}

pub fn resize_window(
    id: &str,
    window_id: Option<String>,
    title_contains: Option<String>,
    class_contains: Option<String>,
    pid: Option<u32>,
    app_id: Option<String>,
    width: u32,
    height: u32,
    timeout_ms: Option<u64>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    validate_window_target_options(&window_id, &title_contains, &class_contains, pid, &app_id)?;
    validate_window_size(width, height)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::ResizeWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            width,
            height,
            timeout_ms: timeout_ms.unwrap_or(DEFAULT_APP_WAIT_TIMEOUT_MS),
        },
    )
}

pub fn raise_window(
    id: &str,
    window_id: Option<String>,
    title_contains: Option<String>,
    class_contains: Option<String>,
    pid: Option<u32>,
    app_id: Option<String>,
    timeout_ms: Option<u64>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    validate_window_target_options(&window_id, &title_contains, &class_contains, pid, &app_id)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::RaiseWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            timeout_ms: timeout_ms.unwrap_or(DEFAULT_APP_WAIT_TIMEOUT_MS),
        },
    )
}

pub fn minimize_window(
    id: &str,
    window_id: Option<String>,
    title_contains: Option<String>,
    class_contains: Option<String>,
    pid: Option<u32>,
    app_id: Option<String>,
    timeout_ms: Option<u64>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    validate_window_target_options(&window_id, &title_contains, &class_contains, pid, &app_id)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::MinimizeWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            timeout_ms: timeout_ms.unwrap_or(DEFAULT_APP_WAIT_TIMEOUT_MS),
        },
    )
}

pub fn show_window(
    id: &str,
    window_id: Option<String>,
    title_contains: Option<String>,
    class_contains: Option<String>,
    pid: Option<u32>,
    app_id: Option<String>,
    timeout_ms: Option<u64>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    validate_window_target_options(&window_id, &title_contains, &class_contains, pid, &app_id)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::ShowWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            timeout_ms: timeout_ms.unwrap_or(DEFAULT_APP_WAIT_TIMEOUT_MS),
        },
    )
}

pub fn click(
    id: &str,
    x: i32,
    y: i32,
    button: Option<u8>,
    count: Option<u8>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    let button = button.unwrap_or(DEFAULT_CLICK_BUTTON);
    let count = count.unwrap_or(DEFAULT_CLICK_COUNT);
    validate_click_options(button, count)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::Click {
            x,
            y,
            button,
            count,
        },
    )
}

pub fn click_window(
    id: &str,
    window_id: Option<String>,
    title_contains: Option<String>,
    class_contains: Option<String>,
    pid: Option<u32>,
    app_id: Option<String>,
    x: i32,
    y: i32,
    button: Option<u8>,
    count: Option<u8>,
    timeout_ms: Option<u64>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    let button = button.unwrap_or(DEFAULT_CLICK_BUTTON);
    let count = count.unwrap_or(DEFAULT_CLICK_COUNT);
    validate_window_target_options(&window_id, &title_contains, &class_contains, pid, &app_id)?;
    validate_relative_click_coordinates(x, y)?;
    validate_click_options(button, count)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::ClickWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            x,
            y,
            button,
            count,
            timeout_ms: timeout_ms.unwrap_or(DEFAULT_APP_WAIT_TIMEOUT_MS),
        },
    )
}

pub fn move_pointer(id: &str, x: i32, y: i32) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::MovePointer { x, y },
    )
}

pub fn move_pointer_window(
    id: &str,
    window_id: Option<String>,
    title_contains: Option<String>,
    class_contains: Option<String>,
    pid: Option<u32>,
    app_id: Option<String>,
    x: i32,
    y: i32,
    timeout_ms: Option<u64>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    validate_window_target_options(&window_id, &title_contains, &class_contains, pid, &app_id)?;
    validate_relative_click_coordinates(x, y)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::MovePointerWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            x,
            y,
            timeout_ms: timeout_ms.unwrap_or(DEFAULT_APP_WAIT_TIMEOUT_MS),
        },
    )
}

pub fn drag(
    id: &str,
    from_x: i32,
    from_y: i32,
    to_x: i32,
    to_y: i32,
    button: Option<u8>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    let button = button.unwrap_or(DEFAULT_CLICK_BUTTON);
    validate_click_options(button, DEFAULT_CLICK_COUNT)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::Drag {
            from_x,
            from_y,
            to_x,
            to_y,
            button,
        },
    )
}

pub fn drag_window(
    id: &str,
    window_id: Option<String>,
    title_contains: Option<String>,
    class_contains: Option<String>,
    pid: Option<u32>,
    app_id: Option<String>,
    from_x: i32,
    from_y: i32,
    to_x: i32,
    to_y: i32,
    button: Option<u8>,
    timeout_ms: Option<u64>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    let button = button.unwrap_or(DEFAULT_CLICK_BUTTON);
    validate_window_target_options(&window_id, &title_contains, &class_contains, pid, &app_id)?;
    validate_relative_click_coordinates(from_x, from_y)?;
    validate_relative_click_coordinates(to_x, to_y)?;
    validate_click_options(button, DEFAULT_CLICK_COUNT)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::DragWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            from_x,
            from_y,
            to_x,
            to_y,
            button,
            timeout_ms: timeout_ms.unwrap_or(DEFAULT_APP_WAIT_TIMEOUT_MS),
        },
    )
}

pub fn scroll(
    id: &str,
    x: i32,
    y: i32,
    direction: ScrollDirection,
    amount: Option<u8>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    let amount = amount.unwrap_or(DEFAULT_SCROLL_AMOUNT);
    validate_scroll_options(direction, amount)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::Scroll {
            x,
            y,
            direction,
            amount,
        },
    )
}

pub fn scroll_window(
    id: &str,
    window_id: Option<String>,
    title_contains: Option<String>,
    class_contains: Option<String>,
    pid: Option<u32>,
    app_id: Option<String>,
    x: i32,
    y: i32,
    direction: ScrollDirection,
    amount: Option<u8>,
    timeout_ms: Option<u64>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    let amount = amount.unwrap_or(DEFAULT_SCROLL_AMOUNT);
    validate_window_target_options(&window_id, &title_contains, &class_contains, pid, &app_id)?;
    validate_relative_click_coordinates(x, y)?;
    validate_scroll_options(direction, amount)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::ScrollWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            x,
            y,
            direction,
            amount,
            timeout_ms: timeout_ms.unwrap_or(DEFAULT_APP_WAIT_TIMEOUT_MS),
        },
    )
}

pub fn key(id: &str, key: String) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    if key.trim().is_empty() {
        bail!("key cannot be empty");
    }
    request(&workspace_socket_path(&id), IpcRequest::Key { key })
}

pub fn key_window(
    id: &str,
    window_id: Option<String>,
    title_contains: Option<String>,
    class_contains: Option<String>,
    pid: Option<u32>,
    app_id: Option<String>,
    key: String,
    timeout_ms: Option<u64>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    validate_window_target_options(&window_id, &title_contains, &class_contains, pid, &app_id)?;
    if key.trim().is_empty() {
        bail!("key cannot be empty");
    }
    request(
        &workspace_socket_path(&id),
        IpcRequest::KeyWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            key,
            timeout_ms: timeout_ms.unwrap_or(DEFAULT_APP_WAIT_TIMEOUT_MS),
        },
    )
}

pub fn type_text(id: &str, text: String) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    if text.is_empty() {
        bail!("text cannot be empty");
    }
    request(&workspace_socket_path(&id), IpcRequest::TypeText { text })
}

pub fn type_window(
    id: &str,
    window_id: Option<String>,
    title_contains: Option<String>,
    class_contains: Option<String>,
    pid: Option<u32>,
    app_id: Option<String>,
    text: String,
    timeout_ms: Option<u64>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    validate_window_target_options(&window_id, &title_contains, &class_contains, pid, &app_id)?;
    if text.is_empty() {
        bail!("text cannot be empty");
    }
    request(
        &workspace_socket_path(&id),
        IpcRequest::TypeWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            text,
            timeout_ms: timeout_ms.unwrap_or(DEFAULT_APP_WAIT_TIMEOUT_MS),
        },
    )
}

pub fn set_clipboard(id: &str, text: String) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    validate_clipboard_text(&text)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::SetClipboard { text },
    )
}

pub fn get_clipboard(id: &str) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    request(&workspace_socket_path(&id), IpcRequest::GetClipboard)
}

pub fn paste_text(id: &str, text: String, key: Option<String>) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    validate_clipboard_text(&text)?;
    let key = normalize_paste_key(key)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::PasteText { text, key },
    )
}

pub fn paste_window(
    id: &str,
    window_id: Option<String>,
    title_contains: Option<String>,
    class_contains: Option<String>,
    pid: Option<u32>,
    app_id: Option<String>,
    text: String,
    key: Option<String>,
    timeout_ms: Option<u64>,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    validate_window_target_options(&window_id, &title_contains, &class_contains, pid, &app_id)?;
    validate_clipboard_text(&text)?;
    let key = normalize_paste_key(key)?;
    request(
        &workspace_socket_path(&id),
        IpcRequest::PasteWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            text,
            key,
            timeout_ms: timeout_ms.unwrap_or(DEFAULT_APP_WAIT_TIMEOUT_MS),
        },
    )
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

pub fn wait_app(
    id: &str,
    app_id: String,
    timeout_ms: Option<u64>,
    kill_on_timeout: bool,
) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    if app_id.trim().is_empty() {
        bail!("app id cannot be empty");
    }
    request(
        &workspace_socket_path(&id),
        IpcRequest::WaitApp {
            app_id,
            timeout_ms: timeout_ms.unwrap_or(DEFAULT_APP_WAIT_TIMEOUT_MS),
            kill_on_timeout,
        },
    )
}

pub fn run_app_with_spec(
    id: &str,
    spec: LaunchSpec,
    timeout_ms: Option<u64>,
    tail_bytes: Option<u64>,
    kill_on_timeout: bool,
) -> Result<WorkspaceRun> {
    let launch = launch_app_with_spec(id, spec)?;
    let app_id =
        response_last_app_id(&launch).context("workspace launch did not return an app id")?;
    let wait = wait_app(id, app_id.clone(), timeout_ms, false)?;
    let completed = wait.ok;
    let timed_out = !completed;
    let kill = if timed_out && kill_on_timeout {
        Some(kill_app(id, app_id.clone())?)
    } else {
        None
    };
    let stdout = read_app_log(id, app_id.clone(), "stdout".to_string(), tail_bytes)?
        .app_log
        .context("workspace stdout log response did not include app_log")?;
    let stderr = read_app_log(id, app_id.clone(), "stderr".to_string(), tail_bytes)?
        .app_log
        .context("workspace stderr log response did not include app_log")?;
    let exit_source = kill.as_ref().unwrap_or(&wait);
    let exit_code = response_app(exit_source, &app_id).and_then(|app| app.exit_code);
    let exit_signal = response_app(exit_source, &app_id).and_then(|app| app.exit_signal);
    let succeeded = completed && exit_code == Some(0);
    Ok(WorkspaceRun {
        app_id,
        launch,
        wait,
        kill,
        stdout,
        stderr,
        completed,
        succeeded,
        timed_out,
        killed_on_timeout: timed_out && kill_on_timeout,
        exit_code,
        exit_signal,
    })
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

pub fn stop_workspace(id: &str, timeout_ms: Option<u64>) -> Result<IpcResponse> {
    let id = sanitize_workspace_id(id)?;
    let socket_path = workspace_socket_path(&id);
    let mut response = request(&socket_path, IpcRequest::Stop)?;
    if response.ok {
        wait_for_socket_removed(
            &socket_path,
            Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_STOP_WAIT_TIMEOUT_MS)),
        )?;
        response.message = "workspace stopped".to_string();
    }
    Ok(response)
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
    let started_at_unix = unix_now();
    let mut state = DaemonState {
        status: WorkspaceStatus {
            id,
            profile_id: options.profile_id,
            applied_policy: options.applied_policy,
            user_acknowledged_hidden_workspace: options.user_acknowledged_hidden_workspace,
            user_acknowledged_unenforced_policy: options.user_acknowledged_unenforced_policy,
            ready: true,
            started_at_unix,
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
        "started_at_unix": state.status.started_at_unix,
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
        let _ = terminate_app_process(&app.info.id, &mut app.child);
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
        ipc: None,
        apps: None,
        windows: None,
        active_window: None,
        screenshot: None,
        app_log: None,
        clipboard: None,
        events: None,
    }
}

fn workspace_ipc_info(status: &WorkspaceStatus) -> WorkspaceIpcInfo {
    WorkspaceIpcInfo {
        protocol: IPC_PROTOCOL_NAME.to_string(),
        protocol_version: IPC_PROTOCOL_VERSION,
        server_version: env!("CARGO_PKG_VERSION").to_string(),
        workspace_id: status.id.clone(),
        socket_path: status.socket_path.clone(),
        transport: "unix_socket".to_string(),
        framing: "newline_delimited_json".to_string(),
        encoding: "utf-8".to_string(),
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
    refresh_apps(state)?;

    let (response, should_stop) = match request {
        IpcRequest::IpcInfo => {
            record_event(state, "ipc_info", serde_json::json!({}))?;
            let mut response =
                response_with_status(true, "workspace IPC info returned", &state.status);
            response.ipc = Some(workspace_ipc_info(&state.status));
            (response, false)
        }
        IpcRequest::Status => {
            let mut response = response_with_status(true, "workspace is running", &state.status);
            response.apps = Some(state.status.apps.clone());
            (response, false)
        }
        IpcRequest::LaunchApp {
            command,
            name,
            profile_id,
            applied_policy,
            user_acknowledged_unenforced_policy,
            cwd,
            env,
        } => match spawn_app(
            state,
            LaunchSpec {
                command,
                name,
                profile_id,
                applied_policy,
                user_acknowledged_unenforced_policy,
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
                        "name": app.name.as_deref(),
                        "pid": app.pid,
                        "command": &app.command,
                        "profile_id": app.profile_id.as_deref(),
                        "cwd": app.cwd.as_ref().map(|path| path.display().to_string()),
                        "network_isolation": &app.network_isolation,
                        "mount_isolation": &app.mount_isolation,
                    }),
                )?;
                (
                    {
                        let mut response =
                            response_with_status(true, "app launched in workspace", &state.status);
                        response.apps = Some(vec![app]);
                        response
                    },
                    false,
                )
            }
            Err(error) => (
                response_with_status(false, error.to_string(), &state.status),
                false,
            ),
        },
        IpcRequest::ListApps {
            app_id,
            name_contains,
            command_contains,
            profile_id,
            running,
        } => {
            match validate_app_list_filters(&app_id, &name_contains, &command_contains, &profile_id)
                .and_then(|()| refresh_apps(state))
                .map(|()| {
                    filter_workspace_apps(
                        &state.status,
                        &app_id,
                        &name_contains,
                        &command_contains,
                        &profile_id,
                        running,
                    )
                }) {
                Ok(apps) => {
                    record_event(
                        state,
                        "list_apps",
                        serde_json::json!({
                            "count": apps.len(),
                            "app_id": app_id.as_deref(),
                            "name_contains": name_contains.as_deref(),
                            "command_contains": command_contains.as_deref(),
                            "profile_id": profile_id.as_deref(),
                            "running": running,
                        }),
                    )?;
                    let mut response =
                        response_with_status(true, "workspace apps listed", &state.status);
                    response.apps = Some(apps);
                    (response, false)
                }
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
            }
        }
        IpcRequest::ListWindows {
            include_hidden,
            title_contains,
            class_contains,
            pid,
            app_id,
        } => {
            let criteria = WindowMatchCriteria {
                title_contains,
                class_contains,
                pid,
                app_id,
            };
            match validate_window_list_filters(
                &criteria.title_contains,
                &criteria.class_contains,
                criteria.pid,
                &criteria.app_id,
            )
            .and_then(|()| refresh_apps(state))
            .and_then(|()| list_matching_workspace_windows(state, include_hidden, &criteria))
            {
                Ok(windows) => {
                    record_event(
                        state,
                        "list_windows",
                        serde_json::json!({
                            "count": windows.len(),
                            "include_hidden": include_hidden,
                            "title_contains": criteria.title_contains.as_deref(),
                            "class_contains": criteria.class_contains.as_deref(),
                            "pid": criteria.pid,
                            "app_id": criteria.app_id.as_deref(),
                        }),
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
            }
        }
        IpcRequest::ActiveWindow => match active_workspace_window(&state.status) {
            Ok(Some(window)) => {
                record_event(
                    state,
                    "active_window",
                    serde_json::json!({ "window_id": &window.id }),
                )?;
                let mut response =
                    response_with_status(true, "workspace active window reported", &state.status);
                response.active_window = Some(window.clone());
                response.windows = Some(vec![window]);
                (response, false)
            }
            Ok(None) => {
                record_event(
                    state,
                    "active_window",
                    serde_json::json!({ "window_id": serde_json::Value::Null }),
                )?;
                let mut response =
                    response_with_status(false, "workspace active window not found", &state.status);
                response.windows = Some(Vec::new());
                (response, false)
            }
            Err(error) => (
                response_with_status(false, error.to_string(), &state.status),
                false,
            ),
        },
        IpcRequest::Observe {
            screenshot,
            include_hidden,
            output_path,
        } => match observe_workspace(state, screenshot, include_hidden, output_path) {
            Ok(mut response) => {
                record_event(
                    state,
                    "observe",
                    serde_json::json!({
                        "windows": response.windows.as_ref().map(Vec::len).unwrap_or_default(),
                        "include_hidden": include_hidden,
                        "active_window_id": response.active_window.as_ref().map(|window| window.id.as_str()),
                        "screenshot": response.screenshot.as_ref().map(|screenshot| screenshot.path.display().to_string()),
                    }),
                )?;
                response.status = Some(state.status.clone());
                (response, false)
            }
            Err(error) => (
                response_with_status(false, error.to_string(), &state.status),
                false,
            ),
        },
        IpcRequest::WaitWindow {
            title_contains,
            class_contains,
            pid,
            app_id,
            timeout_ms,
        } => {
            let criteria = WindowWaitCriteria {
                title_contains,
                class_contains,
                pid,
                app_id,
                timeout_ms,
            };
            match wait_workspace_window(state, &criteria) {
                Ok(windows) => {
                    let found = !windows.is_empty();
                    record_event(
                        state,
                        "wait_window",
                        serde_json::json!({
                            "title_contains": criteria.title_contains.as_deref(),
                                    "class_contains": criteria.class_contains.as_deref(),
                            "pid": criteria.pid,
                            "app_id": criteria.app_id.as_deref(),
                            "timeout_ms": criteria.timeout_ms,
                            "count": windows.len(),
                        }),
                    )?;
                    let mut response = response_with_status(
                        found,
                        if found {
                            "workspace window found"
                        } else {
                            "workspace window not found before timeout"
                        },
                        &state.status,
                    );
                    response.windows = Some(windows);
                    (response, false)
                }
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
            }
        }
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
        IpcRequest::ScreenshotWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            output_path,
            timeout_ms,
        } => {
            let criteria = WindowWaitCriteria {
                title_contains,
                class_contains,
                pid,
                app_id,
                timeout_ms,
            };
            match validate_window_target_options(
                &window_id,
                &criteria.title_contains,
                &criteria.class_contains,
                criteria.pid,
                &criteria.app_id,
            ) {
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
                Ok(()) => match screenshot_workspace_window(
                    state,
                    window_id.as_deref(),
                    &criteria,
                    output_path,
                ) {
                    Ok(Some(result)) => {
                        record_event(
                            state,
                            "screenshot_window",
                            serde_json::json!({
                                "path": result.screenshot.path.display().to_string(),
                                "window_id": &result.window.id,
                                "title_contains": criteria.title_contains.as_deref(),
                                    "class_contains": criteria.class_contains.as_deref(),
                                "pid": criteria.pid,
                                "app_id": criteria.app_id.as_deref(),
                                "timeout_ms": criteria.timeout_ms,
                            }),
                        )?;
                        let mut response = response_with_status(
                            true,
                            "workspace window screenshot captured",
                            &state.status,
                        );
                        response.screenshot = Some(result.screenshot);
                        response.windows = Some(vec![result.window]);
                        (response, false)
                    }
                    Ok(None) => {
                        let mut response = response_with_status(
                            false,
                            "workspace window not found before timeout",
                            &state.status,
                        );
                        response.windows = Some(Vec::new());
                        (response, false)
                    }
                    Err(error) => (
                        response_with_status(false, error.to_string(), &state.status),
                        false,
                    ),
                },
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
        IpcRequest::FocusMatchingWindow {
            title_contains,
            class_contains,
            pid,
            app_id,
            timeout_ms,
        } => {
            let criteria = WindowWaitCriteria {
                title_contains,
                class_contains,
                pid,
                app_id,
                timeout_ms,
            };
            match validate_window_match_options(
                &criteria.title_contains,
                &criteria.class_contains,
                criteria.pid,
                &criteria.app_id,
                true,
            ) {
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
                Ok(()) => match focus_matching_workspace_window(state, &criteria) {
                    Ok(Some(window)) => {
                        record_event(
                            state,
                            "focus_window",
                            serde_json::json!({
                                "window_id": &window.id,
                                "title_contains": criteria.title_contains.as_deref(),
                                    "class_contains": criteria.class_contains.as_deref(),
                                "pid": criteria.pid,
                                "app_id": criteria.app_id.as_deref(),
                                "timeout_ms": criteria.timeout_ms,
                            }),
                        )?;
                        let mut response = response_with_status(
                            true,
                            "workspace matching window focused",
                            &state.status,
                        );
                        response.windows = Some(vec![window]);
                        (response, false)
                    }
                    Ok(None) => {
                        let mut response = response_with_status(
                            false,
                            "workspace window not found before timeout",
                            &state.status,
                        );
                        response.windows = Some(Vec::new());
                        (response, false)
                    }
                    Err(error) => (
                        response_with_status(false, error.to_string(), &state.status),
                        false,
                    ),
                },
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
        IpcRequest::CloseMatchingWindow {
            title_contains,
            class_contains,
            pid,
            app_id,
            timeout_ms,
        } => {
            let criteria = WindowWaitCriteria {
                title_contains,
                class_contains,
                pid,
                app_id,
                timeout_ms,
            };
            match validate_window_match_options(
                &criteria.title_contains,
                &criteria.class_contains,
                criteria.pid,
                &criteria.app_id,
                true,
            ) {
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
                Ok(()) => match close_matching_workspace_window(state, &criteria) {
                    Ok(Some(window)) => {
                        record_event(
                            state,
                            "close_window",
                            serde_json::json!({
                                "window_id": &window.id,
                                "title_contains": criteria.title_contains.as_deref(),
                                    "class_contains": criteria.class_contains.as_deref(),
                                "pid": criteria.pid,
                                "app_id": criteria.app_id.as_deref(),
                                "timeout_ms": criteria.timeout_ms,
                            }),
                        )?;
                        let mut response = response_with_status(
                            true,
                            "workspace matching window close requested",
                            &state.status,
                        );
                        response.windows = Some(vec![window]);
                        (response, false)
                    }
                    Ok(None) => {
                        let mut response = response_with_status(
                            false,
                            "workspace window not found before timeout",
                            &state.status,
                        );
                        response.windows = Some(Vec::new());
                        (response, false)
                    }
                    Err(error) => (
                        response_with_status(false, error.to_string(), &state.status),
                        false,
                    ),
                },
            }
        }
        IpcRequest::MoveWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            x,
            y,
            timeout_ms,
        } => {
            let criteria = WindowWaitCriteria {
                title_contains,
                class_contains,
                pid,
                app_id,
                timeout_ms,
            };
            match validate_window_target_options(
                &window_id,
                &criteria.title_contains,
                &criteria.class_contains,
                criteria.pid,
                &criteria.app_id,
            )
            .and_then(|()| validate_workspace_coordinates(&state.status, x, y, "window move"))
            {
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
                Ok(()) => {
                    match move_workspace_window_target(state, window_id.as_deref(), &criteria, x, y)
                    {
                        Ok(Some(window)) => {
                            record_event(
                                state,
                                "move_window",
                                serde_json::json!({
                                    "window_id": &window.id,
                                    "title_contains": criteria.title_contains.as_deref(),
                                    "class_contains": criteria.class_contains.as_deref(),
                                    "pid": criteria.pid,
                                    "app_id": criteria.app_id.as_deref(),
                                    "x": x,
                                    "y": y,
                                    "timeout_ms": criteria.timeout_ms,
                                }),
                            )?;
                            let mut response =
                                response_with_status(true, "workspace window moved", &state.status);
                            response.windows = Some(vec![window]);
                            (response, false)
                        }
                        Ok(None) => {
                            let mut response = response_with_status(
                                false,
                                "workspace window not found before timeout",
                                &state.status,
                            );
                            response.windows = Some(Vec::new());
                            (response, false)
                        }
                        Err(error) => (
                            response_with_status(false, error.to_string(), &state.status),
                            false,
                        ),
                    }
                }
            }
        }
        IpcRequest::ResizeWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            width,
            height,
            timeout_ms,
        } => {
            let criteria = WindowWaitCriteria {
                title_contains,
                class_contains,
                pid,
                app_id,
                timeout_ms,
            };
            match validate_window_target_options(
                &window_id,
                &criteria.title_contains,
                &criteria.class_contains,
                criteria.pid,
                &criteria.app_id,
            )
            .and_then(|()| validate_window_size(width, height))
            .and_then(|()| validate_window_size_for_workspace(&state.status, width, height))
            {
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
                Ok(()) => match resize_workspace_window_target(
                    state,
                    window_id.as_deref(),
                    &criteria,
                    width,
                    height,
                ) {
                    Ok(Some(window)) => {
                        record_event(
                            state,
                            "resize_window",
                            serde_json::json!({
                                "window_id": &window.id,
                                "title_contains": criteria.title_contains.as_deref(),
                                    "class_contains": criteria.class_contains.as_deref(),
                                "pid": criteria.pid,
                                "app_id": criteria.app_id.as_deref(),
                                "width": width,
                                "height": height,
                                "timeout_ms": criteria.timeout_ms,
                            }),
                        )?;
                        let mut response =
                            response_with_status(true, "workspace window resized", &state.status);
                        response.windows = Some(vec![window]);
                        (response, false)
                    }
                    Ok(None) => {
                        let mut response = response_with_status(
                            false,
                            "workspace window not found before timeout",
                            &state.status,
                        );
                        response.windows = Some(Vec::new());
                        (response, false)
                    }
                    Err(error) => (
                        response_with_status(false, error.to_string(), &state.status),
                        false,
                    ),
                },
            }
        }
        IpcRequest::RaiseWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            timeout_ms,
        } => {
            let criteria = WindowWaitCriteria {
                title_contains,
                class_contains,
                pid,
                app_id,
                timeout_ms,
            };
            match validate_window_target_options(
                &window_id,
                &criteria.title_contains,
                &criteria.class_contains,
                criteria.pid,
                &criteria.app_id,
            ) {
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
                Ok(()) => {
                    match raise_workspace_window_target(state, window_id.as_deref(), &criteria) {
                        Ok(Some(window)) => {
                            record_event(
                                state,
                                "raise_window",
                                serde_json::json!({
                                    "window_id": &window.id,
                                    "title_contains": criteria.title_contains.as_deref(),
                                    "class_contains": criteria.class_contains.as_deref(),
                                    "pid": criteria.pid,
                                    "app_id": criteria.app_id.as_deref(),
                                    "timeout_ms": criteria.timeout_ms,
                                }),
                            )?;
                            let mut response = response_with_status(
                                true,
                                "workspace window raised",
                                &state.status,
                            );
                            response.windows = Some(vec![window]);
                            (response, false)
                        }
                        Ok(None) => {
                            let mut response = response_with_status(
                                false,
                                "workspace window not found before timeout",
                                &state.status,
                            );
                            response.windows = Some(Vec::new());
                            (response, false)
                        }
                        Err(error) => (
                            response_with_status(false, error.to_string(), &state.status),
                            false,
                        ),
                    }
                }
            }
        }
        IpcRequest::MinimizeWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            timeout_ms,
        } => {
            let criteria = WindowWaitCriteria {
                title_contains,
                class_contains,
                pid,
                app_id,
                timeout_ms,
            };
            match validate_window_target_options(
                &window_id,
                &criteria.title_contains,
                &criteria.class_contains,
                criteria.pid,
                &criteria.app_id,
            ) {
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
                Ok(()) => {
                    match minimize_workspace_window_target(state, window_id.as_deref(), &criteria) {
                        Ok(Some(window)) => {
                            record_event(
                                state,
                                "minimize_window",
                                serde_json::json!({
                                    "window_id": &window.id,
                                    "title_contains": criteria.title_contains.as_deref(),
                                    "class_contains": criteria.class_contains.as_deref(),
                                    "pid": criteria.pid,
                                    "app_id": criteria.app_id.as_deref(),
                                    "timeout_ms": criteria.timeout_ms,
                                }),
                            )?;
                            let mut response = response_with_status(
                                true,
                                "workspace window minimized",
                                &state.status,
                            );
                            response.windows = Some(vec![window]);
                            (response, false)
                        }
                        Ok(None) => {
                            let mut response = response_with_status(
                                false,
                                "workspace window not found before timeout",
                                &state.status,
                            );
                            response.windows = Some(Vec::new());
                            (response, false)
                        }
                        Err(error) => (
                            response_with_status(false, error.to_string(), &state.status),
                            false,
                        ),
                    }
                }
            }
        }
        IpcRequest::ShowWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            timeout_ms,
        } => {
            let criteria = WindowWaitCriteria {
                title_contains,
                class_contains,
                pid,
                app_id,
                timeout_ms,
            };
            match validate_window_target_options(
                &window_id,
                &criteria.title_contains,
                &criteria.class_contains,
                criteria.pid,
                &criteria.app_id,
            ) {
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
                Ok(()) => {
                    match show_workspace_window_target(state, window_id.as_deref(), &criteria) {
                        Ok(Some(window)) => {
                            record_event(
                                state,
                                "show_window",
                                serde_json::json!({
                                    "window_id": &window.id,
                                    "title_contains": criteria.title_contains.as_deref(),
                                    "class_contains": criteria.class_contains.as_deref(),
                                    "pid": criteria.pid,
                                    "app_id": criteria.app_id.as_deref(),
                                    "timeout_ms": criteria.timeout_ms,
                                }),
                            )?;
                            let mut response =
                                response_with_status(true, "workspace window shown", &state.status);
                            response.windows = Some(vec![window]);
                            (response, false)
                        }
                        Ok(None) => {
                            let mut response = response_with_status(
                                false,
                                "workspace window not found before timeout",
                                &state.status,
                            );
                            response.windows = Some(Vec::new());
                            (response, false)
                        }
                        Err(error) => (
                            response_with_status(false, error.to_string(), &state.status),
                            false,
                        ),
                    }
                }
            }
        }
        IpcRequest::Click {
            x,
            y,
            button,
            count,
        } => match click_workspace(&state.status, x, y, button, count) {
            Ok(()) => {
                record_event(
                    state,
                    "click",
                    serde_json::json!({ "x": x, "y": y, "button": button, "count": count }),
                )?;
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
        IpcRequest::ClickWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            x,
            y,
            button,
            count,
            timeout_ms,
        } => {
            let criteria = WindowWaitCriteria {
                title_contains,
                class_contains,
                pid,
                app_id,
                timeout_ms,
            };
            match validate_window_target_options(
                &window_id,
                &criteria.title_contains,
                &criteria.class_contains,
                criteria.pid,
                &criteria.app_id,
            )
            .and_then(|()| validate_relative_click_coordinates(x, y))
            .and_then(|()| validate_click_options(button, count))
            {
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
                Ok(()) => {
                    match click_workspace_window(
                        state,
                        window_id.as_deref(),
                        &criteria,
                        x,
                        y,
                        button,
                        count,
                    ) {
                        Ok(Some(clicked)) => {
                            record_event(
                                state,
                                "click_window",
                                serde_json::json!({
                                    "window_id": &clicked.window.id,
                                    "title_contains": criteria.title_contains.as_deref(),
                                    "class_contains": criteria.class_contains.as_deref(),
                                    "pid": criteria.pid,
                                    "app_id": criteria.app_id.as_deref(),
                                    "relative_x": x,
                                    "relative_y": y,
                                    "x": clicked.x,
                                    "y": clicked.y,
                                    "button": button,
                                    "count": count,
                                    "timeout_ms": criteria.timeout_ms,
                                }),
                            )?;
                            let mut response = response_with_status(
                                true,
                                "workspace window click sent",
                                &state.status,
                            );
                            response.windows = Some(vec![clicked.window]);
                            (response, false)
                        }
                        Ok(None) => {
                            let mut response = response_with_status(
                                false,
                                "workspace window not found before timeout",
                                &state.status,
                            );
                            response.windows = Some(Vec::new());
                            (response, false)
                        }
                        Err(error) => (
                            response_with_status(false, error.to_string(), &state.status),
                            false,
                        ),
                    }
                }
            }
        }
        IpcRequest::MovePointer { x, y } => match move_workspace_pointer(&state.status, x, y) {
            Ok(()) => {
                record_event(state, "move_pointer", serde_json::json!({ "x": x, "y": y }))?;
                (
                    response_with_status(true, "workspace pointer moved", &state.status),
                    false,
                )
            }
            Err(error) => (
                response_with_status(false, error.to_string(), &state.status),
                false,
            ),
        },
        IpcRequest::MovePointerWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            x,
            y,
            timeout_ms,
        } => {
            let criteria = WindowWaitCriteria {
                title_contains,
                class_contains,
                pid,
                app_id,
                timeout_ms,
            };
            match validate_window_target_options(
                &window_id,
                &criteria.title_contains,
                &criteria.class_contains,
                criteria.pid,
                &criteria.app_id,
            )
            .and_then(|()| validate_relative_click_coordinates(x, y))
            {
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
                Ok(()) => match move_workspace_pointer_window(
                    state,
                    window_id.as_deref(),
                    &criteria,
                    x,
                    y,
                ) {
                    Ok(Some(moved)) => {
                        record_event(
                            state,
                            "move_pointer_window",
                            serde_json::json!({
                                "window_id": &moved.window.id,
                                "title_contains": criteria.title_contains.as_deref(),
                                    "class_contains": criteria.class_contains.as_deref(),
                                "pid": criteria.pid,
                                "app_id": criteria.app_id.as_deref(),
                                "relative_x": x,
                                "relative_y": y,
                                "x": moved.x,
                                "y": moved.y,
                                "timeout_ms": criteria.timeout_ms,
                            }),
                        )?;
                        let mut response = response_with_status(
                            true,
                            "workspace window pointer moved",
                            &state.status,
                        );
                        response.windows = Some(vec![moved.window]);
                        (response, false)
                    }
                    Ok(None) => {
                        let mut response = response_with_status(
                            false,
                            "workspace window not found before timeout",
                            &state.status,
                        );
                        response.windows = Some(Vec::new());
                        (response, false)
                    }
                    Err(error) => (
                        response_with_status(false, error.to_string(), &state.status),
                        false,
                    ),
                },
            }
        }
        IpcRequest::Drag {
            from_x,
            from_y,
            to_x,
            to_y,
            button,
        } => match drag_workspace(&state.status, from_x, from_y, to_x, to_y, button) {
            Ok(()) => {
                record_event(
                    state,
                    "drag",
                    serde_json::json!({
                        "from_x": from_x,
                        "from_y": from_y,
                        "to_x": to_x,
                        "to_y": to_y,
                        "button": button,
                    }),
                )?;
                (
                    response_with_status(true, "workspace drag sent", &state.status),
                    false,
                )
            }
            Err(error) => (
                response_with_status(false, error.to_string(), &state.status),
                false,
            ),
        },
        IpcRequest::DragWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            from_x,
            from_y,
            to_x,
            to_y,
            button,
            timeout_ms,
        } => {
            let criteria = WindowWaitCriteria {
                title_contains,
                class_contains,
                pid,
                app_id,
                timeout_ms,
            };
            match validate_window_target_options(
                &window_id,
                &criteria.title_contains,
                &criteria.class_contains,
                criteria.pid,
                &criteria.app_id,
            )
            .and_then(|()| validate_relative_click_coordinates(from_x, from_y))
            .and_then(|()| validate_relative_click_coordinates(to_x, to_y))
            .and_then(|()| validate_click_options(button, DEFAULT_CLICK_COUNT))
            {
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
                Ok(()) => match drag_workspace_window(
                    state,
                    window_id.as_deref(),
                    &criteria,
                    from_x,
                    from_y,
                    to_x,
                    to_y,
                    button,
                ) {
                    Ok(Some(dragged)) => {
                        record_event(
                            state,
                            "drag_window",
                            serde_json::json!({
                                "window_id": &dragged.window.id,
                                "title_contains": criteria.title_contains.as_deref(),
                                    "class_contains": criteria.class_contains.as_deref(),
                                "pid": criteria.pid,
                                "app_id": criteria.app_id.as_deref(),
                                "from_x": dragged.from_x,
                                "from_y": dragged.from_y,
                                "to_x": dragged.to_x,
                                "to_y": dragged.to_y,
                                "relative_from_x": from_x,
                                "relative_from_y": from_y,
                                "relative_to_x": to_x,
                                "relative_to_y": to_y,
                                "button": button,
                                "timeout_ms": criteria.timeout_ms,
                            }),
                        )?;
                        let mut response =
                            response_with_status(true, "workspace window drag sent", &state.status);
                        response.windows = Some(vec![dragged.window]);
                        (response, false)
                    }
                    Ok(None) => {
                        let mut response = response_with_status(
                            false,
                            "workspace window not found before timeout",
                            &state.status,
                        );
                        response.windows = Some(Vec::new());
                        (response, false)
                    }
                    Err(error) => (
                        response_with_status(false, error.to_string(), &state.status),
                        false,
                    ),
                },
            }
        }
        IpcRequest::Scroll {
            x,
            y,
            direction,
            amount,
        } => match scroll_workspace(&state.status, x, y, direction, amount) {
            Ok(()) => {
                record_event(
                    state,
                    "scroll",
                    serde_json::json!({
                        "x": x,
                        "y": y,
                        "direction": direction.as_str(),
                        "amount": amount,
                    }),
                )?;
                (
                    response_with_status(true, "workspace scroll sent", &state.status),
                    false,
                )
            }
            Err(error) => (
                response_with_status(false, error.to_string(), &state.status),
                false,
            ),
        },
        IpcRequest::ScrollWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            x,
            y,
            direction,
            amount,
            timeout_ms,
        } => {
            let criteria = WindowWaitCriteria {
                title_contains,
                class_contains,
                pid,
                app_id,
                timeout_ms,
            };
            match validate_window_target_options(
                &window_id,
                &criteria.title_contains,
                &criteria.class_contains,
                criteria.pid,
                &criteria.app_id,
            )
            .and_then(|()| validate_relative_click_coordinates(x, y))
            .and_then(|()| validate_scroll_options(direction, amount))
            {
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
                Ok(()) => match scroll_workspace_window(
                    state,
                    window_id.as_deref(),
                    &criteria,
                    x,
                    y,
                    direction,
                    amount,
                ) {
                    Ok(Some(scrolled)) => {
                        record_event(
                            state,
                            "scroll_window",
                            serde_json::json!({
                                "window_id": &scrolled.window.id,
                                "title_contains": criteria.title_contains.as_deref(),
                                    "class_contains": criteria.class_contains.as_deref(),
                                "pid": criteria.pid,
                                "app_id": criteria.app_id.as_deref(),
                                "relative_x": x,
                                "relative_y": y,
                                "x": scrolled.x,
                                "y": scrolled.y,
                                "direction": direction.as_str(),
                                "amount": amount,
                                "timeout_ms": criteria.timeout_ms,
                            }),
                        )?;
                        let mut response = response_with_status(
                            true,
                            "workspace window scroll sent",
                            &state.status,
                        );
                        response.windows = Some(vec![scrolled.window]);
                        (response, false)
                    }
                    Ok(None) => {
                        let mut response = response_with_status(
                            false,
                            "workspace window not found before timeout",
                            &state.status,
                        );
                        response.windows = Some(Vec::new());
                        (response, false)
                    }
                    Err(error) => (
                        response_with_status(false, error.to_string(), &state.status),
                        false,
                    ),
                },
            }
        }
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
        IpcRequest::KeyWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            key,
            timeout_ms,
        } => {
            let criteria = WindowWaitCriteria {
                title_contains,
                class_contains,
                pid,
                app_id,
                timeout_ms,
            };
            let logged_key = key.trim().to_string();
            match validate_window_target_options(
                &window_id,
                &criteria.title_contains,
                &criteria.class_contains,
                criteria.pid,
                &criteria.app_id,
            )
            .and_then(|()| {
                if logged_key.is_empty() {
                    bail!("key cannot be empty");
                }
                Ok(())
            }) {
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
                Ok(()) => match key_workspace_window(state, window_id.as_deref(), &criteria, key) {
                    Ok(Some(window)) => {
                        record_event(
                            state,
                            "key_window",
                            serde_json::json!({
                                "window_id": &window.id,
                                "title_contains": criteria.title_contains.as_deref(),
                                    "class_contains": criteria.class_contains.as_deref(),
                                "pid": criteria.pid,
                                "app_id": criteria.app_id.as_deref(),
                                "key": logged_key,
                                "timeout_ms": criteria.timeout_ms,
                            }),
                        )?;
                        let mut response =
                            response_with_status(true, "workspace window key sent", &state.status);
                        response.windows = Some(vec![window]);
                        (response, false)
                    }
                    Ok(None) => {
                        let mut response = response_with_status(
                            false,
                            "workspace window not found before timeout",
                            &state.status,
                        );
                        response.windows = Some(Vec::new());
                        (response, false)
                    }
                    Err(error) => (
                        response_with_status(false, error.to_string(), &state.status),
                        false,
                    ),
                },
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
        IpcRequest::TypeWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            text,
            timeout_ms,
        } => {
            let criteria = WindowWaitCriteria {
                title_contains,
                class_contains,
                pid,
                app_id,
                timeout_ms,
            };
            let char_count = text.chars().count();
            match validate_window_target_options(
                &window_id,
                &criteria.title_contains,
                &criteria.class_contains,
                criteria.pid,
                &criteria.app_id,
            )
            .and_then(|()| {
                if text.is_empty() {
                    bail!("text cannot be empty");
                }
                Ok(())
            }) {
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
                Ok(()) => {
                    match type_workspace_window(state, window_id.as_deref(), &criteria, text) {
                        Ok(Some(window)) => {
                            record_event(
                                state,
                                "type_window",
                                serde_json::json!({
                                    "window_id": &window.id,
                                    "title_contains": criteria.title_contains.as_deref(),
                                    "class_contains": criteria.class_contains.as_deref(),
                                    "pid": criteria.pid,
                                    "app_id": criteria.app_id.as_deref(),
                                    "char_count": char_count,
                                    "timeout_ms": criteria.timeout_ms,
                                }),
                            )?;
                            let mut response = response_with_status(
                                true,
                                "workspace window text typed",
                                &state.status,
                            );
                            response.windows = Some(vec![window]);
                            (response, false)
                        }
                        Ok(None) => {
                            let mut response = response_with_status(
                                false,
                                "workspace window not found before timeout",
                                &state.status,
                            );
                            response.windows = Some(Vec::new());
                            (response, false)
                        }
                        Err(error) => (
                            response_with_status(false, error.to_string(), &state.status),
                            false,
                        ),
                    }
                }
            }
        }
        IpcRequest::SetClipboard { text } => {
            let char_count = text.chars().count();
            match validate_clipboard_text(&text)
                .and_then(|()| set_workspace_clipboard(&state.status, &text))
            {
                Ok(clipboard) => {
                    record_event(
                        state,
                        "set_clipboard",
                        serde_json::json!({
                            "selection": &clipboard.selection,
                            "char_count": char_count,
                            "bytes": clipboard.bytes,
                        }),
                    )?;
                    let mut response =
                        response_with_status(true, "workspace clipboard set", &state.status);
                    response.clipboard = Some(clipboard);
                    (response, false)
                }
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
            }
        }
        IpcRequest::GetClipboard => match get_workspace_clipboard(&state.status) {
            Ok(clipboard) => {
                record_event(
                    state,
                    "get_clipboard",
                    serde_json::json!({
                        "selection": &clipboard.selection,
                        "bytes": clipboard.bytes,
                    }),
                )?;
                let mut response =
                    response_with_status(true, "workspace clipboard returned", &state.status);
                response.clipboard = Some(clipboard);
                (response, false)
            }
            Err(error) => (
                response_with_status(false, error.to_string(), &state.status),
                false,
            ),
        },
        IpcRequest::PasteText { text, key } => {
            let char_count = text.chars().count();
            match validate_clipboard_text(&text)
                .and_then(|()| validate_key_text(&key))
                .and_then(|()| paste_workspace_text(&state.status, &text, &key))
            {
                Ok(clipboard) => {
                    record_event(
                        state,
                        "paste_text",
                        serde_json::json!({
                            "selection": &clipboard.selection,
                            "char_count": char_count,
                            "bytes": clipboard.bytes,
                            "key": key.trim(),
                        }),
                    )?;
                    let mut response =
                        response_with_status(true, "workspace text pasted", &state.status);
                    response.clipboard = Some(clipboard);
                    (response, false)
                }
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
            }
        }
        IpcRequest::PasteWindow {
            window_id,
            title_contains,
            class_contains,
            pid,
            app_id,
            text,
            key,
            timeout_ms,
        } => {
            let criteria = WindowWaitCriteria {
                title_contains,
                class_contains,
                pid,
                app_id,
                timeout_ms,
            };
            let char_count = text.chars().count();
            match validate_window_target_options(
                &window_id,
                &criteria.title_contains,
                &criteria.class_contains,
                criteria.pid,
                &criteria.app_id,
            )
            .and_then(|()| validate_clipboard_text(&text))
            .and_then(|()| validate_key_text(&key))
            {
                Err(error) => (
                    response_with_status(false, error.to_string(), &state.status),
                    false,
                ),
                Ok(()) => match paste_workspace_window(
                    state,
                    window_id.as_deref(),
                    &criteria,
                    &text,
                    &key,
                ) {
                    Ok(Some(pasted)) => {
                        record_event(
                            state,
                            "paste_window",
                            serde_json::json!({
                                "window_id": &pasted.window.id,
                                "title_contains": criteria.title_contains.as_deref(),
                                    "class_contains": criteria.class_contains.as_deref(),
                                "pid": criteria.pid,
                                "app_id": criteria.app_id.as_deref(),
                                "selection": &pasted.clipboard.selection,
                                "char_count": char_count,
                                "bytes": pasted.clipboard.bytes,
                                "key": key.trim(),
                                "timeout_ms": criteria.timeout_ms,
                            }),
                        )?;
                        let mut response = response_with_status(
                            true,
                            "workspace window text pasted",
                            &state.status,
                        );
                        response.windows = Some(vec![pasted.window]);
                        response.clipboard = Some(pasted.clipboard);
                        (response, false)
                    }
                    Ok(None) => {
                        let mut response = response_with_status(
                            false,
                            "workspace window not found before timeout",
                            &state.status,
                        );
                        response.windows = Some(Vec::new());
                        (response, false)
                    }
                    Err(error) => (
                        response_with_status(false, error.to_string(), &state.status),
                        false,
                    ),
                },
            }
        }
        IpcRequest::ReadAppLog {
            app_id,
            stream,
            tail_bytes,
        } => match read_workspace_app_log(state, &app_id, &stream, tail_bytes) {
            Ok((app_log, app)) => {
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
                response.apps = Some(vec![app]);
                (response, false)
            }
            Err(error) => (
                response_with_status(false, error.to_string(), &state.status),
                false,
            ),
        },
        IpcRequest::WaitApp {
            app_id,
            timeout_ms,
            kill_on_timeout,
        } => match wait_workspace_app(state, &app_id, timeout_ms, kill_on_timeout) {
            Ok((stopped, killed_on_timeout, app)) => {
                record_event(
                    state,
                    "wait_app",
                    serde_json::json!({
                        "target": &app_id,
                        "timeout_ms": timeout_ms,
                        "stopped": stopped,
                        "kill_on_timeout": kill_on_timeout,
                        "killed_on_timeout": killed_on_timeout,
                    }),
                )?;
                let message = if killed_on_timeout {
                    "workspace app killed after timeout"
                } else if stopped {
                    "workspace app stopped"
                } else {
                    "workspace app still running after timeout"
                };
                let mut response = response_with_status(stopped, message, &state.status);
                response.apps = Some(vec![app]);
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
            Ok((message, app, _killed)) => {
                record_event(
                    state,
                    "kill_app",
                    serde_json::json!({ "target": &app_id, "message": &message }),
                )?;
                let mut response = response_with_status(true, message, &state.status);
                response.apps = Some(vec![app]);
                (response, false)
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
    validate_launch_policy_ack(&spec)?;
    let log_paths = prepare_app_log_paths(&state.status.runtime_dir)?;
    let effective_policy = spec
        .applied_policy
        .as_ref()
        .or(state.status.applied_policy.as_ref());
    let sandbox =
        bubblewrap_sandbox_for_launch(&state.status, effective_policy, spec.cwd.as_deref())?;
    let mount_isolation = sandbox
        .as_ref()
        .map(|sandbox| sandbox.mount_isolation.clone())
        .unwrap_or_else(|| "host".to_string());
    let network_isolation = sandbox
        .as_ref()
        .map(|sandbox| sandbox.network_isolation.clone())
        .unwrap_or_else(|| "host".to_string());
    let mut child_command = if let Some(sandbox) = &sandbox {
        let mut command = Command::new("bwrap");
        command
            .args(&sandbox.args)
            .arg("--")
            .arg(&spec.command[0])
            .args(&spec.command[1..]);
        command
    } else {
        let mut command = Command::new(&spec.command[0]);
        command.args(&spec.command[1..]);
        command
    };
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
    if sandbox.is_none() {
        if let Some(cwd) = &spec.cwd {
            child_command.current_dir(cwd);
        }
    }
    for env_var in &spec.env {
        child_command.env(&env_var.name, &env_var.value);
    }
    child_command.process_group(0);
    let child = child_command
        .spawn()
        .with_context(|| format!("failed to launch {}", launch_description(&spec.command)))?;
    let pid = child.id();
    let stdout_path = rename_app_log(&log_paths.stdout, pid, "stdout")?;
    let stderr_path = rename_app_log(&log_paths.stderr, pid, "stderr")?;
    let info = WorkspaceApp {
        id: format!("app-{pid}"),
        name: spec.name,
        pid,
        profile_id: spec.profile_id,
        mount_isolation,
        network_isolation,
        command: spec.command,
        cwd: spec.cwd,
        env: spec.env,
        stdout_path: Some(stdout_path),
        stderr_path: Some(stderr_path),
        started_at_unix: unix_now(),
        running: true,
        exit_status: None,
        exit_code: None,
        exit_signal: None,
    };
    state.status.apps.push(info.clone());
    state.apps.push(AppProcess {
        info: info.clone(),
        child,
    });
    Ok(info)
}

struct BubblewrapSandbox {
    args: Vec<String>,
    mount_isolation: String,
    network_isolation: String,
}

fn bubblewrap_sandbox_for_launch(
    status: &WorkspaceStatus,
    policy: Option<&AppliedWorkspacePolicy>,
    cwd: Option<&Path>,
) -> Result<Option<BubblewrapSandbox>> {
    let network = uses_bubblewrap_network_isolation(policy);
    let mounts = uses_bubblewrap_mount_isolation(policy);
    if !network && !mounts {
        return Ok(None);
    }

    if mounts {
        Ok(Some(BubblewrapSandbox {
            args: restricted_mount_namespace_args(status, policy, cwd, network)?,
            mount_isolation: "bubblewrap_mount_namespace".to_string(),
            network_isolation: if network {
                "bubblewrap_unshare_net".to_string()
            } else {
                "host".to_string()
            },
        }))
    } else {
        let mut args = vec!["--dev-bind".to_string(), "/".to_string(), "/".to_string()];
        if network {
            args.push("--unshare-net".to_string());
        }
        if let Some(cwd) = cwd {
            args.push("--chdir".to_string());
            args.push(cwd.display().to_string());
        }
        Ok(Some(BubblewrapSandbox {
            args,
            mount_isolation: "host".to_string(),
            network_isolation: if network {
                "bubblewrap_unshare_net".to_string()
            } else {
                "host".to_string()
            },
        }))
    }
}

fn uses_bubblewrap_network_isolation(policy: Option<&AppliedWorkspacePolicy>) -> bool {
    policy.is_some_and(|policy| {
        matches!(policy.network.mode, NetworkMode::Disabled)
            && policy.enforcement.network.enforced
            && policy.runtime_capabilities.bubblewrap.ok
    })
}

fn uses_bubblewrap_mount_isolation(policy: Option<&AppliedWorkspacePolicy>) -> bool {
    policy.is_some_and(|policy| {
        !policy.mounts.is_empty()
            && policy.enforcement.mounts.enforced
            && policy.runtime_capabilities.bubblewrap.ok
    })
}

fn restricted_mount_namespace_args(
    status: &WorkspaceStatus,
    policy: Option<&AppliedWorkspacePolicy>,
    cwd: Option<&Path>,
    network: bool,
) -> Result<Vec<String>> {
    let policy = policy.context("mount namespace requested without an applied policy")?;
    let mut args = Vec::new();
    let mut dirs = BTreeSet::new();
    let mut add_dir = |path: &Path| {
        if path != Path::new("/") {
            dirs.insert(path.to_path_buf());
        }
    };
    add_dir(Path::new("/tmp"));
    add_parent_dirs(&mut dirs, &status.xauthority_path);
    if Path::new("/tmp/.X11-unix").exists() {
        add_parent_dirs(&mut dirs, Path::new("/tmp/.X11-unix"));
    }
    for mount in &policy.mounts {
        if !mount.workspace_path.is_absolute() {
            bail!(
                "profile mount workspace_path {} must be absolute for bubblewrap enforcement",
                mount.workspace_path.display()
            );
        }
        if !mount.host_path.exists() {
            bail!(
                "profile mount host_path {} does not exist",
                mount.host_path.display()
            );
        }
        add_parent_dirs(&mut dirs, &mount.workspace_path);
    }

    for path in ["/usr", "/bin", "/lib", "/lib64", "/etc", "/opt"] {
        if Path::new(path).exists() {
            args.push("--ro-bind".to_string());
            args.push(path.to_string());
            args.push(path.to_string());
        }
    }
    args.push("--proc".to_string());
    args.push("/proc".to_string());
    args.push("--dev-bind".to_string());
    args.push("/dev".to_string());
    args.push("/dev".to_string());

    for dir in dirs {
        args.push("--dir".to_string());
        args.push(dir.display().to_string());
    }
    if Path::new("/tmp/.X11-unix").exists() {
        args.push("--ro-bind".to_string());
        args.push("/tmp/.X11-unix".to_string());
        args.push("/tmp/.X11-unix".to_string());
    }
    args.push("--ro-bind".to_string());
    args.push(status.xauthority_path.display().to_string());
    args.push(status.xauthority_path.display().to_string());

    for mount in &policy.mounts {
        args.push(match mount.mode {
            crate::policy::MountMode::ReadOnly => "--ro-bind".to_string(),
            crate::policy::MountMode::ReadWrite => "--bind".to_string(),
        });
        args.push(mount.host_path.display().to_string());
        args.push(mount.workspace_path.display().to_string());
    }
    if network {
        args.push("--unshare-net".to_string());
    }
    args.push("--chdir".to_string());
    args.push(
        cwd.unwrap_or_else(|| Path::new("/tmp"))
            .display()
            .to_string(),
    );
    Ok(args)
}

fn add_parent_dirs(dirs: &mut BTreeSet<PathBuf>, path: &Path) {
    let mut parents = Vec::new();
    let mut current = path.parent();
    while let Some(parent) = current {
        if parent == Path::new("/") {
            break;
        }
        parents.push(parent.to_path_buf());
        current = parent.parent();
    }
    for parent in parents.into_iter().rev() {
        dirs.insert(parent);
    }
}

fn launch_description(command: &[String]) -> String {
    if command.is_empty() {
        "<empty command>".to_string()
    } else {
        command.join(" ")
    }
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

fn list_workspace_windows(
    status: &WorkspaceStatus,
    include_hidden: bool,
) -> Result<Vec<WorkspaceWindow>> {
    let ids = search_workspace_window_ids(status, !include_hidden)?;
    let visible_ids: BTreeSet<String> = if include_hidden {
        search_workspace_window_ids(status, true)?
            .into_iter()
            .collect()
    } else {
        ids.iter().cloned().collect()
    };

    ids.into_iter()
        .map(|id| {
            let visible = visible_ids.contains(&id);
            window_info_with_visibility(status, &id, visible)
        })
        .collect()
}

fn search_workspace_window_ids(
    status: &WorkspaceStatus,
    only_visible: bool,
) -> Result<Vec<String>> {
    let mut command = workspace_command(status, "xdotool");
    command.arg("search");
    if only_visible {
        command.arg("--onlyvisible");
    }
    let output = command
        .args(["--name", "."])
        .output()
        .context("failed to run xdotool window search")?;
    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter_map(|line| {
            let id = line.trim();
            (!id.is_empty()).then(|| id.to_string())
        })
        .collect())
}

fn active_workspace_window(status: &WorkspaceStatus) -> Result<Option<WorkspaceWindow>> {
    let output = workspace_command(status, "xdotool")
        .arg("getactivewindow")
        .output()
        .context("failed to run xdotool getactivewindow")?;
    if !output.status.success() {
        return Ok(None);
    }
    let text = output_text(output, "xdotool getactivewindow")?;
    let window_id = text.trim();
    if window_id.is_empty() {
        return Ok(None);
    }
    Ok(Some(window_info(status, window_id)?))
}

fn observe_workspace(
    state: &DaemonState,
    screenshot: bool,
    include_hidden: bool,
    output_path: Option<PathBuf>,
) -> Result<IpcResponse> {
    let windows = list_workspace_windows(&state.status, include_hidden)?;
    let active_window = active_workspace_window(&state.status)?;
    let screenshot = if screenshot {
        Some(capture_workspace_screenshot(&state.status, output_path)?)
    } else {
        None
    };
    Ok(IpcResponse {
        ok: true,
        message: "workspace observed".to_string(),
        apps: Some(state.status.apps.clone()),
        status: Some(state.status.clone()),
        ipc: None,
        windows: Some(windows),
        active_window,
        screenshot,
        app_log: None,
        clipboard: None,
        events: None,
    })
}

fn filter_workspace_apps(
    status: &WorkspaceStatus,
    app_id: &Option<String>,
    name_contains: &Option<String>,
    command_contains: &Option<String>,
    profile_id: &Option<String>,
    running: Option<bool>,
) -> Vec<WorkspaceApp> {
    status
        .apps
        .iter()
        .filter(|app| {
            app_id
                .as_ref()
                .is_none_or(|target| matches_app_id(app, target))
        })
        .filter(|app| {
            name_contains.as_ref().is_none_or(|needle| {
                app.name
                    .as_ref()
                    .is_some_and(|name| contains_ascii_case_insensitive(name, needle))
            })
        })
        .filter(|app| {
            command_contains
                .as_ref()
                .is_none_or(|needle| command_matches(&app.command, needle))
        })
        .filter(|app| {
            profile_id
                .as_ref()
                .is_none_or(|target| app.profile_id.as_deref() == Some(target.as_str()))
        })
        .filter(|app| running.is_none_or(|running| app.running == running))
        .cloned()
        .collect()
}

struct WindowMatchCriteria {
    title_contains: Option<String>,
    class_contains: Option<String>,
    pid: Option<u32>,
    app_id: Option<String>,
}

struct WindowWaitCriteria {
    title_contains: Option<String>,
    class_contains: Option<String>,
    pid: Option<u32>,
    app_id: Option<String>,
    timeout_ms: u64,
}

fn wait_workspace_window(
    state: &mut DaemonState,
    criteria: &WindowWaitCriteria,
) -> Result<Vec<WorkspaceWindow>> {
    wait_workspace_window_with_visibility(state, criteria, false)
}

fn wait_workspace_window_with_visibility(
    state: &mut DaemonState,
    criteria: &WindowWaitCriteria,
    include_hidden: bool,
) -> Result<Vec<WorkspaceWindow>> {
    let timeout = Duration::from_millis(criteria.timeout_ms);
    let started = Instant::now();
    loop {
        refresh_apps(state)?;
        let windows = matching_workspace_windows_with_visibility(state, criteria, include_hidden)?;
        if !windows.is_empty() {
            return Ok(windows);
        }
        if started.elapsed() >= timeout {
            return Ok(Vec::new());
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        thread::sleep(remaining.min(Duration::from_millis(100)));
    }
}

fn matching_workspace_windows_with_visibility(
    state: &DaemonState,
    criteria: &WindowWaitCriteria,
    include_hidden: bool,
) -> Result<Vec<WorkspaceWindow>> {
    let match_criteria = WindowMatchCriteria {
        title_contains: criteria.title_contains.clone(),
        class_contains: criteria.class_contains.clone(),
        pid: criteria.pid,
        app_id: criteria.app_id.clone(),
    };
    list_matching_workspace_windows(state, include_hidden, &match_criteria)
}

fn list_matching_workspace_windows(
    state: &DaemonState,
    include_hidden: bool,
    criteria: &WindowMatchCriteria,
) -> Result<Vec<WorkspaceWindow>> {
    let app_root_pid = criteria
        .app_id
        .as_ref()
        .map(|app_id| resolve_workspace_app(&state.status.apps, app_id).map(|app| app.pid))
        .transpose()?;
    Ok(list_workspace_windows(&state.status, include_hidden)?
        .into_iter()
        .filter(|window| {
            criteria
                .title_contains
                .as_ref()
                .is_none_or(|title| window.title.contains(title))
        })
        .filter(|window| {
            criteria.class_contains.as_ref().is_none_or(|class| {
                window
                    .wm_class
                    .as_ref()
                    .is_some_and(|wm_class| contains_ascii_case_insensitive(wm_class, class))
                    || window.wm_instance.as_ref().is_some_and(|wm_instance| {
                        contains_ascii_case_insensitive(wm_instance, class)
                    })
            })
        })
        .filter(|window| {
            if let Some(pid) = criteria.pid {
                return window.pid == Some(pid);
            }
            if let Some(app_root_pid) = app_root_pid {
                return window.pid.is_some_and(|window_pid| {
                    process_is_descendant_or_self(window_pid, app_root_pid)
                });
            }
            true
        })
        .collect())
}

fn process_is_descendant_or_self(pid: u32, ancestor_pid: u32) -> bool {
    let mut current = Some(pid);
    for _ in 0..64 {
        let Some(current_pid) = current else {
            return false;
        };
        if current_pid == ancestor_pid {
            return true;
        }
        current = parent_pid(current_pid);
    }
    false
}

fn parent_pid(pid: u32) -> Option<u32> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_command = stat.rsplit_once(") ")?.1;
    let mut fields = after_command.split_whitespace();
    fields.next()?;
    fields.next()?.parse().ok()
}

fn focus_matching_workspace_window(
    state: &mut DaemonState,
    criteria: &WindowWaitCriteria,
) -> Result<Option<WorkspaceWindow>> {
    let Some(window) = wait_workspace_window(state, criteria)?.into_iter().next() else {
        return Ok(None);
    };
    focus_workspace_window(&state.status, &window.id)?;
    Ok(Some(window))
}

fn close_matching_workspace_window(
    state: &mut DaemonState,
    criteria: &WindowWaitCriteria,
) -> Result<Option<WorkspaceWindow>> {
    let Some(window) = wait_workspace_window(state, criteria)?.into_iter().next() else {
        return Ok(None);
    };
    close_workspace_window(&state.status, &window.id)?;
    Ok(Some(window))
}

fn move_workspace_window_target(
    state: &mut DaemonState,
    window_id: Option<&str>,
    criteria: &WindowWaitCriteria,
    x: i32,
    y: i32,
) -> Result<Option<WorkspaceWindow>> {
    validate_workspace_coordinates(&state.status, x, y, "window move")?;
    let Some(window) = resolve_workspace_window(state, window_id, criteria)? else {
        return Ok(None);
    };
    move_workspace_window(&state.status, &window.id, x, y)
        .map(Some)
        .with_context(|| format!("failed to move workspace window {}", window.id))
}

fn resize_workspace_window_target(
    state: &mut DaemonState,
    window_id: Option<&str>,
    criteria: &WindowWaitCriteria,
    width: u32,
    height: u32,
) -> Result<Option<WorkspaceWindow>> {
    validate_window_size(width, height)?;
    validate_window_size_for_workspace(&state.status, width, height)?;
    let Some(window) = resolve_workspace_window(state, window_id, criteria)? else {
        return Ok(None);
    };
    resize_workspace_window(&state.status, &window.id, width, height)
        .map(Some)
        .with_context(|| format!("failed to resize workspace window {}", window.id))
}

fn raise_workspace_window_target(
    state: &mut DaemonState,
    window_id: Option<&str>,
    criteria: &WindowWaitCriteria,
) -> Result<Option<WorkspaceWindow>> {
    let Some(window) = resolve_workspace_window(state, window_id, criteria)? else {
        return Ok(None);
    };
    raise_workspace_window(&state.status, &window.id)
        .map(Some)
        .with_context(|| format!("failed to raise workspace window {}", window.id))
}

fn minimize_workspace_window_target(
    state: &mut DaemonState,
    window_id: Option<&str>,
    criteria: &WindowWaitCriteria,
) -> Result<Option<WorkspaceWindow>> {
    let Some(window) = resolve_workspace_window(state, window_id, criteria)? else {
        return Ok(None);
    };
    minimize_workspace_window(&state.status, &window.id)
        .with_context(|| format!("failed to minimize workspace window {}", window.id))?;
    Ok(Some(window))
}

fn show_workspace_window_target(
    state: &mut DaemonState,
    window_id: Option<&str>,
    criteria: &WindowWaitCriteria,
) -> Result<Option<WorkspaceWindow>> {
    let Some(window) = resolve_workspace_window_with_visibility(state, window_id, criteria, true)?
    else {
        return Ok(None);
    };
    show_workspace_window(&state.status, &window.id)
        .map(Some)
        .with_context(|| format!("failed to show workspace window {}", window.id))
}

struct WindowClickResult {
    window: WorkspaceWindow,
    x: i32,
    y: i32,
}

struct WindowPointerMoveResult {
    window: WorkspaceWindow,
    x: i32,
    y: i32,
}

struct WindowDragResult {
    window: WorkspaceWindow,
    from_x: i32,
    from_y: i32,
    to_x: i32,
    to_y: i32,
}

struct WindowScrollResult {
    window: WorkspaceWindow,
    x: i32,
    y: i32,
}

struct WindowPasteResult {
    window: WorkspaceWindow,
    clipboard: WorkspaceClipboard,
}

fn click_workspace_window(
    state: &mut DaemonState,
    window_id: Option<&str>,
    criteria: &WindowWaitCriteria,
    x: i32,
    y: i32,
    button: u8,
    count: u8,
) -> Result<Option<WindowClickResult>> {
    validate_relative_click_coordinates(x, y)?;
    validate_click_options(button, count)?;
    let Some(window) = resolve_workspace_window(state, window_id, criteria)? else {
        return Ok(None);
    };
    if x as u32 >= window.geometry.width || y as u32 >= window.geometry.height {
        bail!(
            "window click coordinates {x},{y} are outside window bounds {}x{}",
            window.geometry.width,
            window.geometry.height
        );
    }
    let absolute_x = window
        .geometry
        .x
        .checked_add(x)
        .context("window click X coordinate overflow")?;
    let absolute_y = window
        .geometry
        .y
        .checked_add(y)
        .context("window click Y coordinate overflow")?;
    focus_workspace_window(&state.status, &window.id)?;
    click_workspace(&state.status, absolute_x, absolute_y, button, count)?;
    Ok(Some(WindowClickResult {
        window,
        x: absolute_x,
        y: absolute_y,
    }))
}

fn move_workspace_pointer_window(
    state: &mut DaemonState,
    window_id: Option<&str>,
    criteria: &WindowWaitCriteria,
    x: i32,
    y: i32,
) -> Result<Option<WindowPointerMoveResult>> {
    validate_relative_click_coordinates(x, y)?;
    let Some(window) = resolve_workspace_window(state, window_id, criteria)? else {
        return Ok(None);
    };
    if x as u32 >= window.geometry.width || y as u32 >= window.geometry.height {
        bail!(
            "window pointer coordinates {x},{y} are outside window bounds {}x{}",
            window.geometry.width,
            window.geometry.height
        );
    }
    let absolute_x = window
        .geometry
        .x
        .checked_add(x)
        .context("window pointer X coordinate overflow")?;
    let absolute_y = window
        .geometry
        .y
        .checked_add(y)
        .context("window pointer Y coordinate overflow")?;
    focus_workspace_window(&state.status, &window.id)?;
    move_workspace_pointer(&state.status, absolute_x, absolute_y)?;
    Ok(Some(WindowPointerMoveResult {
        window,
        x: absolute_x,
        y: absolute_y,
    }))
}

fn drag_workspace_window(
    state: &mut DaemonState,
    window_id: Option<&str>,
    criteria: &WindowWaitCriteria,
    from_x: i32,
    from_y: i32,
    to_x: i32,
    to_y: i32,
    button: u8,
) -> Result<Option<WindowDragResult>> {
    validate_relative_click_coordinates(from_x, from_y)?;
    validate_relative_click_coordinates(to_x, to_y)?;
    validate_click_options(button, DEFAULT_CLICK_COUNT)?;
    let Some(window) = resolve_workspace_window(state, window_id, criteria)? else {
        return Ok(None);
    };
    if from_x as u32 >= window.geometry.width || from_y as u32 >= window.geometry.height {
        bail!(
            "window drag start coordinates {from_x},{from_y} are outside window bounds {}x{}",
            window.geometry.width,
            window.geometry.height
        );
    }
    if to_x as u32 >= window.geometry.width || to_y as u32 >= window.geometry.height {
        bail!(
            "window drag end coordinates {to_x},{to_y} are outside window bounds {}x{}",
            window.geometry.width,
            window.geometry.height
        );
    }
    let absolute_from_x = window
        .geometry
        .x
        .checked_add(from_x)
        .context("window drag start X coordinate overflow")?;
    let absolute_from_y = window
        .geometry
        .y
        .checked_add(from_y)
        .context("window drag start Y coordinate overflow")?;
    let absolute_to_x = window
        .geometry
        .x
        .checked_add(to_x)
        .context("window drag end X coordinate overflow")?;
    let absolute_to_y = window
        .geometry
        .y
        .checked_add(to_y)
        .context("window drag end Y coordinate overflow")?;
    focus_workspace_window(&state.status, &window.id)?;
    drag_workspace(
        &state.status,
        absolute_from_x,
        absolute_from_y,
        absolute_to_x,
        absolute_to_y,
        button,
    )?;
    Ok(Some(WindowDragResult {
        window,
        from_x: absolute_from_x,
        from_y: absolute_from_y,
        to_x: absolute_to_x,
        to_y: absolute_to_y,
    }))
}

fn scroll_workspace_window(
    state: &mut DaemonState,
    window_id: Option<&str>,
    criteria: &WindowWaitCriteria,
    x: i32,
    y: i32,
    direction: ScrollDirection,
    amount: u8,
) -> Result<Option<WindowScrollResult>> {
    validate_relative_click_coordinates(x, y)?;
    validate_scroll_options(direction, amount)?;
    let Some(window) = resolve_workspace_window(state, window_id, criteria)? else {
        return Ok(None);
    };
    if x as u32 >= window.geometry.width || y as u32 >= window.geometry.height {
        bail!(
            "window scroll coordinates {x},{y} are outside window bounds {}x{}",
            window.geometry.width,
            window.geometry.height
        );
    }
    let absolute_x = window
        .geometry
        .x
        .checked_add(x)
        .context("window scroll X coordinate overflow")?;
    let absolute_y = window
        .geometry
        .y
        .checked_add(y)
        .context("window scroll Y coordinate overflow")?;
    focus_workspace_window(&state.status, &window.id)?;
    scroll_workspace(&state.status, absolute_x, absolute_y, direction, amount)?;
    Ok(Some(WindowScrollResult {
        window,
        x: absolute_x,
        y: absolute_y,
    }))
}

fn key_workspace_window(
    state: &mut DaemonState,
    window_id: Option<&str>,
    criteria: &WindowWaitCriteria,
    key: String,
) -> Result<Option<WorkspaceWindow>> {
    let Some(window) = focus_workspace_window_target(state, window_id, criteria)? else {
        return Ok(None);
    };
    key_workspace(&state.status, key)?;
    Ok(Some(window))
}

fn type_workspace_window(
    state: &mut DaemonState,
    window_id: Option<&str>,
    criteria: &WindowWaitCriteria,
    text: String,
) -> Result<Option<WorkspaceWindow>> {
    let Some(window) = focus_workspace_window_target(state, window_id, criteria)? else {
        return Ok(None);
    };
    type_workspace_text(&state.status, text)?;
    Ok(Some(window))
}

fn paste_workspace_window(
    state: &mut DaemonState,
    window_id: Option<&str>,
    criteria: &WindowWaitCriteria,
    text: &str,
    key: &str,
) -> Result<Option<WindowPasteResult>> {
    validate_clipboard_text(text)?;
    validate_key_text(key)?;
    let Some(window) = focus_workspace_window_target(state, window_id, criteria)? else {
        return Ok(None);
    };
    let clipboard = paste_workspace_text(&state.status, text, key)?;
    Ok(Some(WindowPasteResult { window, clipboard }))
}

fn focus_workspace_window_target(
    state: &mut DaemonState,
    window_id: Option<&str>,
    criteria: &WindowWaitCriteria,
) -> Result<Option<WorkspaceWindow>> {
    let Some(window) = resolve_workspace_window(state, window_id, criteria)? else {
        return Ok(None);
    };
    focus_workspace_window(&state.status, &window.id)?;
    Ok(Some(window))
}

fn resolve_workspace_window(
    state: &mut DaemonState,
    window_id: Option<&str>,
    criteria: &WindowWaitCriteria,
) -> Result<Option<WorkspaceWindow>> {
    resolve_workspace_window_with_visibility(state, window_id, criteria, false)
}

fn resolve_workspace_window_with_visibility(
    state: &mut DaemonState,
    window_id: Option<&str>,
    criteria: &WindowWaitCriteria,
    include_hidden: bool,
) -> Result<Option<WorkspaceWindow>> {
    if let Some(window_id) = window_id {
        let window_id = sanitize_x11_id(window_id, "window id")?;
        return window_info(&state.status, &window_id).map(Some);
    }
    Ok(
        wait_workspace_window_with_visibility(state, criteria, include_hidden)?
            .into_iter()
            .next(),
    )
}

fn window_info(status: &WorkspaceStatus, id: &str) -> Result<WorkspaceWindow> {
    let visible = search_workspace_window_ids(status, true)?
        .iter()
        .any(|visible_id| visible_id == id);
    window_info_with_visibility(status, id, visible)
}

fn window_info_with_visibility(
    status: &WorkspaceStatus,
    id: &str,
    visible: bool,
) -> Result<WorkspaceWindow> {
    let title = workspace_command(status, "xdotool")
        .args(["getwindowname", id])
        .output()
        .with_context(|| format!("failed to read window name for {id}"))
        .and_then(|output| output_text(output, "xdotool getwindowname"))
        .unwrap_or_default()
        .trim()
        .to_string();
    let (wm_instance, wm_class) = window_class_from_xprop(status, id);
    let pid = workspace_command(status, "xdotool")
        .args(["getwindowpid", id])
        .output()
        .ok()
        .and_then(|output| output.status.success().then_some(output.stdout))
        .and_then(|stdout| String::from_utf8(stdout).ok())
        .and_then(|text| text.trim().parse::<u32>().ok())
        .or_else(|| window_pid_from_xprop(status, id));
    let geometry_output = workspace_command(status, "xdotool")
        .args(["getwindowgeometry", "--shell", id])
        .output()
        .with_context(|| format!("failed to read window geometry for {id}"))?;
    let geometry_text = output_text(geometry_output, "xdotool getwindowgeometry")?;

    Ok(WorkspaceWindow {
        id: id.to_string(),
        title,
        wm_class,
        wm_instance,
        pid,
        app_id: pid.and_then(|pid| workspace_app_id_for_pid(status, pid)),
        visible,
        geometry: parse_window_geometry(&geometry_text)?,
    })
}

fn workspace_app_id_for_pid(status: &WorkspaceStatus, pid: u32) -> Option<String> {
    status
        .apps
        .iter()
        .find(|app| process_is_descendant_or_self(pid, app.pid))
        .map(|app| app.id.clone())
}

fn window_pid_from_xprop(status: &WorkspaceStatus, id: &str) -> Option<u32> {
    let output = workspace_command(status, "xprop")
        .args(["-id", id, "_NET_WM_PID"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    text.rsplit_once('=')
        .and_then(|(_, value)| value.trim().parse::<u32>().ok())
}

fn window_class_from_xprop(status: &WorkspaceStatus, id: &str) -> (Option<String>, Option<String>) {
    let output = workspace_command(status, "xprop")
        .args(["-id", id, "WM_CLASS"])
        .output();
    let Some(stdout) = output
        .ok()
        .and_then(|output| output.status.success().then_some(output.stdout))
    else {
        return (None, None);
    };
    let Some(text) = String::from_utf8(stdout).ok() else {
        return (None, None);
    };
    let Some((_, values)) = text.split_once('=') else {
        return (None, None);
    };
    let mut parts = values.split(',').map(parse_xprop_string);
    let instance = parts.next().flatten();
    let class = parts.next().flatten();
    (instance, class)
}

fn parse_xprop_string(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches('"').trim();
    (!trimmed.is_empty() && trimmed != "not found.").then(|| trimmed.to_string())
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

struct WindowScreenshotResult {
    window: WorkspaceWindow,
    screenshot: WorkspaceScreenshot,
}

fn screenshot_workspace_window(
    state: &mut DaemonState,
    window_id: Option<&str>,
    criteria: &WindowWaitCriteria,
    output_path: Option<PathBuf>,
) -> Result<Option<WindowScreenshotResult>> {
    let Some(window) = resolve_workspace_window(state, window_id, criteria)? else {
        return Ok(None);
    };
    let screenshot = capture_workspace_window_screenshot(&state.status, &window, output_path)?;
    Ok(Some(WindowScreenshotResult { window, screenshot }))
}

fn capture_workspace_window_screenshot(
    status: &WorkspaceStatus,
    window: &WorkspaceWindow,
    output_path: Option<PathBuf>,
) -> Result<WorkspaceScreenshot> {
    let path = resolve_window_screenshot_path(status, &window.id, output_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    if command_path_check("import").ok {
        let output = workspace_command(status, "import")
            .args(["-window", &window.id])
            .arg(&path)
            .output()
            .context("failed to run import for workspace window screenshot")?;
        output_text(output, "import -window")?;
    } else if command_path_check("scrot").ok {
        focus_workspace_window(status, &window.id)?;
        let output = workspace_command(status, "scrot")
            .args(["-u"])
            .arg(&path)
            .output()
            .context("failed to run scrot for workspace window screenshot")?;
        output_text(output, "scrot -u")?;
    } else {
        bail!("missing screenshot command: install ImageMagick import or scrot");
    }

    Ok(WorkspaceScreenshot {
        path,
        width: window.geometry.width,
        height: window.geometry.height,
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

fn resolve_window_screenshot_path(
    status: &WorkspaceStatus,
    window_id: &str,
    output_path: Option<PathBuf>,
) -> PathBuf {
    match output_path {
        Some(path) if path.is_absolute() => path,
        Some(path) => status.runtime_dir.join(path),
        None => status
            .runtime_dir
            .join(format!("screenshot-window-{window_id}-{}.png", unix_now())),
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

fn move_workspace_window(
    status: &WorkspaceStatus,
    window_id: &str,
    x: i32,
    y: i32,
) -> Result<WorkspaceWindow> {
    let window_id = sanitize_x11_id(window_id, "window id")?;
    validate_workspace_coordinates(status, x, y, "window move")?;
    let output = workspace_command(status, "xdotool")
        .args([
            "windowmove",
            "--sync",
            &window_id,
            &x.to_string(),
            &y.to_string(),
        ])
        .output()
        .context("failed to run xdotool windowmove")?;
    output_text(output, "xdotool windowmove")?;
    window_info(status, &window_id)
}

fn resize_workspace_window(
    status: &WorkspaceStatus,
    window_id: &str,
    width: u32,
    height: u32,
) -> Result<WorkspaceWindow> {
    let window_id = sanitize_x11_id(window_id, "window id")?;
    validate_window_size(width, height)?;
    validate_window_size_for_workspace(status, width, height)?;
    let output = workspace_command(status, "xdotool")
        .args([
            "windowsize",
            "--sync",
            &window_id,
            &width.to_string(),
            &height.to_string(),
        ])
        .output()
        .context("failed to run xdotool windowsize")?;
    output_text(output, "xdotool windowsize")?;
    window_info(status, &window_id)
}

fn raise_workspace_window(status: &WorkspaceStatus, window_id: &str) -> Result<WorkspaceWindow> {
    let window_id = sanitize_x11_id(window_id, "window id")?;
    let output = workspace_command(status, "xdotool")
        .args(["windowraise", &window_id])
        .output()
        .context("failed to run xdotool windowraise")?;
    output_text(output, "xdotool windowraise")?;
    window_info(status, &window_id)
}

fn minimize_workspace_window(status: &WorkspaceStatus, window_id: &str) -> Result<()> {
    let window_id = sanitize_x11_id(window_id, "window id")?;
    let output = workspace_command(status, "xdotool")
        .args(["windowminimize", "--sync", &window_id])
        .output()
        .context("failed to run xdotool windowminimize")?;
    output_text(output, "xdotool windowminimize")?;
    Ok(())
}

fn show_workspace_window(status: &WorkspaceStatus, window_id: &str) -> Result<WorkspaceWindow> {
    let window_id = sanitize_x11_id(window_id, "window id")?;
    let output = workspace_command(status, "xdotool")
        .args(["windowmap", "--sync", &window_id])
        .output()
        .context("failed to run xdotool windowmap")?;
    output_text(output, "xdotool windowmap")?;
    window_info(status, &window_id)
}

fn click_workspace(status: &WorkspaceStatus, x: i32, y: i32, button: u8, count: u8) -> Result<()> {
    validate_workspace_coordinates(status, x, y, "click")?;
    validate_click_options(button, count)?;
    let output = workspace_command(status, "xdotool")
        .args(["mousemove", "--sync", &x.to_string(), &y.to_string()])
        .args(["click", "--repeat", &count.to_string(), &button.to_string()])
        .output()
        .context("failed to run xdotool click")?;
    output_text(output, "xdotool click")?;
    Ok(())
}

fn move_workspace_pointer(status: &WorkspaceStatus, x: i32, y: i32) -> Result<()> {
    validate_workspace_coordinates(status, x, y, "pointer")?;
    let output = workspace_command(status, "xdotool")
        .args(["mousemove", "--sync", &x.to_string(), &y.to_string()])
        .output()
        .context("failed to run xdotool mousemove")?;
    output_text(output, "xdotool mousemove")?;
    Ok(())
}

fn drag_workspace(
    status: &WorkspaceStatus,
    from_x: i32,
    from_y: i32,
    to_x: i32,
    to_y: i32,
    button: u8,
) -> Result<()> {
    validate_workspace_coordinates(status, from_x, from_y, "drag start")?;
    validate_workspace_coordinates(status, to_x, to_y, "drag end")?;
    validate_click_options(button, DEFAULT_CLICK_COUNT)?;
    let output = workspace_command(status, "xdotool")
        .args([
            "mousemove",
            "--sync",
            &from_x.to_string(),
            &from_y.to_string(),
        ])
        .args(["mousedown", &button.to_string()])
        .args(["mousemove", "--sync", &to_x.to_string(), &to_y.to_string()])
        .args(["mouseup", &button.to_string()])
        .output()
        .context("failed to run xdotool drag")?;
    output_text(output, "xdotool drag")?;
    Ok(())
}

fn scroll_workspace(
    status: &WorkspaceStatus,
    x: i32,
    y: i32,
    direction: ScrollDirection,
    amount: u8,
) -> Result<()> {
    validate_workspace_coordinates(status, x, y, "scroll")?;
    validate_scroll_options(direction, amount)?;
    let button = direction.x11_button().to_string();
    let amount = amount.to_string();
    let output = workspace_command(status, "xdotool")
        .args(["mousemove", "--sync", &x.to_string(), &y.to_string()])
        .args(["click", "--repeat", &amount, &button])
        .output()
        .context("failed to run xdotool scroll")?;
    output_text(output, "xdotool scroll")?;
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

fn paste_workspace_text(
    status: &WorkspaceStatus,
    text: &str,
    key: &str,
) -> Result<WorkspaceClipboard> {
    validate_clipboard_text(text)?;
    validate_key_text(key)?;
    let clipboard = set_workspace_clipboard(status, text)?;
    key_workspace(status, key.trim().to_string())?;
    Ok(clipboard)
}

fn set_workspace_clipboard(status: &WorkspaceStatus, text: &str) -> Result<WorkspaceClipboard> {
    validate_clipboard_text(text)?;
    if command_path_check("xclip").ok {
        write_clipboard_command(
            status,
            "xclip",
            &["-selection", "clipboard"],
            text,
            "xclip clipboard input",
        )?;
    } else if command_path_check("xsel").ok {
        write_clipboard_command(
            status,
            "xsel",
            &["--clipboard", "--input"],
            text,
            "xsel clipboard input",
        )?;
    } else {
        bail!("missing clipboard command: install xclip or xsel");
    }

    Ok(WorkspaceClipboard {
        selection: "clipboard".to_string(),
        content: None,
        bytes: text.len() as u64,
    })
}

fn get_workspace_clipboard(status: &WorkspaceStatus) -> Result<WorkspaceClipboard> {
    let content = if command_path_check("xclip").ok {
        let output = workspace_command(status, "xclip")
            .args(["-selection", "clipboard", "-out"])
            .output()
            .context("failed to run xclip clipboard output")?;
        output_text(output, "xclip clipboard output")?
    } else if command_path_check("xsel").ok {
        let output = workspace_command(status, "xsel")
            .args(["--clipboard", "--output"])
            .output()
            .context("failed to run xsel clipboard output")?;
        output_text(output, "xsel clipboard output")?
    } else {
        bail!("missing clipboard command: install xclip or xsel");
    };

    Ok(WorkspaceClipboard {
        selection: "clipboard".to_string(),
        bytes: content.len() as u64,
        content: Some(content),
    })
}

fn write_clipboard_command(
    status: &WorkspaceStatus,
    command: &str,
    args: &[&str],
    text: &str,
    label: &str,
) -> Result<()> {
    let mut child = workspace_command(status, command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to run {label}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .with_context(|| format!("failed to write stdin for {label}"))?;
    } else {
        bail!("failed to open stdin for {label}");
    }
    let status = child
        .wait()
        .with_context(|| format!("failed to wait for {label}"))?;
    if !status.success() {
        bail!("{label} failed with {status}");
    }
    Ok(())
}

fn read_workspace_app_log(
    state: &mut DaemonState,
    app_id: &str,
    stream: &str,
    tail_bytes: Option<u64>,
) -> Result<(WorkspaceAppLog, WorkspaceApp)> {
    refresh_apps(state)?;
    let stream = validate_log_stream(stream)?;
    let app = resolve_workspace_app(&state.status.apps, app_id)?;
    let path = match stream.as_str() {
        "stdout" => app.stdout_path.as_ref(),
        "stderr" => app.stderr_path.as_ref(),
        _ => None,
    }
    .ok_or_else(|| anyhow!("workspace app {} has no {stream} log path", app.id))?;
    let (content, bytes_read, truncated) = read_log_content(path, tail_bytes)?;

    Ok((
        WorkspaceAppLog {
            app_id: app.id.clone(),
            stream,
            path: path.clone(),
            content,
            bytes_read,
            truncated,
        },
        app.clone(),
    ))
}

fn wait_workspace_app(
    state: &mut DaemonState,
    app_id: &str,
    timeout_ms: u64,
    kill_on_timeout: bool,
) -> Result<(bool, bool, WorkspaceApp)> {
    let app_id = app_id.trim();
    if app_id.is_empty() {
        bail!("app id cannot be empty");
    }

    let timeout = Duration::from_millis(timeout_ms);
    let started = Instant::now();
    loop {
        refresh_apps(state)?;
        let app = resolve_workspace_app(&state.status.apps, app_id)?;
        if !app.running {
            return Ok((true, false, app.clone()));
        }
        if started.elapsed() >= timeout {
            if kill_on_timeout {
                let (_message, app, killed) = kill_workspace_app(state, app_id)?;
                return Ok((!app.running, killed, app));
            }
            return Ok((false, false, app.clone()));
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        thread::sleep(remaining.min(Duration::from_millis(100)));
    }
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

fn terminate_app_process(app_id: &str, child: &mut Child) -> Result<ExitStatus> {
    if let Some(status) = child
        .try_wait()
        .with_context(|| format!("failed to check workspace app {app_id} status"))?
    {
        return Ok(status);
    }

    let pgid = child.id();
    signal_process_group(pgid, SIGTERM)
        .with_context(|| format!("failed to terminate workspace app {app_id} process group"))?;
    let started = Instant::now();
    loop {
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("failed to check workspace app {app_id} status"))?
        {
            return Ok(status);
        }
        if started.elapsed() >= Duration::from_millis(APP_TERMINATE_GRACE_MS) {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    signal_process_group(pgid, SIGKILL)
        .with_context(|| format!("failed to kill workspace app {app_id} process group"))?;
    let _ = child.kill();
    child
        .wait()
        .with_context(|| format!("failed to wait for workspace app {app_id}"))
}

fn signal_process_group(pgid: u32, signal: i32) -> Result<()> {
    if pgid > i32::MAX as u32 {
        bail!("process group id {pgid} is too large to signal");
    }
    let target = -(pgid as i32);
    let result = unsafe { kill(target, signal) };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(ESRCH) {
        return Ok(());
    }
    Err(error).with_context(|| format!("failed to signal process group {pgid}"))
}

fn kill_workspace_app(
    state: &mut DaemonState,
    app_id: &str,
) -> Result<(String, WorkspaceApp, bool)> {
    let app_id = app_id.trim();
    if app_id.is_empty() {
        bail!("app id cannot be empty");
    }

    let (message, exit_detail, app_info, killed) = {
        let app = resolve_workspace_app_process_mut(&mut state.apps, app_id)?;

        if !app.info.running {
            (
                format!("workspace app {} is already stopped", app.info.id),
                None,
                app.info.clone(),
                false,
            )
        } else if let Some(status) = app
            .child
            .try_wait()
            .context("failed to check app process status")?
        {
            apply_app_exit_status(&mut app.info, status);
            (
                format!("workspace app {} is already stopped", app.info.id),
                Some(app_exit_event_detail(&app.info)),
                app.info.clone(),
                false,
            )
        } else {
            let status = terminate_app_process(&app.info.id, &mut app.child)?;
            apply_app_exit_status(&mut app.info, status);
            (
                format!("workspace app {} killed", app.info.id),
                Some(app_exit_event_detail(&app.info)),
                app.info.clone(),
                true,
            )
        }
    };

    state.status.apps = state.apps.iter().map(|app| app.info.clone()).collect();
    if let Some(detail) = exit_detail {
        record_event(state, "app_exit", detail)?;
    }
    Ok((message, app_info, killed))
}

fn resolve_workspace_app<'a>(apps: &'a [WorkspaceApp], app_id: &str) -> Result<&'a WorkspaceApp> {
    let mut matches = apps.iter().filter(|app| matches_app_id(app, app_id));
    let Some(first) = matches.next() else {
        bail!("workspace app {app_id:?} was not found");
    };
    if let Some(second) = matches.next() {
        let mut labels = vec![app_label(first), app_label(second)];
        labels.extend(matches.map(app_label));
        bail!(
            "workspace app target {app_id:?} matched multiple apps: {}",
            labels.join(", ")
        );
    }
    Ok(first)
}

fn resolve_workspace_app_process_mut<'a>(
    apps: &'a mut [AppProcess],
    app_id: &str,
) -> Result<&'a mut AppProcess> {
    let matches = apps
        .iter()
        .enumerate()
        .filter(|(_, app)| matches_app_id(&app.info, app_id))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => bail!("workspace app {app_id:?} was not found"),
        [index] => Ok(&mut apps[*index]),
        _ => {
            let labels = matches
                .iter()
                .map(|index| app_label(&apps[*index].info))
                .collect::<Vec<_>>()
                .join(", ");
            bail!("workspace app target {app_id:?} matched multiple apps: {labels}")
        }
    }
}

fn app_label(app: &WorkspaceApp) -> String {
    match &app.name {
        Some(name) => format!("{} (name {name:?}, pid {})", app.id, app.pid),
        None => format!("{} (pid {})", app.id, app.pid),
    }
}

fn matches_app_id(app: &WorkspaceApp, app_id: &str) -> bool {
    app.id == app_id || app.pid.to_string() == app_id || app.name.as_deref() == Some(app_id)
}

fn command_matches(command: &[String], needle: &str) -> bool {
    command
        .iter()
        .any(|arg| contains_ascii_case_insensitive(arg, needle))
        || contains_ascii_case_insensitive(&command.join(" "), needle)
}

fn response_last_app_id(response: &IpcResponse) -> Option<String> {
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

fn response_app<'a>(response: &'a IpcResponse, app_id: &str) -> Option<&'a WorkspaceApp> {
    if let Some(app) = response
        .apps
        .as_ref()
        .and_then(|apps| apps.iter().find(|app| matches_app_id(app, app_id)))
    {
        return Some(app);
    }
    response
        .status
        .as_ref()?
        .apps
        .iter()
        .find(|app| matches_app_id(app, app_id))
}

fn apply_app_exit_status(app: &mut WorkspaceApp, status: ExitStatus) {
    app.running = false;
    app.exit_status = Some(status.to_string());
    app.exit_code = status.code();
    app.exit_signal = status.signal();
}

fn mark_app_exit_error(app: &mut WorkspaceApp, error: impl ToString) {
    app.running = false;
    app.exit_status = Some(error.to_string());
    app.exit_code = None;
    app.exit_signal = None;
}

fn app_exit_event_detail(app: &WorkspaceApp) -> serde_json::Value {
    serde_json::json!({
        "app_id": &app.id,
        "name": app.name.as_deref(),
        "pid": app.pid,
        "command": &app.command,
        "profile_id": app.profile_id.as_deref(),
        "exit_status": app.exit_status.as_deref(),
        "exit_code": app.exit_code,
        "exit_signal": app.exit_signal,
    })
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

fn refresh_apps(state: &mut DaemonState) -> Result<()> {
    let mut exit_events = Vec::new();
    for app in &mut state.apps {
        if app.info.running {
            match app.child.try_wait() {
                Ok(Some(status)) => {
                    apply_app_exit_status(&mut app.info, status);
                    exit_events.push(app_exit_event_detail(&app.info));
                }
                Ok(None) => {}
                Err(error) => {
                    mark_app_exit_error(&mut app.info, error);
                    exit_events.push(app_exit_event_detail(&app.info));
                }
            }
        }
    }
    state.status.apps = state.apps.iter().map(|app| app.info.clone()).collect();
    for detail in exit_events {
        record_event(state, "app_exit", detail)?;
    }
    Ok(())
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

fn wait_for_socket_removed(socket_path: &Path, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !socket_path.exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    bail!(
        "timed out waiting for workspace IPC socket {} to be removed",
        socket_path.display()
    );
}

fn pick_display() -> Result<String> {
    for number in DISPLAY_RANGE {
        let display = format!(":{number}");
        let socket = PathBuf::from(format!("/tmp/.X11-unix/X{number}"));
        let lock = PathBuf::from(format!("/tmp/.X{number}-lock"));
        if socket.exists() || lock.exists() {
            if display_is_reachable(&display) {
                continue;
            }
            if remove_dead_x11_display_artifacts(number, &socket, &lock)? {
                return Ok(display);
            }
            continue;
        }
        if !display_is_reachable(&display) {
            return Ok(display);
        }
    }
    bail!("no free X11 display found in range :90..:179");
}

fn display_is_reachable(display: &str) -> bool {
    Command::new("xdpyinfo")
        .arg("-display")
        .arg(display)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn remove_dead_x11_display_artifacts(number: u32, socket: &Path, lock: &Path) -> Result<bool> {
    let Some(pid) = read_x11_lock_pid(lock) else {
        return Ok(false);
    };
    if process_exists(pid) {
        return Ok(false);
    }
    if socket.exists() {
        fs::remove_file(socket)
            .with_context(|| format!("failed to remove stale X11 socket {}", socket.display()))?;
    }
    if lock.exists() {
        fs::remove_file(lock)
            .with_context(|| format!("failed to remove stale X11 lock {}", lock.display()))?;
    }
    eprintln!("removed stale X11 display artifacts for :{number} with dead pid {pid}");
    Ok(true)
}

fn read_x11_lock_pid(lock: &Path) -> Option<u32> {
    if !lock.exists() {
        return None;
    }
    let Ok(content) = fs::read_to_string(lock) else {
        return None;
    };
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<u32>().ok()
}

fn process_exists(pid: u32) -> bool {
    PathBuf::from(format!("/proc/{pid}")).exists()
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

fn validate_app_list_filters(
    app_id: &Option<String>,
    name_contains: &Option<String>,
    command_contains: &Option<String>,
    profile_id: &Option<String>,
) -> Result<()> {
    if app_id
        .as_ref()
        .is_some_and(|app_id| app_id.trim().is_empty())
    {
        bail!("app id cannot be empty");
    }
    if name_contains
        .as_ref()
        .is_some_and(|name| name.trim().is_empty())
    {
        bail!("app name filter cannot be empty");
    }
    if command_contains
        .as_ref()
        .is_some_and(|command| command.trim().is_empty())
    {
        bail!("app command filter cannot be empty");
    }
    if profile_id
        .as_ref()
        .is_some_and(|profile| profile.trim().is_empty())
    {
        bail!("profile id cannot be empty");
    }
    Ok(())
}

fn validate_window_match_options(
    title_contains: &Option<String>,
    class_contains: &Option<String>,
    pid: Option<u32>,
    app_id: &Option<String>,
    require_filter: bool,
) -> Result<()> {
    if title_contains
        .as_ref()
        .is_some_and(|title| title.trim().is_empty())
    {
        bail!("window title filter cannot be empty");
    }
    if class_contains
        .as_ref()
        .is_some_and(|class| class.trim().is_empty())
    {
        bail!("window class filter cannot be empty");
    }
    if app_id
        .as_ref()
        .is_some_and(|app_id| app_id.trim().is_empty())
    {
        bail!("app id cannot be empty");
    }
    if require_filter
        && title_contains.is_none()
        && class_contains.is_none()
        && pid.is_none()
        && app_id.is_none()
    {
        bail!("window match requires --title, --class, --pid, or --app");
    }
    Ok(())
}

fn validate_window_list_filters(
    title_contains: &Option<String>,
    class_contains: &Option<String>,
    pid: Option<u32>,
    app_id: &Option<String>,
) -> Result<()> {
    validate_window_match_options(title_contains, class_contains, pid, app_id, false)
}

fn validate_window_target_options(
    window_id: &Option<String>,
    title_contains: &Option<String>,
    class_contains: &Option<String>,
    pid: Option<u32>,
    app_id: &Option<String>,
) -> Result<()> {
    if let Some(window_id) = window_id {
        sanitize_x11_id(window_id, "window id")?;
    }
    validate_window_match_options(
        title_contains,
        class_contains,
        pid,
        app_id,
        window_id.is_none(),
    )?;
    if window_id.is_some()
        && (title_contains.is_some()
            || class_contains.is_some()
            || pid.is_some()
            || app_id.is_some())
    {
        bail!("window target accepts either a window id or match filters, not both");
    }
    Ok(())
}

fn contains_ascii_case_insensitive(value: &str, needle: &str) -> bool {
    value
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

fn validate_relative_click_coordinates(x: i32, y: i32) -> Result<()> {
    if x < 0 || y < 0 {
        bail!("window click coordinates must be non-negative");
    }
    Ok(())
}

fn validate_workspace_coordinates(
    status: &WorkspaceStatus,
    x: i32,
    y: i32,
    label: &str,
) -> Result<()> {
    if x < 0 || y < 0 || x as u32 >= status.width || y as u32 >= status.height {
        bail!(
            "{label} coordinates {x},{y} are outside workspace bounds {}x{}",
            status.width,
            status.height
        );
    }
    Ok(())
}

fn validate_window_size(width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 {
        bail!("window size must be positive");
    }
    Ok(())
}

fn validate_window_size_for_workspace(
    status: &WorkspaceStatus,
    width: u32,
    height: u32,
) -> Result<()> {
    if width > status.width || height > status.height {
        bail!(
            "window size {}x{} is outside workspace bounds {}x{}",
            width,
            height,
            status.width,
            status.height
        );
    }
    Ok(())
}

fn validate_click_options(button: u8, count: u8) -> Result<()> {
    if !(1..=5).contains(&button) {
        bail!("click button must be between 1 and 5");
    }
    if count == 0 || count > 20 {
        bail!("click count must be between 1 and 20");
    }
    Ok(())
}

fn validate_scroll_options(_direction: ScrollDirection, amount: u8) -> Result<()> {
    if amount == 0 || amount > MAX_SCROLL_AMOUNT {
        bail!("scroll amount must be between 1 and {MAX_SCROLL_AMOUNT}");
    }
    Ok(())
}

fn validate_clipboard_text(text: &str) -> Result<()> {
    if text.is_empty() {
        bail!("clipboard text cannot be empty");
    }
    if text.contains('\0') {
        bail!("clipboard text cannot contain NUL bytes");
    }
    Ok(())
}

fn normalize_paste_key(key: Option<String>) -> Result<String> {
    let key = key.unwrap_or_else(|| DEFAULT_PASTE_KEY.to_string());
    validate_key_text(&key)?;
    Ok(key.trim().to_string())
}

fn validate_key_text(key: &str) -> Result<()> {
    if key.trim().is_empty() {
        bail!("key cannot be empty");
    }
    Ok(())
}

pub fn validate_optional_app_name(name: &Option<String>) -> Result<()> {
    let Some(name) = name else {
        return Ok(());
    };
    if name.trim().is_empty() {
        bail!("app name cannot be empty");
    }
    if name.contains('\0') {
        bail!("app name cannot contain NUL bytes");
    }
    Ok(())
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
