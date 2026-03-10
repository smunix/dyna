"""
test_push_pull_conflict.py — integration tests for concurrent editions with
conflict resolution using the push/pull (promote_local) workflow.

These tests simulate two users making concurrent edits on separate channels,
then merging them via promote_local (the local equivalent of push/pull to main).

Scenario overview:
  1. A "setup" channel creates the base resource and promotes to main.
  2. Two channels ("alice" and "bob") fork from "setup" (inheriting the base).
  3. Both make independent edits and commit.
  4. Alice promotes first (succeeds).
  5. Bob promotes second — may conflict or auto-merge depending on the test.
  6. Conflicts are resolved, and the final state is verified.

Run with:
    pytest tests/test_push_pull_conflict.py -v
"""

import json
import shutil
import tempfile
from pathlib import Path

import pytest
from dyna_py import DynaRepo


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

@pytest.fixture
def repo_dir():
    d = Path(tempfile.mkdtemp(prefix="dyna-py-pushpull-"))
    yield d
    shutil.rmtree(d, ignore_errors=True)


@pytest.fixture
def repo(repo_dir):
    return DynaRepo.init(str(repo_dir), user_name="test-user")


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def write_and_commit(repo, resource_id, data, message):
    """Write a resource, stage it, and commit."""
    repo.write_resource(resource_id, json.dumps(data))
    repo.add(resource_id)
    return repo.commit(message)


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

class TestPushPullConflict:
    """Test concurrent editions with conflict resolution using promote_local."""

    def test_concurrent_conflict_resolution(self, repo):
        """
        Two users edit the same field concurrently.
        The second promote detects a conflict, which is then resolved.
        """
        base_doc = {
            "name": "Project Alpha",
            "status": "active",
            "budget": 50000,
            "team": ["Alice", "Bob"],
        }

        # Setup: create base on a feature channel, promote to main
        repo.create_channel("setup")
        repo.switch_channel("setup")
        write_and_commit(repo, "projects.alpha", base_doc, "Initial setup")
        result = repo.promote_local("setup", "main")
        assert result["success"] is True

        # Fork two channels from setup (so they share the base changeset)
        repo.create_channel("alice", fork_from="setup")
        repo.create_channel("bob", fork_from="setup")

        # Alice: change status to "approved" and budget to 75000
        repo.switch_channel("alice")
        alice_doc = {**base_doc, "status": "approved", "budget": 75000}
        write_and_commit(repo, "projects.alpha", alice_doc, "Approve project")

        # Bob: change status to "rejected" and add a note
        repo.switch_channel("bob")
        bob_doc = {**base_doc, "status": "rejected", "note": "Needs review"}
        write_and_commit(repo, "projects.alpha", bob_doc, "Reject with note")

        # Alice promotes first — should succeed
        result = repo.promote_local("alice", "main")
        assert result["success"] is True, "Alice's promote should succeed"
        print(f"Alice promoted: {result}")

        # Bob promotes second — should detect conflict on /status
        result = repo.promote_local("bob", "main")
        assert result["success"] is False, "Bob's promote should detect conflict"
        assert "conflicts" in result

        # Find the status conflict
        conflicts = result["conflicts"]
        status_conflict = None
        for c in conflicts:
            if c["json_path"] == "/status":
                status_conflict = c
                break
        assert status_conflict is not None, "Should have conflict on /status"
        assert json.loads(status_conflict["base_value"]) == "active"
        print(f"Conflict detected: {status_conflict}")

        # Resolve: keep Alice's "approved" status with Bob's "note"
        resolved_doc = {
            "name": "Project Alpha",
            "status": "approved",
            "budget": 75000,
            "team": ["Alice", "Bob"],
            "note": "Needs review",
        }
        repo.switch_channel("main")
        repo.write_resource("projects.alpha", json.dumps(resolved_doc))
        repo.resolve("projects.alpha")

        # Verify final state
        final = json.loads(repo.read_resource("projects.alpha"))
        assert final["status"] == "approved"
        assert final["budget"] == 75000
        assert final["note"] == "Needs review"
        assert final["team"] == ["Alice", "Bob"]

        # Verify no remaining conflicts
        assert len(repo.list_conflicts()) == 0
        print(f"Final state verified: {final}")

    def test_no_conflict_independent_fields(self, repo):
        """
        Two users edit different fields — auto-merge should succeed.
        """
        base_doc = {"name": "Widget", "price": 10, "stock": 100}

        repo.create_channel("setup")
        repo.switch_channel("setup")
        write_and_commit(repo, "products.widget", base_doc, "Initial product")
        result = repo.promote_local("setup", "main")
        assert result["success"] is True

        # Fork from setup so both channels share the base changeset
        repo.create_channel("alice", fork_from="setup")
        repo.create_channel("bob", fork_from="setup")

        # Alice changes price
        repo.switch_channel("alice")
        alice_doc = {**base_doc, "price": 15}
        write_and_commit(repo, "products.widget", alice_doc, "Update price")

        # Bob changes stock
        repo.switch_channel("bob")
        bob_doc = {**base_doc, "stock": 200}
        write_and_commit(repo, "products.widget", bob_doc, "Update stock")

        # Both promote — should auto-merge without conflict
        result = repo.promote_local("alice", "main")
        assert result["success"] is True, "Alice's promote should succeed"

        result = repo.promote_local("bob", "main")
        assert result["success"] is True, (
            "Non-conflicting changes should auto-merge"
        )
        print("Auto-merge succeeded for independent fields")

    def test_multiple_resources_concurrent(self, repo):
        """
        Two users edit different resources — both promotes should succeed.
        """
        doc_a = {"type": "config", "value": "default"}
        doc_b = {"type": "data", "count": 0}

        repo.create_channel("setup")
        repo.switch_channel("setup")
        write_and_commit(repo, "res.a", doc_a, "Create A")
        write_and_commit(repo, "res.b", doc_b, "Create B")
        result = repo.promote_local("setup", "main")
        assert result["success"] is True

        # Fork from setup
        repo.create_channel("alice", fork_from="setup")
        repo.create_channel("bob", fork_from="setup")

        # Alice edits resource A
        repo.switch_channel("alice")
        write_and_commit(repo, "res.a", {**doc_a, "value": "custom"}, "Update A")

        # Bob edits resource B
        repo.switch_channel("bob")
        write_and_commit(repo, "res.b", {**doc_b, "count": 42}, "Update B")

        result = repo.promote_local("alice", "main")
        assert result["success"] is True

        result = repo.promote_local("bob", "main")
        assert result["success"] is True
        print("Multiple resources concurrent promote succeeded")
