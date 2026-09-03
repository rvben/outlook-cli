# outlook-cli

Microsoft Outlook from your terminal—for humans and agents.

`outlook-cli` is an early Rust CLI for Outlook mail and calendars through the
supported Microsoft Graph API. It aims to provide calm terminal workflows for
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
cargo test
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

## Commands

```console
outlook inbox --limit 20
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
outlook doctor --offline
outlook schema --command 'mail send'
```

Message and event requests opt into Graph's immutable Outlook IDs. Remote
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
