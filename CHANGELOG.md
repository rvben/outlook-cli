# Changelog

## 0.2.3 - 2026-09-08

### Fixed

- Ship prebuilt Python wheels for Windows x64, Intel and Apple Silicon macOS, and x64/ARM64 Linux (glibc and musl), so supported `uv` installations do not need Rust.
- Build and verify the full platform matrix before publishing, including `uv` installation with source builds disabled and standalone executable smoke tests.
- Provide executable archives and SHA-256 checksums for every wheel target.
- Clarify that source installations on other platforms require Rust and native build tools.

## 0.2.2 - 2026-09-08

### Added

- Desktop mail send, reply/reply-all, move, delete, and mark-read/unread actions.
- Desktop draft creation, partial updates, sending, and deletion, including recipient clearing and checks against modifying sent or submitted messages.
- Improved terminal mail views and command help.

### Fixed

- Short output-format flags now control parse-error formatting.

### Compatibility

- Desktop writes respect read-only profiles and deletion confirmation; moves return the new message ID.
- Attachments and calendar commands remain Graph-only.
- Desktop bridge tests pass with mock Outlook objects; live Windows/WSL COM validation remains outstanding.

## 0.2.1 - 2026-09-08

### Added

- Optional classic Outlook desktop backend for Windows and WSL, selected per CLI profile.
- Desktop folder browsing, message listing and reading, literal subject/sender search, and draft listing.
- Folder listing for Microsoft Graph, including child folders and pagination.
- Desktop diagnostics, opaque store/message IDs, bounded pagination, JSON transport, and bridge timeouts.

### Compatibility

- Existing profiles continue to use Microsoft Graph by default.
- Desktop access requires classic Outlook and Windows PowerShell; new Outlook is unsupported.
- Desktop writes, attachments, and calendar operations are not yet supported.
- Desktop integration was tested with mock Outlook objects; live Windows/WSL COM validation remains outstanding.

This release also includes the previously unpublished 0.2.0 work: expanded Graph
mail and attachment workflows, streamlined authentication, and the draft-send
content-length fix.
