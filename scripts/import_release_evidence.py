#!/usr/bin/env python3
"""Import externally collected release evidence into the local audit dirs."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import re
import shutil
import sys
import tempfile
from pathlib import Path
from typing import Any

sys.dont_write_bytecode = True

from release_gate_audit import compute_source_identity
from release_gate_audit import compute_review_scope_identity
from release_gate_audit import DEFAULT_DESKTOP_REPO
from release_gate_audit import DEFAULT_HUMAN_REVIEW_MARKER
from release_gate_audit import DEFAULT_MAX_EVIDENCE_AGE_DAYS
from release_gate_audit import DESKTOP_SOURCE_IDENTITY_PATHS
from release_gate_audit import audit_human_review
from release_gate_audit import app_qa_dogfood_contract_ok
from release_gate_audit import file_sha256
from release_gate_audit import github_explore_dogfood_contract_ok
from release_gate_audit import human_review_metadata_ok
from release_gate_audit import native_wayland_observed
from release_gate_audit import parse_timestamp
from release_gate_audit import real_grocery_cart_draft_interaction_ok
from release_gate_audit import real_grocery_cart_draft_steps_manifest_ok
from release_gate_audit import real_grocery_plan_assertions_ok
from release_gate_audit import real_grocery_profile_directory_ok
from release_gate_audit import real_grocery_safety_contract_ok
from release_gate_audit import real_grocery_snapshot_privacy_ok
from release_gate_audit import real_grocery_target_url
from release_gate_audit import real_grocery_workspace_browser_control_ok
from release_gate_audit import real_grocery_workspace_input_audit_ok
from release_gate_audit import real_grocery_workspace_cleanup_ok
from release_gate_audit import repo_owned_runtime_evidence
from release_gate_audit import RUNTIME_SOURCE_IDENTITY_PATHS
from release_gate_audit import review_scope_matches
from release_gate_audit import source_identity_matches
from release_gate_audit import source_file_paths
from release_gate_audit import viewer_matrix_session_release_eligible


ROOT = Path(__file__).resolve().parent.parent
DEFAULT_VIEWER_DIR = ROOT / "target" / "viewer-desktop-matrix"
DEFAULT_APP_QA_DIR = ROOT / "target" / "app-qa-dogfood"
DEFAULT_GROCERY_DIR = ROOT / "target" / "real-grocery-dogfood"
DEFAULT_GITHUB_EXPLORE_DIR = ROOT / "target" / "github-explore-dogfood"
DEFAULT_HUMAN_REVIEW_ARTIFACT_DIR = ROOT / "target" / "final-review-bundle"
VIEWER_SCHEMA = "agent-workspace-linux.viewer_desktop_matrix.v1"
APP_QA_SCHEMA = "agent-workspace-linux.app_qa_dogfood.v1"
GROCERY_SCHEMA = "agent-workspace-linux.real_grocery_dogfood_probe.v1"
GITHUB_EXPLORE_SCHEMA = "agent-workspace-linux.github_explore_dogfood.v1"
HUMAN_REVIEW_SCHEMA = "agent-workspace-linux.human_final_diff_review.v1"


def read_json(path: Path) -> dict[str, Any] | None:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return None
    return value if isinstance(value, dict) else None


def collect_json_paths(paths: list[Path]) -> list[Path]:
    collected: list[Path] = []
    for path in paths:
        if path.is_file():
            collected.append(path)
        elif path.is_dir():
            collected.extend(sorted(path.rglob("*.json")))
        else:
            collected.append(path)
    return collected


def truthy(value: Any) -> bool:
    return value is True or str(value).strip().lower() in {"1", "true", "yes"}


def report_kind(report: dict[str, Any]) -> str | None:
    schema = report.get("schema")
    if schema == VIEWER_SCHEMA:
        return "viewer_desktop_matrix"
    if schema == APP_QA_SCHEMA:
        return "app_qa_dogfood"
    if schema == GROCERY_SCHEMA:
        return "real_grocery_dogfood"
    if schema == GITHUB_EXPLORE_SCHEMA:
        return "github_explore_dogfood"
    if schema == HUMAN_REVIEW_SCHEMA:
        return "human_final_diff_review"
    return None


def destination_dir(
    kind: str,
    *,
    viewer_dir: Path,
    app_qa_dir: Path,
    grocery_dir: Path,
    github_explore_dir: Path,
) -> Path:
    if kind == "viewer_desktop_matrix":
        return viewer_dir
    if kind == "app_qa_dogfood":
        return app_qa_dir
    if kind == "real_grocery_dogfood":
        return grocery_dir
    if kind == "github_explore_dogfood":
        return github_explore_dir
    if kind == "human_final_diff_review":
        raise ValueError("human review markers use the exact marker destination")
    raise ValueError(f"unknown evidence kind: {kind}")


def timestamp_prefix(report: dict[str, Any]) -> str:
    timestamp = parse_timestamp(report.get("created_at_utc"))
    if timestamp is None:
        timestamp = dt.datetime.now(dt.timezone.utc)
    return timestamp.strftime("%Y%m%dT%H%M%SZ")


def report_digest(report: dict[str, Any]) -> str:
    data = json.dumps(report, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return hashlib.sha256(data).hexdigest()


def safe_label(kind: str, source_path: Path) -> str:
    label = re.sub(r"[^A-Za-z0-9._-]+", "-", source_path.stem).strip(".-")
    if not label:
        label = kind
    return label[:80]


def destination_path(source_path: Path, report: dict[str, Any], kind: str, dest_dir: Path) -> Path:
    return dest_dir / f"{timestamp_prefix(report)}-{safe_label(kind, source_path)}-{report_digest(report)[:12]}.json"


def artifact_candidates(source_path: Path, path_value: str) -> list[Path]:
    original = Path(path_value)
    candidates = []
    if original.is_absolute():
        candidates.append(original)
    else:
        candidates.append(source_path.parent / original)
    candidates.append(source_path.parent / original.name)
    seen: set[Path] = set()
    unique = []
    for candidate in candidates:
        resolved = candidate.expanduser()
        if resolved not in seen:
            seen.add(resolved)
            unique.append(resolved)
    return unique


def prepare_human_review_marker_import(
    report: dict[str, Any],
    source_path: Path,
    *,
    artifact_dir: Path,
) -> tuple[list[str], dict[str, Any], list[tuple[Path, Path]]]:
    errors: list[str] = []
    copies: list[tuple[Path, Path]] = []
    artifacts = report.get("review_artifacts")
    if not isinstance(artifacts, list):
        return ["human review marker must include review_artifacts"], report, copies
    rewritten_artifacts: list[dict[str, Any]] = []
    by_label = {
        artifact.get("label"): artifact
        for artifact in artifacts
        if isinstance(artifact, dict) and isinstance(artifact.get("label"), str)
    }
    for label in ["runtime", "codex_desktop"]:
        artifact = by_label.get(label)
        if not isinstance(artifact, dict):
            errors.append(f"missing {label} review artifact")
            continue
        expected_sha = artifact.get("sha256")
        expected_size = artifact.get("size_bytes")
        path_value = artifact.get("path")
        if (
            not isinstance(path_value, str)
            or not re.fullmatch(r"[0-9a-f]{64}", str(expected_sha or ""))
            or not isinstance(expected_size, int)
            or expected_size <= 0
        ):
            errors.append(f"{label} review artifact metadata is invalid")
            continue
        source_artifact = None
        for candidate in artifact_candidates(source_path, path_value):
            if (
                candidate.is_file()
                and candidate.stat().st_size == expected_size
                and file_sha256(candidate) == expected_sha
            ):
                source_artifact = candidate
                break
        if source_artifact is None:
            errors.append(f"{label} review artifact bytes were not found next to {source_path}")
            continue
        suffix = source_artifact.suffix or ".diff"
        dest_artifact = (
            artifact_dir
            / f"{timestamp_prefix(report)}-{safe_label(label, source_artifact)}-{expected_sha[:12]}{suffix}"
        )
        if dest_artifact.exists() and file_sha256(dest_artifact) != expected_sha:
            errors.append(f"destination review artifact already exists with different bytes: {dest_artifact}")
            continue
        rewritten = dict(artifact)
        rewritten["path"] = str(dest_artifact)
        rewritten["sha256"] = expected_sha
        rewritten["size_bytes"] = expected_size
        rewritten_artifacts.append(rewritten)
        copies.append((source_artifact, dest_artifact))
    if errors:
        return errors, report, copies
    rewritten_report = json.loads(json.dumps(report))
    rewritten_report["review_artifacts"] = rewritten_artifacts
    return [], rewritten_report, copies


def validate_report(
    report: dict[str, Any],
    *,
    current_source: dict[str, Any],
    current_review_scope: dict[str, Any],
    allow_source_mismatch: bool,
    allow_nonpassing: bool,
) -> list[str]:
    errors: list[str] = []
    kind = report_kind(report)
    if kind is None:
        return [f"unsupported schema: {report.get('schema')!r}"]

    source_identity = report.get("source_identity")
    if not isinstance(source_identity, dict):
        errors.append("missing source_identity object")
    elif not allow_source_mismatch and not source_identity_matches(report, current_source):
        errors.append(
            f"source identity does not match current source hash {current_source.get('source_hash')}"
        )

    if kind == "viewer_desktop_matrix":
        if not repo_owned_runtime_evidence(report):
            errors.append("viewer evidence must be collected by the repo-owned runtime collector")
        smoke = report.get("viewer_smoke") or {}
        matrix = report.get("matrix_result") or {}
        if not allow_nonpassing:
            if smoke.get("status") != "passed":
                errors.append("viewer_smoke.status must be passed")
            if matrix.get("counts_for_release_matrix") is not True:
                errors.append("matrix_result.counts_for_release_matrix must be true")
            if not viewer_matrix_session_release_eligible(report):
                errors.append("viewer session/display attestation must be release eligible")
        if truthy(matrix.get("native_wayland_layer_shell_observed")):
            if not str(matrix.get("native_wayland_layer_shell_notes") or "").strip():
                errors.append("native Wayland observation requires notes")
            elif not native_wayland_observed(report):
                errors.append(
                    "native Wayland observation must be positive layer-shell/top-layer evidence from a non-GNOME native Wayland path"
                )

    if kind == "app_qa_dogfood" and not allow_nonpassing:
        if not repo_owned_runtime_evidence(report):
            errors.append("app-QA evidence must be collected by the repo-owned runtime collector")
        if not app_qa_dogfood_contract_ok(report):
            errors.append(
                "app-QA evidence must prove local GUI app launch, screenshot, logs, events, non-destructive input, and clean stop"
            )

    if kind == "real_grocery_dogfood":
        if not repo_owned_runtime_evidence(report):
            errors.append("real grocery evidence must be collected by the repo-owned runtime collector")
        real_browser = report.get("real_browser") or {}
        if not allow_nonpassing:
            if report.get("mode") != "real-browser":
                errors.append("real grocery evidence must use mode=real-browser")
            if real_browser.get("status") != "passed":
                errors.append("real_browser.status must be passed")
            if real_browser.get("checkout_approval_refused") is not True:
                errors.append("real_browser.checkout_approval_refused must be true")
            if real_browser.get("profile_copy_manifest_valid") is not True:
                errors.append("real_browser.profile_copy_manifest_valid must be true")
            if not real_grocery_profile_directory_ok(report):
                errors.append(
                    "real grocery profile directory evidence must be consistent and safe"
                )
            if not real_grocery_target_url(report):
                errors.append("real grocery target URL must be an HTTPS non-local site")
            if not real_grocery_safety_contract_ok(report):
                errors.append("real grocery evidence must include the cart-draft safety contract")
            if not real_grocery_plan_assertions_ok(report):
                errors.append(
                    "real grocery evidence must include passed checkout-blocked plan assertions"
                )
            if not real_grocery_workspace_browser_control_ok(report):
                errors.append(
                    "real grocery evidence must prove workspace-owned browser target discovery and page snapshot through loopback DevTools"
                )
            page_snapshot = (
                ((real_browser.get("chrome_devtools") or {}).get("page_snapshot") or {})
                if isinstance(real_browser, dict)
                else {}
            )
            if not real_grocery_snapshot_privacy_ok(page_snapshot):
                errors.append(
                    "real grocery evidence must omit raw logged-in page text from release artifacts"
                )
            if not real_grocery_cart_draft_interaction_ok(report):
                errors.append(
                    "real grocery evidence must include a passed approved cart-draft interaction"
                )
            if not real_grocery_cart_draft_steps_manifest_ok(report):
                errors.append(
                    "real grocery evidence must bind cart-draft interaction to the approved step-file hash"
                )
            if not real_grocery_workspace_input_audit_ok(report):
                errors.append(
                    "real grocery evidence must prove only declared cart-draft workspace input events"
                )
            if not real_grocery_workspace_cleanup_ok(report):
                errors.append(
                    "real grocery evidence must prove stopped workspace runtime cleanup"
                )
    if kind == "github_explore_dogfood" and not allow_nonpassing:
        if not github_explore_dogfood_contract_ok(report):
            errors.append(
                "GitHub Explore evidence must prove visible workspace-owned browser discovery, workspace_open_viewer metadata, three recommendations, and clean stop"
            )
    if kind == "human_final_diff_review":
        if not allow_source_mismatch and not review_scope_matches(report, current_review_scope):
            errors.append(
                f"review scope identity does not match current review scope hash {current_review_scope.get('review_scope_hash')}"
            )
        if not allow_nonpassing:
            if report.get("status") != "reviewed":
                errors.append("human review marker status must be reviewed")
            if parse_timestamp(report.get("reviewed_at_utc")) is None:
                errors.append("human review marker reviewed_at_utc must be a timestamp")
            if not str(report.get("reviewer") or "").strip():
                errors.append("human review marker reviewer must be non-empty")
            if not human_review_metadata_ok(report):
                errors.append(
                    "human review marker reviewer and notes must be meaningful and not placeholders"
                )
    return errors


def import_reports(
    paths: list[Path],
    *,
    viewer_dir: Path,
    app_qa_dir: Path,
    grocery_dir: Path,
    github_explore_dir: Path,
    desktop_repo: Path,
    dry_run: bool,
    allow_source_mismatch: bool,
    allow_nonpassing: bool,
    human_review_marker: Path = DEFAULT_HUMAN_REVIEW_MARKER,
    human_review_artifact_dir: Path = DEFAULT_HUMAN_REVIEW_ARTIFACT_DIR,
) -> dict[str, Any]:
    current_source = compute_source_identity(ROOT, desktop_repo=desktop_repo)
    current_review_scope = compute_review_scope_identity(ROOT, desktop_repo=desktop_repo)
    results = []
    for source_path in collect_json_paths(paths):
        result: dict[str, Any] = {
            "source_path": str(source_path),
            "status": "rejected",
            "errors": [],
        }
        report = read_json(source_path)
        if report is None:
            result["errors"] = ["not a JSON object or unreadable JSON"]
            results.append(result)
            continue
        kind = report_kind(report)
        result["kind"] = kind
        errors = validate_report(
            report,
            current_source=current_source,
            current_review_scope=current_review_scope,
            allow_source_mismatch=allow_source_mismatch,
            allow_nonpassing=allow_nonpassing,
        )
        artifact_copies: list[tuple[Path, Path]] = []
        import_report = report
        if not errors and kind == "human_final_diff_review":
            artifact_errors, import_report, artifact_copies = prepare_human_review_marker_import(
                report,
                source_path,
                artifact_dir=human_review_artifact_dir,
            )
            errors.extend(artifact_errors)
        if errors:
            result["errors"] = errors
            results.append(result)
            continue
        assert kind is not None
        if (
            kind == "human_final_diff_review"
            and not dry_run
            and (allow_source_mismatch or allow_nonpassing)
        ):
            result["errors"] = [
                "diagnostic human review marker imports with --allow-source-mismatch or --allow-nonpassing must use --dry-run"
            ]
            results.append(result)
            continue
        if kind == "human_final_diff_review":
            dest_path = human_review_marker
        else:
            dest_dir = destination_dir(
                kind,
                viewer_dir=viewer_dir,
                app_qa_dir=app_qa_dir,
                grocery_dir=grocery_dir,
                github_explore_dir=github_explore_dir,
            )
            dest_path = destination_path(source_path, import_report, kind, dest_dir)
        serialized = json.dumps(import_report, indent=2, sort_keys=True) + "\n"
        result.update(
            {
                "status": "dry-run" if dry_run else "imported",
                "destination_path": str(dest_path),
                "source_identity": import_report.get("source_identity"),
            }
        )
        if not dry_run:
            dest_path.parent.mkdir(parents=True, exist_ok=True)
            if dest_path.exists():
                existing_digest = hashlib.sha256(dest_path.read_bytes()).hexdigest()
                new_digest = hashlib.sha256(serialized.encode("utf-8")).hexdigest()
                if existing_digest == new_digest:
                    result["status"] = "already-present"
                else:
                    result["errors"] = [f"destination already exists with different content: {dest_path}"]
                    result["status"] = "rejected"
            if result["status"] in {"imported", "already-present"} and kind == "human_final_diff_review":
                human_review_artifact_dir.mkdir(parents=True, exist_ok=True)
                for src, dest in artifact_copies:
                    if not dest.exists():
                        shutil.copy2(src, dest)
            if result["status"] == "imported":
                dest_path.write_text(
                    serialized,
                    encoding="utf-8",
                )
        results.append(result)

    return {
        "schema": "agent-workspace-linux.release_evidence_import.v1",
        "current_source_identity": current_source,
        "current_review_scope_identity": current_review_scope,
        "dry_run": dry_run,
        "allow_source_mismatch": allow_source_mismatch,
        "allow_nonpassing": allow_nonpassing,
        "results": results,
        "accepted": len([item for item in results if item["status"] in {"dry-run", "imported", "already-present"}]),
        "rejected": len([item for item in results if item["status"] == "rejected"]),
    }


def assert_self_test(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(f"release evidence import self-test failed: {message}")


def copy_identity_sources(src_root: Path, dest_root: Path, source_paths: list[str]) -> None:
    for rel_path in source_file_paths(src_root, source_paths):
        src = src_root / rel_path
        dest = dest_root / rel_path
        if src.is_file():
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(src.read_bytes())


def run_self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="agent-workspace-evidence-import-") as temp:
        root = Path(temp)
        viewer_dir = root / "viewer"
        app_qa_dir = root / "app-qa"
        grocery_dir = root / "grocery"
        github_explore_dir = root / "github-explore"
        original_import_reports = globals()["import_reports"]

        def import_reports(paths: list[Path], **kwargs: Any) -> dict[str, Any]:
            return original_import_reports(
                paths,
                github_explore_dir=github_explore_dir,
                **kwargs,
            )
        current_source = compute_source_identity(ROOT)
        evidence_boundary = {
            "collector": "agent-workspace-linux",
            "collector_script": "self-test",
            "repo_owned_runtime": True,
            "codex_app_mcp_used": False,
            "computer_use_mcp_used": False,
            "codex_desktop_bridge_used": False,
            "playwright_mcp_used": False,
        }
        viewer_report = {
            "schema": VIEWER_SCHEMA,
            "created_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
            "source_identity": current_source,
            "evidence_boundary": evidence_boundary,
            "session": {
                "xdg_current_desktop": "GNOME",
                "desktop_session": "gnome",
                "xdg_session_type": "x11",
            },
            "viewer_smoke": {"status": "passed"},
            "matrix_result": {
                "counts_for_release_matrix": True,
                "desktop_label": "GNOME / x11",
                "display_attestation": {
                    "release_eligible": True,
                    "problems": [],
                    "warnings": [],
                    "display_protocols": ["x11"],
                    "sockets": [
                        {
                            "kind": "x11",
                            "path": "/tmp/.X11-unix/X0",
                            "exists": True,
                            "processes": [
                                {"command": "Xorg", "pid": 100, "args": "/usr/lib/Xorg :0"}
                            ],
                        }
                    ],
                    "known_nested_or_headless_processes": [],
                    "lsof_available": True,
                },
            },
        }
        viewer_path = root / "viewer-row.json"
        viewer_path.write_text(json.dumps(viewer_report), encoding="utf-8")

        imported = import_reports(
            [viewer_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=False,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(imported["accepted"] == 1, "passing viewer row should import")
        assert_self_test(len(list(viewer_dir.glob("*.json"))) == 1, "viewer row should be copied")
        viewer_without_boundary = json.loads(json.dumps(viewer_report))
        viewer_without_boundary.pop("evidence_boundary")
        viewer_without_boundary_path = root / "viewer-row-without-boundary.json"
        viewer_without_boundary_path.write_text(
            json.dumps(viewer_without_boundary),
            encoding="utf-8",
        )
        missing_boundary = import_reports(
            [viewer_without_boundary_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            missing_boundary["rejected"] == 1,
            "viewer evidence without repo-owned runtime boundary should reject",
        )
        viewer_without_playwright_boundary = json.loads(json.dumps(viewer_report))
        viewer_without_playwright_boundary["evidence_boundary"].pop("playwright_mcp_used")
        viewer_without_playwright_path = root / "viewer-row-without-playwright-boundary.json"
        viewer_without_playwright_path.write_text(
            json.dumps(viewer_without_playwright_boundary),
            encoding="utf-8",
        )
        missing_playwright_boundary = import_reports(
            [viewer_without_playwright_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            missing_playwright_boundary["rejected"] == 1,
            "viewer evidence without explicit Playwright MCP boundary should reject",
        )

        app_qa_report = {
            "schema": APP_QA_SCHEMA,
            "created_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
            "source_identity": current_source,
            "evidence_boundary": evidence_boundary,
            "mode": "local-gui-app",
            "status": "passed",
            "inputs": {
                "task_intent": "app_qa",
                "target_app": "xmessage",
                "real_world_action_approved": False,
            },
            "safety_contract": {
                "hidden_workspace_acknowledged": True,
                "app_qa_only": True,
                "host_desktop_input_targeted": False,
                "real_world_or_account_mutation": False,
                "non_destructive_input_only": True,
            },
            "workspace": {
                "status": "passed",
                "launch_ok": True,
                "launch_window_count": 1,
                "launch_screenshot_bytes": 1000,
                "observe_screenshot_bytes": 1000,
                "active_window_title": "App QA Dogfood Target",
                "event_count": 1,
                "logs_ok": True,
                "event_log_artifact_present": True,
                "stopped_by_workspace_stop": True,
                "stop_ok": True,
            },
        }
        app_qa_path = root / "app-qa-row.json"
        app_qa_path.write_text(json.dumps(app_qa_report), encoding="utf-8")
        app_qa_imported = import_reports(
            [app_qa_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=False,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(app_qa_imported["accepted"] == 1, "passing app-QA row should import")
        assert_self_test(len(list(app_qa_dir.glob("*.json"))) == 1, "app-QA row should be copied")
        unsafe_app_qa = dict(app_qa_report)
        unsafe_app_qa["safety_contract"] = {
            **app_qa_report["safety_contract"],
            "host_desktop_input_targeted": True,
        }
        unsafe_app_qa_path = root / "unsafe-app-qa-row.json"
        unsafe_app_qa_path.write_text(json.dumps(unsafe_app_qa), encoding="utf-8")
        unsafe_app_qa_rejected = import_reports(
            [unsafe_app_qa_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            unsafe_app_qa_rejected["rejected"] == 1,
            "app-QA row with host desktop mutation drift should be rejected",
        )

        extracted_root = root / "extracted-bundle"
        extracted_runtime = extracted_root / "agent-workspace-linux"
        extracted_desktop = extracted_root / "codex-desktop-linux"
        copy_identity_sources(ROOT, extracted_runtime, RUNTIME_SOURCE_IDENTITY_PATHS)
        copy_identity_sources(DEFAULT_DESKTOP_REPO, extracted_desktop, DESKTOP_SOURCE_IDENTITY_PATHS)
        extracted_identity = compute_source_identity(
            extracted_runtime,
            desktop_repo=extracted_desktop,
        )
        assert_self_test(
            not source_identity_matches({"source_identity": extracted_identity}, current_source),
            "recomputing source identity from an extracted no-git bundle should differ",
        )

        manifest_stamped_report = dict(viewer_report)
        manifest_stamped_path = root / "manifest-stamped-bundle-row.json"
        manifest_stamped_path.write_text(json.dumps(manifest_stamped_report), encoding="utf-8")
        manifest_stamped = import_reports(
            [manifest_stamped_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            manifest_stamped["accepted"] == 1,
            "manifest-stamped external bundle viewer row should import",
        )

        no_manifest_report = dict(viewer_report)
        no_manifest_report["source_identity"] = extracted_identity
        no_manifest_path = root / "no-manifest-recomputed-bundle-row.json"
        no_manifest_path.write_text(json.dumps(no_manifest_report), encoding="utf-8")
        no_manifest = import_reports(
            [no_manifest_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            no_manifest["rejected"] == 1,
            "external bundle viewer row with no-git recomputed source identity should reject",
        )

        stale_report = dict(viewer_report)
        stale_report["source_identity"] = {**current_source, "source_hash": "wrong"}
        stale_path = root / "stale-row.json"
        stale_path.write_text(json.dumps(stale_report), encoding="utf-8")
        rejected = import_reports(
            [stale_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(rejected["rejected"] == 1, "source mismatch should reject by default")

        skipped_report = dict(viewer_report)
        skipped_report["viewer_smoke"] = {"status": "skipped"}
        skipped_path = root / "skipped-row.json"
        skipped_path.write_text(json.dumps(skipped_report), encoding="utf-8")
        skipped = import_reports(
            [skipped_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(skipped["rejected"] == 1, "nonpassing evidence should reject by default")

        spoofed_report = dict(viewer_report)
        spoofed_report["matrix_result"] = {
            "counts_for_release_matrix": True,
            "session_consistency": {
                "release_eligible": False,
                "problems": ["XDG_SESSION_TYPE='x11' conflicts with loginctl Type='wayland'"],
            },
        }
        spoofed_path = root / "spoofed-session-row.json"
        spoofed_path.write_text(json.dumps(spoofed_report), encoding="utf-8")
        spoofed = import_reports(
            [spoofed_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            spoofed["rejected"] == 1,
            "viewer evidence with contradictory session attestation should reject",
        )

        nested_display_report = json.loads(json.dumps(viewer_report))
        nested_display_report["matrix_result"]["display_attestation"] = {
            "release_eligible": False,
            "problems": ["display server appears nested or headless"],
            "warnings": [],
            "display_protocols": ["x11"],
            "sockets": [
                {
                    "kind": "x11",
                    "path": "/tmp/.X11-unix/X99",
                    "exists": True,
                    "processes": [{"command": "Xvfb", "pid": 999, "args": "Xvfb :99"}],
                }
            ],
            "known_nested_or_headless_processes": [
                {"command": "Xvfb", "pid": 999, "args": "Xvfb :99"}
            ],
            "lsof_available": True,
        }
        nested_display_path = root / "nested-display-row.json"
        nested_display_path.write_text(json.dumps(nested_display_report), encoding="utf-8")
        nested_display = import_reports(
            [nested_display_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            nested_display["rejected"] == 1,
            "viewer evidence from nested/headless display servers should reject",
        )

        weak_display_report = json.loads(json.dumps(viewer_report))
        weak_display_report["matrix_result"]["display_attestation"] = {
            "release_eligible": True,
            "problems": [],
            "warnings": [],
            "display_protocols": ["x11"],
            "sockets": [],
            "known_nested_or_headless_processes": [],
            "lsof_available": True,
        }
        weak_display_path = root / "weak-display-row.json"
        weak_display_path.write_text(json.dumps(weak_display_report), encoding="utf-8")
        weak_display = import_reports(
            [weak_display_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            weak_display["rejected"] == 1,
            "viewer evidence without display socket/process proof should reject",
        )

        invalid_native_report = dict(viewer_report)
        invalid_native_report["session"] = {
            "xdg_current_desktop": "GNOME",
            "desktop_session": "gnome",
            "xdg_session_type": "wayland",
        }
        invalid_native_report["matrix_result"] = {
            "counts_for_release_matrix": True,
            "desktop_label": "GNOME / wayland",
            "native_wayland_layer_shell_observed": True,
            "native_wayland_layer_shell_notes": "Observed a normal Xwayland toplevel, not layer-shell.",
        }
        invalid_native_path = root / "invalid-native-wayland-row.json"
        invalid_native_path.write_text(json.dumps(invalid_native_report), encoding="utf-8")
        invalid_native = import_reports(
            [invalid_native_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            invalid_native["rejected"] == 1,
            "viewer evidence with GNOME/Xwayland native Wayland notes should reject",
        )

        cart_draft_steps_path = "/tmp/release-gate-self-test-cart-draft-steps.json"
        cart_draft_steps_sha256 = "0" * 64
        cart_draft_steps_size_bytes = 512
        cart_draft_step_summaries = [
            {
                "index": 1,
                "action": "key_window",
                "safety_label": "Focus the grocery site search field.",
                "cart_mutation": False,
                "text_bytes": None,
            },
            {
                "index": 2,
                "action": "paste_window",
                "safety_label": "Enter only the approved shopping-list text.",
                "cart_mutation": False,
                "text_bytes": 29,
            },
            {
                "index": 3,
                "action": "key_window",
                "safety_label": "Confirm the approved draft-cart action only.",
                "cart_mutation": True,
                "text_bytes": None,
            },
            {
                "index": 4,
                "action": "observe",
                "safety_label": "Collect evidence that the cart draft is visible.",
                "cart_mutation": False,
                "text_bytes": None,
            },
        ]
        grocery_report = {
            "schema": GROCERY_SCHEMA,
            "created_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
            "source_identity": current_source,
            "evidence_boundary": evidence_boundary,
            "mode": "real-browser",
            "inputs": {
                "target_url": "https://grocery-release-gate.example-retailer.com",
                "profile_directory": "Profile 1",
                "checkout_or_real_world_approved_env": False,
                "profile_is_disposable_copy_env": True,
                "cart_mutation_approved_env": True,
                "final_cart_reviewed_env": True,
                "real_browser_interaction_mode": "cart-draft-approved",
                "cart_draft_steps_path": cart_draft_steps_path,
            },
            "safety_contract": {
                "refuses_checkout_or_real_world_approval": True,
                "real_browser_requires_disposable_profile_copy": True,
                "real_browser_interaction_mode": "cart-draft-approved",
                "cart_draft_requires_explicit_approval": True,
                "checkout_order_or_account_change_blocked": True,
                "real_browser_sends_no_workspace_input": False,
                "real_browser_observes_only": False,
                "real_browser_allows_only_declared_cart_draft_input": True,
                "real_browser_cleans_workspace_runtime": True,
            },
            "plan_assertions": {
                "status": "passed",
                "unapproved_next_boundary": {"kind": "approval"},
                "cart_only_next_boundary": {"kind": "approval"},
                "checkout_still_blocked_after_cart_approval": True,
            },
            "real_browser": {
                "status": "passed",
                "interaction_mode": "cart-draft-approved",
                "workspace_id": "release-gate-self-test",
                "browser_app_id": "app-release-grocery",
                "profile_directory": "Profile 1",
                "checkout_approval_refused": True,
                "profile_copy_manifest_valid": True,
                "profile_copy_manifest": {
                    "path": "/tmp/profile-copy-manifest.json",
                    "profile_directory": "Profile 1",
                },
                "cart_draft_steps": {
                    "path": cart_draft_steps_path,
                    "sha256": cart_draft_steps_sha256,
                    "size_bytes": cart_draft_steps_size_bytes,
                    "step_count": 4,
                    "input_step_count": 3,
                    "cart_mutation_step_count": 1,
                    "summaries": cart_draft_step_summaries,
                },
                "cart_draft_interaction": {
                    "status": "passed",
                    "mode": "cart-draft-approved",
                    "steps_path": cart_draft_steps_path,
                    "steps_sha256": cart_draft_steps_sha256,
                    "steps_size_bytes": cart_draft_steps_size_bytes,
                    "step_count": 4,
                    "input_step_count": 3,
                    "cart_mutation_step_count": 1,
                    "forbidden_step_count": 0,
                    "cart_mutation_approval_confirmed": True,
                    "final_cart_reviewed_confirmed": True,
                    "checkout_or_real_world_approval_refused": True,
                    "executed_steps": [
                        {
                            "index": 1,
                            "action": "key_window",
                            "safety_label": "Focus the grocery site search field.",
                            "cart_mutation": False,
                            "text_bytes": None,
                            "result": {"ok": True},
                        },
                        {
                            "index": 2,
                            "action": "paste_window",
                            "safety_label": "Enter only the approved shopping-list text.",
                            "cart_mutation": False,
                            "text_bytes": 29,
                            "result": {"ok": True},
                        },
                        {
                            "index": 3,
                            "action": "key_window",
                            "safety_label": "Confirm the approved draft-cart action only.",
                            "cart_mutation": True,
                            "text_bytes": None,
                            "result": {"ok": True},
                        },
                        {
                            "index": 4,
                            "action": "observe",
                            "safety_label": "Collect evidence that the cart draft is visible.",
                            "cart_mutation": False,
                            "text_bytes": None,
                            "result": {"ok": True},
                        },
                    ],
                },
                "workspace_input_audit": {
                    "checked": True,
                    "event_scope": "since_sequence",
                    "events_since_sequence": 9,
                    "events_tail_requested": 120,
                    "minimum_events_tail_required": 120,
                    "total_events": 3,
                    "expected_input_step_count": 3,
                    "input_event_count": 3,
                    "input_event_count_covers_expected": True,
                    "input_event_kinds": ["key_window", "paste_window"],
                    "input_event_sequences": [10, 11, 12],
                    "allowed_input_event_kinds": [
                        "click_window",
                        "key_window",
                        "paste_window",
                        "scroll_window",
                        "type_window",
                    ],
                    "unexpected_input_event_count": 0,
                    "unexpected_input_event_kinds": [],
                    "unexpected_input_event_sequences": [],
                },
                "chrome_devtools": {
                    "status": "passed",
                    "control_surface": "workspace_chrome_devtools",
                    "workspace_owned_browser": True,
                    "host_chrome_bridge_used": False,
                    "coordinate_input_used": False,
                    "endpoint": "http://127.0.0.1:42222",
                    "target_count": 1,
                    "target": {
                        "id": "page-release-grocery",
                        "type": "page",
                        "title": "Release Grocery",
                        "url": "https://grocery-release-gate.example-retailer.com/cart",
                    },
                    "workspace_browser_targets": {
                        "ok": True,
                        "message": "workspace browser targets returned for app app-release-grocery through workspace-owned Chrome DevTools",
                        "app_id": "app-release-grocery",
                        "app_pid": 4242,
                        "workspace_user_data_dir": "/tmp/grocery-profile-copy",
                        "host_user_data_dir": "/tmp/grocery-profile-copy",
                        "devtools_endpoint": "http://127.0.0.1:42222",
                        "devtools_active_port_path": "/tmp/grocery-profile-copy/DevToolsActivePort",
                        "target_count": 1,
                        "selected_page_target": {
                            "id": "page-release-grocery",
                            "type": "page",
                            "title": "Release Grocery",
                            "url": "https://grocery-release-gate.example-retailer.com/cart",
                            "webSocketDebuggerUrl": "ws://127.0.0.1:42222/devtools/page/page-release-grocery",
                        },
                        "warnings": [],
                    },
                    "workspace_browser_snapshot": {
                        "ok": True,
                        "message": "workspace browser page snapshot captured through workspace-owned Chrome DevTools",
                        "app_id": "app-release-grocery",
                        "target_id": "page-release-grocery",
                        "page_url": "https://grocery-release-gate.example-retailer.com/cart",
                        "page_title": "Release Grocery",
                        "text_chars": 128,
                        "text_truncated": False,
                        "warnings": [],
                    },
                    "page_snapshot": {
                        "source": "workspace_browser_snapshot",
                        "title": "Release Grocery",
                        "url": "https://grocery-release-gate.example-retailer.com/cart",
                        "text_chars": 128,
                        "text_truncated": False,
                        "raw_text_omitted": True,
                        "raw_text_omission_reason": "release evidence avoids storing logged-in grocery page text",
                    },
                },
                "cleanup": {
                    "dry_run": False,
                    "removed": [{"id": "release-gate-self-test"}],
                    "skipped": [],
                },
            },
        }
        grocery_path = root / "real-grocery.json"
        grocery_path.write_text(json.dumps(grocery_report), encoding="utf-8")
        grocery_imported = import_reports(
            [grocery_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=False,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(grocery_imported["accepted"] == 1, "passing grocery row should import")

        missing_browser_targets = json.loads(json.dumps(grocery_report))
        missing_browser_targets["real_browser"].pop("chrome_devtools")
        missing_browser_targets_path = root / "missing-browser-targets-grocery.json"
        missing_browser_targets_path.write_text(
            json.dumps(missing_browser_targets),
            encoding="utf-8",
        )
        missing_browser_targets_rejected = import_reports(
            [missing_browser_targets_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            missing_browser_targets_rejected["rejected"] == 1,
            "grocery evidence without workspace browser target proof should reject",
        )

        privacy_leak = json.loads(json.dumps(grocery_report))
        privacy_leak["real_browser"]["chrome_devtools"]["page_snapshot"][
            "text_excerpt"
        ] = "Private address 123 Main Street"
        privacy_leak_path = root / "privacy-leak-grocery.json"
        privacy_leak_path.write_text(json.dumps(privacy_leak), encoding="utf-8")
        privacy_leak_rejected = import_reports(
            [privacy_leak_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            privacy_leak_rejected["rejected"] == 1,
            "grocery evidence with raw logged-in page text should reject",
        )

        mismatched_profile = json.loads(json.dumps(grocery_report))
        mismatched_profile["real_browser"]["profile_directory"] = "Default"
        mismatched_profile_path = root / "mismatched-profile-grocery.json"
        mismatched_profile_path.write_text(json.dumps(mismatched_profile), encoding="utf-8")
        mismatched_profile_rejected = import_reports(
            [mismatched_profile_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            mismatched_profile_rejected["rejected"] == 1,
            "grocery evidence with mismatched Chrome profile directory should reject",
        )

        unsafe_profile = json.loads(json.dumps(grocery_report))
        unsafe_profile["inputs"]["profile_directory"] = "../Profile 1"
        unsafe_profile_path = root / "unsafe-profile-grocery.json"
        unsafe_profile_path.write_text(json.dumps(unsafe_profile), encoding="utf-8")
        unsafe_profile_rejected = import_reports(
            [unsafe_profile_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            unsafe_profile_rejected["rejected"] == 1,
            "grocery evidence with unsafe Chrome profile directory should reject",
        )

        missing_steps = json.loads(json.dumps(grocery_report))
        missing_steps["real_browser"]["cart_draft_interaction"]["executed_steps"] = []
        missing_steps_path = root / "missing-steps-grocery.json"
        missing_steps_path.write_text(json.dumps(missing_steps), encoding="utf-8")
        missing_steps_rejected = import_reports(
            [missing_steps_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            missing_steps_rejected["rejected"] == 1,
            "grocery evidence without executed cart-draft steps should reject",
        )

        forbidden_steps = json.loads(json.dumps(grocery_report))
        forbidden_steps["real_browser"]["cart_draft_interaction"]["executed_steps"][2][
            "safety_label"
        ] = "Click checkout to place order"
        forbidden_steps_path = root / "forbidden-steps-grocery.json"
        forbidden_steps_path.write_text(json.dumps(forbidden_steps), encoding="utf-8")
        forbidden_steps_rejected = import_reports(
            [forbidden_steps_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            forbidden_steps_rejected["rejected"] == 1,
            "grocery evidence with checkout/payment/account step labels should reject",
        )

        failed_step = json.loads(json.dumps(grocery_report))
        failed_step["real_browser"]["cart_draft_interaction"]["executed_steps"][1][
            "result"
        ] = {"ok": False, "message": "input failed"}
        failed_step_path = root / "failed-step-grocery.json"
        failed_step_path.write_text(json.dumps(failed_step), encoding="utf-8")
        failed_step_rejected = import_reports(
            [failed_step_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            failed_step_rejected["rejected"] == 1,
            "grocery evidence with a failed executed cart-draft step should reject",
        )

        unsafe_grocery = dict(grocery_report)
        unsafe_grocery["safety_contract"] = {
            **grocery_report["safety_contract"],
            "real_browser_allows_only_declared_cart_draft_input": False,
        }
        unsafe_path = root / "unsafe-grocery.json"
        unsafe_path.write_text(json.dumps(unsafe_grocery), encoding="utf-8")
        unsafe_rejected = import_reports(
            [unsafe_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            unsafe_rejected["rejected"] == 1,
            "grocery evidence without cart-draft safety contract should reject",
        )

        weak_plan = dict(grocery_report)
        weak_plan["plan_assertions"] = {
            **grocery_report["plan_assertions"],
            "checkout_still_blocked_after_cart_approval": False,
        }
        weak_plan_path = root / "weak-plan-grocery.json"
        weak_plan_path.write_text(json.dumps(weak_plan), encoding="utf-8")
        weak_plan_rejected = import_reports(
            [weak_plan_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            weak_plan_rejected["rejected"] == 1,
            "grocery evidence without checkout-blocked plan assertions should reject",
        )

        input_events = dict(grocery_report)
        input_events["real_browser"] = {
            **grocery_report["real_browser"],
            "workspace_input_audit": {
                "checked": True,
                "event_scope": "since_sequence",
                "events_since_sequence": 6,
                "events_tail_requested": 120,
                "minimum_events_tail_required": 120,
                "total_events": 4,
                "expected_input_step_count": 3,
                "input_event_count": 4,
                "input_event_count_covers_expected": True,
                "input_event_kinds": ["kill_app", "paste_window"],
                "input_event_sequences": [7, 8, 9, 10],
                "allowed_input_event_kinds": [
                    "click_window",
                    "key_window",
                    "paste_window",
                    "scroll_window",
                    "type_window",
                ],
                "unexpected_input_event_count": 1,
                "unexpected_input_event_kinds": ["kill_app"],
                "unexpected_input_event_sequences": [10],
            },
        }
        input_events_path = root / "input-events-grocery.json"
        input_events_path.write_text(json.dumps(input_events), encoding="utf-8")
        input_events_rejected = import_reports(
            [input_events_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            input_events_rejected["rejected"] == 1,
            "grocery evidence with unexpected workspace input events should reject",
        )

        missing_input_events = dict(grocery_report)
        missing_input_events["real_browser"] = {
            **grocery_report["real_browser"],
            "workspace_input_audit": {
                **grocery_report["real_browser"]["workspace_input_audit"],
                "input_event_count": 2,
                "input_event_count_covers_expected": False,
                "input_event_sequences": [10, 11],
            },
        }
        missing_input_events_path = root / "missing-input-events-grocery.json"
        missing_input_events_path.write_text(json.dumps(missing_input_events), encoding="utf-8")
        missing_input_events_rejected = import_reports(
            [missing_input_events_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            missing_input_events_rejected["rejected"] == 1,
            "grocery evidence missing declared input events should reject",
        )

        shallow_tail = dict(grocery_report)
        shallow_tail["real_browser"] = {
            **grocery_report["real_browser"],
            "workspace_input_audit": {
                **grocery_report["real_browser"]["workspace_input_audit"],
                "events_tail_requested": 30,
            },
        }
        shallow_tail_path = root / "shallow-tail-grocery.json"
        shallow_tail_path.write_text(json.dumps(shallow_tail), encoding="utf-8")
        shallow_tail_rejected = import_reports(
            [shallow_tail_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            shallow_tail_rejected["rejected"] == 1,
            "grocery evidence with too-shallow event tail should reject",
        )

        missing_cleanup = json.loads(json.dumps(grocery_report))
        missing_cleanup["real_browser"]["cleanup"] = {
            "dry_run": False,
            "removed": [],
            "skipped": [],
        }
        missing_cleanup_path = root / "missing-cleanup-grocery.json"
        missing_cleanup_path.write_text(json.dumps(missing_cleanup), encoding="utf-8")
        missing_cleanup_rejected = import_reports(
            [missing_cleanup_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
        )
        assert_self_test(
            missing_cleanup_rejected["rejected"] == 1,
            "grocery evidence without workspace runtime cleanup should reject",
        )

        human_marker_path = root / "release-gate-human-review.json"
        human_artifact_dir = root / "review-artifacts"
        runtime_artifact = root / "runtime-review.diff"
        desktop_artifact = root / "codex-desktop-review.diff"
        runtime_artifact.write_text("runtime diff reviewed\n", encoding="utf-8")
        desktop_artifact.write_text("desktop diff reviewed\n", encoding="utf-8")
        current_review_scope = compute_review_scope_identity(
            ROOT,
            desktop_repo=DEFAULT_DESKTOP_REPO,
        )
        human_marker = {
            "schema": HUMAN_REVIEW_SCHEMA,
            "status": "reviewed",
            "reviewed_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
            "reviewer": "release-import-self-test",
            "notes": "Reviewed runtime and Desktop diffs.",
            "source_identity": current_source,
            "review_scope_identity": current_review_scope,
            "review_artifacts": [
                {
                    "label": "runtime",
                    "path": str(runtime_artifact),
                    "sha256": file_sha256(runtime_artifact),
                    "size_bytes": runtime_artifact.stat().st_size,
                },
                {
                    "label": "codex_desktop",
                    "path": str(desktop_artifact),
                    "sha256": file_sha256(desktop_artifact),
                    "size_bytes": desktop_artifact.stat().st_size,
                },
            ],
        }
        human_marker_source = root / "external-human-review-marker.json"
        human_marker_source.write_text(json.dumps(human_marker), encoding="utf-8")
        human_imported = import_reports(
            [human_marker_source],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=False,
            allow_source_mismatch=False,
            allow_nonpassing=False,
            human_review_marker=human_marker_path,
            human_review_artifact_dir=human_artifact_dir,
        )
        assert_self_test(human_imported["accepted"] == 1, "passing human marker should import")
        imported_marker = read_json(human_marker_path) or {}
        imported_artifact_paths = [
            Path(str(artifact.get("path")))
            for artifact in imported_marker.get("review_artifacts") or []
            if isinstance(artifact, dict)
        ]
        assert_self_test(
            all(path.parent == human_artifact_dir for path in imported_artifact_paths),
            "human marker import should rewrite artifact paths to the local artifact dir",
        )
        human_gate = audit_human_review(
            human_marker_path,
            now=dt.datetime.now(dt.timezone.utc),
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=current_source,
            review_scope_identity=current_review_scope,
        )
        assert_self_test(
            human_gate["status"] == "passed",
            "imported human marker should pass the release audit",
        )
        mismatched_human_marker = json.loads(json.dumps(human_marker))
        mismatched_human_marker["review_scope_identity"]["review_scope_hash"] = "wrong"
        mismatched_human_marker_path = root / "mismatched-human-review-marker.json"
        mismatched_human_marker_path.write_text(
            json.dumps(mismatched_human_marker),
            encoding="utf-8",
        )
        mismatched_human_rejected = import_reports(
            [mismatched_human_marker_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
            human_review_marker=root / "unused-human-review-marker.json",
            human_review_artifact_dir=root / "unused-review-artifacts",
        )
        assert_self_test(
            mismatched_human_rejected["rejected"] == 1,
            "human marker with mismatched review scope should reject",
        )
        diagnostic_human_marker = json.loads(json.dumps(human_marker))
        diagnostic_human_marker["status"] = "diagnostic"
        diagnostic_human_marker_path = root / "diagnostic-human-review-marker.json"
        diagnostic_human_marker_path.write_text(
            json.dumps(diagnostic_human_marker),
            encoding="utf-8",
        )
        diagnostic_human_dry_run = import_reports(
            [diagnostic_human_marker_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=True,
            human_review_marker=root / "diagnostic-human-review-marker-dry-run.json",
            human_review_artifact_dir=root / "diagnostic-review-artifacts-dry-run",
        )
        assert_self_test(
            diagnostic_human_dry_run["accepted"] == 1,
            "diagnostic human marker should be dry-runnable with allow flags",
        )
        diagnostic_human_rejected = import_reports(
            [diagnostic_human_marker_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=False,
            allow_source_mismatch=False,
            allow_nonpassing=True,
            human_review_marker=root / "diagnostic-human-review-marker-import.json",
            human_review_artifact_dir=root / "diagnostic-review-artifacts-import",
        )
        assert_self_test(
            diagnostic_human_rejected["rejected"] == 1
            and not (root / "diagnostic-human-review-marker-import.json").exists(),
            "diagnostic human marker imports with allow flags must not write a marker file",
        )
        placeholder_human_marker = json.loads(json.dumps(human_marker))
        placeholder_human_marker["notes"] = "<what was reviewed and accepted>"
        placeholder_human_marker_path = root / "placeholder-human-review-marker.json"
        placeholder_human_marker_path.write_text(
            json.dumps(placeholder_human_marker),
            encoding="utf-8",
        )
        placeholder_human_rejected = import_reports(
            [placeholder_human_marker_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
            human_review_marker=root / "unused-placeholder-human-review-marker.json",
            human_review_artifact_dir=root / "unused-placeholder-review-artifacts",
        )
        assert_self_test(
            placeholder_human_rejected["rejected"] == 1,
            "human marker with placeholder notes should reject",
        )
        missing_artifact_marker = json.loads(json.dumps(human_marker))
        missing_artifact_marker["review_artifacts"][0]["path"] = str(root / "missing-runtime.diff")
        missing_artifact_marker_path = root / "missing-artifact-human-review-marker.json"
        missing_artifact_marker_path.write_text(
            json.dumps(missing_artifact_marker),
            encoding="utf-8",
        )
        missing_artifact_rejected = import_reports(
            [missing_artifact_marker_path],
            viewer_dir=viewer_dir,
            app_qa_dir=app_qa_dir,
            grocery_dir=grocery_dir,
            desktop_repo=DEFAULT_DESKTOP_REPO,
            dry_run=True,
            allow_source_mismatch=False,
            allow_nonpassing=False,
            human_review_marker=root / "unused-human-review-marker.json",
            human_review_artifact_dir=root / "unused-review-artifacts",
        )
        assert_self_test(
            missing_artifact_rejected["rejected"] == 1,
            "human marker with missing reviewed artifact bytes should reject",
        )

    print("release evidence import self-test passed")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="*", type=Path, help="JSON files or directories to import")
    parser.add_argument("--viewer-dir", type=Path, default=DEFAULT_VIEWER_DIR)
    parser.add_argument("--app-qa-dir", type=Path, default=DEFAULT_APP_QA_DIR)
    parser.add_argument("--grocery-dir", type=Path, default=DEFAULT_GROCERY_DIR)
    parser.add_argument("--github-explore-dir", type=Path, default=DEFAULT_GITHUB_EXPLORE_DIR)
    parser.add_argument(
        "--human-review-marker",
        type=Path,
        default=DEFAULT_HUMAN_REVIEW_MARKER,
        help="destination for imported human final-diff review markers",
    )
    parser.add_argument(
        "--human-review-artifact-dir",
        type=Path,
        default=DEFAULT_HUMAN_REVIEW_ARTIFACT_DIR,
        help="directory where imported human review artifact bytes are copied",
    )
    parser.add_argument(
        "--desktop-repo",
        type=Path,
        default=DEFAULT_DESKTOP_REPO,
        help="sibling Codex Desktop repo whose agent-workspace feature source is part of release identity",
    )
    parser.add_argument("--dry-run", action="store_true", help="validate without copying reports")
    parser.add_argument(
        "--allow-source-mismatch",
        action="store_true",
        help="accept evidence from a different source hash; release audit will still reject it by default",
    )
    parser.add_argument(
        "--allow-nonpassing",
        action="store_true",
        help="import skipped/failed reports for diagnostics; release audit will not count them",
    )
    parser.add_argument("--self-test", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.self_test:
        run_self_test()
        return 0
    if not args.paths:
        raise SystemExit("provide at least one JSON report or directory, or use --self-test")
    result = import_reports(
        args.paths,
        viewer_dir=args.viewer_dir,
        app_qa_dir=args.app_qa_dir,
        grocery_dir=args.grocery_dir,
        github_explore_dir=args.github_explore_dir,
        desktop_repo=args.desktop_repo,
        dry_run=args.dry_run,
        allow_source_mismatch=args.allow_source_mismatch,
        allow_nonpassing=args.allow_nonpassing,
        human_review_marker=args.human_review_marker,
        human_review_artifact_dir=args.human_review_artifact_dir,
    )
    print(json.dumps(result, indent=2, sort_keys=True))
    return 1 if result["rejected"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
