"""
test_conflict_resolution.py — integration test for conflict resolution during
local promote between two channels that make concurrent edits.

Scenario:
  1. Create a base resource on a "setup" channel, commit it.
  2. Promote "setup" → "main" (first merge, no conflict).
  3. Fork two channels ("alice" and "bob") from "setup".
  4. Alice modifies `status` to "approved" and `budget` to 75000, commits.
  5. Bob modifies `status` to "rejected" and adds a `note`, commits.
  6. Promote "alice" → "main" — succeeds (no conflict).
  7. Promote "bob" → "main" — detects conflict on `status` field.
  8. Resolve the conflict by keeping alice's status + bob's note.
  9. Verify final state on main.

Run with:
    pytest tests/test_conflict_resolution.py -v
"""
import json
import shutil
import tempfile
from pathlib import Path

import pytest
from dyna_py import DynaRepo


@pytest.fixture
def repo_dir():
    """Create a temporary directory and clean up after the test."""
    d = Path(tempfile.mkdtemp(prefix="dyna-py-conflict-test-"))
    yield d
    shutil.rmtree(d, ignore_errors=True)


@pytest.fixture
def repo(repo_dir):
    """Initialise a fresh DynaRepo in a temp directory."""
    return DynaRepo.init(str(repo_dir), user_name="test-user")


BASE_DOC = {
    "name": "Project Alpha",
    "status": "active",
    "budget": 50000,
    "team": ["Alice", "Bob"],
}


def write_and_commit(repo, resource_id, data, message):
    """Write a resource, stage it, and commit."""
    repo.write_resource(resource_id, json.dumps(data))
    repo.add(resource_id)
    return repo.commit(message)


class TestConflictResolution:
    """Test conflict resolution during local promote between concurrent channels."""

    def test_full_conflict_resolution_workflow(self, repo):
        # ── Step 1: Create base resource on "setup" channel ──────────────
        repo.create_channel("setup")
        repo.switch_channel("setup")

        base_cs = write_and_commit(
            repo, "projects.alpha", BASE_DOC, "Initial project setup"
        )
        assert base_cs is not None
        print(f"Base changeset: {base_cs[:16]}")

        # ── Step 2: Promote setup → main (first merge, no conflict) ──────
        result = repo.promote_local("setup", "main")
        assert result["success"] is True
        print(f"Setup promoted to main: {result['promoted_count']} changeset(s)")

        # ── Step 3: Fork alice and bob from setup ────────────────────────
        repo.create_channel("alice")
        repo.create_channel("bob")

        # ── Step 4: Alice modifies status and budget ─────────────────────
        repo.switch_channel("alice")
        alice_doc = {
            "name": "Project Alpha",
            "status": "approved",
            "budget": 75000,
            "team": ["Alice", "Bob"],
        }
        alice_cs = write_and_commit(
            repo, "projects.alpha", alice_doc, "Approve project and increase budget"
        )
        print(f"Alice changeset: {alice_cs[:16]}")

        # ── Step 5: Bob modifies status and adds note ────────────────────
        repo.switch_channel("bob")
        bob_doc = {
            "name": "Project Alpha",
            "status": "rejected",
            "budget": 50000,
            "team": ["Alice", "Bob"],
            "note": "Needs more review",
        }
        bob_cs = write_and_commit(
            repo, "projects.alpha", bob_doc, "Reject project with note"
        )
        print(f"Bob changeset: {bob_cs[:16]}")

        # ── Step 6: Promote alice → main (should succeed) ────────────────
        result = repo.promote_local("alice", "main")
        assert result["success"] is True, "Alice → main should succeed"
        print("Alice promoted to main successfully")

        # ── Step 7: Promote bob → main (should detect conflict) ──────────
        result = repo.promote_local("bob", "main")
        assert result["success"] is False, "Bob → main should detect conflict"
        assert "conflicts" in result
        conflicts = result["conflicts"]
        assert len(conflicts) > 0, "Should have at least one conflict"

        # Find the status conflict
        status_conflict = None
        for c in conflicts:
            if c["json_path"] == "/status":
                status_conflict = c
                break

        assert status_conflict is not None, "Should have conflict on /status"
        assert json.loads(status_conflict["base_value"]) == "active"
        assert json.loads(status_conflict["local_value"]) == "approved"
        assert json.loads(status_conflict["remote_value"]) == "rejected"

        print(
            f"Conflict detected: path={status_conflict['json_path']}, "
            f"base={status_conflict['base_value']}, "
            f"local={status_conflict['local_value']}, "
            f"remote={status_conflict['remote_value']}"
        )

        # ── Step 8: Resolve the conflict ─────────────────────────────────
        # Decision: keep alice's "approved" status, accept bob's "note"
        resolved_doc = {
            "name": "Project Alpha",
            "status": "approved",
            "budget": 75000,
            "team": ["Alice", "Bob"],
            "note": "Needs more review",
        }

        # Write the resolved document to the main channel's working dir
        # by switching to main, writing, and clearing conflicts
        repo.switch_channel("main")
        repo.write_resource("projects.alpha", json.dumps(resolved_doc))
        repo.resolve("projects.alpha")

        print("Conflict resolved: kept 'approved' status with Bob's note")

        # ── Step 9: Verify final state ───────────────────────────────────
        final_data = json.loads(repo.read_resource("projects.alpha"))
        assert final_data["name"] == "Project Alpha"
        assert final_data["status"] == "approved"
        assert final_data["budget"] == 75000
        assert final_data["note"] == "Needs more review"
        assert final_data["team"] == ["Alice", "Bob"]

        print(
            f"Final state: status={final_data['status']}, "
            f"budget={final_data['budget']}, "
            f"note={final_data['note']}"
        )

        # Verify no remaining conflicts
        conflicted = repo.list_conflicts()
        assert len(conflicted) == 0, "No conflicts should remain after resolution"
        print("All conflicts resolved successfully")

    def test_non_conflicting_promote(self, repo):
        """Test that non-conflicting changes auto-merge cleanly."""
        repo.create_channel("setup")
        repo.switch_channel("setup")
        write_and_commit(repo, "projects.alpha", BASE_DOC, "Initial setup")

        # Promote setup → main
        result = repo.promote_local("setup", "main")
        assert result["success"] is True

        # Fork two channels
        repo.create_channel("alice")
        repo.create_channel("bob")

        # Alice changes budget (no overlap with bob)
        repo.switch_channel("alice")
        alice_doc = {**BASE_DOC, "budget": 75000}
        write_and_commit(repo, "projects.alpha", alice_doc, "Increase budget")

        # Bob adds a note (no overlap with alice)
        repo.switch_channel("bob")
        bob_doc = {**BASE_DOC, "note": "Looks good"}
        write_and_commit(repo, "projects.alpha", bob_doc, "Add note")

        # Promote alice → main
        result = repo.promote_local("alice", "main")
        assert result["success"] is True

        # Promote bob → main — should auto-merge (no conflict)
        result = repo.promote_local("bob", "main")
        assert result["success"] is True, (
            "Non-conflicting changes should auto-merge"
        )
        print("Non-conflicting promote succeeded with auto-merge")

    def test_new_resource_promote(self, repo):
        """Test promoting a channel that adds a completely new resource."""
        repo.create_channel("feature")
        repo.switch_channel("feature")

        new_doc = {"name": "New Feature", "version": "1.0"}
        write_and_commit(repo, "features.new", new_doc, "Add new feature")

        # Promote to main — should succeed (new resource, no conflict)
        result = repo.promote_local("feature", "main")
        assert result["success"] is True
        print("New resource promote succeeded")
