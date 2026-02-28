# Dyna CLI Enhancements

- [ ] Status command: detect and display deleted tracked files (files with snapshots but missing from filesystem)
- [ ] Stage file removals: allow `dyna add <deleted-file>` to stage a deletion patch (null value)
- [ ] Restore command: revert a file to its snapshot from a specific channel or changeset
  - [ ] `dyna restore <file> --channel <name>` — restore from channel head snapshot
  - [ ] `dyna restore <file> --changeset <id>` — restore from a specific changeset in any channel
  - [ ] Default (no flags): restore from current channel's latest snapshot
- [ ] Register restore subcommand in main.rs CLI entry point
- [ ] Compile successfully
- [ ] Push to feat/changeset-model
