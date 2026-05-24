use crate::profile::{self, WorkspaceProfile};
use crate::workspace::{
    self, EnvVar, IpcResponse, LaunchSpec, ScrollDirection, WorkspaceStartOptions, WorkspaceStatus,
    DEFAULT_WORKSPACE_ID,
};
use anyhow::Result;
use rmcp::{
    handler::server::wrapper::{Json, Parameters},
    schemars::JsonSchema,
    tool, tool_handler, tool_router, ServerHandler, ServiceExt,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Default)]
pub struct AgentWorkspaceLinux;

#[tool_router]
impl AgentWorkspaceLinux {
    #[tool(
        name = "workspace_doctor",
        description = "Report readiness for isolated Linux agent workspaces, including optional policy backend candidates such as bubblewrap, firejail, unshare, and slirp4netns.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_doctor(&self) -> Json<workspace::DoctorReport> {
        Json(workspace::doctor_report())
    }

    #[tool(
        name = "profile_path",
        description = "Return the local JSON file path used to persist agent workspace profiles.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn profile_path(&self) -> Json<profile::ProfilePath> {
        Json(profile::profile_path())
    }

    #[tool(
        name = "profile_list",
        description = "List saved agent workspace profiles.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn profile_list(&self) -> Json<profile::ProfileList> {
        Json(
            profile::list_profiles().unwrap_or_else(|error| profile::ProfileList {
                path: std::path::PathBuf::new(),
                profiles: vec![WorkspaceProfile {
                    id: "error".to_string(),
                    description: Some(error.to_string()),
                    width: None,
                    height: None,
                    cwd: None,
                    env: Vec::new(),
                    mounts: Vec::new(),
                    network: Default::default(),
                    setup_commands: Vec::new(),
                    startup_apps: Vec::new(),
                }],
            }),
        )
    }

    #[tool(
        name = "profile_get",
        description = "Get one saved agent workspace profile by id.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn profile_get(
        &self,
        Parameters(params): Parameters<ProfileIdParams>,
    ) -> Json<ProfileGetResult> {
        Json(match profile::get_profile(&params.id) {
            Ok(profile) => ProfileGetResult {
                ok: true,
                message: "profile returned".to_string(),
                profile: Some(profile),
            },
            Err(error) => ProfileGetResult {
                ok: false,
                message: error.to_string(),
                profile: None,
            },
        })
    }

    #[tool(
        name = "profile_check",
        description = "Preflight a saved profile against the current machine. Returns the applied policy, backend candidates, warnings, and acknowledgement requirements before starting a workspace.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn profile_check(
        &self,
        Parameters(params): Parameters<ProfileIdParams>,
    ) -> Json<ProfileCheckResult> {
        Json(match profile::check_profile(&params.id) {
            Ok(check) => ProfileCheckResult {
                ok: true,
                message: "profile check returned".to_string(),
                check: Some(check),
            },
            Err(error) => ProfileCheckResult {
                ok: false,
                message: error.to_string(),
                check: None,
            },
        })
    }

    #[tool(
        name = "profile_template",
        description = "Generate a starter profile without saving it. The project-dev template mounts a project directory read-write at /workspace/project.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn profile_template(
        &self,
        Parameters(params): Parameters<ProfileTemplateParams>,
    ) -> Json<ProfileGetResult> {
        Json(
            match profile::template_profile(&params.kind, params.id, params.host_path) {
                Ok(profile) => ProfileGetResult {
                    ok: true,
                    message: "profile template returned".to_string(),
                    profile: Some(profile),
                },
                Err(error) => ProfileGetResult {
                    ok: false,
                    message: error.to_string(),
                    profile: None,
                },
            },
        )
    }

    #[tool(
        name = "profile_put",
        description = "Create or replace an agent workspace profile. Mounts, network, and setup commands are persisted as declared intent and surfaced in workspace status; display size, cwd, and env are currently applied by the X11 runtime.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn profile_put(
        &self,
        Parameters(params): Parameters<ProfilePutParams>,
    ) -> Json<ProfileGetResult> {
        Json(match profile::put_profile(params.profile) {
            Ok(profile) => ProfileGetResult {
                ok: true,
                message: "profile saved".to_string(),
                profile: Some(profile),
            },
            Err(error) => ProfileGetResult {
                ok: false,
                message: error.to_string(),
                profile: None,
            },
        })
    }

    #[tool(
        name = "profile_delete",
        description = "Delete one saved agent workspace profile by id.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn profile_delete(
        &self,
        Parameters(params): Parameters<ProfileIdParams>,
    ) -> Json<profile::ProfileDeleteResult> {
        Json(
            profile::delete_profile(&params.id).unwrap_or(profile::ProfileDeleteResult {
                id: params.id,
                deleted: false,
            }),
        )
    }

    #[tool(
        name = "workspace_start",
        description = "Start an isolated X11 agent workspace with its own display and control IPC socket. Set acknowledge_hidden_workspace=true to confirm the user knows this creates a separate agent-controlled environment. If the selected profile requests currently unenforced mount or network restrictions, also set acknowledge_unenforced_policy=true. Mount profiles and disabled-network profiles are enforced with bubblewrap when available.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_start(
        &self,
        Parameters(params): Parameters<WorkspaceStartParams>,
    ) -> Json<IpcResponse> {
        Json(result_response(
            params.into_options().and_then(workspace::start_workspace),
        ))
    }

    #[tool(
        name = "workspace_open_profile",
        description = "Start a profile-backed isolated workspace, optionally run profile setup, and launch that profile's startup apps in one operation.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    fn workspace_open_profile(
        &self,
        Parameters(params): Parameters<WorkspaceOpenProfileParams>,
    ) -> Json<ProfileWorkspaceOpenResult> {
        Json(
            match params.into_options().and_then(|(options, profile_id)| {
                profile::open_profile_workspace(options, &profile_id, params.into_open_options())
            }) {
                Ok(open) => ProfileWorkspaceOpenResult {
                    ok: true,
                    message: "profile workspace opened".to_string(),
                    open: Some(open),
                },
                Err(error) => ProfileWorkspaceOpenResult {
                    ok: false,
                    message: error.to_string(),
                    open: None,
                },
            },
        )
    }

    #[tool(
        name = "workspace_status",
        description = "Return status for an isolated agent workspace, including the applied profile policy snapshot and enforcement report when a profile started the workspace.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_status(
        &self,
        Parameters(params): Parameters<WorkspaceIdParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(match workspace::status_workspace(&id) {
            Ok(status) => IpcResponse {
                ok: true,
                message: "workspace status returned".to_string(),
                status: Some(status),
                windows: None,
                active_window: None,
                screenshot: None,
                app_log: None,
                events: None,
            },
            Err(error) => error_response(error.to_string(), None),
        })
    }

    #[tool(
        name = "workspace_list",
        description = "List known isolated agent workspace runtime directories and whether each workspace is currently running.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_list(&self) -> Json<workspace::WorkspaceList> {
        Json(
            workspace::list_workspaces().unwrap_or_else(|error| workspace::WorkspaceList {
                runtime_base_dir: std::path::PathBuf::new(),
                workspaces: vec![workspace::WorkspaceListEntry {
                    id: "error".to_string(),
                    runtime_dir: std::path::PathBuf::new(),
                    socket_path: std::path::PathBuf::new(),
                    running: false,
                    status: None,
                    error: Some(error.to_string()),
                }],
            }),
        )
    }

    #[tool(
        name = "workspace_cleanup_stale",
        description = "Remove stale isolated workspace runtime directories. Running workspaces are skipped.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_cleanup_stale(
        &self,
        Parameters(params): Parameters<WorkspaceCleanupParams>,
    ) -> Json<workspace::WorkspaceCleanup> {
        Json(
            workspace::cleanup_stale_workspaces(params.id).unwrap_or_else(|error| {
                workspace::WorkspaceCleanup {
                    runtime_base_dir: std::path::PathBuf::new(),
                    removed: Vec::new(),
                    skipped: vec![workspace::WorkspaceCleanupEntry {
                        id: "error".to_string(),
                        runtime_dir: std::path::PathBuf::new(),
                        reason: error.to_string(),
                    }],
                }
            }),
        )
    }

    #[tool(
        name = "workspace_launch_app",
        description = "Launch an app inside an isolated agent workspace. The command runs with the workspace DISPLAY and XAUTHORITY. If a launch profile is provided, its cwd/env and mount/network policy apply to this app; set acknowledge_unenforced_policy=true if that launch profile requests policy that remains unenforced.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    fn workspace_launch_app(
        &self,
        Parameters(params): Parameters<WorkspaceLaunchParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .clone()
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(
            params
                .into_launch_spec()
                .and_then(|spec| workspace::launch_app_with_spec(&id, spec)),
        ))
    }

    #[tool(
        name = "workspace_run_app",
        description = "Launch an app inside an isolated agent workspace, wait for it to exit or time out, and return stdout/stderr logs in one response.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    fn workspace_run_app(
        &self,
        Parameters(params): Parameters<WorkspaceRunParams>,
    ) -> Json<WorkspaceRunResult> {
        let id = params
            .id
            .clone()
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        let timeout_ms = params.timeout_ms;
        let tail_bytes = params.tail_bytes;
        Json(
            match params
                .into_launch_spec()
                .and_then(|spec| workspace::run_app_with_spec(&id, spec, timeout_ms, tail_bytes))
            {
                Ok(run) => WorkspaceRunResult {
                    ok: true,
                    message: if run.succeeded {
                        "workspace app completed successfully".to_string()
                    } else if run.completed {
                        "workspace app completed with non-zero status".to_string()
                    } else {
                        "workspace app did not complete before timeout".to_string()
                    },
                    run: Some(run),
                },
                Err(error) => WorkspaceRunResult {
                    ok: false,
                    message: error.to_string(),
                    run: None,
                },
            },
        )
    }

    #[tool(
        name = "workspace_list_windows",
        description = "List visible windows inside an isolated agent workspace.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_list_windows(
        &self,
        Parameters(params): Parameters<WorkspaceIdParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::list_windows(&id)))
    }

    #[tool(
        name = "workspace_active_window",
        description = "Report the currently active visible window inside an isolated agent workspace.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_active_window(
        &self,
        Parameters(params): Parameters<WorkspaceIdParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::active_window(&id)))
    }

    #[tool(
        name = "workspace_observe",
        description = "Return status, visible windows, active window, and optionally a root screenshot for an isolated agent workspace.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_observe(
        &self,
        Parameters(params): Parameters<WorkspaceObserveParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::observe(
            &id,
            params.screenshot,
            params.output_path,
        )))
    }

    #[tool(
        name = "workspace_wait_window",
        description = "Wait for a visible window inside an isolated agent workspace, optionally filtered by title substring, pid, or launched app id/pid.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_wait_window(
        &self,
        Parameters(params): Parameters<WorkspaceWaitWindowParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::wait_window(
            &id,
            params.title_contains,
            params.pid,
            params.app_id,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_screenshot",
        description = "Capture a screenshot of the isolated agent workspace root display and return the PNG path.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_screenshot(
        &self,
        Parameters(params): Parameters<WorkspaceScreenshotParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::screenshot(
            &id,
            params.output_path,
        )))
    }

    #[tool(
        name = "workspace_screenshot_window",
        description = "Capture a screenshot of a specific visible window inside an isolated agent workspace, targeted by X11 window id or by title/pid/app filters.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_screenshot_window(
        &self,
        Parameters(params): Parameters<WorkspaceScreenshotWindowParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::screenshot_window(
            &id,
            params.window_id,
            params.title_contains,
            params.pid,
            params.app_id,
            params.output_path,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_focus_window",
        description = "Focus a visible window inside an isolated agent workspace by X11 window id.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_focus_window(
        &self,
        Parameters(params): Parameters<WorkspaceWindowTargetParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::focus_window(
            &id,
            params.window_id,
        )))
    }

    #[tool(
        name = "workspace_focus_matching_window",
        description = "Wait for and focus a visible window inside an isolated agent workspace, filtered by title substring, pid, or launched app id/pid.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_focus_matching_window(
        &self,
        Parameters(params): Parameters<WorkspaceWaitWindowParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::focus_matching_window(
            &id,
            params.title_contains,
            params.pid,
            params.app_id,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_close_window",
        description = "Request that a window inside an isolated agent workspace close by X11 window id.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_close_window(
        &self,
        Parameters(params): Parameters<WorkspaceWindowTargetParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::close_window(
            &id,
            params.window_id,
        )))
    }

    #[tool(
        name = "workspace_close_matching_window",
        description = "Wait for and request close of a visible window inside an isolated agent workspace, filtered by title substring, pid, or launched app id/pid.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_close_matching_window(
        &self,
        Parameters(params): Parameters<WorkspaceWaitWindowParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::close_matching_window(
            &id,
            params.title_contains,
            params.pid,
            params.app_id,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_click",
        description = "Click workspace-local coordinates inside an isolated agent workspace, optionally setting button and repeat count.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    fn workspace_click(
        &self,
        Parameters(params): Parameters<WorkspaceClickParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::click(
            &id,
            params.x,
            params.y,
            params.button,
            params.count,
        )))
    }

    #[tool(
        name = "workspace_click_window",
        description = "Click a coordinate relative to a visible window inside an isolated agent workspace, targeted by X11 window id or by title/pid/app filters, optionally setting button and repeat count.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_click_window(
        &self,
        Parameters(params): Parameters<WorkspaceClickWindowParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::click_window(
            &id,
            params.window_id,
            params.title_contains,
            params.pid,
            params.app_id,
            params.x,
            params.y,
            params.button,
            params.count,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_move_pointer",
        description = "Move the pointer to workspace-local coordinates inside an isolated agent workspace without clicking.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    fn workspace_move_pointer(
        &self,
        Parameters(params): Parameters<WorkspacePointerParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::move_pointer(
            &id, params.x, params.y,
        )))
    }

    #[tool(
        name = "workspace_move_pointer_window",
        description = "Move the pointer to coordinates relative to a visible window inside an isolated agent workspace, targeted by X11 window id or by title/pid/app filters, without clicking.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_move_pointer_window(
        &self,
        Parameters(params): Parameters<WorkspacePointerWindowParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::move_pointer_window(
            &id,
            params.window_id,
            params.title_contains,
            params.pid,
            params.app_id,
            params.x,
            params.y,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_drag",
        description = "Drag from one workspace-local coordinate to another inside an isolated agent workspace, optionally setting the mouse button.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    fn workspace_drag(
        &self,
        Parameters(params): Parameters<WorkspaceDragParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::drag(
            &id,
            params.from_x,
            params.from_y,
            params.to_x,
            params.to_y,
            params.button,
        )))
    }

    #[tool(
        name = "workspace_drag_window",
        description = "Drag between coordinates relative to a visible window inside an isolated agent workspace, targeted by X11 window id or by title/pid/app filters, optionally setting the mouse button.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_drag_window(
        &self,
        Parameters(params): Parameters<WorkspaceDragWindowParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::drag_window(
            &id,
            params.window_id,
            params.title_contains,
            params.pid,
            params.app_id,
            params.from_x,
            params.from_y,
            params.to_x,
            params.to_y,
            params.button,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_scroll",
        description = "Scroll at workspace-local coordinates inside an isolated agent workspace. Direction is up, down, left, or right; amount is wheel ticks.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    fn workspace_scroll(
        &self,
        Parameters(params): Parameters<WorkspaceScrollParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::scroll(
            &id,
            params.x,
            params.y,
            params.direction,
            params.amount,
        )))
    }

    #[tool(
        name = "workspace_scroll_window",
        description = "Scroll at coordinates relative to a visible window inside an isolated agent workspace, targeted by X11 window id or by title/pid/app filters. Direction is up, down, left, or right; amount is wheel ticks.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_scroll_window(
        &self,
        Parameters(params): Parameters<WorkspaceScrollWindowParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::scroll_window(
            &id,
            params.window_id,
            params.title_contains,
            params.pid,
            params.app_id,
            params.x,
            params.y,
            params.direction,
            params.amount,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_key",
        description = "Send a key chord to the isolated agent workspace with xdotool syntax, for example Return or ctrl+l.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    fn workspace_key(
        &self,
        Parameters(params): Parameters<WorkspaceKeyParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::key(&id, params.key)))
    }

    #[tool(
        name = "workspace_key_window",
        description = "Send a key chord to a specific visible window inside an isolated agent workspace, targeted by X11 window id or by title/pid/app filters.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_key_window(
        &self,
        Parameters(params): Parameters<WorkspaceKeyWindowParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::key_window(
            &id,
            params.window_id,
            params.title_contains,
            params.pid,
            params.app_id,
            params.key,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_type_text",
        description = "Type literal text into the focused app inside an isolated agent workspace.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    fn workspace_type_text(
        &self,
        Parameters(params): Parameters<WorkspaceTypeTextParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::type_text(&id, params.text)))
    }

    #[tool(
        name = "workspace_type_window",
        description = "Type literal text into a specific visible window inside an isolated agent workspace, targeted by X11 window id or by title/pid/app filters.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_type_window(
        &self,
        Parameters(params): Parameters<WorkspaceTypeWindowParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::type_window(
            &id,
            params.window_id,
            params.title_contains,
            params.pid,
            params.app_id,
            params.text,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_read_app_log",
        description = "Read stdout or stderr captured from an app launched inside an isolated agent workspace.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_read_app_log(
        &self,
        Parameters(params): Parameters<WorkspaceReadAppLogParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::read_app_log(
            &id,
            params.app_id,
            params.stream.unwrap_or_else(|| "stdout".to_string()),
            params.tail_bytes,
        )))
    }

    #[tool(
        name = "workspace_wait_app",
        description = "Wait until an app launched inside an isolated agent workspace exits, or until timeout_ms elapses. Defaults to 30000ms.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_wait_app(
        &self,
        Parameters(params): Parameters<WorkspaceWaitAppParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::wait_app(
            &id,
            params.app_id,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_events",
        description = "Read the workspace-local IPC event log. Sensitive typed text is recorded only as metadata such as character count.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_events(
        &self,
        Parameters(params): Parameters<WorkspaceEventsParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::read_events(&id, params.tail)))
    }

    #[tool(
        name = "workspace_run_profile_setup",
        description = "Launch setup commands declared by a saved profile inside an already running isolated workspace. Set wait=true or timeout_ms to supervise setup in sequence and report completion/success.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    fn workspace_run_profile_setup(
        &self,
        Parameters(params): Parameters<WorkspaceSetupParams>,
    ) -> Json<ProfileSetupResult> {
        Json(
            match profile::launch_profile_setup(
                &params
                    .id
                    .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string()),
                &params.profile,
                profile::ProfileSetupOptions {
                    wait: params.wait || params.timeout_ms.is_some(),
                    timeout_ms: params.timeout_ms,
                    acknowledge_unenforced_policy: params.acknowledge_unenforced_policy,
                },
            ) {
                Ok(run) => ProfileSetupResult {
                    ok: true,
                    message: if run.succeeded == Some(true) {
                        "profile setup completed successfully".to_string()
                    } else if run.completed == Some(true) {
                        "profile setup completed with failures".to_string()
                    } else if run.wait {
                        "profile setup did not complete before timeout".to_string()
                    } else {
                        "profile setup launched".to_string()
                    },
                    run: Some(run),
                },
                Err(error) => ProfileSetupResult {
                    ok: false,
                    message: error.to_string(),
                    run: None,
                },
            },
        )
    }

    #[tool(
        name = "workspace_launch_profile_apps",
        description = "Launch startup apps declared by a saved profile inside an already running isolated workspace.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    fn workspace_launch_profile_apps(
        &self,
        Parameters(params): Parameters<WorkspaceProfileLaunchParams>,
    ) -> Json<ProfileStartupResult> {
        Json(
            match profile::launch_profile_startup_apps(
                &params
                    .id
                    .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string()),
                &params.profile,
                profile::ProfileStartupOptions {
                    acknowledge_unenforced_policy: params.acknowledge_unenforced_policy,
                },
            ) {
                Ok(run) => ProfileStartupResult {
                    ok: true,
                    message: "profile startup apps launched".to_string(),
                    run: Some(run),
                },
                Err(error) => ProfileStartupResult {
                    ok: false,
                    message: error.to_string(),
                    run: None,
                },
            },
        )
    }

    #[tool(
        name = "workspace_kill_app",
        description = "Terminate an app launched inside an isolated agent workspace by app id or pid.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_kill_app(
        &self,
        Parameters(params): Parameters<WorkspaceKillAppParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::kill_app(&id, params.app_id)))
    }

    #[tool(
        name = "workspace_stop",
        description = "Stop an isolated agent workspace and terminate apps launched inside it.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_stop(
        &self,
        Parameters(params): Parameters<WorkspaceIdParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::stop_workspace(&id)))
    }
}

#[tool_handler(
    name = "agent-workspace-linux",
    version = "0.1.0",
    instructions = "Use workspace_doctor to check runtime readiness and optional policy backend candidates. Use profile_list/profile_get/profile_check/profile_template/profile_put/profile_delete to manage saved environment profiles. profile_template can generate starter JSON such as project-dev before saving with profile_put. profile_check preflights acknowledgement requirements and unenforced policy warnings before workspace_start. workspace_start requires acknowledge_hidden_workspace=true before creating a new hidden agent-controlled environment. If a profile requests policy that remains unenforced, workspace_start also requires acknowledge_unenforced_policy=true. Mount profiles and disabled-network profiles are enforced with bubblewrap when bubblewrap is available; network allowlists are still declared but not enforced by the X11 runtime. workspace_status reports the applied profile policy snapshot, discovered backend candidates from start time, and enforcement state. Use workspace_list to discover known/running workspaces and workspace_cleanup_stale to remove unreachable runtime directories. Use workspace_open_profile to start a profile-backed workspace, optionally run setup, and open startup apps in one call. Use workspace_start before launching apps manually. workspace_launch_profile_apps opens startup apps declared by the selected profile. workspace_run_app is the preferred one-shot helper for QA commands that should return stdout/stderr. workspace_launch_app, workspace_run_profile_setup, workspace_focus_window, workspace_focus_matching_window, workspace_close_window, workspace_close_matching_window, workspace_click, workspace_click_window, workspace_move_pointer, workspace_move_pointer_window, workspace_drag, workspace_drag_window, workspace_scroll, workspace_scroll_window, workspace_key, workspace_key_window, workspace_type_text, and workspace_type_window run only inside the isolated agent workspace; they do not target the user's host desktop. Use workspace_wait_window, workspace_active_window, workspace_observe, workspace_focus_matching_window, workspace_click_window, workspace_move_pointer_window, workspace_drag_window, workspace_scroll_window, workspace_key_window, or workspace_type_window after launching GUI apps. Prefer window-targeted tools when acting on a specific app window rather than the workspace root or current focus. Use workspace_run_profile_setup with wait=true when setup command completion matters. Use workspace_observe, workspace_screenshot, workspace_screenshot_window, workspace_list_windows, workspace_active_window, workspace_wait_app, workspace_read_app_log, and workspace_events to inspect the workspace before acting. workspace_screenshot_window captures a specific app window by id/title/pid/app filters. workspace_events records IPC activity without storing raw typed text. workspace_close_window, workspace_close_matching_window, and workspace_kill_app terminate only workspace-local windows/apps. workspace_stop terminates the workspace and apps launched inside it."
)]
impl ServerHandler for AgentWorkspaceLinux {}

pub async fn serve_mcp() -> Result<()> {
    AgentWorkspaceLinux
        .serve(rmcp::transport::stdio())
        .await?
        .waiting()
        .await?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct ProfileIdParams {
    id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct ProfilePutParams {
    profile: WorkspaceProfile,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct ProfileTemplateParams {
    kind: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    host_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
struct ProfileGetResult {
    ok: bool,
    message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profile: Option<WorkspaceProfile>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
struct ProfileCheckResult {
    ok: bool,
    message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    check: Option<profile::ProfileCheck>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
struct ProfileSetupResult {
    ok: bool,
    message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    run: Option<profile::ProfileSetupRun>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
struct ProfileStartupResult {
    ok: bool,
    message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    run: Option<profile::ProfileStartupRun>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
struct ProfileWorkspaceOpenResult {
    ok: bool,
    message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    open: Option<profile::ProfileWorkspaceOpen>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
struct WorkspaceRunResult {
    ok: bool,
    message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    run: Option<workspace::WorkspaceRun>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceStartParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    acknowledge_hidden_workspace: bool,
    #[serde(default)]
    acknowledge_unenforced_policy: bool,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
}

impl WorkspaceStartParams {
    fn into_options(self) -> Result<WorkspaceStartOptions> {
        let width_explicit = self.width.is_some();
        let height_explicit = self.height.is_some();
        let mut options = WorkspaceStartOptions::default();
        if let Some(profile_id) = self.profile {
            profile::apply_profile_to_start_options(
                &profile_id,
                &mut options,
                width_explicit,
                height_explicit,
            )?;
        }
        if let Some(id) = self.id {
            options.id = id;
        }
        if let Some(width) = self.width {
            options.width = width;
        }
        if let Some(height) = self.height {
            options.height = height;
        }
        options.user_acknowledged_hidden_workspace = self.acknowledge_hidden_workspace;
        options.user_acknowledged_unenforced_policy = self.acknowledge_unenforced_policy;
        Ok(options)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspaceOpenProfileParams {
    #[serde(default)]
    id: Option<String>,
    profile: String,
    #[serde(default)]
    acknowledge_hidden_workspace: bool,
    #[serde(default)]
    acknowledge_unenforced_policy: bool,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    run_setup: bool,
    #[serde(default)]
    setup_timeout_ms: Option<u64>,
}

impl WorkspaceOpenProfileParams {
    fn into_options(&self) -> Result<(WorkspaceStartOptions, String)> {
        let width_explicit = self.width.is_some();
        let height_explicit = self.height.is_some();
        let profile_id = self.profile.clone();
        let mut options = WorkspaceStartOptions::default();
        profile::apply_profile_to_start_options(
            &profile_id,
            &mut options,
            width_explicit,
            height_explicit,
        )?;
        if let Some(id) = self.id.clone() {
            options.id = id;
        }
        if let Some(width) = self.width {
            options.width = width;
        }
        if let Some(height) = self.height {
            options.height = height;
        }
        options.user_acknowledged_hidden_workspace = self.acknowledge_hidden_workspace;
        options.user_acknowledged_unenforced_policy = self.acknowledge_unenforced_policy;
        Ok((options, profile_id))
    }

    fn into_open_options(&self) -> profile::ProfileWorkspaceOpenOptions {
        profile::ProfileWorkspaceOpenOptions {
            run_setup: self.run_setup || self.setup_timeout_ms.is_some(),
            setup: profile::ProfileSetupOptions {
                wait: self.setup_timeout_ms.is_some(),
                timeout_ms: self.setup_timeout_ms,
                acknowledge_unenforced_policy: self.acknowledge_unenforced_policy,
            },
            startup: profile::ProfileStartupOptions {
                acknowledge_unenforced_policy: self.acknowledge_unenforced_policy,
            },
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceIdParams {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceCleanupParams {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspaceLaunchParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    acknowledge_unenforced_policy: bool,
    command: Vec<String>,
    #[serde(default)]
    cwd: Option<PathBuf>,
    #[serde(default)]
    env: Vec<EnvVar>,
}

impl WorkspaceLaunchParams {
    fn into_launch_spec(self) -> Result<LaunchSpec> {
        let cwd_explicit = self.cwd.is_some();
        let mut spec = LaunchSpec {
            command: self.command,
            profile_id: None,
            applied_policy: None,
            user_acknowledged_unenforced_policy: self.acknowledge_unenforced_policy,
            cwd: self.cwd,
            env: self.env,
        };
        if let Some(profile_id) = self.profile {
            profile::apply_profile_to_launch_spec(&profile_id, &mut spec, cwd_explicit)?;
        }
        Ok(spec)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspaceRunParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    acknowledge_unenforced_policy: bool,
    command: Vec<String>,
    #[serde(default)]
    cwd: Option<PathBuf>,
    #[serde(default)]
    env: Vec<EnvVar>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    tail_bytes: Option<u64>,
}

impl WorkspaceRunParams {
    fn into_launch_spec(self) -> Result<LaunchSpec> {
        let cwd_explicit = self.cwd.is_some();
        let mut spec = LaunchSpec {
            command: self.command,
            profile_id: None,
            applied_policy: None,
            user_acknowledged_unenforced_policy: self.acknowledge_unenforced_policy,
            cwd: self.cwd,
            env: self.env,
        };
        if let Some(profile_id) = self.profile {
            profile::apply_profile_to_launch_spec(&profile_id, &mut spec, cwd_explicit)?;
        }
        Ok(spec)
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceScreenshotParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    output_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceObserveParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    screenshot: bool,
    #[serde(default)]
    output_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceScreenshotWindowParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    window_id: Option<String>,
    #[serde(default)]
    title_contains: Option<String>,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    app_id: Option<String>,
    #[serde(default)]
    output_path: Option<PathBuf>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspaceWindowTargetParams {
    #[serde(default)]
    id: Option<String>,
    window_id: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceWaitWindowParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title_contains: Option<String>,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    app_id: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspaceClickParams {
    #[serde(default)]
    id: Option<String>,
    x: i32,
    y: i32,
    #[serde(default)]
    button: Option<u8>,
    #[serde(default)]
    count: Option<u8>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceClickWindowParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    window_id: Option<String>,
    #[serde(default)]
    title_contains: Option<String>,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    app_id: Option<String>,
    x: i32,
    y: i32,
    #[serde(default)]
    button: Option<u8>,
    #[serde(default)]
    count: Option<u8>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspacePointerParams {
    #[serde(default)]
    id: Option<String>,
    x: i32,
    y: i32,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspacePointerWindowParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    window_id: Option<String>,
    #[serde(default)]
    title_contains: Option<String>,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    app_id: Option<String>,
    x: i32,
    y: i32,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspaceDragParams {
    #[serde(default)]
    id: Option<String>,
    from_x: i32,
    from_y: i32,
    to_x: i32,
    to_y: i32,
    #[serde(default)]
    button: Option<u8>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceDragWindowParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    window_id: Option<String>,
    #[serde(default)]
    title_contains: Option<String>,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    app_id: Option<String>,
    from_x: i32,
    from_y: i32,
    to_x: i32,
    to_y: i32,
    #[serde(default)]
    button: Option<u8>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspaceScrollParams {
    #[serde(default)]
    id: Option<String>,
    x: i32,
    y: i32,
    direction: ScrollDirection,
    #[serde(default)]
    amount: Option<u8>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspaceScrollWindowParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    window_id: Option<String>,
    #[serde(default)]
    title_contains: Option<String>,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    app_id: Option<String>,
    x: i32,
    y: i32,
    direction: ScrollDirection,
    #[serde(default)]
    amount: Option<u8>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspaceKeyParams {
    #[serde(default)]
    id: Option<String>,
    key: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceKeyWindowParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    window_id: Option<String>,
    #[serde(default)]
    title_contains: Option<String>,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    app_id: Option<String>,
    key: String,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspaceTypeTextParams {
    #[serde(default)]
    id: Option<String>,
    text: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceTypeWindowParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    window_id: Option<String>,
    #[serde(default)]
    title_contains: Option<String>,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    app_id: Option<String>,
    text: String,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspaceReadAppLogParams {
    #[serde(default)]
    id: Option<String>,
    app_id: String,
    #[serde(default)]
    stream: Option<String>,
    #[serde(default)]
    tail_bytes: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspaceWaitAppParams {
    #[serde(default)]
    id: Option<String>,
    app_id: String,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceEventsParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    tail: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspaceSetupParams {
    #[serde(default)]
    id: Option<String>,
    profile: String,
    #[serde(default)]
    wait: bool,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    acknowledge_unenforced_policy: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspaceProfileLaunchParams {
    #[serde(default)]
    id: Option<String>,
    profile: String,
    #[serde(default)]
    acknowledge_unenforced_policy: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspaceKillAppParams {
    #[serde(default)]
    id: Option<String>,
    app_id: String,
}

fn result_response(result: Result<IpcResponse>) -> IpcResponse {
    match result {
        Ok(response) => response,
        Err(error) => error_response(error.to_string(), None),
    }
}

fn error_response(message: String, status: Option<WorkspaceStatus>) -> IpcResponse {
    IpcResponse {
        ok: false,
        message,
        status,
        windows: None,
        active_window: None,
        screenshot: None,
        app_log: None,
        events: None,
    }
}
