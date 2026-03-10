# dyna-py: Python client for the Dyna Distributed CRUD System

A Python client for the Dyna distributed CRUD system, providing a complete API for interacting with Dyna repositories.

## Problem Statement

The Dyna ecosystem provides a powerful infrastructure for collaborative JSON editing with a changeset-centric version control model. While the core components are written in Rust for performance and safety, broader adoption requires client libraries in other popular languages. `dyna-py` addresses this need by providing a full-featured Python client, enabling Python developers to build applications and scripts that leverage the Dyna system. Without this library, Python developers would have to interact with the Dyna REST API directly, which is complex and error-prone.

## Intent and Goals

The primary goal of `dyna-py` is to offer a Pythonic and intuitive interface to the Dyna system. It is designed to be a comprehensive client library that exposes the full functionality of Dyna, from basic CRUD operations to advanced version control features. The design philosophy is to mirror the concepts of the Dyna CLI (`dyna`) as closely as possible, providing a familiar experience for users who are already acquainted with the Dyna ecosystem.

Key goals include:

*   **Complete API Coverage:** Expose all functionalities of the Dyna system, including repository management, changeset operations, and channel manipulation.
*   **Pythonic Interface:** Provide an API that feels natural to Python developers, using Python classes and data structures.
*   **Ease of Use:** Simplify the interaction with the Dyna server by handling the complexities of the REST API and data serialization.
*   **Integration:** Enable seamless integration of Dyna into Python applications, scripts, and data science workflows.

## Architecture

`dyna-py` acts as a client library that communicates with a `dyna-server` instance. It is built on top of the `dyna-core` and `dyna-cli` Rust libraries, using PyO3 to create Python bindings. This architecture allows `dyna-py` to reuse the robust and performant core logic of the Dyna system while providing a high-level Python interface.

```
+----------------------------------------------------------------------+
|                                                                      |
|                        Python Application                            |
|                                                                      |
+----------------------------------------------------------------------+
|                                  |                                     |
|                           +------v------+                              |
|                           |             |                              |
|                           |  dyna-py    |                              |
|                           | (Python)    |                              |
|                           |             |                              |
|                           +-------------+                              |
|                                  |                                     |
|                +-----------------v----------------+                    |
|                |                                  |                    |
|                |        PyO3/maturin              |                    |
|                |                                  |                    |
|                +----------------------------------+                    |
|                                  |                                     |
|                 +----------------v----------------+                    |
|                 |                                 |                    |
|                 | dyna-cli (Rust) + dyna-core     |                    |
|                 |                                 |                    |
|                 +---------------------------------+                    |
|                                  |                                     |
|                +-----------------v----------------+                    |
|                |                                  |                    |
|                |        Dyna REST API             |                    |
|                |                                  |                    |
|                +----------------------------------+                    |
|                                  |                                     |
|                          +-------v-------+                             |
|                          |               |                             |
|                          |  dyna-server  |                             |
|                          |   (Rust)      |                             |
|                          |               |                             |
|                          +---------------+                             |
|                                                                      |
+----------------------------------------------------------------------+
```

## API Reference

The `dyna-py` library exposes a `DynaRepo` class that provides an interface to a Dyna repository. The methods of this class closely mirror the commands of the `dyna-cli`.

| Method                | Parameters                               | Return Type | Description                                                                                                                              |
| --------------------- | ---------------------------------------- | ----------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `__init__`            | `path: str`                              | `DynaRepo`  | Creates a new `DynaRepo` instance for the repository at the given path.                                                                  |
| `init`                |                                          | `None`      | Initializes a new Dyna repository at the specified path.                                                                                 |
| `clone`               | `url: str`                               | `None`      | Clones a remote Dyna repository to the local path.                                                                                       |
| `add`                 | `resource_ids: List[str]`                | `None`      | Stages changes to the specified resources.                                                                                               |
| `commit`              | `message: str`                           | `str`       | Creates a new changeset with the staged changes and the given commit message. Returns the changeset ID.                                      |
| `describe`            | `changeset_id: str`                      | `dict`      | Shows the details of a specific changeset.                                                                                               |
| `squash`              | `changeset_ids: List[str]`               | `str`       | Squashes multiple changesets into a single one. Returns the new changeset ID.                                                            |
| `push`                | `remote: str = 'origin'`                 | `None`      | Pushes local changesets to a remote repository.                                                                                          |
| `pull`                | `remote: str = 'origin'`                 | `None`      | Fetches and integrates changes from a remote repository.                                                                                 |
| `promote`             | `channel: str, changeset_id: str`        | `None`      | Promotes a changeset to the head of a channel.                                                                                           |
| `channel`             | `action: str, name: str = None`          | `list`      | Manages channels. Actions can be 'create', 'delete', or 'list'.                                                                          |
| `log`                 |                                          | `list`      | Shows the commit history of the current channel.                                                                                         |
| `status`              |                                          | `dict`      | Shows the status of the repository, including unstaged and uncommitted changes.                                                          |
| `diff`                | `changeset_id: str = None`               | `list`      | Shows the differences between the working directory and a changeset, or between two changesets.                                        |
| `history`             | `resource_id: str`                       | `list`      | Shows the history of a specific resource.                                                                                                |
| `revert`              | `changeset_id: str`                      | `None`      | Reverts the changes of a specific changeset.                                                                                             |
| `cherry_pick`         | `changeset_id: str`                      | `None`      | Applies the changes of a specific changeset to the current working directory.                                                            |
| `load_file`           | `file_path: str, resource_id: str`       | `None`      | Loads a JSON file into a resource.                                                                                                       |
| `unstage`             | `resource_ids: List[str]`                | `None`      | Unstages changes to the specified resources.                                                                                             |
| `delete`              | `resource_id: str`                       | `None`      | Deletes a resource.                                                                                                                      |
| `restore`             | `resource_id: str`                       | `None`      | Restores a deleted resource.                                                                                                             |
| `cleanup`             |                                          | `None`      | Cleans up the local repository.                                                                                                          |

## Technical Design

`dyna-py` is a Python wrapper around the `dyna-cli` and `dyna-core` Rust libraries. It uses [PyO3](https://pyo3.rs/) to create Python bindings for the Rust code, which allows for a seamless and performant integration.

### Data Structures

The library internally uses the same data structures as `dyna-core`, such as `Resource`, `Patch`, `Changeset`, and `Channel`. These Rust structs are exposed as Python classes, with their fields accessible as attributes.

### Concurrency

Asynchronous operations are handled by Tokio in the underlying Rust code. The Python library exposes a synchronous API for ease of use, but the I/O operations are performed concurrently under the hood.

### Error Handling

Errors from the Rust libraries are translated into Python exceptions. This allows for idiomatic error handling in Python using `try...except` blocks.

### Compression

All data is compressed using gzip before being sent to the `dyna-server` and decompressed upon receipt. This is handled transparently by the library.

## Integration Examples

Here are some examples of how to use `dyna-py` in your Python projects.

**Initialize a new repository and create a resource:**

```python
from dyna_py import DynaRepo
import json

# Initialize a new repository
repo = DynaRepo('./my-repo')
repo.init()

# Create a new resource
user_data = {'name': 'Alice', 'email': 'alice@example.com'}
with open('./my-repo/acme.user.alice.json', 'w') as f:
    json.dump(user_data, f)

# Add and commit the new resource
repo.add(['acme.user.alice'])
changeset_id = repo.commit('Add user Alice')
print(f'Committed changeset: {changeset_id}')
```

**Clone a remote repository and read a resource:**

```python
from dyna_py import DynaRepo
import json

# Clone a remote repository
repo = DynaRepo('./cloned-repo')
repo.clone('http://dyna.example.com/my-repo')

# Read a resource
with open('./cloned-repo/acme.user.bob.json', 'r') as f:
    user_data = json.load(f)

print(f'User data: {user_data}')
```

## Building

### With Nix

If you have Nix installed, you can build the project by running:

```bash
nix build
```

### Without Nix

To build `dyna-py` without Nix, you will need to have the Rust toolchain and Python installed. You can then build and install the library using `pip`:

```bash
pip install .
```

## Testing

To run the test suite, you can use `pytest`:

```bash
pytest
```

## Related Projects

*   **dyna-core**: The core Rust library for the Dyna system.
*   **dyna-cli**: The command-line interface for Dyna.
*   **dyna-server**: The Dyna server.
*   **dyna-wasm**: The WebAssembly client for Dyna.
*   **dyna-go**: The Go client for Dyna.
*   **dyna-app**: A web-based UI for Dyna.
*   **lazy-cat**: A lazy resource loader for Dyna.
*   **lazy-go**: A lazy resource loader for Dyna in Go.
*   **lazy-py**: A lazy resource loader for Dyna in Python.
*   **lazy-wasm**: A lazy resource loader for Dyna in WebAssembly.
*   **lazy-elm-demo**: A demo application for `lazy-wasm`.

### The `dyna-py` CLI

In addition to the Python library, `dyna-py` also includes a command-line interface (CLI) built with `click`. This CLI provides the same functionality as the `DynaRepo` class, but can be used directly from the shell.

**Usage:**

```bash
dyna-py --repo-path ./my-repo <command> [options]
```

**Commands:**

*   `init`: Initializes a new Dyna repository.
*   `clone`: Clones a remote repository.
*   `add`: Stages changes to resources.
*   `commit`: Creates a new changeset.
*   `push`: Pushes changes to a remote repository.
*   `pull`: Pulls changes from a remote repository.
*   And all other commands available in the `DynaRepo` class.

This CLI is a convenient way to interact with Dyna repositories without writing any Python code.

### Advanced Integration Examples

**Working with Channels:**

```python
from dyna_py import DynaRepo

repo = DynaRepo('./my-repo')

# Create a new channel
repo.channel('create', 'feature-branch')

# Switch to the new channel
# (Note: dyna-py manages the current channel implicitly based on the checked-out branch)

# Make some changes and commit them to the new channel
# ...

# Promote the changeset to the main channel
repo.promote('main', '...')
```

**Handling Merge Conflicts:**

When a `pull` operation results in a merge conflict, `dyna-py` will raise a `ConflictException`. You can then inspect the conflicting files and resolve the conflict manually.

```python
from dyna_py import DynaRepo, ConflictException

repo = DynaRepo('./my-repo')

try:
    repo.pull()
except ConflictException as e:
    print(f'Merge conflict detected: {e.conflicts}')
    # Manually resolve conflicts in the files
    # ...

    # Add the resolved files and commit the merge
    repo.add(['acme.user.alice'])
    repo.commit('Merge remote changes')
```

### Expanded Technical Design

#### Data Structure Mapping

As an example of how Rust structs are mapped to Python classes, consider the `Changeset` struct in `dyna-core`:

```rust
// In dyna-core (Rust)
pub struct Changeset {
    pub id: String,
    pub message: String,
    pub author: String,
    pub timestamp: i64,
    pub patches: Vec<Patch>,
}
```

This is exposed in Python as the `Changeset` class:

```python
# In dyna-py (Python)
class Changeset:
    def __init__(self, id, message, author, timestamp, patches):
        self.id = id
        self.message = message
        self.author = author
        self.timestamp = timestamp
        self.patches = patches
```

PyO3 handles the conversion between the Rust and Python types automatically.

#### Detailed Error Handling

`dyna-py` defines a set of custom exceptions that correspond to specific error conditions in the Dyna system. For example, if you try to commit with no staged changes, a `NoStagedChangesError` will be raised.

```python
from dyna_py import DynaRepo, NoStagedChangesError

repo = DynaRepo('./my-repo')

try:
    repo.commit('This will fail')
except NoStagedChangesError:
    print('Nothing to commit!')
```

This allows for fine-grained error handling in your Python code.

### More on Building

To build `dyna-py` from source, you will need:

*   **Rust:** Install the Rust toolchain using `rustup`. You can find instructions at [https://rustup.rs/](https://rustup.rs/).
*   **Python:** `dyna-py` requires Python 3.7 or later.
*   **Maturin:** Maturin is a tool for building and publishing Rust-based Python packages. You can install it with `pip`:

    ```bash
    pip install maturin
    ```

Once you have these prerequisites, you can clone the `dyna-py` repository and build it:

```bash
git clone <repository>
cd dyna-py
maturin build --release
```

This will create a wheel file in the `target/wheels` directory, which you can then install with `pip`.

### More on Testing

The `dyna-py` test suite is divided into two parts:

*   **Unit tests:** These tests cover individual functions and classes in the `dyna-py` library. They are located in the `tests/` directory and can be run with `pytest`.
*   **Integration tests:** These tests cover the interaction between `dyna-py` and a `dyna-server` instance. They require a running `dyna-server` and are located in the `tests/integration/` directory. You can run them with `pytest --integration`.

To run the full test suite, including both unit and integration tests, you will need to have a `dyna-server` running and then run:

```bash
pytest --integration
```
