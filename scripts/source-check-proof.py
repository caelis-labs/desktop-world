#!/usr/bin/env python3
"""Reuse only an explicit, successful source job on this exact release commit."""
import argparse
import json
import os
import re
import subprocess

REQUIRED = ("Source contract", "Release version consistency")


def proven(run, jobs, repo, sha):
    if not re.fullmatch(r"[0-9a-f]{40}", sha):
        return False
    if not (run.get("head_sha") == sha and run.get("event") == "push"
            and run.get("head_branch") == "main"
            and run.get("head_repository", {}).get("full_name") == repo
            and run.get("path") == ".github/workflows/ci.yml"
            and run.get("status") == "completed" and run.get("conclusion") == "success"):
        return False
    for name in REQUIRED:
        matches = [j for j in jobs if j.get("name") == name]
        if len(matches) != 1 or matches[0].get("status") != "completed" or matches[0].get("conclusion") != "success":
            return False
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", required=True)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--run-id", default="")
    args = parser.parse_args()
    reuse = False
    if args.run_id.isdecimal():
        try:
            path = f"repos/{args.repo}/actions/runs/{args.run_id}"
            run = json.loads(subprocess.check_output(["gh", "api", path], stderr=subprocess.DEVNULL))
            pages = json.loads(subprocess.check_output(
                ["gh", "api", path + "/jobs?filter=latest&per_page=100", "--paginate", "--slurp"],
                stderr=subprocess.DEVNULL))
            reuse = proven(run, [job for page in pages for job in page["jobs"]], args.repo, args.sha)
        except (subprocess.CalledProcessError, KeyError, TypeError, ValueError):
            pass  # No proof: run the source checks, never skip them.
    value = "true" if reuse else "false"
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as out:
            out.write(f"reuse={value}\n")
            out.write(f"source_revision={args.sha}\n")
    print(json.dumps({"reuse": reuse, "source_sha": args.sha, "run_id": args.run_id}))


if __name__ == "__main__":
    main()
