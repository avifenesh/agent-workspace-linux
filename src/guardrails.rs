use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GuardrailSummary {
    pub version: u32,
    pub acknowledgements: Vec<GuardrailRule>,
    pub previews: Vec<GuardrailRule>,
    pub explicit_overrides: Vec<GuardrailRule>,
    pub timeout_terminations: Vec<GuardrailRule>,
    pub scope_notes: Vec<GuardrailRule>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GuardrailRule {
    pub id: String,
    pub kind: String,
    pub surfaces: Vec<String>,
    pub default_behavior: String,
    pub opt_in: String,
    pub effect: String,
}

pub fn guardrail_summary() -> GuardrailSummary {
    GuardrailSummary {
        version: 1,
        acknowledgements: vec![
            rule(
                "hidden-workspace-start",
                "acknowledgement",
                &[
                    "workspace start",
                    "workspace open-profile",
                    "workspace_start",
                    "workspace_open_profile",
                ],
                "Starting a hidden agent-controlled workspace is rejected.",
                "--ack-hidden-workspace or acknowledge_hidden_workspace=true",
                "Records explicit user awareness that a separate workspace exists outside the visible desktop.",
            ),
            rule(
                "unenforced-profile-policy",
                "acknowledgement",
                &[
                    "workspace start --profile",
                    "workspace open-profile",
                    "workspace launch --profile",
                    "workspace run --profile",
                    "workspace setup",
                    "workspace launch-profile-apps",
                    "profile_check",
                ],
                "Profiles that request mount or network policy not enforced by the runtime are rejected at action time.",
                "--ack-unenforced-policy or acknowledge_unenforced_policy=true",
                "Records that the user accepted running with declared policy that is visible but not fully enforced.",
            ),
        ],
        previews: vec![
            rule(
                "profile-put-preview",
                "dry_run",
                &["profile put", "profile_put"],
                "Profile save writes to the profile store only when dry_run is false.",
                "--dry-run or dry_run=true",
                "Returns whether the profile would be created, replaced, or rejected, including any existing profile.",
            ),
            rule(
                "profile-delete-preview",
                "dry_run",
                &["profile delete", "profile_delete"],
                "Profile delete removes a saved profile only when dry_run is false.",
                "--dry-run or dry_run=true",
                "Returns the profile that would be removed without deleting it.",
            ),
            rule(
                "stale-cleanup-preview",
                "dry_run",
                &["workspace cleanup", "workspace_cleanup_stale"],
                "Cleanup removes stale runtime directories only when dry_run is false.",
                "--dry-run or dry_run=true",
                "Returns stale runtime directory candidates without deleting them.",
            ),
            rule(
                "window-close-preview",
                "dry_run",
                &[
                    "workspace close-window",
                    "workspace_close_window",
                    "workspace_close_matching_window",
                ],
                "Window close requests are sent only when dry_run is false.",
                "--dry-run or dry_run=true",
                "Resolves and returns the targeted window without closing it.",
            ),
            rule(
                "app-kill-preview",
                "dry_run",
                &["workspace kill-app", "workspace_kill_app"],
                "App termination is sent only when dry_run is false.",
                "--dry-run or dry_run=true",
                "Resolves and returns the matched app without terminating it.",
            ),
            rule(
                "workspace-stop-preview",
                "dry_run",
                &["workspace stop", "workspace_stop"],
                "Workspace shutdown and app termination happen only when dry_run is false.",
                "--dry-run or dry_run=true",
                "Returns currently running apps without stopping the workspace.",
            ),
        ],
        explicit_overrides: vec![
            rule(
                "profile-replace",
                "explicit_override",
                &["profile put", "profile_put"],
                "Saving a profile with an existing id is rejected.",
                "--replace or replace=true",
                "Allows intentional overwrite of a saved environment profile.",
            ),
            rule(
                "profile-export-replace",
                "explicit_override",
                &["profile export", "profile_export"],
                "Exporting to an existing output file is rejected.",
                "--replace or replace=true",
                "Allows intentional overwrite of an existing exported profile JSON file.",
            ),
        ],
        timeout_terminations: vec![
            rule(
                "run-app-kill-on-timeout",
                "explicit_termination",
                &["workspace run", "workspace_run_app"],
                "Timed-out commands are left running unless the caller opts into cleanup.",
                "--kill-on-timeout or kill_on_timeout=true",
                "Terminates the launched app process group when the wait timeout elapses.",
            ),
            rule(
                "wait-app-kill-on-timeout",
                "explicit_termination",
                &["workspace wait-app", "workspace_wait_app"],
                "Timed-out existing apps are left running unless the caller opts into cleanup.",
                "--kill-on-timeout or kill_on_timeout=true",
                "Terminates the matched app process group when the wait timeout elapses.",
            ),
            rule(
                "profile-setup-kill-on-timeout",
                "explicit_termination",
                &[
                    "workspace setup",
                    "workspace open-profile --setup",
                    "workspace_run_profile_setup",
                    "workspace_open_profile",
                ],
                "Timed-out setup commands are left running unless the caller opts into cleanup.",
                "--kill-on-timeout, --setup-kill-on-timeout, kill_on_timeout=true, or setup_kill_on_timeout=true",
                "Terminates timed-out setup command process groups.",
            ),
        ],
        scope_notes: vec![
            rule(
                "workspace-local-actions",
                "scope",
                &[
                    "workspace input tools",
                    "workspace window tools",
                    "workspace app tools",
                ],
                "Tools attach to the isolated workspace display and control socket, not the host desktop.",
                "workspace start creates DISPLAY/XAUTHORITY/AGENT_WORKSPACE_SOCKET for the hidden workspace",
                "Pointer, keyboard, window, clipboard, and app controls are scoped to the isolated X11 workspace.",
            ),
            rule(
                "event-log-redaction",
                "privacy",
                &["workspace events", "workspace_events"],
                "Event logs do not store raw typed text, raw pasted text, or raw clipboard-set text.",
                "workspace events or workspace_events",
                "Sensitive text actions are recorded as metadata such as character counts.",
            ),
        ],
    }
}

fn rule(
    id: &str,
    kind: &str,
    surfaces: &[&str],
    default_behavior: &str,
    opt_in: &str,
    effect: &str,
) -> GuardrailRule {
    GuardrailRule {
        id: id.to_string(),
        kind: kind.to_string(),
        surfaces: surfaces.iter().map(|surface| surface.to_string()).collect(),
        default_behavior: default_behavior.to_string(),
        opt_in: opt_in.to_string(),
        effect: effect.to_string(),
    }
}
