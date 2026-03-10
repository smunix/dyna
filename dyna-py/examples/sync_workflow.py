#!/usr/bin/env python3
"""
sync_workflow.py — demonstrates remote sync operations with dyna-py.

This script shows how to use push, pull, clone, and promote with a
running Dyna server.

Prerequisites:
    1. Start a Dyna server:  cargo run -p dyna-server
    2. Run this script:      python examples/sync_workflow.py

Adjust SERVER_URL below if your server runs on a different address.
"""

import json
import shutil
import tempfile
from pathlib import Path

from dyna_py import DynaRepo

SERVER_URL = "http://localhost:8080"


def section(title: str) -> None:
    print(f"\n{'─' * 60}")
    print(f"  {title}")
    print(f"{'─' * 60}\n")


def main() -> None:
    repo_a_dir = Path(tempfile.mkdtemp(prefix="dyna-py-alice-"))
    repo_b_dir = Path(tempfile.mkdtemp(prefix="dyna-py-bob-"))

    try:
        # ── Alice initialises and pushes ────────────────────────────
        section("Alice: init, write, commit, push")
        alice = DynaRepo.init(str(repo_a_dir), remote_url=SERVER_URL, user_name="alice")

        alice.write_resource("project.Config", json.dumps({
            "name": "My Project",
            "version": "0.1.0",
        }))
        alice.add("project.Config")
        cid = alice.commit("Initial project config")
        print(f"  Committed: {cid[:12]}")

        result = alice.push()
        print(f"  Push result: {result}")

        # ── Bob clones ──────────────────────────────────────────────
        section("Bob: clone from server")
        bob = DynaRepo.clone_repo(SERVER_URL, str(repo_b_dir), user_name="bob")
        resources = bob.list_resources()
        print(f"  Cloned {len(resources)} resource(s): {resources}")

        config = json.loads(bob.read_resource("project.Config"))
        print(f"  project.Config = {json.dumps(config, indent=2)}")

        # ── Bob makes changes and pushes ────────────────────────────
        section("Bob: modify, commit, push")
        config["version"] = "0.2.0"
        config["author"] = "bob"
        bob.write_resource("project.Config", json.dumps(config))
        bob.add("project.Config")
        cid2 = bob.commit("Bump version and add author")
        print(f"  Committed: {cid2[:12]}")

        result = bob.push()
        print(f"  Push result: {result}")

        # ── Alice pulls Bob's changes ───────────────────────────────
        section("Alice: pull")
        result = alice.pull()
        print(f"  Pull result: {result}")

        updated = json.loads(alice.read_resource("project.Config"))
        print(f"  project.Config = {json.dumps(updated, indent=2)}")

        # ── Feature branch workflow ─────────────────────────────────
        section("Alice: feature branch → promote")
        alice.create_channel("feature/add-readme")
        alice.switch_channel("feature/add-readme")
        print(f"  On channel: {alice.current_channel()}")

        alice.write_resource("project.README", json.dumps({
            "title": "My Project",
            "description": "A collaborative JSON project managed by Dyna.",
        }))
        alice.add("project.README")
        cid3 = alice.commit("Add README resource")
        print(f"  Committed: {cid3[:12]}")

        result = alice.push(channel="feature/add-readme")
        print(f"  Push result: {result}")

        result = alice.promote(channel="feature/add-readme")
        print(f"  Promote result: {result}")

        # ── Log ─────────────────────────────────────────────────────
        section("Alice: full log")
        alice.switch_channel("main")
        alice.pull()
        for entry in alice.log():
            print(f"  ● {entry['change_id'][:12]}  {entry['message']}")

        section("Done!")
        print("  Sync workflow completed successfully.")

    except RuntimeError as exc:
        print(f"\n  Error: {exc}")
        print("  Make sure the Dyna server is running at", SERVER_URL)

    finally:
        shutil.rmtree(repo_a_dir, ignore_errors=True)
        shutil.rmtree(repo_b_dir, ignore_errors=True)
        print(f"\n  Cleaned up temp directories")


if __name__ == "__main__":
    main()
