use crate::guardrails;
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
        name = "workspace_guardrails",
        description = "Return the machine-readable guardrail summary for isolated workspace actions, including acknowledgement, policy mode, dry-run, explicit override, and timeout-termination requirements.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_guardrails(&self) -> Json<guardrails::GuardrailSummary> {
        Json(guardrails::guardrail_summary())
    }

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
                    require_enforced_policy: false,
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
        description = "Preflight a saved profile against the current machine. Returns the applied policy, per-capability state/backend/limitations, warnings, and acknowledgement requirements before starting a workspace.",
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
        description = "Create an agent workspace profile. Existing profile ids are rejected unless replace=true is set explicitly. Set dry_run=true to preview whether the profile would be created, replaced, or rejected without writing. Mounts, network, require_enforced_policy, and setup commands are persisted as declared intent and surfaced in workspace status; display size, cwd, and env are currently applied by the X11 runtime.",
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
    ) -> Json<profile::ProfilePutResult> {
        let requested_profile = params.profile.clone();
        Json(
            profile::put_profile(params.profile, params.replace, params.dry_run).unwrap_or_else(
                |error| {
                    profile::ProfilePutResult::error(
                        requested_profile,
                        params.replace,
                        params.dry_run,
                        error.to_string(),
                    )
                },
            ),
        )
    }

    #[tool(
        name = "profile_export",
        description = "Return one saved profile by id and optionally write it as pretty JSON to output_path. Existing output files are rejected unless replace=true is set explicitly.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn profile_export(
        &self,
        Parameters(params): Parameters<ProfileExportParams>,
    ) -> Json<ProfileExportResponse> {
        Json(
            match profile::export_profile(&params.id, params.output_path, params.replace) {
                Ok(export) => ProfileExportResponse {
                    ok: true,
                    message: if export.wrote {
                        "profile exported".to_string()
                    } else {
                        "profile returned".to_string()
                    },
                    export: Some(export),
                },
                Err(error) => ProfileExportResponse {
                    ok: false,
                    message: error.to_string(),
                    export: None,
                },
            },
        )
    }

    #[tool(
        name = "profile_delete",
        description = "Delete one saved agent workspace profile by id. Set dry_run=true to return the profile that would be deleted without removing it.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn profile_delete(
        &self,
        Parameters(params): Parameters<ProfileDeleteParams>,
    ) -> Json<profile::ProfileDeleteResult> {
        Json(
            profile::delete_profile(&params.id, params.dry_run).unwrap_or(
                profile::ProfileDeleteResult {
                    id: params.id,
                    deleted: false,
                    would_delete: false,
                    dry_run: params.dry_run,
                    profile: None,
                },
            ),
        )
    }

    #[tool(
        name = "workspace_start",
        description = "Start an isolated X11 agent workspace with its own display and control IPC socket. Set acknowledge_hidden_workspace=true to confirm the user knows this creates a separate agent-controlled environment. Optional purpose records a human-readable reason in status and the start event. If the selected profile requests currently unenforced mount or network restrictions, also set acknowledge_unenforced_policy=true. Mount profiles and disabled-network profiles are enforced with bubblewrap when available; local_only and allowlist network profiles are declared intent until dedicated backends exist. Profiles with require_enforced_policy=true reject unenforced policy instead of accepting acknowledgement.",
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
        description = "Start a profile-backed isolated workspace, optionally record a human-readable purpose, optionally wait for profile setup, optionally kill timed-out setup commands, and launch that profile's startup apps in one operation after setup succeeds. Set startup_wait_window=true to wait for each startup app's first visible window, or startup_screenshot_window=true to also capture each first startup window.",
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
                apps: Some(status.apps.clone()),
                status: Some(status),
                ipc: None,
                environment: None,
                windows: None,
                active_window: None,
                pointer: None,
                screenshot: None,
                app_log: None,
                clipboard: None,
                events: None,
            },
            Err(error) => error_response(error.to_string(), None),
        })
    }

    #[tool(
        name = "workspace_manifest",
        description = "Read the saved workspace manifest from disk for live or stopped workspace inspection without contacting the workspace daemon. This is read-only saved state, not live status.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_manifest(
        &self,
        Parameters(params): Parameters<WorkspaceIdParams>,
    ) -> Json<workspace::WorkspaceManifestRead> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(workspace::read_manifest(&id))
    }

    #[tool(
        name = "workspace_artifacts",
        description = "Return a read-only inventory of files left in a workspace runtime directory, including manifest, event log, daemon logs, app logs, and screenshots when present. Set existing_only=true to return only paths that currently exist.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_artifacts(
        &self,
        Parameters(params): Parameters<WorkspaceArtifactsParams>,
    ) -> Json<workspace::WorkspaceArtifacts> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(workspace::artifacts(&id, params.existing_only))
    }

    #[tool(
        name = "workspace_ipc_info",
        description = "Return daemon IPC protocol metadata for an isolated agent workspace, including protocol version, transport, framing, encoding, and socket path.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_ipc_info(
        &self,
        Parameters(params): Parameters<WorkspaceIdParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::ipc_info(&id)))
    }

    #[tool(
        name = "workspace_env",
        description = "Return the workspace-local environment needed to attach external tools to an isolated agent workspace, including DISPLAY, XAUTHORITY, runtime directory, and control socket path.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_env(
        &self,
        Parameters(params): Parameters<WorkspaceIdParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::environment(&id)))
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
                    manifest: None,
                    manifest_error: None,
                    status: None,
                    error: Some(error.to_string()),
                }],
            }),
        )
    }

    #[tool(
        name = "workspace_cleanup_stale",
        description = "Remove stale isolated workspace runtime directories. Running workspaces are skipped. Set dry_run=true to return removable candidates without deleting them.",
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
            workspace::cleanup_stale_workspaces(params.id, params.dry_run).unwrap_or_else(
                |error| workspace::WorkspaceCleanup {
                    runtime_base_dir: std::path::PathBuf::new(),
                    dry_run: params.dry_run,
                    candidates: Vec::new(),
                    removed: Vec::new(),
                    skipped: vec![workspace::WorkspaceCleanupEntry {
                        id: "error".to_string(),
                        runtime_dir: std::path::PathBuf::new(),
                        reason: error.to_string(),
                    }],
                },
            ),
        )
    }

    #[tool(
        name = "workspace_launch_app",
        description = "Launch an optionally named app inside an isolated agent workspace. The command runs with the workspace attachment environment, including DISPLAY, XAUTHORITY, AGENT_WORKSPACE_ID, AGENT_WORKSPACE_RUNTIME_DIR, and AGENT_WORKSPACE_SOCKET. Set wait_window=true to wait for the launched app's first visible window and return it in the same response. Set screenshot_window=true to also capture the first visible launched-app window; this implies waiting for a window. If a launch profile is provided, its cwd/env and mount/network policy apply to this app; set acknowledge_unenforced_policy=true if that launch profile requests policy that remains unenforced.",
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
        let wait_window = params.wait_window;
        let window_timeout_ms = params.window_timeout_ms;
        let screenshot_window = params.screenshot_window;
        Json(result_response(params.into_launch_spec().and_then(
            |spec| {
                workspace::launch_app_with_options(
                    &id,
                    spec,
                    wait_window,
                    window_timeout_ms,
                    screenshot_window,
                )
            },
        )))
    }

    #[tool(
        name = "workspace_run_app",
        description = "Launch an optionally named app inside an isolated agent workspace, wait for it to exit or time out, optionally kill it on timeout, and return stdout/stderr logs in one response. The command uses the same workspace attachment environment, optional cwd/env overrides, and optional launch profile policy as workspace_launch_app.",
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
        let kill_on_timeout = params.kill_on_timeout;
        Json(
            match params.into_launch_spec().and_then(|spec| {
                workspace::run_app_with_spec(&id, spec, timeout_ms, tail_bytes, kill_on_timeout)
            }) {
                Ok(run) => WorkspaceRunResult {
                    ok: true,
                    message: if run.succeeded {
                        "workspace app completed successfully".to_string()
                    } else if run.completed {
                        "workspace app completed with non-zero status".to_string()
                    } else if run.killed_on_timeout {
                        "workspace app timed out and was killed".to_string()
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
        name = "workspace_list_apps",
        description = "List apps launched inside an isolated agent workspace, optionally filtering by app id/pid/name, app name substring, command substring, profile id, or running/stopped state. Falls back to the saved app snapshot when the workspace daemon has stopped.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_list_apps(
        &self,
        Parameters(params): Parameters<WorkspaceListAppsParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::list_apps(
            &id,
            params.app_id,
            params.name_contains,
            params.command_contains,
            params.profile_id,
            params.running,
        )))
    }

    #[tool(
        name = "workspace_list_windows",
        description = "List windows inside an isolated agent workspace, optionally filtering by title/class/pid/app. By default this returns visible windows; set include_hidden=true to include minimized/hidden windows with visibility metadata.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_list_windows(
        &self,
        Parameters(params): Parameters<WorkspaceListWindowsParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::list_windows(
            &id,
            params.include_hidden,
            params.title_contains,
            params.class_contains,
            params.pid,
            params.app_id,
        )))
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
        name = "workspace_pointer",
        description = "Report the current pointer coordinates inside an isolated agent workspace.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_pointer(
        &self,
        Parameters(params): Parameters<WorkspaceIdParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::pointer(&id)))
    }

    #[tool(
        name = "workspace_observe",
        description = "Return status, windows, active window, pointer coordinates, optional root screenshot, and optional recent/incremental events for an isolated agent workspace. By default windows are visible-only; set include_hidden=true to include minimized/hidden windows.",
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
            params.include_hidden,
            params.output_path,
            params.events,
            params.events_tail,
            params.events_since_sequence,
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
            params.class_contains,
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
        description = "Capture a screenshot of a specific visible window inside an isolated agent workspace, targeted by X11 window id or by title/class/pid/app filters.",
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
            params.class_contains,
            params.pid,
            params.app_id,
            params.output_path,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_focus_window",
        description = "Focus a visible window inside an isolated agent workspace by X11 window id. Response includes the focused window and active_window when focus can be resolved after the action.",
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
        description = "Wait for and focus a visible window inside an isolated agent workspace, filtered by title substring, class substring, pid, or launched app id/pid. Response includes the matched window and active_window when focus can be resolved after the action.",
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
            params.class_contains,
            params.pid,
            params.app_id,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_close_window",
        description = "Request that a window inside an isolated agent workspace close by X11 window id. Response includes the window record that was targeted for close. Set dry_run=true to resolve and return the targeted window without closing it.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_close_window(
        &self,
        Parameters(params): Parameters<WorkspaceCloseWindowParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::close_window(
            &id,
            params.window_id,
            params.dry_run,
        )))
    }

    #[tool(
        name = "workspace_close_matching_window",
        description = "Wait for and request close of a visible window inside an isolated agent workspace, filtered by title substring, class substring, pid, or launched app id/pid. Set dry_run=true to resolve and return the matched window without closing it.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_close_matching_window(
        &self,
        Parameters(params): Parameters<WorkspaceCloseMatchingWindowParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::close_matching_window(
            &id,
            params.title_contains,
            params.class_contains,
            params.pid,
            params.app_id,
            params.timeout_ms,
            params.dry_run,
        )))
    }

    #[tool(
        name = "workspace_move_window",
        description = "Move a visible window inside an isolated agent workspace by X11 id or by title/class/pid/app filters. Coordinates are workspace-local top-left pixels.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_move_window(
        &self,
        Parameters(params): Parameters<WorkspaceMoveWindowParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::move_window(
            &id,
            params.window_id,
            params.title_contains,
            params.class_contains,
            params.pid,
            params.app_id,
            params.x,
            params.y,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_resize_window",
        description = "Resize a visible window inside an isolated agent workspace by X11 id or by title/class/pid/app filters. Size is in pixels.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_resize_window(
        &self,
        Parameters(params): Parameters<WorkspaceResizeWindowParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::resize_window(
            &id,
            params.window_id,
            params.title_contains,
            params.class_contains,
            params.pid,
            params.app_id,
            params.width,
            params.height,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_raise_window",
        description = "Raise a visible window inside an isolated agent workspace above other windows, targeted by X11 id or title/class/pid/app filters.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_raise_window(
        &self,
        Parameters(params): Parameters<WorkspaceTargetedWindowParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::raise_window(
            &id,
            params.window_id,
            params.title_contains,
            params.class_contains,
            params.pid,
            params.app_id,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_minimize_window",
        description = "Minimize a visible window inside an isolated agent workspace, targeted by X11 id or title/class/pid/app filters. Response includes the refreshed window record after minimization.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_minimize_window(
        &self,
        Parameters(params): Parameters<WorkspaceTargetedWindowParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::minimize_window(
            &id,
            params.window_id,
            params.title_contains,
            params.class_contains,
            params.pid,
            params.app_id,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_show_window",
        description = "Show/map a minimized workspace window by X11 window id or title/class/pid/app filters. Match filters include hidden windows so minimized apps can be restored directly.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_show_window(
        &self,
        Parameters(params): Parameters<WorkspaceTargetedWindowParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::show_window(
            &id,
            params.window_id,
            params.title_contains,
            params.class_contains,
            params.pid,
            params.app_id,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_click",
        description = "Click workspace-local coordinates inside an isolated agent workspace, optionally setting button and repeat count. Response includes pointer and active_window when focus can be resolved after the action.",
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
        description = "Click a coordinate relative to a visible window inside an isolated agent workspace, targeted by X11 window id or by title/class/pid/app filters, optionally setting button and repeat count. Response includes pointer, target window, and active_window when focus can be resolved after the action.",
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
            params.class_contains,
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
        description = "Move the pointer to workspace-local coordinates inside an isolated agent workspace without clicking. Response includes pointer and active_window when focus can be resolved after the action.",
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
        description = "Move the pointer to coordinates relative to a visible window inside an isolated agent workspace, targeted by X11 window id or by title/class/pid/app filters, without clicking. Response includes pointer, target window, and active_window when focus can be resolved after the action.",
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
            params.class_contains,
            params.pid,
            params.app_id,
            params.x,
            params.y,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_drag",
        description = "Drag from one workspace-local coordinate to another inside an isolated agent workspace, optionally setting the mouse button. Response includes pointer and active_window when focus can be resolved after the action.",
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
        description = "Drag between coordinates relative to a visible window inside an isolated agent workspace, targeted by X11 window id or by title/class/pid/app filters, optionally setting the mouse button. Response includes pointer, target window, and active_window when focus can be resolved after the action.",
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
            params.class_contains,
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
        description = "Scroll at workspace-local coordinates inside an isolated agent workspace. Direction is up, down, left, or right; amount is wheel ticks. Response includes pointer and active_window when focus can be resolved after the action.",
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
        description = "Scroll at coordinates relative to a visible window inside an isolated agent workspace, targeted by X11 window id or by title/class/pid/app filters. Direction is up, down, left, or right; amount is wheel ticks. Response includes pointer, target window, and active_window when focus can be resolved after the action.",
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
            params.class_contains,
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
        description = "Send a key chord to the isolated agent workspace with xdotool syntax, for example Return or ctrl+l. Response includes active_window when focus can be resolved after the action.",
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
        description = "Send a key chord to a specific visible window inside an isolated agent workspace, targeted by X11 window id or by title/class/pid/app filters. Response includes the target window and active_window when focus can be resolved after the action.",
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
            params.class_contains,
            params.pid,
            params.app_id,
            params.key,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_type_text",
        description = "Type literal text into the focused app inside an isolated agent workspace. Response includes active_window when focus can be resolved after the action.",
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
        description = "Type literal text into a specific visible window inside an isolated agent workspace, targeted by X11 window id or by title/class/pid/app filters. Response includes the target window and active_window when focus can be resolved after the action.",
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
            params.class_contains,
            params.pid,
            params.app_id,
            params.text,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_set_clipboard",
        description = "Set the clipboard selection inside an isolated agent workspace. Event logs store only size metadata, not the raw clipboard text.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_set_clipboard(
        &self,
        Parameters(params): Parameters<WorkspaceClipboardSetParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::set_clipboard(&id, params.text)))
    }

    #[tool(
        name = "workspace_get_clipboard",
        description = "Read the clipboard selection inside an isolated agent workspace.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_get_clipboard(
        &self,
        Parameters(params): Parameters<WorkspaceIdParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::get_clipboard(&id)))
    }

    #[tool(
        name = "workspace_paste_text",
        description = "Set the isolated workspace clipboard to text, then send a paste key chord to the focused app. Defaults to ctrl+v. Response includes active_window when focus can be resolved after the action. Event logs store only size metadata, not the raw text.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_paste_text(
        &self,
        Parameters(params): Parameters<WorkspacePasteTextParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::paste_text(
            &id,
            params.text,
            params.key,
        )))
    }

    #[tool(
        name = "workspace_paste_window",
        description = "Set the isolated workspace clipboard to text, focus a visible window by X11 id/title/class/pid/app filter, then send a paste key chord. Defaults to ctrl+v. Response includes the target window and active_window when focus can be resolved after the action. Event logs store only size metadata, not the raw text.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn workspace_paste_window(
        &self,
        Parameters(params): Parameters<WorkspacePasteWindowParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::paste_window(
            &id,
            params.window_id,
            params.title_contains,
            params.class_contains,
            params.pid,
            params.app_id,
            params.text,
            params.key,
            params.timeout_ms,
        )))
    }

    #[tool(
        name = "workspace_read_app_log",
        description = "Read stdout or stderr captured from an app launched inside an isolated agent workspace, falling back to saved log paths when the workspace daemon has stopped.",
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
        description = "Wait until an app launched inside an isolated agent workspace exits, or until timeout_ms elapses. Defaults to 30000ms. Set kill_on_timeout=true to terminate the app process group when the timeout elapses.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
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
            params.kill_on_timeout,
        )))
    }

    #[tool(
        name = "workspace_events",
        description = "Read the workspace-local IPC event log, falling back to saved event history when the workspace daemon has stopped. Use since_sequence to poll events after a previously seen sequence number. Sensitive typed text is recorded only as metadata such as character count.",
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
        Json(result_response(workspace::read_events(
            &id,
            params.tail,
            params.since_sequence,
        )))
    }

    #[tool(
        name = "workspace_run_profile_setup",
        description = "Launch setup commands declared by a saved profile inside an already running isolated workspace. Set wait=true or timeout_ms to supervise setup in sequence and report completion/success; set kill_on_timeout=true to terminate timed-out setup commands.",
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
                    kill_on_timeout: params.kill_on_timeout,
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
        description = "Launch startup apps declared by a saved profile inside an already running isolated workspace. Set wait_window=true to wait for each startup app's first visible window, or screenshot_window=true to also capture each first startup window.",
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
                    wait_window: params.wait_window,
                    window_timeout_ms: params.window_timeout_ms,
                    screenshot_window: params.screenshot_window,
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
        description = "Terminate an app launched inside an isolated agent workspace by app id or pid. Set dry_run=true to resolve and return the matched app without terminating it.",
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
        Json(result_response(workspace::kill_app(
            &id,
            params.app_id,
            params.dry_run,
        )))
    }

    #[tool(
        name = "workspace_stop",
        description = "Stop an isolated agent workspace, terminate apps launched inside it, return the apps stopped by the shutdown, and wait for the daemon IPC socket to close. Defaults to a 30000ms wait; set timeout_ms to override. Set dry_run=true to return currently running apps without stopping the workspace.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn workspace_stop(
        &self,
        Parameters(params): Parameters<WorkspaceStopParams>,
    ) -> Json<IpcResponse> {
        let id = params
            .id
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::stop_workspace(
            &id,
            params.timeout_ms,
            params.dry_run,
        )))
    }
}

#[tool_handler(
    name = "agent-workspace-linux",
    version = "0.1.0",
    instructions = "Use workspace_guardrails to inspect acknowledgement, dry-run, explicit override, timeout-termination, and workspace-scope rules for UI approval flows. Use workspace_doctor to check runtime readiness and optional policy backend candidates. Use profile_list/profile_get/profile_check/profile_template/profile_put/profile_export/profile_delete to manage saved environment profiles. Use profile_export to return a saved profile and optionally write it to output_path; set replace=true only when intentionally overwriting an existing export file. Use profile_put with dry_run=true to preview whether a profile would be created, replaced, or rejected without writing. Use profile_delete with dry_run=true to return the saved profile without deleting it. profile_template can generate starter JSON such as project-dev before saving with profile_put. profile_check preflights acknowledgement requirements and unenforced policy warnings before workspace_start. workspace_start requires acknowledge_hidden_workspace=true before creating a new hidden agent-controlled environment. Pass purpose when a human-readable reason should be shown in workspace_status and the start event. If a profile requests policy that remains unenforced, workspace_start also requires acknowledge_unenforced_policy=true. Mount profiles and disabled-network profiles are enforced with bubblewrap when bubblewrap is available; network allowlists are still declared but not enforced by the X11 runtime. workspace_status reports live daemon state, including the applied profile policy snapshot, discovered backend candidates from start time, and enforcement state. Use workspace_manifest to inspect saved manifest state from disk for live or stopped workspaces without contacting the daemon. Use workspace_artifacts to inventory saved runtime files such as manifest, event log, daemon logs, app logs, and screenshots; set existing_only=true when only present paths are needed. Use workspace_ipc_info to verify daemon IPC protocol metadata, transport, framing, encoding, and socket path. Use workspace_env to get DISPLAY, XAUTHORITY, runtime directory, and control socket values for external tools that need to attach to the hidden workspace. Use workspace_list to discover known/running workspaces and workspace_cleanup_stale with dry_run=true to preview unreachable runtime directories before deletion. Use workspace_list_apps to inspect launched apps, including named apps and running/stopped state; it can read the saved app snapshot after a workspace has stopped. App action and log-read responses include the directly affected app in the top-level apps field when available. Use workspace_open_profile to start a profile-backed workspace, optionally wait for setup, and open startup apps after setup succeeds in one call. Use workspace_start before launching apps manually. workspace_launch_profile_apps opens startup apps declared by the selected profile. workspace_run_app is the preferred one-shot helper for QA commands that should return stdout/stderr; set kill_on_timeout=true to terminate timed-out commands. workspace_wait_app also accepts kill_on_timeout=true to terminate an already launched app when its wait timeout elapses. workspace_launch_app and workspace_run_app accept optional names, workspace_list_apps can filter by app name or command substring, and named apps can be referenced anywhere an app target is accepted, including logs, waits, kill dry-runs, kills, and window app_id filters. workspace_launch_app, workspace_run_profile_setup, workspace_focus_window, workspace_focus_matching_window, workspace_close_window, workspace_close_matching_window, workspace_move_window, workspace_resize_window, workspace_raise_window, workspace_minimize_window, workspace_show_window, workspace_click, workspace_click_window, workspace_move_pointer, workspace_move_pointer_window, workspace_drag, workspace_drag_window, workspace_scroll, workspace_scroll_window, workspace_key, workspace_key_window, workspace_type_text, workspace_type_window, workspace_set_clipboard, workspace_get_clipboard, workspace_paste_text, and workspace_paste_window run only inside the isolated agent workspace; they do not target the user's host desktop. Use workspace_wait_window, workspace_active_window, workspace_pointer, workspace_observe, workspace_focus_matching_window, workspace_move_window, workspace_resize_window, workspace_raise_window, workspace_minimize_window, workspace_show_window, workspace_click_window, workspace_move_pointer_window, workspace_drag_window, workspace_scroll_window, workspace_key_window, workspace_type_window, or workspace_paste_window after launching GUI apps. Prefer window-targeted tools when acting on a specific app window rather than the workspace root or current focus. Window match filters accept title_contains, class_contains, pid, or app_id; class_contains matches wm_class and wm_instance. Use workspace_move_window, workspace_resize_window, workspace_raise_window, workspace_minimize_window, and workspace_show_window to arrange app windows before screenshots or repeated QA interactions. workspace_show_window match filters include hidden windows, so it can restore minimized apps by title/class/pid/app without first listing their raw X11 ids. Use workspace_list_windows with title_contains, class_contains, pid, or app_id filters to inspect specific current app windows. Use workspace_list_windows or workspace_observe with include_hidden=true when a minimized or hidden app needs to be found again; returned windows include wm_class, wm_instance, and app_id when X11/process metadata is available. Use workspace_paste_text or workspace_paste_window when inserting long text is more reliable than synthetic typing. Use workspace_run_profile_setup with wait=true when setup command completion matters; set kill_on_timeout=true to clean up timed-out setup commands. Use workspace_status or workspace_observe when a full live app snapshot is useful. Use workspace_observe, workspace_screenshot, workspace_screenshot_window, workspace_list_apps, workspace_list_windows, workspace_active_window, workspace_pointer, workspace_wait_app, workspace_read_app_log, workspace_get_clipboard, and workspace_events to inspect the workspace before acting. For incremental event polling, pass workspace_events since_sequence with the last seen event sequence. workspace_screenshot_window captures a specific app window by id/title/class/pid/app filters. workspace_read_app_log can read saved stdout/stderr after a workspace has stopped when its manifest remains on disk. workspace_events records IPC activity without storing raw typed text, raw clipboard-set text, or raw pasted text, and can read saved event history after a workspace has stopped. workspace_close_window, workspace_close_matching_window, workspace_kill_app, workspace_minimize_window, and workspace_show_window affect only workspace-local windows/apps. workspace_close_window and workspace_close_matching_window with dry_run=true resolve the targeted window without closing it. workspace_kill_app with dry_run=true resolves the matched app without terminating it. workspace_stop with dry_run=true previews currently running apps without stopping; without dry_run it terminates the workspace and apps launched inside it, then waits for the daemon IPC socket to close."
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
struct ProfileDeleteParams {
    id: String,
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct ProfilePutParams {
    profile: WorkspaceProfile,
    #[serde(default)]
    replace: bool,
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct ProfileExportParams {
    id: String,
    #[serde(default)]
    output_path: Option<PathBuf>,
    #[serde(default)]
    replace: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
struct ProfileExportResponse {
    ok: bool,
    message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    export: Option<profile::ProfileExportResult>,
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
    purpose: Option<String>,
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
        options.purpose = self.purpose;
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
    #[serde(default)]
    purpose: Option<String>,
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
    #[serde(default)]
    setup_kill_on_timeout: bool,
    #[serde(default)]
    startup_wait_window: bool,
    #[serde(default)]
    startup_window_timeout_ms: Option<u64>,
    #[serde(default)]
    startup_screenshot_window: bool,
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
        options.purpose = self.purpose.clone();
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
            run_setup: self.run_setup
                || self.setup_timeout_ms.is_some()
                || self.setup_kill_on_timeout,
            setup: profile::ProfileSetupOptions {
                wait: self.run_setup
                    || self.setup_timeout_ms.is_some()
                    || self.setup_kill_on_timeout,
                timeout_ms: self.setup_timeout_ms,
                kill_on_timeout: self.setup_kill_on_timeout,
                acknowledge_unenforced_policy: self.acknowledge_unenforced_policy,
            },
            startup: profile::ProfileStartupOptions {
                acknowledge_unenforced_policy: self.acknowledge_unenforced_policy,
                wait_window: self.startup_wait_window
                    || self.startup_window_timeout_ms.is_some()
                    || self.startup_screenshot_window,
                window_timeout_ms: self.startup_window_timeout_ms,
                screenshot_window: self.startup_screenshot_window,
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
struct WorkspaceArtifactsParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    existing_only: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceListAppsParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    app_id: Option<String>,
    #[serde(default)]
    name_contains: Option<String>,
    #[serde(default)]
    command_contains: Option<String>,
    #[serde(default)]
    profile_id: Option<String>,
    #[serde(default)]
    running: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceListWindowsParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    include_hidden: bool,
    #[serde(default)]
    title_contains: Option<String>,
    #[serde(default)]
    class_contains: Option<String>,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    app_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceCleanupParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceStopParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspaceLaunchParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
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
    wait_window: bool,
    #[serde(default)]
    window_timeout_ms: Option<u64>,
    #[serde(default)]
    screenshot_window: bool,
}

impl WorkspaceLaunchParams {
    fn into_launch_spec(self) -> Result<LaunchSpec> {
        let cwd_explicit = self.cwd.is_some();
        let mut spec = LaunchSpec {
            command: self.command,
            name: self.name,
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
    name: Option<String>,
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
    #[serde(default)]
    kill_on_timeout: bool,
}

impl WorkspaceRunParams {
    fn into_launch_spec(self) -> Result<LaunchSpec> {
        let cwd_explicit = self.cwd.is_some();
        let mut spec = LaunchSpec {
            command: self.command,
            name: self.name,
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
    include_hidden: bool,
    #[serde(default)]
    output_path: Option<PathBuf>,
    #[serde(default)]
    events: bool,
    #[serde(default)]
    events_tail: Option<usize>,
    #[serde(default)]
    events_since_sequence: Option<u64>,
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
    class_contains: Option<String>,
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

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspaceCloseWindowParams {
    #[serde(default)]
    id: Option<String>,
    window_id: String,
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceWaitWindowParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title_contains: Option<String>,
    #[serde(default)]
    class_contains: Option<String>,
    pid: Option<u32>,
    #[serde(default)]
    app_id: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceCloseMatchingWindowParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title_contains: Option<String>,
    #[serde(default)]
    class_contains: Option<String>,
    pid: Option<u32>,
    #[serde(default)]
    app_id: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceMoveWindowParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    window_id: Option<String>,
    #[serde(default)]
    title_contains: Option<String>,
    #[serde(default)]
    class_contains: Option<String>,
    pid: Option<u32>,
    #[serde(default)]
    app_id: Option<String>,
    x: i32,
    y: i32,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceResizeWindowParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    window_id: Option<String>,
    #[serde(default)]
    title_contains: Option<String>,
    #[serde(default)]
    class_contains: Option<String>,
    pid: Option<u32>,
    #[serde(default)]
    app_id: Option<String>,
    width: u32,
    height: u32,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceTargetedWindowParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    window_id: Option<String>,
    #[serde(default)]
    title_contains: Option<String>,
    #[serde(default)]
    class_contains: Option<String>,
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
    class_contains: Option<String>,
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
    class_contains: Option<String>,
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
    class_contains: Option<String>,
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
    class_contains: Option<String>,
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
    class_contains: Option<String>,
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
    class_contains: Option<String>,
    pid: Option<u32>,
    #[serde(default)]
    app_id: Option<String>,
    text: String,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspaceClipboardSetParams {
    #[serde(default)]
    id: Option<String>,
    text: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspacePasteTextParams {
    #[serde(default)]
    id: Option<String>,
    text: String,
    #[serde(default)]
    key: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspacePasteWindowParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    window_id: Option<String>,
    #[serde(default)]
    title_contains: Option<String>,
    #[serde(default)]
    class_contains: Option<String>,
    pid: Option<u32>,
    #[serde(default)]
    app_id: Option<String>,
    text: String,
    #[serde(default)]
    key: Option<String>,
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
    #[serde(default)]
    kill_on_timeout: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceEventsParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    tail: Option<usize>,
    #[serde(default)]
    since_sequence: Option<u64>,
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
    kill_on_timeout: bool,
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
    #[serde(default)]
    wait_window: bool,
    #[serde(default)]
    window_timeout_ms: Option<u64>,
    #[serde(default)]
    screenshot_window: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspaceKillAppParams {
    #[serde(default)]
    id: Option<String>,
    app_id: String,
    #[serde(default)]
    dry_run: bool,
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
        ipc: None,
        environment: None,
        apps: None,
        windows: None,
        active_window: None,
        pointer: None,
        screenshot: None,
        app_log: None,
        clipboard: None,
        events: None,
    }
}
