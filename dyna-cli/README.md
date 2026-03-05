# dyna-cli: Command-Line Interface for the Dyna Distributed CRUD System

A powerful and intuitive CLI for interacting with the Dyna ecosystem, providing a Git-like experience for managing versioned JSON resources.

## 1. Problem Statement

Modern applications increasingly rely on structured, collaborative data that needs to be versioned, synchronized, and accessed from multiple environments. Developers often struggle with a mishmash of tools for managing this data: REST clients for API interaction, separate version control systems for schemas, and custom scripts for data synchronization. This fragmented approach is inefficient, error-prone, and lacks a unified workflow. `dyna-cli` addresses this by providing a single, comprehensive command-line interface for the Dyna ecosystem, a distributed CRUD system designed for collaborative JSON editing. It unifies data and schema management, version control, and synchronization into a seamless, Git-like workflow, dramatically simplifying the developer experience.

## 2. Intent and Goals

`dyna-cli` is designed with the following principles in mind:

*   **Intuitive and Familiar:** The CLI adopts the proven, battle-tested command structure of Git. Commands like `clone`, `add`, `commit`, `push`, and `pull` behave as developers expect, lowering the learning curve and enabling rapid adoption.
*   **First-Class Version Control:** Changesets are the core of Dyna, and `dyna-cli` treats them as first-class citizens. The CLI provides powerful tools for creating, inspecting, and manipulating changesets, offering a robust versioning model for JSON data.
*   **Local-First Workflow:** Developers can work entirely offline, making changes and committing them to a local repository. The CLI manages a local filesystem representation of the remote resources, ensuring a fast and responsive experience. Synchronization with the remote server is an explicit step, giving developers full control over the data flow.
*   **Seamless Integration:** `dyna-cli` is a core component of the broader Dyna ecosystem. It works in concert with `dyna-server`, `dyna-core`, and other client libraries to provide a consistent and reliable experience across all platforms.
*   **Extensibility:** The CLI is built on the `dyna-core` Rust library, which provides a solid foundation for future extensions and integrations.

## 3. Architecture

`dyna-cli` acts as a client to the `dyna-server`, which in turn communicates with an S3-compatible storage backend. The CLI maintains a local repository in the `.dyna` directory, which mirrors the structure of the remote resources.

```
+-----------------+      +------------------+      +---------------------+      +-----------------------+
|                 |      |                  |      |                     |      |                       |
|   User / Shell  |----->|     dyna-cli     |----->|     dyna-server     |----->|   S3-Compatible     |
|                 |      |                  |      |   (REST & WebSocket)  |      |       Storage       |
|                 |      |                  |      |                     |      |                       |
+-----------------+      +--------+---------+      +----------+----------+      +-----------------------+
                                  |
                                  |
                                  v
                          +-------+--------+
                          |                |
                          |  Local .dyna/  |
                          |   Repository   |
                          |                |
                          +----------------+
```

## 4. API Reference

### Repository Commands

| Command | Description |
|---|---|
| `init` | Initializes a new Dyna repository in the current directory. |
| `clone <url>` | Clones a remote repository to a new directory. |

### Local Changes

| Command | Description |
|---|---|
| `add <file...>` | Stages changes from the working directory to the index. |
| `commit -m <msg>` | Creates a new changeset from the staged changes. |
| `describe <changeset>` | Attaches a detailed description to a changeset. |
| `squash <changeset...>` | Combines multiple changesets into a single one. |
| `unstage <file...>` | Removes changes from the index. |
| `revert <changeset>` | Creates a new changeset that reverts the changes of a previous one. |
| `cherry-pick <changeset>` | Applies the changes from a specific changeset to the current branch. |

### History and Status

| Command | Description |
|---|---|
| `log` | Shows the changeset history. |
| `status` | Shows the status of the working directory and the index. |
| `diff` | Shows the differences between the working directory and the index. |
| `history <resource_id>` | Shows the version history of a specific resource. |

### Remote Synchronization

| Command | Description |
|---|---|
| `push` | Pushes local changesets to the remote repository. |
| `pull` | Fetches changesets from the remote repository and merges them. |
| `promote <channel> <changeset>` | Promotes a changeset to a specific channel. |

### Channel Management

| Command | Description |
|---|---|
| `channel create <name>` | Creates a new channel. |
| `channel delete <name>` | Deletes a channel. |
| `channel list` | Lists all channels. |

### Resource Management

| Command | Description |
|---|---|
| `load-file <file>` | Loads a JSON file as a resource. |
| `delete <resource_id>` | Deletes a resource. |
| `restore <resource_id>` | Restores a deleted resource. |

### Housekeeping

| Command | Description |
|---|---|
| `cleanup` | Cleans up unused data and optimizes the local repository. |

## 5. Technical Design

`dyna-cli` is a Rust binary that leverages several key libraries:

*   **`dyna-core`:** Provides the fundamental data models, protocol types, and business logic for the Dyna ecosystem.
*   **`clap`:** A powerful command-line argument parser for Rust.
*   **`tokio`:** An asynchronous runtime for writing network applications.
*   **`reqwest`:** A high-level HTTP client for making requests to the `dyna-server`.
*   **`vfs`:** A virtual filesystem abstraction that allows `dyna-cli` to work with both in-memory and on-disk repositories.
*   **`serde`:** A framework for serializing and deserializing Rust data structures efficiently.

### Data Structures

The core data structures are defined in the `dyna-core` crate and include:

*   **`Resource`:** A JSON document identified by a dot-separated ID.
*   **`Patch`:** A set of RFC 6902 JSON Patch operations.
*   **`Changeset`:** A content-addressed group of patches, forming the fundamental unit of versioning.
*   **`Channel`:** A named bookmark that points to a specific changeset, similar to a Git branch.

### Concurrency Model

`dyna-cli` uses the `tokio` asynchronous runtime to handle network requests concurrently. This allows the CLI to remain responsive while performing long-running operations like cloning or pushing large amounts of data.

### Error Handling

The CLI uses the `anyhow` and `thiserror` crates for robust error handling. Errors are propagated up the call stack and presented to the user in a clear and informative way.

### Compression

All data is compressed using gzip before being stored or transmitted. This significantly reduces storage and bandwidth requirements.

## 6. Integration Examples

### Initializing a Repository and Creating a Resource

```bash
# Initialize a new repository
dyna init

# Create a new resource from a JSON file
echo '{"hello": "world"}' > my-resource.json
dyna load-file my-resource.json --id acme.greeting

# Stage and commit the new resource
dyna add acme.greeting.json
dyna commit -m "Add initial greeting"
```

### Cloning a Remote Repository and Making Changes

```bash
# Clone a remote repository
dyna clone https://dyna.example.com/my-project

# Change a resource
edit my-project/acme.greeting.json

# Commit and push the changes
dyna commit -am "Update greeting"
dyna push
```

## 7. Building

### With Nix

If you have Nix installed, you can build `dyna-cli` with a single command:

```bash
nix build
```

### Without Nix

If you don't have Nix, you can build `dyna-cli` with Cargo:

```bash
cargo build --release
```

## 8. Testing

To run the test suite, use the following command:

```bash
cargo test
```

## 9. Related Projects

*   **dyna-core**: The core Rust library for the Dyna ecosystem.
*   **dyna-server**: The `axum`-based server for Dyna.
*   **dyna-wasm**: A WebAssembly client for using Dyna in the browser.
*   **dyna-py**: Python bindings for Dyna.
*   **dyna-go**: A Go client library for Dyna.
*   **dyna-app**: An Elm web UI for Dyna.
*   **lazy-cat**: A lazy resource loader for Dyna.
*   **lazy-go**: A Go lazy resource loader for Dyna.
*   **lazy-py**: A Python lazy resource loader for Dyna.
*   **lazy-wasm**: A WebAssembly lazy resource loader for Dyna.
*   **lazy-elm-demo**: An Elm UI demo for `lazy-wasm`.

## 10. Advanced Usage

### Working with Channels

Channels in Dyna are similar to branches in Git. They allow you to work on different versions of your resources in parallel. The `main` channel is the default channel.

```bash
# Create a new channel
dyna channel create feature-x

# Switch to the new channel
# (Note: Dyna does not have an explicit 'switch' or 'checkout' command. 
# You work on a channel by promoting changesets to it.)

# Make some changes and commit them
edit acme.greeting.json
dyna commit -am "Implement feature-x"

# Promote the new changeset to the 'feature-x' channel
dyna promote feature-x HEAD

# To merge the changes back to main, you would promote the same changeset to main
dyna promote main HEAD
```

### Conflict Resolution

Conflicts can occur when merging changes from different channels. `dyna-cli` provides tools to help you resolve these conflicts.

When a `pull` or `promote` operation results in a conflict, the conflicting resource will be marked. You can then use the `diff` command to see the conflicting changes and manually resolve them in the file. Once resolved, you can `add` and `commit` the resolved file.

### Interactive Committing

For more granular control over your commits, you can use the interactive mode of the `add` command:

```bash
dyna add -p
```

This will allow you to review and select individual changes to be included in the next commit.

## 11. Configuration

`dyna-cli` can be configured using a TOML file located at `~/.config/dyna/config.toml`. The following configuration options are available:

*   `default_remote_url`: The default URL for remote repositories.
*   `user.name`: Your name, to be used in commit metadata.
*   `user.email`: Your email, to be used in commit metadata.

Example configuration file:

```toml
[user]
name = "Jane Doe"
email = "jane.doe@example.com"

[remote]
default_url = "https://dyna.example.com"
```

## 12. Internals

### The `.dyna` Directory

The `.dyna` directory is the heart of a Dyna repository. It contains the following:

*   `config`: The repository-specific configuration file.
*   `HEAD`: A file containing a reference to the current channel.
*   `objects/`: The directory where all changeset and resource objects are stored, organized by their content-addressable hash.
*   `refs/`: A directory containing references to channels.
*   `index`: The staging area, which stores information about the next commit.

### Content-Addressable Storage

All data in Dyna is stored as objects that are addressed by their content. The address is the SHA-256 hash of the object's content. This has several advantages:

*   **Deduplication:** The same data is never stored twice.
*   **Integrity:** The content of an object can be verified by hashing it and comparing the result to its address.
*   **Immutability:** Objects are immutable. To change an object, you create a new one.

This design is fundamental to how Dyna achieves its distributed and versioned nature.

## 13. Future Work

`dyna-cli` is under active development. Some of the features planned for future releases include:

*   **GPG Signing:** The ability to sign changesets with a GPG key to verify their authenticity.
*   **Hooks:** The ability to run custom scripts at different points in the workflow (e.g., pre-commit, post-push).
*   **Improved Diffing:** More advanced diffing and merging tools for complex JSON structures.
*   **Shell Completion:** Shell completion scripts for Bash, Zsh, and Fish.

## 14. Command-Line API in Detail

### `dyna init`

Initializes a new, empty Dyna repository in the current directory. This creates the `.dyna` subdirectory, which contains all the necessary files for the repository.

### `dyna clone <url>`

Creates a copy of an existing remote repository. The `<url>` is the address of the `dyna-server` instance.

### `dyna add <file...>`

This command updates the index using the current content found in the working tree, to prepare the content for the next commit. It can be used to stage new files or modifications to existing files.

### `dyna commit -m <message>`

Records changes to the repository. It creates a new changeset object based on the staged changes in the index. The `-m` flag is used to provide a commit message.

### `dyna push`

Updates remote refs using local ones, while sending objects necessary to complete the given refs.

### `dyna pull`

Fetches from and integrates with another repository or a local branch.

### `dyna log`

Shows the commit logs. The command can be used with various options to control the output format.

### `dyna status`

Shows the working tree status. It displays paths that have differences between the index file and the current HEAD commit, paths that have differences between the working tree and the index file, and paths in the working tree that are not tracked by Dyna.

### `dyna diff`

Shows changes between the working tree and the index or a tree, changes between the index and a tree, changes between two trees, changes between two blob objects, or changes between two files on disk.
