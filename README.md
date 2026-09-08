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
# or, using a prebuilt wheel (supported platforms below)
uv tool install outlook-cli-rs
```

Both packages install the `outlook` executable.

Starting with 0.2.3, PyPI wheels cover Windows x64, macOS Intel/Apple Silicon,
and Linux x64/ARM64 (glibc 2.17+ or musl 1.2+). WSL uses the Linux wheel.
These wheels contain the compiled executable, so Rust is not needed to install
or run them. Other platforms require Rust and native build tools to build the
source distribution. To require a prebuilt package and fail instead of compiling:

```sh
uv tool install --no-build outlook-cli-rs
```

See [the release process](docs/releases.md) for the build matrix and publication checks.


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

Desktop supports folder and message listing, reading, search, send, reply/reply-all,
move, delete, mark-read/unread, and draft list/create/update/send/delete. Attachments,
calendar commands, and `whoami` return `unsupported`. Read-only profiles block writes;
deleting messages or drafts requires confirmation (or `--yes`).

New messages and replies use plain text and Outlook's configured sending account.
A successful send means Outlook accepted the message for sending; offline Outlook
may queue it. Draft updates preserve omitted fields; `--clear-to`, `--clear-cc`,
and `--clear-bcc` remove the corresponding recipients. Draft update/send/delete
reject sent messages and messages already submitted for sending. Moves return the
message's new desktop ID; use that ID for subsequent commands. Delete follows
Outlook's behavior, including permanent deletion from Deleted Items. If a write
times out or the bridge fails, check Outlook before retrying: it may have completed.
There is no automatic fallback to Graph. Existing profiles without a backend field
continue to use Graph, which remains the default for `init`.

Desktop search is **case-insensitive literal text** in subject, sender name, or
sender address, limited to one folder (inbox by default). KQL and regular expressions
are not interpreted. Sender addresses may be Exchange legacy addresses as exposed
by Outlook. Results reflect the desktop client's available/synchronized data.

Text output includes copyable message IDs and continuation tokens; JSON exposes
the same values as structured fields. Desktop IDs encode
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

## Interactive inbox

```console
outlook tui                         # Browse the active profile's inbox
outlook tui --folder sentitems       # Open another folder
outlook --profile local tui          # Classic Outlook on Windows/WSL
outlook tui --demo                   # Try sample mail without signing in
outlook tui --demo --snapshot        # Print an offline sample screen
```

The browser is read-only: previewing a message does not mark it as read.
Wide terminals show a message list and a reading pane; below 100 columns,
Enter or Tab switches between them. The minimum usable size is 36 × 12.
Colors follow your terminal palette and respect `--no-color` and `NO_COLOR`.

| Key | Action |
| --- | --- |
| ↑ / ↓ or `j` / `k` | Select a message, or scroll the focused preview |
| Enter / Tab | Switch between list and preview |
| Page Up / Page Down | Scroll the message by ten lines |
| Home / End | Jump to the beginning or end of the focused preview |
| `/` | Search the current folder; Enter applies, Esc cancels |
| Esc in the list | Clear the search, or quit if no search is active |
| `f` | Browse folders; Enter opens a folder's mail |
| → / ← in folders | Browse children / return to the parent |
| `n` | Load the next page when available |
| `r` | Refresh the focused pane, or retry a failed request |
| `?` | Open the keyboard guide |
| `q` / Ctrl+C | Quit |

Graph searches use Outlook syntax; desktop searches use literal subject/sender
text. Folder browsing includes child folders and pagination. An empty desktop
search page can still have more matches to scan; press `n` when offered.
Reads run asynchronously, with a timeout and cancellable previews. On WSL,
the desktop bridge's existing Windows-process cleanup limitations still apply.
Sign in with `outlook init` or `outlook auth login` before opening a live inbox.
Interactive mode requires terminal input and output; use the regular commands
and JSON for scripts. `--snapshot` always prints plain sample text and supports
`--width` (36–240) and `--height` (12–100).

## Terminal experience

Run `outlook` in a terminal for a quick-start guide. Mail lists show full subjects,
sender details, unread and attachment indicators, and copyable IDs. Message reading
includes recipients and preserves body paragraphs. Folders show total and unread
counts; attachments show readable sizes; agendas include timezone and location
when supplied by the backend. Paginated text output includes the next cursor,
including when a desktop search returns an empty page with more items to scan.

Headings use a restrained cyan accent on terminals. Set `NO_COLOR=1` or pass
`--no-color` to disable colors. Redirected text has no color escapes. Mail views
remove terminal control sequences from remote content before displaying it.

```console
outlook inbox --output text          # Readable output even when redirected
outlook inbox --output json          # Structured records for scripts
outlook --no-color mail read MESSAGE_ID
```

## Status

The command surface is under active development. Rich
HTML composition, inline attachments, meeting responses, contacts, and delta
synchronization are planned after the core Graph and authentication contracts
settle.
