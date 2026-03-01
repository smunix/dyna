"""
test_history_correctness.py — tests for the correctness of the log (history)
command in dyna-py.

The `log` method returns the local changeset history for the current channel.
These tests verify:
  - Linear ordering (newest first)
  - Metadata correctness (author, message, timestamps, patch counts)
  - Channel isolation (each channel has its own history)
  - History after revert (revert creates a new entry)
  - History after promote (target channel gains source's changesets)
  - Verbose mode (patches are included)

Run with:
    pytest tests/test_history_correctness.py -v
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
    d = Path(tempfile.mkdtemp(prefix="dyna-py-history-"))
    yield d
    shutil.rmtree(d, ignore_errors=True)


@pytest.fixture
def repo(repo_dir):
    return DynaRepo.init(str(repo_dir), user_name="historian")


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

class TestHistoryCorrectness:
    """Test the correctness of the log (history) command."""

    def test_linear_history_order(self, repo):
        """
        Commits should appear in reverse chronological order (newest first).
        """
        repo.create_channel("dev")
        repo.switch_channel("dev")

        ids = []
        for i in range(5):
            cid = write_and_commit(
                repo, "data.x", {"step": i}, f"Step {i}"
            )
            ids.append(cid)

        entries = repo.log()
        assert len(entries) == 5

        # Newest first
        for idx, entry in enumerate(entries):
            expected_msg = f"Step {4 - idx}"
            assert entry["message"] == expected_msg, (
                f"Entry {idx} should be '{expected_msg}', got '{entry['message']}'"
            )

        # Verify change_ids match (reversed)
        log_ids = [e["change_id"] for e in entries]
        assert log_ids == list(reversed(ids))
        print("Linear history order verified")

    def test_metadata_correctness(self, repo):
        """
        Each log entry should contain correct metadata.
        """
        repo.create_channel("feature")
        repo.switch_channel("feature")

        cid = write_and_commit(
            repo, "users.alice", {"name": "Alice", "role": "admin"}, "Add Alice"
        )

        entries = repo.log(verbose=True)
        assert len(entries) == 1
        entry = entries[0]

        assert entry["change_id"] == cid
        assert entry["message"] == "Add Alice"
        assert entry["author"] == "historian"
        assert entry["patch_count"] >= 1
        assert "commit_hash" in entry
        assert entry["commit_hash"].startswith("sha256:")
        assert "created_at" in entry
        assert "parents" in entry
        assert entry["immutable"] is False

        # Verbose mode should include patches
        assert "patches" in entry
        assert len(entry["patches"]) >= 1
        patch = entry["patches"][0]
        assert patch["target_resource"] == "users.alice"
        assert "hash" in patch
        assert patch["op_count"] >= 1
        print(f"Metadata verified: {entry['change_id'][:16]}")

    def test_history_count_limit(self, repo):
        """
        The count parameter should limit the number of entries returned.
        """
        repo.create_channel("dev")
        repo.switch_channel("dev")

        for i in range(10):
            write_and_commit(repo, "data.x", {"v": i}, f"Commit {i}")

        assert len(repo.log()) == 10
        assert len(repo.log(count=3)) == 3
        assert len(repo.log(count=1)) == 1
        assert len(repo.log(count=100)) == 10  # More than available

        # The limited entries should be the most recent
        limited = repo.log(count=2)
        assert limited[0]["message"] == "Commit 9"
        assert limited[1]["message"] == "Commit 8"
        print("Count limit verified")

    def test_channel_isolation(self, repo):
        """
        Each channel should have its own independent history.
        """
        # Channel A: 2 commits
        repo.create_channel("chan-a")
        repo.switch_channel("chan-a")
        write_and_commit(repo, "data.a", {"v": 1}, "A commit 1")
        write_and_commit(repo, "data.a", {"v": 2}, "A commit 2")

        # Channel B: 3 commits
        repo.create_channel("chan-b")
        repo.switch_channel("chan-b")
        write_and_commit(repo, "data.b", {"v": 10}, "B commit 1")
        write_and_commit(repo, "data.b", {"v": 20}, "B commit 2")
        write_and_commit(repo, "data.b", {"v": 30}, "B commit 3")

        # Verify isolation
        repo.switch_channel("chan-a")
        a_entries = repo.log()
        assert len(a_entries) == 2
        assert all("A commit" in e["message"] for e in a_entries)

        repo.switch_channel("chan-b")
        b_entries = repo.log()
        assert len(b_entries) == 3
        assert all("B commit" in e["message"] for e in b_entries)

        # Main should have no commits
        repo.switch_channel("main")
        main_entries = repo.log()
        assert len(main_entries) == 0
        print("Channel isolation verified")

    def test_history_with_revert(self, repo):
        """
        Reverting a changeset should add a new entry to the history.
        """
        repo.create_channel("dev")
        repo.switch_channel("dev")

        cid1 = write_and_commit(
            repo, "data.x", {"value": 1}, "First commit"
        )
        cid2 = write_and_commit(
            repo, "data.x", {"value": 2}, "Second commit"
        )

        # Revert the second commit
        revert_id = repo.revert(cid2)

        entries = repo.log()
        assert len(entries) == 3  # original 2 + revert

        # Newest entry should be the revert
        assert "Revert" in entries[0]["message"]
        assert entries[0]["change_id"] == revert_id

        # Verify the snapshot was reverted (restore working file to match)
        repo.restore("data.x")
        data = json.loads(repo.read_resource("data.x"))
        assert data["value"] == 1
        print("History with revert verified")

    def test_history_after_promote(self, repo):
        """
        After promoting a channel to main, the target channel should
        contain the promoted changesets.
        """
        repo.create_channel("feature")
        repo.switch_channel("feature")

        write_and_commit(repo, "data.x", {"v": 1}, "Feature commit 1")
        write_and_commit(repo, "data.x", {"v": 2}, "Feature commit 2")

        # Promote to main
        result = repo.promote_local("feature", "main")
        assert result["success"] is True

        # Main should now have the promoted changesets
        repo.switch_channel("main")
        main_entries = repo.log()
        assert len(main_entries) == 2
        messages = [e["message"] for e in main_entries]
        assert "Feature commit 1" in messages
        assert "Feature commit 2" in messages
        print("History after promote verified")

    def test_parent_chain_integrity(self, repo):
        """
        Each changeset (except the first) should reference its parent.
        """
        repo.create_channel("dev")
        repo.switch_channel("dev")

        ids = []
        for i in range(4):
            cid = write_and_commit(repo, "data.x", {"v": i}, f"Commit {i}")
            ids.append(cid)

        entries = repo.log()
        # entries are newest-first, so reverse to get chronological
        chronological = list(reversed(entries))

        # First commit has no parents (or empty parents)
        assert len(chronological[0]["parents"]) == 0

        # Each subsequent commit should reference the previous one
        for i in range(1, len(chronological)):
            assert chronological[i - 1]["change_id"] in chronological[i]["parents"], (
                f"Commit {i} should reference commit {i-1} as parent"
            )
        print("Parent chain integrity verified")
