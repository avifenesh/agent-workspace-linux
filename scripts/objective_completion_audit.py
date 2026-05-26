#!/usr/bin/env python3
"""Audit the active product objective against current release evidence."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import sys
from pathlib import Path
from typing import Any

sys.dont_write_bytecode = True

from release_gate_audit import DEFAULT_DESKTOP_REPO
from release_gate_audit import compute_review_scope_identity
from release_gate_audit import compute_source_identity


ROOT = Path(__file__).resolve().parent.parent
DEFAULT_OUTPUT_DIR = ROOT / "target" / "objective-completion-audit"
DEFAULT_SMOKE_DIR = ROOT / "target" / "prod-readiness-smoke"
DEFAULT_RELEASE_AUDIT_DIR = ROOT / "target" / "release-gate-audit"
DEFAULT_FINAL_BUNDLE_DIR = ROOT / "target" / "final-review-bundle"
DIRECT_MCP_VIEWER_LIFECYCLE_CHECK_ID = "direct_mcp_viewer_lifecycle"
DIRECT_MCP_VIEWER_LIFECYCLE_SKIPPED_NO_NEW_VIEWER_CHECK_ID = (
    "direct_mcp_viewer_lifecycle_skipped_no_new_viewer"
)
DIRECT_MCP_WORKSPACE_BROWSER_CHECK_ID = "direct_mcp_workspace_browser_cdp"


def latest_file(directory: Path, pattern: str) -> Path | None:
    if not directory.exists():
        return None
    files = sorted(directory.glob(pattern))
    return files[-1] if files else None


def read_json(path: Path | None) -> dict[str, Any] | None:
    if path is None:
        return None
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return None
    return value if isinstance(value, dict) else None


def identity_matches(report_identity: dict[str, Any], current_identity: dict[str, Any]) -> bool:
    return (
        isinstance(report_identity, dict)
        and report_identity.get("source_hash") == current_identity.get("source_hash")
        and report_identity.get("git_head") == current_identity.get("git_head")
    )


def review_scope_matches(
    report_identity: dict[str, Any],
    current_identity: dict[str, Any],
) -> bool:
    return (
        isinstance(report_identity, dict)
        and report_identity.get("review_scope_hash") == current_identity.get("review_scope_hash")
        and report_identity.get("git_head") == current_identity.get("git_head")
    )


def gate_by_id(release_audit: dict[str, Any] | None) -> dict[str, dict[str, Any]]:
    gates = (release_audit or {}).get("gates") or []
    return {str(gate.get("id")): gate for gate in gates if isinstance(gate, dict)}


def passed_gate(gates: dict[str, dict[str, Any]], gate_id: str) -> bool:
    return (gates.get(gate_id) or {}).get("status") == "passed"


def gate_missing(gates: dict[str, dict[str, Any]], gate_id: str) -> list[str]:
    gate = gates.get(gate_id) or {}
    return [str(item) for item in gate.get("missing") or []]


def source_contains(path: Path, needles: list[str]) -> bool:
    try:
        source = path.read_text(encoding="utf-8")
    except OSError:
        return False
    return all(needle in source for needle in needles)


def smoke_has_completed_check(smoke: dict[str, Any] | None, check_id: str) -> bool:
    if not isinstance(smoke, dict):
        return False
    completed = smoke.get("completed_check_ids")
    if isinstance(completed, list) and check_id in {str(item) for item in completed}:
        return True
    checks = smoke.get("completed_checks")
    if isinstance(checks, list):
        return any(isinstance(item, dict) and item.get("id") == check_id for item in checks)
    return False


def direct_mcp_viewer_lifecycle_harness_present() -> bool:
    return source_contains(
        ROOT / "scripts" / "prod_readiness_smoke.sh",
        [
            "node --check scripts/mcp_viewer_lifecycle_smoke.js",
            "node scripts/mcp_viewer_lifecycle_smoke.js",
            DIRECT_MCP_VIEWER_LIFECYCLE_CHECK_ID,
        ],
    ) and source_contains(
        ROOT / "scripts" / "mcp_viewer_lifecycle_smoke.js",
        [
            "workspace_open_viewer",
            "workspace_list_viewers",
            "workspace_close_viewer",
            "direct mcp viewer lifecycle smoke passed",
        ],
    )


def direct_mcp_workspace_browser_harness_present() -> bool:
    return source_contains(
        ROOT / "scripts" / "prod_readiness_smoke.sh",
        [
            "node --check scripts/mcp_workspace_browser_cdp_smoke.js",
            "node scripts/mcp_workspace_browser_cdp_smoke.js",
            DIRECT_MCP_WORKSPACE_BROWSER_CHECK_ID,
        ],
    ) and source_contains(
        ROOT / "scripts" / "mcp_workspace_browser_cdp_smoke.js",
        [
            "workspace_browser_targets",
            "workspace_browser_snapshot",
            "workspace_browser_search_results",
            "min_vram_gb",
            "workspace_browser_navigate",
            "devtools_endpoint",
            "webSocketDebuggerUrl",
            "workspace browser CDP smoke passed",
        ],
    )


def requirement(
    req_id: str,
    title: str,
    passed: bool,
    *,
    evidence: list[str],
    missing: list[str],
) -> dict[str, Any]:
    return {
        "id": req_id,
        "title": title,
        "status": "passed" if passed else "pending",
        "evidence": evidence,
        "missing": [] if passed else missing,
    }


def build_requirements(
    *,
    smoke_current: bool,
    release_current: bool,
    final_bundle_current: bool,
    direct_mcp_viewer_lifecycle_current: bool,
    direct_mcp_workspace_browser_current: bool,
    gates: dict[str, dict[str, Any]],
    smoke_path: Path | None,
    release_audit_path: Path | None,
    final_bundle_path: Path | None,
    direct_mcp_viewer_lifecycle_skipped_no_new_viewer: bool = False,
) -> list[dict[str, Any]]:
    smoke_evidence = [str(smoke_path)] if smoke_path else []
    release_evidence = [str(release_audit_path)] if release_audit_path else []
    bundle_evidence = [str(final_bundle_path)] if final_bundle_path else []
    base_missing = (
        []
        if smoke_current
        else ["current prod-readiness smoke report for the current source and review-scope identity"]
    )
    release_missing = (
        []
        if release_current
        else ["current release-gate audit for the current source and review-scope identity"]
    )
    final_bundle_missing = (
        []
        if final_bundle_current
        else ["current final human-review bundle for the current source and review-scope identity"]
    )
    if direct_mcp_viewer_lifecycle_current:
        direct_mcp_viewer_lifecycle_missing = []
    elif direct_mcp_viewer_lifecycle_skipped_no_new_viewer:
        direct_mcp_viewer_lifecycle_missing = [
            "current prod-readiness smoke ran with AGENT_WORKSPACE_NO_NEW_VIEWER=1, "
            "so direct_mcp_viewer_lifecycle was intentionally skipped to avoid opening "
            "a second GPUI viewer; run without that flag only when strict viewer "
            "lifecycle evidence is needed"
        ]
    else:
        direct_mcp_viewer_lifecycle_missing = [
            "current prod-readiness smoke report must record completed_check_ids including direct_mcp_viewer_lifecycle, backed by the direct repo-owned MCP viewer lifecycle smoke"
        ]
    direct_mcp_workspace_browser_missing = (
        []
        if direct_mcp_workspace_browser_current
        else [
            "current prod-readiness smoke report must record completed_check_ids including direct_mcp_workspace_browser_cdp, backed by the direct repo-owned MCP workspace browser CDP smoke"
        ]
    )
    local_proven = smoke_current and release_current

    requirements = [
        requirement(
            "mcp_permission_boundaries",
            "MCP permission boundaries are enforced by configured MCP parameters",
            local_proven,
            evidence=smoke_evidence + release_evidence,
            missing=base_missing + release_missing,
        ),
        requirement(
            "empty_mcp_classifies_without_ceiling",
            "Empty/default MCP imposes no extra ceiling and only classifies action type",
            local_proven,
            evidence=smoke_evidence + release_evidence,
            missing=base_missing + release_missing,
        ),
        requirement(
            "agent_ux_intent_derivation",
            "Agent UX can derive likely intent and safe next actions",
            local_proven,
            evidence=smoke_evidence + release_evidence,
            missing=base_missing + release_missing,
        ),
        requirement(
            "headless_requires_explicit_flag",
            "Headless operation works only through the explicit MCP headless mode",
            local_proven,
            evidence=smoke_evidence,
            missing=base_missing,
        ),
        requirement(
            "gpui_local_control_surface",
            "GPUI viewer provides local live control, observe, stop, and non-topmost behavior",
            local_proven,
            evidence=smoke_evidence + release_evidence,
            missing=base_missing + release_missing,
        ),
        requirement(
            "repo_owned_mcp_viewer_lifecycle",
            "Repo-owned direct MCP can open, list, and close GPUI viewers without Codex app MCP evidence",
            local_proven and direct_mcp_viewer_lifecycle_current,
            evidence=smoke_evidence + [str(ROOT / "scripts" / "mcp_viewer_lifecycle_smoke.js")],
            missing=base_missing + release_missing + direct_mcp_viewer_lifecycle_missing,
        ),
        requirement(
            "workspace_owned_browser_control",
            "Browser control is proven through the workspace Chrome/Chromium instance instead of the host Chrome bridge",
            local_proven and direct_mcp_workspace_browser_current,
            evidence=smoke_evidence
            + [str(ROOT / "scripts" / "mcp_workspace_browser_cdp_smoke.js")],
            missing=base_missing + release_missing + direct_mcp_workspace_browser_missing,
        ),
        requirement(
            "codex_desktop_thin_integration",
            "Codex Desktop integration stays thin and does not revive the embedded screen",
            release_current and passed_gate(gates, "desktop_thin_integration"),
            evidence=release_evidence,
            missing=release_missing + gate_missing(gates, "desktop_thin_integration"),
        ),
        requirement(
            "app_qa_dogfood",
            "App-QA dogfood has a real local GUI evidence path",
            release_current and passed_gate(gates, "app_qa_dogfood"),
            evidence=release_evidence,
            missing=release_missing + gate_missing(gates, "app_qa_dogfood"),
        ),
        requirement(
            "modern_linux_viewer_matrix",
            "Floating GPUI viewer is proven for the release-required Linux desktop coverage",
            release_current and passed_gate(gates, "viewer_desktop_matrix"),
            evidence=release_evidence,
            missing=release_missing + gate_missing(gates, "viewer_desktop_matrix"),
        ),
        requirement(
            "github_explore_dogfood",
            "GitHub Explore dogfood has a visible workspace browser repository-discovery path",
            release_current and passed_gate(gates, "github_explore_dogfood"),
            evidence=release_evidence,
            missing=release_missing + gate_missing(gates, "github_explore_dogfood"),
        ),
        requirement(
            "human_final_diff_review",
            "Human final diff review is bound to current source, review scope, and diff artifacts",
            release_current
            and final_bundle_current
            and passed_gate(gates, "human_final_diff_review"),
            evidence=release_evidence + bundle_evidence,
            missing=release_missing
            + final_bundle_missing
            + gate_missing(gates, "human_final_diff_review"),
        ),
    ]
    return requirements


def build_report(
    *,
    desktop_repo: Path,
    smoke_dir: Path,
    release_audit_dir: Path,
    final_bundle_dir: Path,
) -> dict[str, Any]:
    current_source = compute_source_identity(ROOT, desktop_repo=desktop_repo)
    current_review_scope = compute_review_scope_identity(ROOT, desktop_repo=desktop_repo)
    smoke_path = latest_file(smoke_dir, "*.json")
    release_audit_path = latest_file(release_audit_dir, "*.json")
    final_bundle_path = latest_file(final_bundle_dir, "*.json")
    smoke = read_json(smoke_path)
    release_audit = read_json(release_audit_path)
    final_bundle = read_json(final_bundle_path)

    smoke_current = (
        isinstance(smoke, dict)
        and smoke.get("schema") == "agent-workspace-linux.prod_readiness_smoke.v1"
        and smoke.get("status") == "passed"
        and identity_matches(smoke.get("source_identity") or {}, current_source)
        and review_scope_matches(smoke.get("review_scope_identity") or {}, current_review_scope)
    )
    release_inputs = (release_audit or {}).get("inputs") or {}
    release_current = (
        isinstance(release_audit, dict)
        and release_audit.get("schema") == "agent-workspace-linux.release_gate_audit.v1"
        and identity_matches(release_inputs.get("source_identity") or {}, current_source)
        and review_scope_matches(release_inputs.get("review_scope_identity") or {}, current_review_scope)
    )
    final_bundle_current = (
        isinstance(final_bundle, dict)
        and final_bundle.get("schema") == "agent-workspace-linux.final_human_review_bundle.v1"
        and identity_matches(final_bundle.get("source_identity") or {}, current_source)
        and review_scope_matches(
            final_bundle.get("review_scope_identity") or {},
            current_review_scope,
        )
    )
    gates = gate_by_id(release_audit)
    direct_mcp_viewer_lifecycle_current = (
        smoke_current
        and smoke_has_completed_check(smoke, DIRECT_MCP_VIEWER_LIFECYCLE_CHECK_ID)
        and direct_mcp_viewer_lifecycle_harness_present()
    )
    direct_mcp_viewer_lifecycle_skipped_no_new_viewer = (
        smoke_current
        and smoke_has_completed_check(
            smoke, DIRECT_MCP_VIEWER_LIFECYCLE_SKIPPED_NO_NEW_VIEWER_CHECK_ID
        )
    )
    direct_mcp_workspace_browser_current = (
        smoke_current
        and smoke_has_completed_check(smoke, DIRECT_MCP_WORKSPACE_BROWSER_CHECK_ID)
        and direct_mcp_workspace_browser_harness_present()
    )
    requirements = build_requirements(
        smoke_current=smoke_current,
        release_current=release_current,
        final_bundle_current=final_bundle_current,
        direct_mcp_viewer_lifecycle_current=direct_mcp_viewer_lifecycle_current,
        direct_mcp_workspace_browser_current=direct_mcp_workspace_browser_current,
        gates=gates,
        smoke_path=smoke_path,
        release_audit_path=release_audit_path,
        final_bundle_path=final_bundle_path,
        direct_mcp_viewer_lifecycle_skipped_no_new_viewer=direct_mcp_viewer_lifecycle_skipped_no_new_viewer,
    )
    status = "passed" if all(item["status"] == "passed" for item in requirements) else "pending"
    return {
        "schema": "agent-workspace-linux.objective_completion_audit.v1",
        "created_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "status": status,
        "current_source_identity": current_source,
        "current_review_scope_identity": current_review_scope,
        "latest_evidence": {
            "prod_readiness_smoke": str(smoke_path) if smoke_path else None,
            "release_gate_audit": str(release_audit_path) if release_audit_path else None,
            "final_review_bundle": str(final_bundle_path) if final_bundle_path else None,
        },
        "consistency": {
            "prod_readiness_smoke_matches_current": smoke_current,
            "release_gate_audit_matches_current": release_current,
            "final_bundle_matches_current": final_bundle_current,
            "direct_mcp_viewer_lifecycle_check_present": direct_mcp_viewer_lifecycle_current,
            "direct_mcp_viewer_lifecycle_skipped_no_new_viewer": direct_mcp_viewer_lifecycle_skipped_no_new_viewer,
            "direct_mcp_workspace_browser_check_present": direct_mcp_workspace_browser_current,
            "final_bundle_schema": (final_bundle or {}).get("schema"),
            "release_gate_status": (release_audit or {}).get("status"),
        },
        "requirements": requirements,
        "pending_requirements": [
            {
                "id": item["id"],
                "missing": item["missing"],
            }
            for item in requirements
            if item["status"] != "passed"
        ],
    }


def write_report(report: dict[str, Any], output_dir: Path) -> Path:
    output_dir.mkdir(parents=True, exist_ok=True)
    timestamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    path = output_dir / f"{timestamp}.json"
    path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return path


def print_summary(report: dict[str, Any], path: Path | None) -> None:
    if path is not None:
        print(f"objective completion audit report: {path}")
    print(f"objective completion audit status: {report['status']}")
    for item in report["requirements"]:
        if item["status"] == "passed":
            print(f"- {item['id']}: passed")
        else:
            print(f"- {item['id']}: pending")
            for missing in item["missing"]:
                print(f"  missing: {missing}")


def run_self_test() -> None:
    assert smoke_has_completed_check(
        {"completed_check_ids": [DIRECT_MCP_VIEWER_LIFECYCLE_CHECK_ID]},
        DIRECT_MCP_VIEWER_LIFECYCLE_CHECK_ID,
    )
    assert smoke_has_completed_check(
        {"completed_checks": [{"id": DIRECT_MCP_VIEWER_LIFECYCLE_CHECK_ID}]},
        DIRECT_MCP_VIEWER_LIFECYCLE_CHECK_ID,
    )
    assert smoke_has_completed_check(
        {"completed_check_ids": [DIRECT_MCP_WORKSPACE_BROWSER_CHECK_ID]},
        DIRECT_MCP_WORKSPACE_BROWSER_CHECK_ID,
    )
    assert not smoke_has_completed_check(
        {"completed_check_ids": ["mcp_permissions"]},
        DIRECT_MCP_VIEWER_LIFECYCLE_CHECK_ID,
    )
    assert smoke_has_completed_check(
        {
            "completed_check_ids": [
                DIRECT_MCP_VIEWER_LIFECYCLE_SKIPPED_NO_NEW_VIEWER_CHECK_ID
            ]
        },
        DIRECT_MCP_VIEWER_LIFECYCLE_SKIPPED_NO_NEW_VIEWER_CHECK_ID,
    )
    gates = {
        "desktop_thin_integration": {"status": "passed", "missing": []},
        "app_qa_dogfood": {"status": "passed", "missing": []},
        "viewer_desktop_matrix": {
            "status": "passed",
            "missing": [],
            "advisory_missing": ["KDE row"],
        },
        "github_explore_dogfood": {"status": "pending", "missing": ["GitHub Explore"]},
        "human_final_diff_review": {"status": "pending", "missing": ["marker"]},
    }
    requirements = build_requirements(
        smoke_current=True,
        release_current=True,
        final_bundle_current=True,
        direct_mcp_viewer_lifecycle_current=True,
        direct_mcp_workspace_browser_current=True,
        gates=gates,
        smoke_path=Path("/tmp/smoke.json"),
        release_audit_path=Path("/tmp/audit.json"),
        final_bundle_path=Path("/tmp/bundle.json"),
    )
    by_id = {item["id"]: item for item in requirements}
    assert by_id["mcp_permission_boundaries"]["status"] == "passed"
    assert by_id["repo_owned_mcp_viewer_lifecycle"]["status"] == "passed"
    assert by_id["workspace_owned_browser_control"]["status"] == "passed"
    assert by_id["app_qa_dogfood"]["status"] == "passed"
    assert by_id["modern_linux_viewer_matrix"]["status"] == "passed"
    assert by_id["github_explore_dogfood"]["status"] == "pending"
    assert by_id["human_final_diff_review"]["status"] == "pending"
    requirements = build_requirements(
        smoke_current=False,
        release_current=True,
        final_bundle_current=True,
        direct_mcp_viewer_lifecycle_current=True,
        direct_mcp_workspace_browser_current=True,
        gates=gates,
        smoke_path=None,
        release_audit_path=Path("/tmp/audit.json"),
        final_bundle_path=Path("/tmp/bundle.json"),
    )
    by_id = {item["id"]: item for item in requirements}
    assert by_id["mcp_permission_boundaries"]["status"] == "pending"
    assert "current prod-readiness smoke report" in by_id["mcp_permission_boundaries"]["missing"][0]
    requirements = build_requirements(
        smoke_current=True,
        release_current=True,
        final_bundle_current=True,
        direct_mcp_viewer_lifecycle_current=False,
        direct_mcp_workspace_browser_current=True,
        gates=gates,
        smoke_path=Path("/tmp/smoke.json"),
        release_audit_path=Path("/tmp/audit.json"),
        final_bundle_path=Path("/tmp/bundle.json"),
    )
    by_id = {item["id"]: item for item in requirements}
    assert by_id["repo_owned_mcp_viewer_lifecycle"]["status"] == "pending"
    assert any(
        "completed_check_ids" in missing
        for missing in by_id["repo_owned_mcp_viewer_lifecycle"]["missing"]
    )
    requirements = build_requirements(
        smoke_current=True,
        release_current=True,
        final_bundle_current=True,
        direct_mcp_viewer_lifecycle_current=False,
        direct_mcp_workspace_browser_current=True,
        gates=gates,
        smoke_path=Path("/tmp/smoke.json"),
        release_audit_path=Path("/tmp/audit.json"),
        final_bundle_path=Path("/tmp/bundle.json"),
        direct_mcp_viewer_lifecycle_skipped_no_new_viewer=True,
    )
    by_id = {item["id"]: item for item in requirements}
    assert by_id["repo_owned_mcp_viewer_lifecycle"]["status"] == "pending"
    assert any(
        "AGENT_WORKSPACE_NO_NEW_VIEWER=1" in missing
        for missing in by_id["repo_owned_mcp_viewer_lifecycle"]["missing"]
    )
    requirements = build_requirements(
        smoke_current=True,
        release_current=True,
        final_bundle_current=True,
        direct_mcp_viewer_lifecycle_current=True,
        direct_mcp_workspace_browser_current=False,
        gates=gates,
        smoke_path=Path("/tmp/smoke.json"),
        release_audit_path=Path("/tmp/audit.json"),
        final_bundle_path=Path("/tmp/bundle.json"),
    )
    by_id = {item["id"]: item for item in requirements}
    assert by_id["workspace_owned_browser_control"]["status"] == "pending"
    assert any(
        "direct_mcp_workspace_browser_cdp" in missing
        for missing in by_id["workspace_owned_browser_control"]["missing"]
    )
    requirements = build_requirements(
        smoke_current=True,
        release_current=True,
        final_bundle_current=False,
        direct_mcp_viewer_lifecycle_current=True,
        direct_mcp_workspace_browser_current=True,
        gates={
            "desktop_thin_integration": {"status": "passed", "missing": []},
            "app_qa_dogfood": {"status": "passed", "missing": []},
            "viewer_desktop_matrix": {"status": "passed", "missing": []},
            "github_explore_dogfood": {"status": "passed", "missing": []},
            "human_final_diff_review": {"status": "passed", "missing": []},
        },
        smoke_path=Path("/tmp/smoke.json"),
        release_audit_path=Path("/tmp/audit.json"),
        final_bundle_path=Path("/tmp/bundle.json"),
    )
    by_id = {item["id"]: item for item in requirements}
    assert by_id["human_final_diff_review"]["status"] == "pending"
    assert any(
        "current final human-review bundle" in missing
        for missing in by_id["human_final_diff_review"]["missing"]
    )
    print("objective completion audit self-test passed")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT_DIR)
    parser.add_argument("--desktop-repo", type=Path, default=DEFAULT_DESKTOP_REPO)
    parser.add_argument("--smoke-dir", type=Path, default=DEFAULT_SMOKE_DIR)
    parser.add_argument("--release-audit-dir", type=Path, default=DEFAULT_RELEASE_AUDIT_DIR)
    parser.add_argument("--final-bundle-dir", type=Path, default=DEFAULT_FINAL_BUNDLE_DIR)
    parser.add_argument("--json", action="store_true", help="print only the JSON report")
    parser.add_argument("--require-complete", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.self_test:
        run_self_test()
        return 0
    report = build_report(
        desktop_repo=args.desktop_repo,
        smoke_dir=args.smoke_dir,
        release_audit_dir=args.release_audit_dir,
        final_bundle_dir=args.final_bundle_dir,
    )
    path = write_report(report, args.output_dir)
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print_summary(report, path)
    if args.require_complete and report["status"] != "passed":
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
