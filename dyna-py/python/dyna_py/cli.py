"""
dyna-py CLI — a Python command-line interface for the Dyna distributed CRUD system.

This module provides a ``click``-based CLI that wraps ``DynaRepo`` methods.
Install with ``pip install dyna-py[cli]`` to get the ``dyna-py`` command.
"""

import json
import sys

try:
    import click
except ImportError:
    print("The CLI requires 'click'. Install with: pip install dyna-py[cli]", file=sys.stderr)
    sys.exit(1)

from dyna_py import DynaRepo


def _repo(path: str = ".") -> DynaRepo:
    """Open an existing repository at *path*."""
    try:
        return DynaRepo(path)
    except RuntimeError as exc:
        click.echo(f"error: {exc}", err=True)
        sys.exit(1)


@click.group()
@click.version_option(version="0.1.0", prog_name="dyna-py")
def main():
    """Dyna — distributed CRUD for collaborative JSON editing (Python client)."""


# ── init ────────────────────────────────────────────────────────────────────

@main.command()
@click.argument("path", default=".")
@click.option("--remote", "-r", help="Remote server URL")
@click.option("--user", "-u", default="python-user", help="User name")
def init(path: str, remote: str | None, user: str):
    """Initialise a new Dyna repository."""
    repo = DynaRepo.init(path, remote_url=remote, user_name=user)
    click.echo(f"Initialised Dyna repository at {path}")
    if remote:
        click.echo(f"  remote: {remote}")


# ── clone ───────────────────────────────────────────────────────────────────

@main.command("clone")
@click.argument("url")
@click.argument("path", default=".")
@click.option("--user", "-u", default="python-user", help="User name")
def clone_cmd(url: str, path: str, user: str):
    """Clone a remote Dyna repository."""
    repo = DynaRepo.clone_repo(url, path, user_name=user)
    resources = repo.list_resources()
    click.echo(f"Cloned into {path}")
    click.echo(f"  {len(resources)} resource(s)")


# ── status ──────────────────────────────────────────────────────────────────

@main.command()
@click.option("--path", "-C", default=".", help="Repository path")
def status(path: str):
    """Show repository status."""
    repo = _repo(path)
    st = repo.status()

    click.echo(f"On channel: {st['channel']}")

    if st["staged"]:
        click.echo("\nStaged changes:")
        for s in st["staged"]:
            click.echo(f"  {s['kind']:>10}  {s['resource_id']} ({s['op_count']} ops)")

    if st["modified"]:
        click.echo("\nModified but not staged:")
        for rid in st["modified"]:
            click.echo(f"  modified  {rid}")

    if st["deleted"]:
        click.echo("\nDeleted tracked files:")
        for rid in st["deleted"]:
            click.echo(f"  deleted   {rid}")

    if st["conflicts"]:
        click.echo("\nConflicts:")
        for rid in st["conflicts"]:
            click.echo(f"  conflict  {rid}")

    if not st["staged"] and not st["modified"] and not st["deleted"] and not st["conflicts"]:
        click.echo("Working directory clean")


# ── add ─────────────────────────────────────────────────────────────────────

@main.command()
@click.argument("resource_id")
@click.option("--delete", "-d", is_flag=True, help="Stage a deletion")
@click.option("--path", "-C", default=".", help="Repository path")
def add(resource_id: str, delete: bool, path: str):
    """Stage a resource for commit."""
    repo = _repo(path)
    if delete:
        repo.add_delete(resource_id)
        click.echo(f"Staged deletion of {resource_id}")
    else:
        repo.add(resource_id)
        click.echo(f"Staged {resource_id}")


# ── commit ──────────────────────────────────────────────────────────────────

@main.command()
@click.option("--message", "-m", required=True, help="Commit message")
@click.option("--path", "-C", default=".", help="Repository path")
def commit(message: str, path: str):
    """Commit staged changes."""
    repo = _repo(path)
    change_id = repo.commit(message)
    click.echo(f"Committed: {change_id}")


# ── push ────────────────────────────────────────────────────────────────────

@main.command()
@click.option("--channel", "-c", help="Channel to push")
@click.option("--path", "-C", default=".", help="Repository path")
def push(channel: str | None, path: str):
    """Push local changesets to the remote server."""
    repo = _repo(path)
    result = repo.push(channel=channel)
    click.echo(f"Pushed {result['changesets_pushed']} changeset(s) on '{result['channel']}'")


# ── pull ────────────────────────────────────────────────────────────────────

@main.command()
@click.option("--channel", "-c", help="Channel to pull")
@click.option("--path", "-C", default=".", help="Repository path")
def pull(channel: str | None, path: str):
    """Pull changesets from the remote server."""
    repo = _repo(path)
    result = repo.pull(channel=channel)
    click.echo(
        f"Pulled {result['changesets_pulled']} changeset(s), "
        f"{result['resources_updated']} resource(s) updated on '{result['channel']}'"
    )


# ── promote ─────────────────────────────────────────────────────────────────

@main.command()
@click.option("--channel", "-c", help="Source channel to promote from")
@click.option("--path", "-C", default=".", help="Repository path")
def promote(channel: str | None, path: str):
    """Promote changesets from a channel to main."""
    repo = _repo(path)
    result = repo.promote(channel=channel)
    click.echo(
        f"Promoted {result['promoted_count']} changeset(s) "
        f"from '{result['source_channel']}' to main"
    )


# ── log ─────────────────────────────────────────────────────────────────────

@main.command("log")
@click.option("--count", "-n", type=int, help="Number of entries to show")
@click.option("--verbose", "-v", is_flag=True, help="Show patch details")
@click.option("--path", "-C", default=".", help="Repository path")
def log_cmd(count: int | None, verbose: bool, path: str):
    """Show changeset log."""
    repo = _repo(path)
    entries = repo.log(count=count, verbose=verbose)
    for entry in entries:
        immutable_marker = " (immutable)" if entry["immutable"] else ""
        click.echo(f"● {entry['change_id'][:12]}  ({entry['commit_hash'][:12]}){immutable_marker}")
        click.echo(f"  {entry['message']}")
        parents = ", ".join(p[:12] for p in entry.get("parents", []))
        click.echo(f"  {entry['author']} · {entry['created_at']} · parents: [{parents}]")
        if verbose and "patches" in entry:
            for p in entry["patches"]:
                click.echo(f"    {p['resource_id']} ({p['op_count']} ops) [{p['hash'][:8]}]")
        click.echo()


# ── diff ────────────────────────────────────────────────────────────────────

@main.command()
@click.argument("resource_id")
@click.option("--path", "-C", default=".", help="Repository path")
def diff(resource_id: str, path: str):
    """Show diff for a resource."""
    repo = _repo(path)
    ops_json = repo.diff(resource_id)
    click.echo(ops_json)


# ── channel ─────────────────────────────────────────────────────────────────

@main.command()
@click.argument("name", required=False)
@click.option("--create", "-c", is_flag=True, help="Create a new channel")
@click.option("--list", "-l", "list_", is_flag=True, help="List all channels")
@click.option("--path", "-C", default=".", help="Repository path")
def channel(name: str | None, create: bool, list_: bool, path: str):
    """Manage channels."""
    repo = _repo(path)
    if list_:
        current = repo.current_channel()
        channels = repo.list_channels()
        for ch in channels:
            marker = " *" if ch == current else ""
            click.echo(f"  {ch}{marker}")
    elif create and name:
        repo.create_channel(name)
        click.echo(f"Created channel '{name}'")
    elif name:
        repo.switch_channel(name)
        click.echo(f"Switched to channel '{name}'")
    else:
        click.echo(f"Current channel: {repo.current_channel()}")


# ── restore ─────────────────────────────────────────────────────────────────

@main.command()
@click.argument("resource_id")
@click.option("--channel", "-c", help="Restore from a specific channel")
@click.option("--changeset", "-s", help="Restore from a specific changeset")
@click.option("--path", "-C", default=".", help="Repository path")
def restore(resource_id: str, channel: str | None, changeset: str | None, path: str):
    """Restore a resource to its snapshot state."""
    repo = _repo(path)
    repo.restore(resource_id, _channel=channel, changeset=changeset)
    source = f"changeset {changeset}" if changeset else f"channel '{channel or repo.current_channel()}'"
    click.echo(f"Restored {resource_id} from {source}")


# ── squash ──────────────────────────────────────────────────────────────────

@main.command()
@click.option("--revision", "-r", help="Changeset to squash")
@click.option("--into", "-i", help="Target changeset to squash into")
@click.option("--message", "-m", help="Override commit message")
@click.option("--path", "-C", default=".", help="Repository path")
def squash(revision: str | None, into: str | None, message: str | None, path: str):
    """Squash a changeset into its parent."""
    repo = _repo(path)
    target_id = repo.squash(revision=revision, into=into, message=message)
    click.echo(f"Squashed into {target_id}")


# ── describe ────────────────────────────────────────────────────────────────

@main.command()
@click.argument("change_id")
@click.option("--message", "-m", required=True, help="New commit message")
@click.option("--path", "-C", default=".", help="Repository path")
def describe(change_id: str, message: str, path: str):
    """Update the message of a changeset."""
    repo = _repo(path)
    repo.describe(change_id, message)
    click.echo(f"Updated message for {change_id}")


# ── history ─────────────────────────────────────────────────────────────────

@main.command()
@click.argument("resource_id")
@click.option("--path", "-C", default=".", help="Repository path")
def history(resource_id: str, path: str):
    """Query the change history of a resource from the remote server."""
    repo = _repo(path)
    entries = repo.history(resource_id)
    if not entries:
        click.echo(f"No history found for '{resource_id}'")
        return
    for entry in entries:
        click.echo(f"● {entry['change_id'][:12]}  ({entry['commit_hash'][:12]})")
        click.echo(f"  {entry['message']}")
        click.echo(f"  {entry['author']} · {entry['timestamp']} · channel: {entry['channel']}")
        ops = json.loads(entry["operations"])
        for op in ops:
            click.echo(f"    {op}")
        click.echo()


# ── resolve ─────────────────────────────────────────────────────────────────

@main.command()
@click.argument("resource_id")
@click.option("--path", "-C", default=".", help="Repository path")
def resolve(resource_id: str, path: str):
    """Resolve a conflict by accepting the current working file."""
    repo = _repo(path)
    repo.resolve(resource_id)
    click.echo(f"Resolved conflict for {resource_id}")


if __name__ == "__main__":
    main()
