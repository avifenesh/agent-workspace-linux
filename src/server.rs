use crate::workspace::{
    self, IpcResponse, WorkspaceStartOptions, WorkspaceStatus, DEFAULT_WORKSPACE_ID,
};
use anyhow::Result;
use rmcp::{
    handler::server::wrapper::{Json, Parameters},
    schemars::JsonSchema,
    tool, tool_handler, tool_router, ServerHandler, ServiceExt,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default)]
pub struct AgentWorkspaceLinux;

#[tool_router]
impl AgentWorkspaceLinux {
    #[tool(
        name = "workspace_doctor",
        description = "Report readiness for isolated Linux agent workspaces.",
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
        name = "workspace_start",
        description = "Start an isolated X11 agent workspace with its own display and control IPC socket.",
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
        Json(result_response(workspace::start_workspace(
            params.into_options(),
        )))
    }

    #[tool(
        name = "workspace_status",
        description = "Return status for an isolated agent workspace.",
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
            },
            Err(error) => error_response(error.to_string(), None),
        })
    }

    #[tool(
        name = "workspace_launch_app",
        description = "Launch an app inside an isolated agent workspace. The command runs with the workspace DISPLAY and XAUTHORITY.",
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
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
        Json(result_response(workspace::launch_app(&id, params.command)))
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
    instructions = "Use workspace_doctor to check runtime readiness. Use workspace_start before launching apps. workspace_launch_app runs commands only inside the isolated agent workspace; it does not target the user's host desktop. workspace_stop terminates the workspace and apps launched inside it."
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

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceStartParams {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
}

impl WorkspaceStartParams {
    fn into_options(self) -> WorkspaceStartOptions {
        let mut options = WorkspaceStartOptions::default();
        if let Some(id) = self.id {
            options.id = id;
        }
        if let Some(width) = self.width {
            options.width = width;
        }
        if let Some(height) = self.height {
            options.height = height;
        }
        options
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
struct WorkspaceIdParams {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WorkspaceLaunchParams {
    #[serde(default)]
    id: Option<String>,
    command: Vec<String>,
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
    }
}
