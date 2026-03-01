#!/usr/bin/env python3
"""
basic_usage.py — demonstrates the dyna-py Python API.

This script exercises every major DynaRepo method against a local
repository.  No remote server is required for the local operations
(init, write, add, commit, log, diff, status, channels, squash,
describe, restore).

Run with:
    python examples/basic_usage.py
"""

import json
import shutil
import tempfile
from pathlib import Path

from dyna_py import DynaRepo


def section(title: str) -> None:
    print(f"\n{'─' * 60}")
    print(f"  {title}")
    print(f"{'─' * 60}\n")


def main() -> None:
    work_dir = Path(tempfile.mkdtemp(prefix="dyna-py-example-"))
    print(f"Working directory: {work_dir}")

    try:
        # ── 1. Initialise ───────────────────────────────────────────
        section("1. Initialise a new repository")
        repo = DynaRepo.init(str(work_dir), user_name="alice")
        print(f"  User:    {repo.user_name()}")
        print(f"  Channel: {repo.current_channel()}")
        print(f"  Remote:  {repo.remote_url()}")

        # ── 2. Write resources ──────────────────────────────────────
        section("2. Write JSON resources")
        users = {
            "acme.entity.User.alice": {"name": "Alice", "role": "admin", "active": True},
            "acme.entity.User.bob": {"name": "Bob", "role": "viewer", "active": True},
            "acme.entity.Config.app": {"theme": "dark", "version": "1.0.0"},
        }
        for rid, data in users.items():
            repo.write_resource(rid, json.dumps(data))
            print(f"  Wrote {rid}")

        # ── 3. List resources ───────────────────────────────────────
        section("3. List resources")
        for rid in sorted(repo.list_resources()):
            print(f"  {rid}")

        # ── 4. Read a resource ──────────────────────────────────────
        section("4. Read a resource")
        content = repo.read_resource("acme.entity.User.alice")
        print(f"  acme.entity.User.alice = {content}")

        # ── 5. Status (before staging) ──────────────────────────────
        section("5. Status (before staging)")
        st = repo.status()
        print(f"  Channel:    {st['channel']}")
        print(f"  Staged:     {len(st['staged'])}")
        print(f"  Untracked:  {st['untracked']}")

        # ── 6. Stage all resources ──────────────────────────────────
        section("6. Stage (add) all resources")
        for rid in users:
            repo.add(rid)
            print(f"  Staged {rid}")

        # ── 7. Status (after staging) ───────────────────────────────
        section("7. Status (after staging)")
        st = repo.status()
        for s in st["staged"]:
            print(f"  {s['kind']:>10}  {s['resource_id']} ({s['op_count']} ops)")

        # ── 8. Commit ───────────────────────────────────────────────
        section("8. Commit")
        change_id = repo.commit("Initial commit: add users and config")
        print(f"  Change ID: {change_id}")

        # ── 9. Log ──────────────────────────────────────────────────
        section("9. Log")
        for entry in repo.log(verbose=True):
            print(f"  ● {entry['change_id'][:12]}  {entry['message']}")
            print(f"    Author: {entry['author']}")
            print(f"    Patches: {entry['patch_count']}")
            if "patches" in entry:
                for p in entry["patches"]:
                    print(f"      {p['target_resource']} ({p['op_count']} ops)")

        # ── 10. Modify and diff ─────────────────────────────────────
        section("10. Modify a resource and diff")
        updated_alice = {"name": "Alice", "role": "superadmin", "active": True, "email": "alice@acme.io"}
        repo.write_resource("acme.entity.User.alice", json.dumps(updated_alice))
        diff_json = repo.diff("acme.entity.User.alice")
        print(f"  Diff operations:\n{diff_json}")

        # ── 11. Stage, commit the modification ──────────────────────
        section("11. Stage and commit the modification")
        repo.add("acme.entity.User.alice")
        change_id_2 = repo.commit("Promote Alice to superadmin")
        print(f"  Change ID: {change_id_2}")

        # ── 12. Squash ──────────────────────────────────────────────
        section("12. Squash the two commits")
        target = repo.squash(message="Initial setup with superadmin Alice")
        print(f"  Squashed into: {target}")

        # ── 13. Log after squash ────────────────────────────────────
        section("13. Log after squash")
        for entry in repo.log():
            print(f"  ● {entry['change_id'][:12]}  {entry['message']}")

        # ── 14. Describe (amend message) ────────────────────────────
        section("14. Describe (amend commit message)")
        entries = repo.log()
        if entries:
            cid = entries[0]["change_id"]
            repo.describe(cid, "Bootstrap: users + config (squashed)")
            print(f"  Updated message for {cid[:12]}")
            for entry in repo.log():
                print(f"  ● {entry['change_id'][:12]}  {entry['message']}")

        # ── 15. Channels ────────────────────────────────────────────
        section("15. Channels")
        repo.create_channel("feature/dark-mode")
        print(f"  Created 'feature/dark-mode'")
        print(f"  Channels: {repo.list_channels()}")
        repo.switch_channel("feature/dark-mode")
        print(f"  Switched to: {repo.current_channel()}")
        repo.switch_channel("main")
        print(f"  Switched back to: {repo.current_channel()}")

        # ── 16. Delete a resource ───────────────────────────────────
        section("16. Delete a resource")
        repo.delete_resource("acme.entity.User.bob")
        print(f"  Deleted acme.entity.User.bob from working directory")
        st = repo.status()
        print(f"  Deleted (tracked): {st['deleted']}")

        # ── 17. Restore ────────────────────────────────────────────
        section("17. Restore a deleted resource")
        repo.restore("acme.entity.User.bob")
        print(f"  Restored acme.entity.User.bob")
        print(f"  Exists: {repo.resource_exists('acme.entity.User.bob')}")

        # ── 18. Utility methods ─────────────────────────────────────
        section("18. Utility methods")
        print(f"  work_dir:              {repo.work_dir()}")
        print(f"  path_to_resource_id:   {repo.path_to_resource_id('acme/entity/User/alice.json')}")
        print(f"  resource_id_to_path:   {repo.resource_id_to_path('acme.entity.User.alice')}")

        # ── 19. Config ──────────────────────────────────────────────
        section("19. Config management")
        repo.set_remote("http://localhost:8080")
        print(f"  Remote URL: {repo.remote_url()}")
        repo.set_user_name("alice-v2")
        print(f"  User name:  {repo.user_name()}")

        # ── Done ────────────────────────────────────────────────────
        section("Done!")
        print("  All local operations completed successfully.")
        print(f"  Repository at: {work_dir}")

    finally:
        # Clean up
        shutil.rmtree(work_dir, ignore_errors=True)
        print(f"\n  Cleaned up {work_dir}")


if __name__ == "__main__":
    main()
