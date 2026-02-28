# VFS Crate API Notes (v0.12.2)

## Creating roots
- `VfsPath::new(PhysicalFS::new("/some/path"))` - physical FS rooted at path
- `VfsPath::new(MemoryFS::new())` - in-memory FS for tests
- `let root: VfsPath = PhysicalFS::new(dir).into();`

## Key VfsPath methods (all return VfsResult)
- `path.join("subpath")? -> VfsPath` - join path segment
- `path.exists()? -> bool`
- `path.is_dir()? -> bool`
- `path.is_file()? -> bool`
- `path.create_dir()? -> ()` - create single dir
- `path.create_dir_all()? -> ()` - create dir and parents
- `path.create_file()? -> Box<dyn SeekAndWrite>` - create/overwrite file, returns writer
- `path.open_file()? -> Box<dyn SeekAndRead>` - open for reading
- `path.append_file()? -> Box<dyn SeekAndWrite>` - open for appending
- `path.read_to_string()? -> String` - convenience read
- `path.remove_file()? -> ()`
- `path.remove_dir()? -> ()`
- `path.remove_dir_all()? -> ()`
- `path.read_dir()? -> Box<dyn Iterator<Item = VfsPath>>` - list directory
- `path.walk_dir()? -> WalkDirIterator` - recursive walk
- `path.parent() -> VfsPath` - parent (root returns itself)
- `path.filename() -> String`
- `path.extension() -> Option<String>`
- `path.as_str() -> &str` - string representation

## Writing files
```rust
use std::io::Write;
write!(path.create_file()?, "content")?;
// or
path.create_file()?.write_all(b"content")?;
```

## Reading files
```rust
use std::io::Read;
let content = path.read_to_string()?;
// or
let mut content = String::new();
path.open_file()?.read_to_string(&mut content)?;
```

## Key differences from std::fs
- `exists()` returns `VfsResult<bool>` not `bool`
- `is_dir()` returns `VfsResult<bool>` not `bool`
- `read_dir()` returns iterator of `VfsPath` not `DirEntry`
- No `read_to_string` as free function - it's a method on VfsPath
- Writing: `create_file()` returns a writer, use `write!()` or `.write_all()`
- Errors are `VfsError` not `io::Error`

## Mapping errors
- `VfsError` implements `std::error::Error`
- Can use `.map_err(|e| anyhow::anyhow!("{}", e))` or implement From
