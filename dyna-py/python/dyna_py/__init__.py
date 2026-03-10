"""
dyna-py — Python bindings for the Dyna distributed CRUD system.

This package provides a ``DynaRepo`` class that wraps the Rust
``dyna-cli`` library via PyO3.  Every CLI command is exposed as a
Python method.

Quick start::

    from dyna_py import DynaRepo

    # Initialise a new repository
    repo = DynaRepo.init("/tmp/my-repo", remote_url="http://localhost:8080")

    # Write a resource
    repo.write_resource("acme.entity.User", '{"name": "Alice"}')

    # Stage, commit, push
    repo.add("acme.entity.User")
    change_id = repo.commit("Add user Alice")
    repo.push()
"""

from dyna_py._native import DynaRepo

__all__ = ["DynaRepo"]
__version__ = "0.1.0"
