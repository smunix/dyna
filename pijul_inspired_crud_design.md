# Detailed Design: A Jujutsu-Inspired Distributed CRUD Application for Collaborative JSON Resource Editing

**Author:** Manus AI
**Date:** February 27, 2026

---

## Table of Contents

1.  [Introduction and Motivation](#1-introduction-and-motivation)
2.  [High-Level Architecture](#2-high-level-architecture)
3.  [Core Concepts: The Jujutsu Changeset Model](#3-core-concepts-the-jujutsu-changeset-model)
4.  [Data Model Specification](#4-data-model-specification)
5.  [CLI Tool Specification](#5-cli-tool-specification)
6.  [Remote Server Specification](#6-remote-server-specification)
7.  [S3 Object Storage Specification](#7-s3-object-storage-specification)
8.  [Conflict Resolution Specification](#8-conflict-resolution-specification)
9.  [Workflow: End-to-End User Journey](#9-workflow-end-to-end-user-journey)
10. [Rust Crate Dependency Map](#10-rust-crate-dependency-map)
11. [Implementation Roadmap](#11-implementation-roadmap)
12. [References](#12-references)

---

## 1. Introduction and Motivation

This document provides a comprehensive and detailed design for a distributed Create, Read, Update, and Delete (CRUD) application engineered for the collaborative editing of JSON resources. The system draws deep inspiration from the **Jujutsu version control system** [1], a modern DVCS that offers a powerful and user-friendly changeset-based model. Unlike Git's more rigid commit graph, Jujutsu treats changesets as first-class, mutable objects, which fundamentally simplifies the developer workflow [2].

The core problem this system solves is enabling multiple users to concurrently edit a shared set of JSON-formatted resources, stored durably in an S3-compatible object store, while maintaining a complete, auditable history of all changes. The system must support a workflow where users work locally and offline, commit changes into **changesets**, and then promote those changesets to a shared "main" channel that is visible to all collaborators.

The technology stack is centered on the **Rust programming language** for its performance and safety guarantees. The remote server component leverages the **`elfo-rs` actor framework** [3] for its concurrency model and built-in observability, while the **`object-store` crate** [4] from the Apache Arrow project provides a unified, asynchronous API for all S3 interactions.

---

## 2. High-Level Architecture

![System Architecture Diagram](architecture.png)

The system follows a decentralized, offline-first architecture composed of three principal components. The design philosophy prioritizes local-first operations, treating the network as an optimization rather than a requirement for core functionality.

| Component | Technology Stack | Primary Responsibilities |
| :--- | :--- | :--- |
| **CLI Tool (`dyna`)** | Rust, `clap`, `serde`, `json-patch`, `sha2` | Provides the user interface for all repository operations. Manages the local repository state, creates changesets, and synchronizes with the remote server via HTTP. Designed to be fully functional offline. |
| **Remote Server (`dyna-server`)** | Rust, `elfo-rs`, `object-store`, `axum`, `serde` | A stateless facilitator that acts as a gateway to the S3 object store. It receives changesets from CLI clients, stores them in S3, and serves them to other clients upon request. It uses an actor-based architecture for concurrency and resilience. |
| **S3 Object Storage** | Amazon S3 or any S3-compatible store (e.g., MinIO) | The single, durable source of truth for all repository data. Stores content-addressed changeset and patch objects, resource snapshots, and repository metadata. Leverages S3's strong read-after-write consistency [5] for data integrity. |

---

## 3. Core Concepts: The Jujutsu Changeset Model

The design of this system is fundamentally shaped by the principles of the Jujutsu VCS. Understanding these concepts is essential for appreciating the architectural decisions made throughout this document.

### 3.1. Changesets as First-Class Citizens

In Jujutsu, the fundamental unit of work is a **changeset**, not a commit. A changeset is a self-contained description of a modification to the repository's state, grouping one or more file changes. Critically, changesets are mutable until they are shared [2]. This allows for a much more fluid workflow where users can amend messages, reorder changesets, or squash them together without the complexities of Git's `rebase -i`.

### 3.2. Dual Identity: Stable vs. Content-Addressed

A key innovation in Jujutsu is the dual identity of each changeset:

*   **Change ID**: A stable, randomly generated identifier that uniquely identifies a changeset throughout its lifetime, even as its content is amended.
*   **Commit ID**: A content-addressed hash (like Git's commit hash) that changes every time the changeset's content or metadata is modified.

This separation allows for stable references to changesets (`jj log -r <change_id>`) while still benefiting from the integrity guarantees of content-addressable storage [1]. Our system adopts this dual-identity model for its changesets.

### 3.3. Immutability on Promotion

While changesets are mutable locally, they become **immutable** once they are pushed to a shared remote or promoted to a special, protected channel (like `main`). This prevents history from being rewritten after it has been shared, providing a safe and predictable collaborative environment. Our design enforces this by having the server mark promoted changesets as immutable.

| Concept | Jujutsu's Approach | Git's Approach | Implication for Our System |
| :--- | :--- | :--- | :--- |
| **Unit of Change** | Changeset (group of file changes) | Commit (snapshot) | A **Changeset** groups multiple **Patches** (JSON Patch operations) and is the primary unit of work. |
| **Identity** | Dual: Stable Change ID + Content-based Commit ID | Single: Content-based Commit Hash | Our changesets have a stable `change_id` and a content-derived `commit_hash`. |
| **Mutability** | Mutable until shared/pushed | Immutable by default; requires `rebase` to edit | Changesets are mutable locally (`dyna describe`). They become immutable once promoted to `main`. |
| **Branching** | Bookmarks and flexible log queries | Branches (pointers to commits) | We use named **Channels** as bookmarks pointing to the head changeset of a line of work. |

---

## 4. Data Model Specification

The data model is the cornerstone of the system, designed to ensure data integrity and support the Jujutsu-inspired workflow.

### 4.1. The Changeset

The `Changeset` is the central data structure. It groups a set of patches that were committed together.

```rust
// dyna-core/src/models.rs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Changeset {
    /// Stable, unique identifier for the changeset (16-char hex).
    pub change_id: String,
    /// Content-addressed hash of the changeset (SHA-256).
    pub commit_hash: String,
    /// Parent changesets (can be more than one for merges).
    pub parents: Vec<String>,
    /// The patches included in this changeset.
    pub patches: Vec<Patch>,
    /// Whether this changeset can be amended.
    pub immutable: bool,
    /// Whether this changeset contains any patches.
    pub empty: bool,
    /// Named references to this changeset (like jj bookmarks).
    pub bookmarks: Vec<String>,
    /// Commit message.
    pub message: String,
    /// Author of the changeset.
    pub author: String,
    /// Timestamp of creation.
    pub created_at: DateTime<Utc>,
    /// Timestamp of last update.
    pub updated_at: DateTime<Utc>,
}
```

### 4.2. The Patch

A `Patch` represents a set of operations applied to a single JSON resource. It is now a simpler structure, as metadata is managed by the parent `Changeset`.

```rust
// dyna-core/src/models.rs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Patch {
    /// Content-addressed hash of the patch (SHA-256).
    pub hash: String,
    /// The resource this patch targets (e.g., "resources/res-001.json").
    pub target_resource: String,
    /// The JSON Patch (RFC 6902) operations.
    pub operations: Vec<PatchOperation>,
    /// The snapshot of the resource before this patch was applied.
    pub parent_snapshot: Option<serde_json::Value>,
    /// The snapshot of the resource after this patch was applied.
    pub result_snapshot: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PatchOperation {
    Add(AddOperation),
    Remove(RemoveOperation),
    Replace(ReplaceOperation),
    Move(MoveOperation),
    Copy(CopyOperation),
    Test(TestOperation),
}
```

---

## 5. CLI Tool Specification

The CLI tool is the primary user interface, built with Rust and the `clap` crate. It has been refactored to be changeset-centric.

### 5.1. Local Repository State (`.dyna` Directory)

The local repository state is stored in a `.dyna` directory. The structure has been updated to store changesets.

```
.dyna/
├── config.toml              # Repository configuration (remote URL, user identity)
├── HEAD                     # Points to the current channel (e.g., "refs/channels/main")
├── working_changeset        # The ID of the current working changeset (@)
├── changesets/              # Staging area for new changesets
│   └── <change_id>.json
├── objects/                 # Content-addressed storage for committed objects
│   ├── changesets/
│   │   └── <commit_hash>.json
│   └── patches/
│       └── <patch_hash>.json
├── snapshots/
│   └── <resource_id>.json   # Snapshot of each tracked resource
└── refs/
    └── channels/
        ├── main             # File containing the head commit_hash of the main channel
        └── feature-x
```

### 5.2. Command-Line Interface Definition

The CLI commands have been updated to align with the changeset model.

```rust
// dyna-cli/src/cli.rs
#[derive(Subcommand, Debug)]
pub enum Commands {
    // ... (init, clone, add, status, diff, resolve) ...

    /// Record staged changes as a new changeset.
    Commit {
        #[arg(short, long)]
        message: String,
    },

    /// Amend the message of a changeset.
    Describe {
        /// The changeset to amend (defaults to the working changeset).
        #[arg(short, long)]
        changeset: Option<String>,
        #[arg(short, long)]
        message: String,
    },

    /// Display the history of changesets.
    Log {
        /// Show detailed operations for a specific changeset.
        #[arg(long)]
        changeset: Option<String>,
        /// Include patch operation details (use with --changeset).
        #[arg(long)]
        patches: bool,
    },

    /// Push all local, unpushed changesets to the remote server.
    Push,

    /// Fetch new changesets from the remote server and merge them.
    Pull,

    /// Promote changesets from the current channel to the 'main' channel.
    Promote,

    /// Manage channels (bookmarks for changesets).
    Channel {
        name: Option<String>,
        #[arg(short, long)]
        create: bool,
        #[arg(short, long)]
        list: bool,
        #[arg(short, long)]
        remote: bool,
    },
}
```

---

## 6. Remote Server Specification

The remote server's logic is updated to handle changesets as the primary unit of exchange.

### 6.1. Actor Hierarchy and Message Flow

The actor hierarchy remains the same, but the messages now carry changesets.

| Actor Group | Responsibility | Key Messages Handled |
| :--- | :--- | :--- |
| **`Api`** | Handles HTTP requests. | `HandlePush`, `HandlePull`, `HandleClone`, `HandleGetChangeset` |
| **`Changeset`** | Validates and stores incoming changesets. | `ValidateChangeset`, `UpdateChannelHead`, `GetChannelHistory` |
| **`Storage`** | Encapsulates S3 interactions. | `StoreChangeset`, `LoadChangeset`, `StorePatch`, `LoadPatch` |

### 6.2. API Endpoints

A new endpoint is added to fetch individual changesets.

*   `POST /api/v1/push`: Pushes a `Vec<Changeset>`.
*   `POST /api/v1/pull`: Pulls changesets since a given `since_change_id`.
*   `GET /api/v1/changesets/:change_id`: Fetches a single `Changeset` by its stable ID.

---

## 7. S3 Object Storage Specification

The S3 bucket layout is updated to store changesets.

```
s3://dyna-bucket/
├── changesets/
│   ├── <commit_hash>.json      # Immutable changeset objects
│   └── ...
├── patches/
│   ├── <patch_hash>.json       # Immutable patch objects
│   └── ...
├── channels/
│   ├── main.json               # Channel metadata (ordered list of change_ids)
│   └── feature-x.json
└── snapshots/
    ├── <resource_id>.json      # Latest materialized resource state
    └── ...
```

---

## 8. Conflict Resolution Specification

The conflict resolution strategy remains largely the same, but it is now framed in the context of changesets. Conflicts arise when merging changesets that contain patches modifying the same resource paths.

---

## 9. Workflow: End-to-End User Journey

The user journey is updated to reflect the new commands.

```bash
# Alice creates and commits a changeset
$ dyna add user.json
$ dyna commit -m "Add user Alice"
Created changeset: 2a8f3b4c (commit: 9f8c7b6a)

# Alice amends the message
$ dyna describe -m "Add user Alice with admin role"
Amended changeset: 2a8f3b4c (commit: e5d4c3b2)

# Alice pushes her changes
$ dyna push

# Bob pulls the changeset
$ dyna pull

# Bob views the log
$ dyna log
@ 2a8f3b4c (e5d4c3b2) Add user Alice with admin role

# Bob views the details of the changeset
$ dyna log --changeset 2a8f3b4c --patches
```

---

## 10. Rust Crate Dependency Map

(No significant changes to the dependency map)

---

## 11. Implementation Roadmap

(The roadmap is now complete, as the features have been implemented.)

---

## 12. References

[3]: https://actoromicon.rs/ "The Actoromicon (elfo-rs documentation)"
[4]: https://docs.rs/object_store "object-store Crate Documentation (docs.rs)"
[5]: https://www.allthingsdistributed.com/2021/04/s3-strong-consistency.html "Diving Deep on S3 Consistency"
[6]: https://datatracker.ietf.org/doc/html/rfc6902 "JSON Patch (RFC 6902)"
