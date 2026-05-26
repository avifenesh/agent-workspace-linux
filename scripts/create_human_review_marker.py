#!/usr/bin/env python3
"""Create the human final-diff review marker after real human review."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import sys
import tempfile
from pathlib import Path
from typing import Any

sys.dont_write_bytecode = True

from final_review_bundle import marker_template
from final_review_bundle import write_review_artifacts
from release_gate_audit import DEFAULT_DESKTOP_REPO
from release_gate_audit import DEFAULT_HUMAN_REVIEW_MARKER
from release_gate_audit import DEFAULT_MAX_EVIDENCE_AGE_DAYS
from release_gate_audit import audit_human_review
from release_gate_audit import compute_review_scope_identity
from release_gate_audit import compute_source_identity
from release_gate_audit import meaningful_human_review_text
from release_gate_audit import validate_bundle_manifest_source_contents


ROOT = Path(__file__).resolve().parent.parent
DEFAULT_ARTIFACT_DIR = ROOT / "target" / "final-review-bundle"


def assert_self_test(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(f"human review marker self-test failed: {message}")


def iso_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat()


def release_bundle_manifest() -> tuple[Path, dict[str, Any]] | None:
    manifest_path = os.environ.get("AGENT_WORKSPACE_RELEASE_BUNDLE_MANIFEST")
    if not manifest_path:
        return None
    path = Path(manifest_path)
    manifest = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(manifest, dict):
        raise RuntimeError(f"release bundle manifest is not a JSON object: {path}")
    return path, manifest


def current_identities(desktop_repo: Path) -> tuple[dict[str, Any], dict[str, Any]]:
    loaded_manifest = release_bundle_manifest()
    if loaded_manifest is not None:
        manifest_path, manifest = loaded_manifest
        validate_bundle_manifest_source_contents(manifest, root=ROOT, desktop_repo=desktop_repo)
        source_identity = manifest.get("source_identity")
        review_scope_identity = manifest.get("review_scope_identity")
        if not isinstance(source_identity, dict) or not source_identity.get("source_hash"):
            raise RuntimeError(
                f"release bundle manifest does not contain source_identity: {manifest_path}"
            )
        if (
            not isinstance(review_scope_identity, dict)
            or not review_scope_identity.get("review_scope_hash")
        ):
            raise RuntimeError(
                f"release bundle manifest does not contain review_scope_identity: {manifest_path}"
            )
        return source_identity, review_scope_identity
    return (
        compute_source_identity(ROOT, desktop_repo=desktop_repo),
        compute_review_scope_identity(ROOT, desktop_repo=desktop_repo),
    )


def build_marker(
    *,
    reviewer: str,
    notes: str,
    desktop_repo: Path,
    artifact_dir: Path,
    reviewed_at_utc: str | None = None,
) -> dict[str, Any]:
    artifact_dir.mkdir(parents=True, exist_ok=True)
    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    source_identity, review_scope_identity = current_identities(desktop_repo)
    review_artifacts = write_review_artifacts(
        artifact_dir,
        stamp,
        desktop_repo=desktop_repo,
    )
    marker = marker_template(source_identity, review_scope_identity, review_artifacts)
    marker["reviewed_at_utc"] = reviewed_at_utc or iso_now()
    marker["reviewer"] = reviewer
    marker["notes"] = notes
    return marker


def write_marker(
    marker: dict[str, Any],
    output_path: Path,
    *,
    replace: bool,
) -> None:
    if output_path.exists() and not replace:
        raise SystemExit(
            f"{output_path} already exists; pass --replace after reviewing the regenerated artifacts"
        )
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(marker, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def run_self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="agent-workspace-human-review-marker-") as temp:
        temp_path = Path(temp)
        artifact_dir = temp_path / "artifacts"
        marker_path = temp_path / "release-gate-human-review.json"
        marker = build_marker(
            reviewer="release-marker-self-test",
            notes="Self-test marker generated from current review artifacts.",
            desktop_repo=DEFAULT_DESKTOP_REPO,
            artifact_dir=artifact_dir,
            reviewed_at_utc=iso_now(),
        )
        write_marker(marker, marker_path, replace=False)
        source_identity, review_scope_identity = current_identities(DEFAULT_DESKTOP_REPO)
        gate = audit_human_review(
            marker_path,
            now=dt.datetime.now(dt.timezone.utc),
            max_evidence_age_days=DEFAULT_MAX_EVIDENCE_AGE_DAYS,
            source_identity=source_identity,
            review_scope_identity=review_scope_identity,
        )
        assert_self_test(gate["status"] == "passed", "generated marker should pass audit")
        assert_self_test(
            len(marker.get("review_artifacts") or []) == 2,
            "marker should include runtime and Desktop review artifacts",
        )
        try:
            write_marker(marker, marker_path, replace=False)
        except SystemExit:
            pass
        else:
            raise AssertionError("human review marker self-test failed: existing marker should require --replace")
        source_identity, review_scope_identity = current_identities(DEFAULT_DESKTOP_REPO)
        good_manifest_path = temp_path / "good-release-bundle-manifest.json"
        good_manifest_path.write_text(
            json.dumps(
                {
                    "source_identity": source_identity,
                    "review_scope_identity": review_scope_identity,
                }
            ),
            encoding="utf-8",
        )
        bad_source_identity = json.loads(json.dumps(source_identity))
        bad_source_identity["components"]["runtime"]["source_hash"] = "0" * 64
        bad_manifest_path = temp_path / "bad-release-bundle-manifest.json"
        bad_manifest_path.write_text(
            json.dumps(
                {
                    "source_identity": bad_source_identity,
                    "review_scope_identity": review_scope_identity,
                }
            ),
            encoding="utf-8",
        )
        previous_manifest = os.environ.get("AGENT_WORKSPACE_RELEASE_BUNDLE_MANIFEST")
        try:
            os.environ["AGENT_WORKSPACE_RELEASE_BUNDLE_MANIFEST"] = str(good_manifest_path)
            current_identities(DEFAULT_DESKTOP_REPO)
            os.environ["AGENT_WORKSPACE_RELEASE_BUNDLE_MANIFEST"] = str(bad_manifest_path)
            try:
                current_identities(DEFAULT_DESKTOP_REPO)
            except RuntimeError as error:
                assert_self_test(
                    "source bytes no longer match" in str(error),
                    "tampered manifest rejection should explain the byte mismatch",
                )
            else:
                raise AssertionError(
                    "human review marker self-test failed: tampered bundle source should reject"
                )
        finally:
            if previous_manifest is None:
                os.environ.pop("AGENT_WORKSPACE_RELEASE_BUNDLE_MANIFEST", None)
            else:
                os.environ["AGENT_WORKSPACE_RELEASE_BUNDLE_MANIFEST"] = previous_manifest
    print("human review marker self-test passed")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--reviewer",
        help="human reviewer name recorded in the marker",
    )
    parser.add_argument(
        "--notes",
        help="scope, concerns, or approval notes recorded in the marker",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=DEFAULT_HUMAN_REVIEW_MARKER,
        help="marker path to write",
    )
    parser.add_argument(
        "--artifact-dir",
        type=Path,
        default=DEFAULT_ARTIFACT_DIR,
        help="directory for generated runtime/Desktop review artifacts",
    )
    parser.add_argument(
        "--desktop-repo",
        type=Path,
        default=DEFAULT_DESKTOP_REPO,
        help="sibling Codex Desktop repo included in the review scope",
    )
    parser.add_argument(
        "--replace",
        action="store_true",
        help="replace an existing marker after reviewing regenerated artifacts",
    )
    parser.add_argument(
        "--confirm-reviewed",
        action="store_true",
        help="required acknowledgement that a human reviewed the generated artifacts",
    )
    parser.add_argument("--self-test", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.self_test:
        run_self_test()
        return 0
    if not args.confirm_reviewed:
        raise SystemExit("refusing to create marker without --confirm-reviewed")
    if not args.reviewer or not args.reviewer.strip():
        raise SystemExit("--reviewer is required")
    if not meaningful_human_review_text(args.reviewer):
        raise SystemExit("--reviewer must be meaningful and not a placeholder")
    if not args.notes or not meaningful_human_review_text(args.notes):
        raise SystemExit("--notes must describe what was reviewed and accepted")
    marker = build_marker(
        reviewer=args.reviewer.strip(),
        notes=args.notes.strip(),
        desktop_repo=args.desktop_repo,
        artifact_dir=args.artifact_dir,
    )
    write_marker(marker, args.output, replace=args.replace)
    print(f"human review marker: {args.output}")
    print(f"source hash: {marker['source_identity'].get('source_hash')}")
    print(f"review scope hash: {marker['review_scope_identity'].get('review_scope_hash')}")
    for artifact in marker.get("review_artifacts") or []:
        print(f"review artifact: {artifact.get('path')}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
