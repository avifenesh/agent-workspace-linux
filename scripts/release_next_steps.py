#!/usr/bin/env python3
"""Print the concise release roadmap from the latest final review bundle."""

from __future__ import annotations

import argparse
import json
import sys
import tempfile
from pathlib import Path
from typing import Any

sys.dont_write_bytecode = True

from release_gate_audit import DEFAULT_DESKTOP_REPO
from release_gate_audit import compute_review_scope_identity
from release_gate_audit import compute_source_identity


ROOT = Path(__file__).resolve().parent.parent
DEFAULT_BUNDLE_DIR = ROOT / "target" / "final-review-bundle"


def latest_bundle_path(bundle_dir: Path = DEFAULT_BUNDLE_DIR) -> Path | None:
    if not bundle_dir.exists():
        return None
    candidates = sorted(bundle_dir.glob("*.json"))
    return candidates[-1] if candidates else None


def read_bundle(path: Path | None) -> tuple[Path, dict[str, Any]]:
    if path is None:
        resolved = latest_bundle_path()
        if resolved is None:
            raise SystemExit(
                f"no final review bundle found under {DEFAULT_BUNDLE_DIR}; run scripts/final_review_bundle.py first"
            )
        path = resolved
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise SystemExit(f"bundle is not a JSON object: {path}")
    if value.get("schema") != "agent-workspace-linux.final_human_review_bundle.v1":
        raise SystemExit(f"unsupported bundle schema in {path}: {value.get('schema')!r}")
    return path, value


def flatten_commands(bundle: dict[str, Any]) -> list[dict[str, Any]]:
    commands: list[dict[str, Any]] = []
    for step in bundle.get("next_evidence_steps") or []:
        step_id = step.get("id")
        for command in step.get("commands") or []:
            commands.append(
                {
                    "gate": step_id,
                    "title": step.get("title"),
                    "label": command.get("label"),
                    "command": command.get("command"),
                    "notes": command.get("notes") or [],
                }
            )
    return commands


def build_summary(
    path: Path,
    bundle: dict[str, Any],
    *,
    current_source_identity: dict[str, Any] | None = None,
    current_review_scope_identity: dict[str, Any] | None = None,
) -> dict[str, Any]:
    consistency = bundle.get("release_gate_consistency") or {}
    latest_evidence = bundle.get("latest_evidence") or {}
    source_bundle = latest_evidence.get("release_evidence_source_bundle") or {}
    source_bundle_manifest = source_bundle.get("manifest") or {}
    source_bundle_identity = source_bundle_manifest.get("source_identity") or {}
    source_bundle_review_scope = source_bundle_manifest.get("review_scope_identity") or {}
    current_source_identity = current_source_identity or compute_source_identity(
        ROOT,
        desktop_repo=DEFAULT_DESKTOP_REPO,
    )
    current_review_scope_identity = (
        current_review_scope_identity
        or compute_review_scope_identity(ROOT, desktop_repo=DEFAULT_DESKTOP_REPO)
    )
    current_source_hash = current_source_identity.get("source_hash")
    current_source_head = current_source_identity.get("git_head")
    release_gate_source_hash = consistency.get("release_gate_source_hash")
    release_gate_source_head = consistency.get("release_gate_git_head")
    current_review_scope_hash = current_review_scope_identity.get("review_scope_hash")
    current_review_scope_head = current_review_scope_identity.get("git_head")
    release_gate_review_scope_hash = consistency.get("release_gate_review_scope_hash")
    release_gate_review_scope_head = consistency.get("release_gate_review_scope_git_head")
    missing = bundle.get("release_gate_missing") or []
    commands = flatten_commands(bundle)
    return {
        "schema": "agent-workspace-linux.release_next_steps.v1",
        "bundle_path": str(path),
        "release_gate_status": bundle.get("release_gate_status"),
        "current_source_hash": current_source_hash,
        "release_gate_source_hash": release_gate_source_hash,
        "matches_current_source": bool(
            current_source_hash
            and release_gate_source_hash
            and current_source_hash == release_gate_source_hash
            and current_source_head == release_gate_source_head
        ),
        "current_review_scope_hash": current_review_scope_hash,
        "release_gate_review_scope_hash": release_gate_review_scope_hash,
        "matches_current_review_scope": bool(
            current_review_scope_hash
            and release_gate_review_scope_hash
            and current_review_scope_hash == release_gate_review_scope_hash
            and current_review_scope_head == release_gate_review_scope_head
        ),
        "bundle_recorded_current_source_hash": consistency.get("current_source_hash"),
        "bundle_recorded_current_review_scope_hash": consistency.get("current_review_scope_hash"),
        "source_bundle_path": source_bundle.get("path"),
        "source_bundle_source_hash": source_bundle_identity.get("source_hash"),
        "source_bundle_review_scope_hash": source_bundle_review_scope.get(
            "review_scope_hash"
        ),
        "source_bundle_matches_current_source": bool(
            source_bundle.get("path")
            and source_bundle_identity.get("source_hash")
            and source_bundle_identity.get("source_hash") == current_source_hash
        ),
        "source_bundle_matches_current_review_scope": bool(
            source_bundle.get("path")
            and source_bundle_review_scope.get("review_scope_hash")
            and source_bundle_review_scope.get("review_scope_hash")
            == current_review_scope_hash
            and source_bundle_review_scope.get("git_head")
            == current_review_scope_head
        ),
        "pending_gates": [
            {
                "id": gate.get("id"),
                "missing": gate.get("missing") or [],
            }
            for gate in missing
        ],
        "commands": commands,
        "human_review_marker_path": bundle.get("human_review_marker_path"),
        "strict_release_command": "REQUIRE_RELEASE_GATES=1 scripts/prod_readiness_smoke.sh",
    }


def render_markdown(summary: dict[str, Any]) -> str:
    lines = [
        "# Release Next Steps",
        "",
        f"- Bundle: `{summary['bundle_path']}`",
        f"- Release gate status: `{summary.get('release_gate_status')}`",
        f"- Current combined source hash: `{summary.get('current_source_hash')}`",
        f"- Audit matches current source: `{summary.get('matches_current_source')}`",
        f"- Current review scope hash: `{summary.get('current_review_scope_hash')}`",
        f"- Audit matches current review scope: `{summary.get('matches_current_review_scope')}`",
        f"- External evidence source bundle: `{summary.get('source_bundle_path')}`",
        f"- Source bundle matches current source: `{summary.get('source_bundle_matches_current_source')}`",
        f"- Source bundle matches current review scope: `{summary.get('source_bundle_matches_current_review_scope')}`",
        "",
    ]
    if not summary.get("matches_current_source") or not summary.get(
        "matches_current_review_scope"
    ):
        lines.extend(
            [
                "> Latest release audit does not match the current source or review-scope identity.",
                "> Run `scripts/prod_readiness_smoke.sh` before collecting or importing final evidence.",
                "",
            ]
        )
    if summary.get("source_bundle_path") and (
        not summary.get("source_bundle_matches_current_source")
        or not summary.get("source_bundle_matches_current_review_scope")
    ):
        lines.extend(
            [
                "> Latest source bundle does not match the current source or review-scope identity.",
                "> Re-run `scripts/export_release_evidence_bundle.py` before external evidence collection or off-machine human review.",
                "",
            ]
        )
    pending = summary.get("pending_gates") or []
    lines.extend(["## Pending Gates", ""])
    if pending:
        for gate in pending:
            missing = "; ".join(str(item) for item in gate.get("missing") or [])
            lines.append(f"- `{gate.get('id')}`: {missing}")
    else:
        lines.append("- None in the latest bundle.")
    lines.extend(["", "## Commands", ""])
    commands = summary.get("commands") or []
    if commands:
        for index, command in enumerate(commands, start=1):
            lines.extend(
                [
                    f"{index}. {command.get('label')} (`{command.get('gate')}`)",
                    "",
                    "```bash",
                    str(command.get("command") or ""),
                    "```",
                    "",
                ]
            )
            for note in command.get("notes") or []:
                lines.append(f"   - {note}")
            if command.get("notes"):
                lines.append("")
    else:
        lines.append("- No commands remain in the latest bundle.")
    lines.extend(
        [
            "## Final Gate",
            "",
            "After external evidence is imported and human review marker exists:",
            "",
            "```bash",
            summary["strict_release_command"],
            "```",
            "",
        ]
    )
    return "\n".join(lines)


def run_self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="agent-workspace-release-next-") as temp:
        path = Path(temp) / "bundle.json"
        bundle = {
            "schema": "agent-workspace-linux.final_human_review_bundle.v1",
            "release_gate_status": "pending",
            "release_gate_consistency": {
                "current_source_hash": "current",
                "release_gate_source_hash": "old",
                "matches_current_source": False,
                "current_review_scope_hash": "scope-current",
                "release_gate_review_scope_hash": "scope-old",
                "matches_current_review_scope": False,
            },
            "release_gate_missing": [
                {"id": "viewer_desktop_matrix", "missing": ["KDE row"]},
                {"id": "human_final_diff_review", "missing": ["marker"]},
            ],
            "next_evidence_steps": [
                {
                    "id": "viewer_desktop_matrix",
                    "title": "Collect viewer row",
                    "commands": [
                        {
                            "label": "KDE row",
                            "command": "REQUIRE_VIEWER_SMOKE=1 scripts/viewer_desktop_matrix_probe.sh",
                            "notes": ["Run on KDE."],
                        }
                    ],
                }
            ],
            "latest_evidence": {
                "release_evidence_source_bundle": {
                    "path": "/tmp/source-bundle.tar.gz",
                    "manifest": {
                        "source_identity": {
                            "source_hash": "current",
                        },
                        "review_scope_identity": {
                            "git_head": "scope-head",
                            "review_scope_hash": "scope-current",
                        },
                    },
                }
            },
            "human_review_marker_path": "target/release-gate-human-review.json",
        }
        path.write_text(json.dumps(bundle), encoding="utf-8")
        summary = build_summary(
            path,
            bundle,
            current_source_identity={"source_hash": "current", "git_head": "head"},
            current_review_scope_identity={
                "review_scope_hash": "scope-current",
                "git_head": "scope-head",
            },
        )
        assert summary["matches_current_source"] is False
        assert summary["matches_current_review_scope"] is False
        assert summary["pending_gates"][0]["id"] == "viewer_desktop_matrix"
        assert summary["commands"][0]["label"] == "KDE row"
        assert summary["source_bundle_matches_current_source"] is True
        assert summary["source_bundle_matches_current_review_scope"] is True
        rendered = render_markdown(summary)
        assert "source or review-scope identity" in rendered
        assert "External evidence source bundle" in rendered
        assert "REQUIRE_RELEASE_GATES=1 scripts/prod_readiness_smoke.sh" in rendered
        stale_bundle = json.loads(json.dumps(bundle))
        stale_bundle["latest_evidence"]["release_evidence_source_bundle"]["manifest"][
            "review_scope_identity"
        ]["review_scope_hash"] = "stale-scope"
        stale_summary = build_summary(
            path,
            stale_bundle,
            current_source_identity={"source_hash": "current", "git_head": "head"},
            current_review_scope_identity={
                "review_scope_hash": "scope-current",
                "git_head": "scope-head",
            },
        )
        assert stale_summary["source_bundle_matches_current_source"] is True
        assert stale_summary["source_bundle_matches_current_review_scope"] is False
        stale_rendered = render_markdown(stale_summary)
        assert "off-machine human review" in stale_rendered
    print("release next steps self-test passed")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, help="final review bundle JSON path")
    parser.add_argument(
        "--desktop-repo",
        type=Path,
        default=DEFAULT_DESKTOP_REPO,
        help="sibling Codex Desktop repo included in live source and review-scope identity",
    )
    parser.add_argument("--json", action="store_true", help="emit machine-readable JSON")
    parser.add_argument("--self-test", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.self_test:
        run_self_test()
        return 0
    path, bundle = read_bundle(args.bundle)
    summary = build_summary(
        path,
        bundle,
        current_source_identity=compute_source_identity(ROOT, desktop_repo=args.desktop_repo),
        current_review_scope_identity=compute_review_scope_identity(
            ROOT,
            desktop_repo=args.desktop_repo,
        ),
    )
    if args.json:
        print(json.dumps(summary, indent=2, sort_keys=True))
    else:
        print(render_markdown(summary))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
