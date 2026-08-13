#!/usr/bin/env python3
"""Verify the versioned functional execution matrix against collected CI evidence."""

from __future__ import annotations

import argparse
import json
import sys
import tomllib
from pathlib import Path
from typing import Any


def load_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as stream:
        return tomllib.load(stream)


def scenario_ids(suite: Path) -> set[str]:
    identifiers: set[str] = set()
    for path in sorted((suite / "scenarios").glob("*.toml")):
        document = load_toml(path)
        identifier = document.get("id")
        if not isinstance(identifier, str) or not identifier:
            raise ValueError(f"{path}: scenario id must be a non-empty string")
        if identifier in identifiers:
            raise ValueError(f"{path}: duplicate scenario id {identifier!r}")
        identifiers.add(identifier)
    return identifiers


def validate_catalog(suite: Path, matrix_path: Path) -> list[str]:
    errors: list[str] = []
    try:
        approved = scenario_ids(suite)
        matrix = load_toml(matrix_path)
    except (OSError, ValueError, tomllib.TOMLDecodeError) as error:
        return [str(error)]
    if matrix.get("schema") != 1:
        errors.append(f"{matrix_path}: unsupported matrix schema {matrix.get('schema')!r}")
    listed: list[str] = []
    for run in matrix.get("scenario_runs", []):
        identifier = run.get("scenario_id")
        targets = run.get("targets")
        if not isinstance(identifier, str) or not identifier:
            errors.append("scenario_runs entry has no scenario_id")
            continue
        listed.append(identifier)
        if not isinstance(targets, list) or not targets or any(
            not isinstance(target, str) or not target for target in targets
        ):
            errors.append(f"scenario {identifier!r} has invalid targets")
        elif len(targets) != len(set(targets)):
            errors.append(f"scenario {identifier!r} has duplicate targets")
    for run in matrix.get("playwright_runs", []):
        identifier = run.get("scenario_id")
        projects = run.get("projects")
        identity = run.get("identity")
        if not isinstance(identifier, str) or not identifier:
            errors.append("playwright_runs entry has no scenario_id")
            continue
        listed.append(identifier)
        if not isinstance(identity, str) or not identity:
            errors.append(f"Playwright scenario {identifier!r} has no identity")
        if not isinstance(projects, list) or not projects or any(
            not isinstance(project, str) or not project for project in projects
        ):
            errors.append(f"Playwright scenario {identifier!r} has invalid projects")
        elif len(projects) != len(set(projects)):
            errors.append(f"Playwright scenario {identifier!r} has duplicate projects")
    duplicates = sorted({identifier for identifier in listed if listed.count(identifier) > 1})
    for identifier in duplicates:
        errors.append(f"scenario {identifier!r} appears more than once in the matrix")
    listed_set = set(listed)
    for identifier in sorted(approved - listed_set):
        errors.append(f"approved scenario {identifier!r} is missing from the matrix")
    for identifier in sorted(listed_set - approved):
        errors.append(f"matrix references unknown scenario {identifier!r}")
    return sorted(set(errors))


def expected_runs(matrix: dict[str, Any]):
    scenarios = {
        (run["scenario_id"], target)
        for run in matrix.get("scenario_runs", [])
        for target in run["targets"]
    }
    browsers = {
        (run["scenario_id"], project, run["identity"])
        for run in matrix.get("playwright_runs", [])
        for project in run["projects"]
    }
    return scenarios, browsers


def read_scenario_evidence(path: Path, errors: list[str]):
    try:
        events = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line]
    except (OSError, json.JSONDecodeError) as error:
        errors.append(f"{path}: cannot parse NDJSON: {error}")
        return None
    if not events:
        errors.append(f"{path}: empty NDJSON evidence")
        return None
    run_id = events[0].get("run_id")
    if not isinstance(run_id, dict):
        errors.append(f"{path}: missing run_id")
        return None
    expected_sequence = 1
    for event in events:
        if event.get("schema") != 1 or event.get("run_id") != run_id:
            errors.append(f"{path}: mixed schema or run ids")
        if event.get("sequence") != expected_sequence:
            errors.append(f"{path}: non-contiguous evidence sequence")
        expected_sequence += 1
    scenario = run_id.get("scenario_id")
    target = run_id.get("target")
    attempt = run_id.get("attempt")
    if not isinstance(scenario, str) or not isinstance(target, str):
        errors.append(f"{path}: invalid scenario or target")
        return None
    if attempt != 0:
        errors.append(f"{path}: semantic scenario retry is forbidden (attempt={attempt!r})")
    finishes = [event for event in events if event.get("event") == "scenario_finished"]
    if len(finishes) != 1 or finishes[0].get("outcome") != "passed":
        errors.append(f"{path}: scenario did not finish exactly once with outcome passed")
        return None
    return scenario, target


def iter_specs(value: dict[str, Any], parent: str = ""):
    for suite in value.get("suites", []):
        title = suite.get("title", "")
        prefix = f"{parent} › {title}" if parent and title else f"{parent}{title}"
        for spec in suite.get("specs", []):
            spec_title = spec.get("title", "")
            identity = f"{prefix} › {spec_title}" if prefix else spec_title
            yield identity, spec
        yield from iter_specs(suite, prefix)


def read_playwright_evidence(path: Path, errors: list[str]):
    try:
        report = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        errors.append(f"{path}: cannot parse Playwright report: {error}")
        return set()
    configured = [
        project.get("name")
        for project in report.get("config", {}).get("projects", [])
        if isinstance(project.get("name"), str)
    ]
    passed: set[tuple[str, str]] = set()
    for identity, spec in iter_specs(report):
        for test in spec.get("tests", []):
            project = test.get("projectName")
            if not isinstance(project, str) and len(configured) == 1:
                project = configured[0]
            results = test.get("results", [])
            if len(results) != 1:
                errors.append(
                    f"{path}: Playwright retry is forbidden for {identity!r} ({len(results)} results)"
                )
                continue
            if results[0].get("status") != "passed":
                errors.append(f"{path}: Playwright evidence {identity!r} did not pass")
                continue
            if not isinstance(project, str) or not project:
                errors.append(f"{path}: Playwright evidence {identity!r} has no project name")
                continue
            passed.add((project, identity))
    return passed


def validate_evidence(matrix_path: Path, evidence_root: Path) -> tuple[list[str], int, int]:
    matrix = load_toml(matrix_path)
    expected_scenarios, expected_browsers = expected_runs(matrix)
    errors: list[str] = []
    observed_scenarios: set[tuple[str, str]] = set()
    observed_browsers: set[tuple[str, str, str]] = set()
    browser_lookup = {
        (project, identity): (scenario, project, identity)
        for scenario, project, identity in expected_browsers
    }
    for path in sorted(evidence_root.rglob("*.ndjson")):
        run = read_scenario_evidence(path, errors)
        if run is None:
            continue
        if run not in expected_scenarios:
            errors.append(f"{path}: scenario run {run!r} is outside the fixed matrix")
        elif run in observed_scenarios:
            errors.append(f"{path}: duplicate scenario run {run!r}")
        else:
            observed_scenarios.add(run)
    for path in sorted(evidence_root.rglob("*.playwright.json")):
        for browser in read_playwright_evidence(path, errors):
            expected = browser_lookup.get(browser)
            if expected is None:
                errors.append(f"{path}: Playwright run {browser!r} is outside the fixed matrix")
            elif expected in observed_browsers:
                errors.append(f"{path}: duplicate Playwright run {expected!r}")
            else:
                observed_browsers.add(expected)
    for run in sorted(expected_scenarios - observed_scenarios):
        errors.append(f"required scenario run {run!r} is missing")
    for run in sorted(expected_browsers - observed_browsers):
        errors.append(f"required Playwright run {run!r} is missing")
    return sorted(set(errors)), len(observed_scenarios), len(observed_browsers)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--suite", type=Path, required=True)
    parser.add_argument("--matrix", type=Path, required=True)
    parser.add_argument("--evidence-root", type=Path, required=True)
    args = parser.parse_args()
    errors = validate_catalog(args.suite, args.matrix)
    scenario_count = 0
    playwright_count = 0
    if not errors:
        try:
            evidence_errors, scenario_count, playwright_count = validate_evidence(
                args.matrix, args.evidence_root
            )
            errors.extend(evidence_errors)
        except (OSError, tomllib.TOMLDecodeError, KeyError, TypeError) as error:
            errors.append(str(error))
    if errors:
        for error in sorted(set(errors)):
            print(f"FUNCTIONAL_MATRIX: {error}", file=sys.stderr)
        return 1
    print(
        json.dumps(
            {
                "ok": True,
                "schema": 1,
                "scenario_runs": scenario_count,
                "playwright_runs": playwright_count,
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
