# Changelog

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
