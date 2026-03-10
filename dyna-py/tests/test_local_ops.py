"""
test_local_ops.py — unit tests for dyna-py local repository operations.

These tests exercise the DynaRepo API without requiring a remote server.
Run with:
    pytest tests/test_local_ops.py -v
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
    d = Path(tempfile.mkdtemp(prefix="dyna-py-test-"))
    yield d
    shutil.rmtree(d, ignore_errors=True)


@pytest.fixture
def repo(repo_dir):
    """Initialise a fresh DynaRepo in a temp directory."""
    return DynaRepo.init(str(repo_dir), user_name="test-user")


class TestInit:
    def test_init_creates_dyna_dir(self, repo_dir):
        DynaRepo.init(str(repo_dir))
        assert (repo_dir / ".dyna").is_dir()

    def test_init_sets_user_name(self, repo_dir):
        r = DynaRepo.init(str(repo_dir), user_name="alice")
        assert r.user_name() == "alice"

    def test_init_sets_remote_url(self, repo_dir):
        r = DynaRepo.init(str(repo_dir), remote_url="http://example.com")
        assert r.remote_url() == "http://example.com"

    def test_open_existing(self, repo):
        r2 = DynaRepo(repo.work_dir())
        assert r2.current_channel() == repo.current_channel()

    def test_open_nonexistent_raises(self, tmp_path):
        with pytest.raises(RuntimeError, match="No .dyna directory"):
            DynaRepo(str(tmp_path / "nonexistent"))


class TestResourceIO:
    def test_write_and_read(self, repo):
        data = {"key": "value"}
        repo.write_resource("test.Resource", json.dumps(data))
        result = json.loads(repo.read_resource("test.Resource"))
        assert result == data

    def test_write_invalid_json_raises(self, repo):
        with pytest.raises(RuntimeError, match="Invalid JSON"):
            repo.write_resource("test.Bad", "not json")

    def test_list_resources(self, repo):
        repo.write_resource("a.B", '{"x": 1}')
        repo.write_resource("c.D", '{"y": 2}')
        resources = repo.list_resources()
        assert "a.B" in resources
        assert "c.D" in resources

    def test_resource_exists(self, repo):
        assert not repo.resource_exists("test.Missing")
        repo.write_resource("test.Present", '{}')
        assert repo.resource_exists("test.Present")

    def test_delete_resource(self, repo):
        repo.write_resource("test.Del", '{}')
        assert repo.resource_exists("test.Del")
        repo.delete_resource("test.Del")
        assert not repo.resource_exists("test.Del")


class TestStaging:
    def test_add_stages_resource(self, repo):
        repo.write_resource("test.A", '{"v": 1}')
        repo.add("test.A")
        st = repo.status()
        assert len(st["staged"]) == 1
        assert st["staged"][0]["resource_id"] == "test.A"
        assert st["staged"][0]["kind"] == "new"

    def test_add_no_changes_raises(self, repo):
        repo.write_resource("test.A", '{"v": 1}')
        repo.add("test.A")
        repo.commit("first")
        # No changes to the file
        with pytest.raises(RuntimeError, match="No changes detected"):
            repo.add("test.A")


class TestCommit:
    def test_commit_returns_change_id(self, repo):
        repo.write_resource("test.A", '{"v": 1}')
        repo.add("test.A")
        cid = repo.commit("test commit")
        assert isinstance(cid, str)
        assert len(cid) > 0

    def test_commit_nothing_staged_raises(self, repo):
        with pytest.raises(RuntimeError, match="Nothing staged"):
            repo.commit("empty")

    def test_commit_clears_staging(self, repo):
        repo.write_resource("test.A", '{"v": 1}')
        repo.add("test.A")
        repo.commit("first")
        st = repo.status()
        assert len(st["staged"]) == 0


class TestLog:
    def test_log_returns_entries(self, repo):
        repo.write_resource("test.A", '{"v": 1}')
        repo.add("test.A")
        repo.commit("first commit")
        entries = repo.log()
        assert len(entries) == 1
        assert entries[0]["message"] == "first commit"
        assert entries[0]["author"] == "test-user"

    def test_log_count(self, repo):
        for i in range(5):
            repo.write_resource("test.A", json.dumps({"v": i}))
            repo.add("test.A")
            repo.commit(f"commit {i}")
        assert len(repo.log(count=3)) == 3
        assert len(repo.log(count=10)) == 5

    def test_log_verbose(self, repo):
        repo.write_resource("test.A", '{"v": 1}')
        repo.add("test.A")
        repo.commit("with patches")
        entries = repo.log(verbose=True)
        assert "patches" in entries[0]
        assert len(entries[0]["patches"]) > 0


class TestDiff:
    def test_diff_shows_operations(self, repo):
        repo.write_resource("test.A", '{"v": 1}')
        repo.add("test.A")
        repo.commit("first")
        repo.write_resource("test.A", '{"v": 2, "new_key": true}')
        ops_json = repo.diff("test.A")
        ops = json.loads(ops_json)
        assert len(ops) > 0


class TestChannels:
    def test_default_channel_is_main(self, repo):
        assert repo.current_channel() == "main"

    def test_create_and_list(self, repo):
        repo.create_channel("feature/x")
        channels = repo.list_channels()
        assert "main" in channels
        assert "feature/x" in channels

    def test_switch_channel(self, repo):
        repo.create_channel("dev")
        repo.switch_channel("dev")
        assert repo.current_channel() == "dev"

    def test_switch_nonexistent_raises(self, repo):
        with pytest.raises(RuntimeError):
            repo.switch_channel("nonexistent")


class TestStatus:
    def test_clean_status(self, repo):
        st = repo.status()
        assert st["channel"] == "main"
        assert len(st["staged"]) == 0
        assert len(st["modified"]) == 0
        assert len(st["deleted"]) == 0
        assert len(st["untracked"]) == 0

    def test_untracked_files(self, repo):
        repo.write_resource("test.New", '{}')
        st = repo.status()
        assert "test.New" in st["untracked"]

    def test_modified_files(self, repo):
        repo.write_resource("test.A", '{"v": 1}')
        repo.add("test.A")
        repo.commit("first")
        repo.write_resource("test.A", '{"v": 2}')
        st = repo.status()
        assert "test.A" in st["modified"]

    def test_deleted_files(self, repo):
        repo.write_resource("test.A", '{"v": 1}')
        repo.add("test.A")
        repo.commit("first")
        repo.delete_resource("test.A")
        st = repo.status()
        assert "test.A" in st["deleted"]


class TestRestore:
    def test_restore_to_snapshot(self, repo):
        original = {"v": 1}
        repo.write_resource("test.A", json.dumps(original))
        repo.add("test.A")
        repo.commit("first")
        repo.write_resource("test.A", '{"v": 999}')
        repo.restore("test.A")
        restored = json.loads(repo.read_resource("test.A"))
        assert restored == original


class TestDescribe:
    def test_describe_updates_message(self, repo):
        repo.write_resource("test.A", '{"v": 1}')
        repo.add("test.A")
        cid = repo.commit("old message")
        repo.describe(cid, "new message")
        entries = repo.log()
        assert entries[0]["message"] == "new message"


class TestSquash:
    def test_squash_merges_commits(self, repo):
        repo.write_resource("test.A", '{"v": 1}')
        repo.add("test.A")
        repo.commit("first")

        repo.write_resource("test.A", '{"v": 2}')
        repo.add("test.A")
        repo.commit("second")

        assert len(repo.log()) == 2
        repo.squash(message="squashed")
        entries = repo.log()
        assert len(entries) == 1
        assert entries[0]["message"] == "squashed"


class TestConfig:
    def test_set_and_get_remote(self, repo):
        assert repo.remote_url() is None
        repo.set_remote("http://example.com")
        assert repo.remote_url() == "http://example.com"

    def test_set_and_get_user_name(self, repo):
        repo.set_user_name("bob")
        assert repo.user_name() == "bob"


class TestUtility:
    def test_path_to_resource_id(self, repo):
        rid = repo.path_to_resource_id("acme/entity/User.json")
        assert "acme" in rid

    def test_resource_id_to_path(self, repo):
        path = repo.resource_id_to_path("acme.entity.User")
        assert "acme" in path

    def test_work_dir(self, repo, repo_dir):
        assert repo.work_dir() == str(repo_dir)
