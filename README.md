# outlook-cli

Microsoft Outlook from your terminal—for humans and agents.

`outlook-cli` is an early Rust CLI for Outlook mail and calendars through the
supported Microsoft Graph API, with an optional classic Outlook desktop backend
for Windows and WSL. It aims to provide calm terminal workflows for
people and a deterministic, introspectable command contract for automation.

## Current milestone

- Work/school and personal Microsoft accounts through delegated OAuth
- Tokens protected by the operating system credential store
- Mail listing, reading, search, sending, replying, moving, deletion, and read-state updates
- Draft lifecycle and attachment upload/download up to 150 MiB
- Calendar agenda and event creation
- Text output on a terminal and JSON when piped
- Read-only profiles, bounded collections, stable errors, and CLI Spec v0.3

## Build

```console
cargo build
cargo test --locked --all-targets
```

Windows tests also execute the bundled PowerShell bridge against mock Outlook
objects, without accessing a mailbox. On other platforms this test can be run
with a portable PowerShell installation:

```console
OUTLOOK_TEST_POWERSHELL=/path/to/pwsh cargo test --test desktop_bridge -- --ignored
```

## Install

```console
cargo install outlook-cli --locked
# or, without a Rust toolchain
uv tool install outlook-cli-rs
```

Both packages install the `outlook` executable.

## Configure

`outlook-cli` includes a maintained multitenant Microsoft Entra public-client
registration, so the normal setup is simply:

```console
outlook init
```

The registration requests delegated Microsoft Graph permissions only:
`User.Read`, `Mail.ReadWrite`, `Mail.Send`, and `Calendars.ReadWrite`. A
read-only profile requests `Mail.Read` and `Calendars.Read` instead.

For a staged or headless setup:

```console
outlook init --no-login
outlook auth login
```

Read-only profiles also block remote writes locally:

```console
outlook init --read-only
```

Organizations with restrictive consent policies can use their own public-client
registration. Enable public client flows and supply its Application (client) ID:

```console
outlook init --client-id YOUR_APPLICATION_ID
```

The same override is available through `OUTLOOK_CLIENT_ID`. Tenant policy can
still require administrator approval, and the maintained registration is not
yet publisher verified while the project is in its early development phase.

For short-lived automation, `OUTLOOK_ACCESS_TOKEN` overrides stored
credentials.

## Classic Outlook on Windows / WSL

Use the same executable with a desktop profile:

```console
outlook init --profile local --backend desktop
outlook --profile local inbox --limit 10
outlook --profile local mail folders
outlook --profile local mail folders --parent inbox
outlook --profile local mail list --folder sentitems
outlook --profile local mail search 'quarterly report' --limit 20
outlook --profile local mail read MESSAGE_ID
outlook --profile local doctor
```

Requires **classic Outlook**, a configured and signed-in Windows Outlook profile,
and `powershell.exe` on PATH. From WSL, Windows executable interop must be enabled.
New Outlook does not support COM/OOM or MAPI. See Microsoft's
[Outlook feature comparison](https://support.microsoft.com/en-us/outlook/getstarted/feature-comparison-between-new-outlook-and-classic-outlook).

Desktop initialization checks the connection; `--no-login` saves a profile without
launching Outlook, including when preparing configuration on another platform.
Authentication is managed in Windows Outlook. No Graph OAuth registration or token
is used. `auth status` verifies desktop access; `auth status --offline` only reports
configuration and leaves sign-in state unknown. `doctor --offline` checks platform
and PowerShell availability without launching Outlook.

The CLI uses Outlook's active Windows profile and default store. A CLI profile
selects the backend; it does **not** switch the Windows Outlook profile or account.
Well-known folders resolve in the default store. `mail folders` lists its top-level
mail folders; use `--parent` with a returned ID to browse further. Supported names
are `inbox`, `sentitems`, `deleteditems`, `outbox`, `drafts`, and `junkemail`.

Desktop currently supports folder listing, inbox/folder mail listing, message
reading, search, and draft listing. Writes, attachments, calendar commands, and
`whoami` return `unsupported`; read-only profiles also retain their write guard.
There is no automatic fallback to Graph. Existing profiles without a backend field
continue to use Graph, which remains the default for `init`.

Desktop search is **case-insensitive literal text** in subject, sender name, or
sender address, limited to one folder (inbox by default). KQL and regular expressions
are not interpreted. Sender addresses may be Exchange legacy addresses as exposed
by Outlook. Results reflect the desktop client's available/synchronized data.

Use JSON output to obtain message IDs and continuation tokens. Desktop IDs encode
both EntryID and StoreID, are unrelated to Graph IDs, and can change after moves.
Pages scan at most 1,000 items or about 30 seconds before returning a continuation;
a search page can be empty with more items still to scan. Pass `--cursor` with the
same command, folder, and query to continue. Pagination uses item positions, so
mailbox changes between calls can cause skipped or repeated results. Restart listing
after switching the Windows Outlook profile or account.

The bundled PowerShell bridge receives input as JSON on stdin, returns JSON, and
releases COM references without quitting Outlook. The CLI waits up to 45 seconds;
Outlook may show Windows profile or security prompts. A timeout terminates the
local bridge process, but WSL interop can leave a Windows-side process or prompt
running; inspect Windows if a call times out.

## Commands

```console
outlook inbox --limit 20
outlook mail folders
outlook mail read MESSAGE_ID
outlook mail search 'subject:"quarterly report"' --limit 20
outlook mail mark-read MESSAGE_ID
printf 'All set.' | outlook mail reply MESSAGE_ID --body -
outlook mail send --to person@example.com --bcc archive@example.com \
  --subject 'Hello' --body 'Hi'
outlook mail move MESSAGE_ID --destination archive
outlook mail delete MESSAGE_ID --yes
outlook mail draft create --to person@example.com --subject 'Report' --body 'Attached.'
outlook mail attachment add DRAFT_ID ./report.pdf --content-type application/pdf
outlook mail attachment list DRAFT_ID
outlook mail attachment download MESSAGE_ID ATTACHMENT_ID ./report.pdf
outlook mail draft send DRAFT_ID
outlook calendar agenda --start 2026-09-03T00:00:00Z --end 2026-09-10T00:00:00Z
outlook calendar create --subject 'Project sync' \
  --start 2026-09-04T09:00:00 --end 2026-09-04T09:30:00 \
  --timezone Europe/Amsterdam --attendee person@example.com
outlook auth status                 # verify the selected credential
outlook auth status --offline       # inspect local credential state only
outlook profile list
outlook profile use work
outlook profile remove old --yes
outlook config show
outlook config path
outlook doctor --offline
outlook schema --command 'mail send'
```

Graph message and event requests opt into immutable Outlook IDs. Remote
writes are blocked when the active profile is read-only. Destructive commands
confirm on a terminal and require `--yes` in automation. Attachment downloads
never overwrite unless `--force` is supplied; uploads switch automatically to
Microsoft's resumable upload sessions at 3 MiB. Stdout contains data;
diagnostics and sign-in instructions go to stderr.

## Status

The command surface is under active development. A keyboard-first TUI, rich
HTML composition, inline attachments, meeting responses, contacts, and delta
synchronization are planned after the core Graph and authentication contracts
settle.
