"""
test_cherry_pick_correctness.py — tests for the correctness of the cherry-pick
command in dyna-py.

Cherry-pick applies the diff from a source changeset onto the current channel's
snapshot, creating a new changeset with a fresh change_id and commit_hash.

Note: cherry-pick updates the snapshot but does NOT update the working file.
After cherry-picking, call ``repo.restore(resource_id)`` to sync the working
file with the snapshot.

These tests verify:
  - Simple cherry-pick applies the correct diff
  - Cherry-pick onto a divergent channel merges correctly
  - Cherry-pick with multiple resources
  - Cherry-pick hash integrity (new changeset, different hashes)
  - Cherry-pick preserves source and destination channel histories

Run with:
    pytest tests/test_cherry_pick_correctness.py -v
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
    d = Path(tempfile.mkdtemp(prefix="dyna-py-cherrypick-"))
    yield d
    shutil.rmtree(d, ignore_errors=True)


@pytest.fixture
def repo(repo_dir):
    return DynaRepo.init(str(repo_dir), user_name="picker")


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def write_and_commit(repo, resource_id, data, message):
    repo.write_resource(resource_id, json.dumps(data))
    repo.add(resource_id)
    return repo.commit(message)


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

class TestCherryPickCorrectness:
    """Test the correctness of the cherry-pick command."""

    def test_simple_cherry_pick(self, repo):
        """
        Cherry-pick a single changeset from one channel to another.
        The destination snapshot should have the change applied.
        """
        # Create base on src channel
        repo.create_channel("src")
        repo.switch_channel("src")
        write_and_commit(
            repo, "config.app", {"debug": False, "timeout": 30}, "Initial config"
        )

        # Make a change on src
        src_cid = write_and_commit(
            repo, "config.app", {"debug": True, "timeout": 30}, "Enable debug"
        )

        # Create dest channel with same base
        repo.create_channel("dest")
        repo.switch_channel("dest")
        write_and_commit(
            repo, "config.app", {"debug": False, "timeout": 30}, "Sync base"
        )

        # Cherry-pick src's change onto dest
        cherry_id = repo.cherry_pick(src_cid)

        # Restore working file to match the updated snapshot
        repo.restore("config.app")

        # Verify the change was applied
        data = json.loads(repo.read_resource("config.app"))
        assert data["debug"] is True, "debug should be True after cherry-pick"
        assert data["timeout"] == 30, "timeout should remain unchanged"

        # Verify it's a new changeset
        assert cherry_id != src_cid
        print(f"Simple cherry-pick verified: {cherry_id[:16]}")

    def test_cherry_pick_onto_divergent_channel(self, repo):
        """
        Cherry-pick onto a channel that has diverged from the source.
        The cherry-pick should merge the diff correctly.
        """
        # Create base
        repo.create_channel("src")
        repo.switch_channel("src")
        base = {"name": "App", "version": "1.0", "features": ["auth"]}
        write_and_commit(repo, "app.config", base, "Initial app config")

        # Src adds a feature
        src_cid = write_and_commit(
            repo,
            "app.config",
            {"name": "App", "version": "1.0", "features": ["auth", "logging"]},
            "Add logging feature",
        )

        # Dest diverges: changes version
        repo.create_channel("dest")
        repo.switch_channel("dest")
        write_and_commit(
            repo, "app.config", {**base, "version": "2.0"}, "Bump version"
        )

        # Cherry-pick src's change
        cherry_id = repo.cherry_pick(src_cid)
        repo.restore("app.config")

        # Verify: dest should have version 2.0 AND the logging feature
        data = json.loads(repo.read_resource("app.config"))
        assert data["version"] == "2.0", "Dest's version change should be preserved"
        assert "logging" in data["features"], "Cherry-picked feature should be present"
        assert "auth" in data["features"], "Original feature should be preserved"
        print(f"Cherry-pick onto divergent channel verified: {data}")

    def test_cherry_pick_multi_resource(self, repo):
        """
        Cherry-pick a changeset that modifies multiple resources.
        """
        repo.create_channel("src")
        repo.switch_channel("src")
        write_and_commit(repo, "items.a", {"count": 0}, "Init A")
        write_and_commit(repo, "items.b", {"count": 0}, "Init B")

        # Make a multi-resource change
        repo.write_resource("items.a", json.dumps({"count": 10}))
        repo.write_resource("items.b", json.dumps({"count": 20}))
        repo.add("items.a")
        repo.add("items.b")
        src_cid = repo.commit("Update both items")

        # Create dest with same base
        repo.create_channel("dest")
        repo.switch_channel("dest")
        write_and_commit(repo, "items.a", {"count": 0}, "Sync A")
        write_and_commit(repo, "items.b", {"count": 0}, "Sync B")

        # Cherry-pick
        cherry_id = repo.cherry_pick(src_cid)
        repo.restore("items.a")
        repo.restore("items.b")

        a = json.loads(repo.read_resource("items.a"))
        b = json.loads(repo.read_resource("items.b"))
        assert a["count"] == 10
        assert b["count"] == 20
        print(f"Multi-resource cherry-pick verified: A={a}, B={b}")

    def test_cherry_pick_hash_integrity(self, repo):
        """
        The cherry-picked changeset should have a different change_id
        and commit_hash from the source, and should verify correctly.
        """
        repo.create_channel("src")
        repo.switch_channel("src")
        write_and_commit(repo, "data.x", {"value": 42}, "Create x")
        src_cid = write_and_commit(repo, "data.x", {"value": 100}, "Update x")

        # Create dest with divergent state
        repo.create_channel("dest")
        repo.switch_channel("dest")
        write_and_commit(repo, "data.x", {"value": 42}, "Sync base")
        write_and_commit(
            repo, "data.x", {"value": 42, "extra": "dest-only"}, "Diverge"
        )

        # Cherry-pick
        cherry_id = repo.cherry_pick(src_cid)
        repo.restore("data.x")

        # Verify different IDs
        assert cherry_id != src_cid

        # Verify log entries
        entries = repo.log(verbose=True)
        cherry_entry = entries[0]
        assert cherry_entry["change_id"] == cherry_id
        assert cherry_entry["commit_hash"].startswith("sha256:")
        assert "Cherry-pick" in cherry_entry["message"]

        # Verify the result
        data = json.loads(repo.read_resource("data.x"))
        assert data["value"] == 100
        assert data["extra"] == "dest-only"
        print(f"Cherry-pick hash integrity verified: {cherry_id[:16]}")

    def test_cherry_pick_preserves_channel_history(self, repo):
        """
        Cherry-picking should not alter the source channel's history.
        The destination channel should gain exactly one new entry.
        """
        # Source channel: 3 commits
        repo.create_channel("feature")
        repo.switch_channel("feature")
        write_and_commit(repo, "data.x", {"v": 1}, "Feature 1")
        target_cid = write_and_commit(repo, "data.x", {"v": 2}, "Feature 2")
        write_and_commit(repo, "data.x", {"v": 3}, "Feature 3")

        src_log_before = repo.log()
        assert len(src_log_before) == 3

        # Dest channel: 1 commit
        repo.create_channel("dest")
        repo.switch_channel("dest")
        write_and_commit(repo, "data.x", {"v": 1}, "Sync base")

        # Cherry-pick Feature 2
        repo.cherry_pick(target_cid)

        # Dest should have 2 entries (original + cherry-pick)
        dest_log = repo.log()
        assert len(dest_log) == 2
        assert "Cherry-pick" in dest_log[0]["message"]
        assert dest_log[1]["message"] == "Sync base"

        # Source should still have exactly 3 entries
        repo.switch_channel("feature")
        src_log_after = repo.log()
        assert len(src_log_after) == 3
        for before, after in zip(src_log_before, src_log_after):
            assert before["change_id"] == after["change_id"]
        print("Cherry-pick preserves channel history verified")

    def test_cherry_pick_already_present_raises(self, repo):
        """
        Cherry-picking a changeset that already exists in the channel
        should raise an error.
        """
        repo.create_channel("dev")
        repo.switch_channel("dev")
        cid = write_and_commit(repo, "data.x", {"v": 1}, "First commit")

        # Try to cherry-pick a changeset that's already in this channel
        with pytest.raises(RuntimeError, match="already in this channel"):
            repo.cherry_pick(cid)
        print("Cherry-pick duplicate detection verified")

    def test_cherry_pick_nonexistent_raises(self, repo):
        """
        Cherry-picking a nonexistent changeset should raise an error.
        """
        repo.create_channel("dev")
        repo.switch_channel("dev")

        with pytest.raises(RuntimeError, match="No changeset found"):
            repo.cherry_pick("nonexistent_id_that_does_not_exist")
        print("Cherry-pick nonexistent detection verified")
