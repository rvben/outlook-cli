# Windows PowerShell 5.1. Fixed code; untrusted request data is read only as JSON.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::InputEncoding = New-Object System.Text.UTF8Encoding($false)
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$script:refs = New-Object 'System.Collections.Generic.List[object]'
function Keep($object) {
    if ($null -ne $object) { $script:refs.Add($object) }
    return ,$object
}
function Release($object) {
    if ($null -ne $object -and [Runtime.InteropServices.Marshal]::IsComObject($object)) {
        [void][Runtime.InteropServices.Marshal]::ReleaseComObject($object)
    }
}
function Identifier($entry, $store) {
    $json = [ordered]@{entry=[string]$entry; store=[string]$store} | ConvertTo-Json -Compress
    return 'desktop:' + [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($json))
}
function Folder($spec) {
    if ($null -eq $spec) {
        $store = Keep ($script:ns.DefaultStore)
        return ,(Keep ($store.GetRootFolder()))
    }
    if ($spec -is [long] -or $spec -is [int]) {
        return ,(Keep ($script:ns.GetDefaultFolder([int]$spec)))
    }
    return ,(Keep ($script:ns.GetFolderFromID([string]$spec.entry, [string]$spec.store)))
}
function Message($mail, $storeId, $detail) {
    $result = @{
        id = Identifier $mail.EntryID $storeId
        subject = [string]$mail.Subject
        from = @{emailAddress=@{name=[string]$mail.SenderName; address=[string]$mail.SenderEmailAddress}}
        receivedDateTime = $mail.ReceivedTime.ToUniversalTime().ToString('o')
        isRead = -not $mail.UnRead
        isDraft = -not $mail.Sent
        importance = @('low','normal','high')[[int]$mail.Importance]
    }
    $attachments = $null
    try {
        $attachments = $mail.Attachments
        $result.hasAttachments = $attachments.Count -gt 0
    } finally { Release $attachments }
    if ($detail) { $result.body = @{contentType='text'; content=[string]$mail.Body} }
    return $result
}
function Mail($id) {
    $mail = Keep ($script:ns.GetItemFromID([string]$id.entry, [string]$id.store))
    if ($mail.Class -ne 43) { Invalid 'The requested item is not an email message.' }
    return ,$mail
}
function Invalid($message) { throw [ArgumentException]::new($message) }
function RequireDraft($mail) {
    if ($mail.Sent -or $mail.Submitted) { Invalid 'The requested message is not an editable draft.' }
}
function EditMail($mail, $request) {
    $recipients = Keep ($mail.Recipients)
    $type = 0
    foreach ($field in @('to','cc','bcc')) {
        $type++
        if ($null -eq $request.$field) { continue }
        for ($i = $recipients.Count; $i -ge 1; $i--) {
            $recipient = Keep ($recipients.Item($i))
            if ($recipient.Type -eq $type) { $recipients.Remove($i) }
        }
        foreach ($address in $request.$field) {
            $recipient = Keep ($recipients.Add([string]$address))
            $recipient.Type = $type
        }
    }
    if ($null -ne $request.subject) { $mail.Subject = [string]$request.subject }
    if ($null -ne $request.body) { $mail.BodyFormat = 1; $mail.Body = [string]$request.body }
}
function SendMail($mail) {
    $recipients = Keep ($mail.Recipients)
    if ($recipients.Count -eq 0 -or -not $recipients.ResolveAll()) {
        Invalid 'The message needs recipients that Outlook can resolve.'
    }
    $mail.Send()
}
function SavedMessage($mail) {
    $parent = Keep ($mail.Parent)
    return (Message $mail $parent.StoreID $true)
}
try {
    $request = [Console]::In.ReadToEnd() | ConvertFrom-Json
    $app = Keep (New-Object -ComObject Outlook.Application)
    $script:ns = Keep ($app.GetNamespace('MAPI'))
    switch ($request.operation) {
        'probe' {
            $store = Keep ($script:ns.DefaultStore)
            $root = Keep ($store.GetRootFolder())
            $result = @{available=$true; outlookProfile=[string]$script:ns.CurrentProfileName; defaultStore=[string]$store.DisplayName; rootFolderId=(Identifier $root.EntryID $root.StoreID)}
        }
        'read' {
            $mail = Mail $request.id
            $result = Message $mail $request.id.store $true
        }
        { $_ -in 'send','draft_create' } {
            $mail = Keep ($app.CreateItem(0))
            EditMail $mail $request
            if ($request.operation -eq 'send') {
                SendMail $mail
                $result = @{sent=$true; to=@($request.to); cc=@($request.cc); bcc=@($request.bcc); subject=[string]$request.subject}
            } else {
                $mail.Save()
                $result = SavedMessage $mail
            }
        }
        { $_ -in 'mark_read','move','delete','reply','draft_update','draft_send','draft_delete' } {
            $mail = Mail $request.id
            $id = Identifier $request.id.entry $request.id.store
            if ($request.operation -like 'draft_*') { RequireDraft $mail }
            switch ($request.operation) {
                'mark_read' {
                    $mail.UnRead = -not [bool]$request.read
                    $mail.Save()
                    $result = SavedMessage $mail
                }
                'move' {
                    $destination = Folder $request.folder
                    if ($destination.DefaultItemType -ne 0) { Invalid 'The destination is not a mail folder.' }
                    $moved = Keep ($mail.Move($destination))
                    $result = SavedMessage $moved
                }
                { $_ -in 'delete','draft_delete' } {
                    $mail.Delete()
                    $result = @{deleted=$true; message_id=$id}
                }
                'reply' {
                    if ($request.all) { $reply = Keep ($mail.ReplyAll()) }
                    else { $reply = Keep ($mail.Reply()) }
                    $reply.BodyFormat = 1
                    $reply.Body = [string]$request.body + "`r`n`r`n" + [string]$reply.Body
                    SendMail $reply
                    $result = @{sent=$true; message_id=$id; reply_all=[bool]$request.all}
                }
                'draft_update' {
                    EditMail $mail $request
                    $mail.Save()
                    $result = SavedMessage $mail
                }
                'draft_send' {
                    SendMail $mail
                    $result = @{sent=$true; draft_id=$id}
                }
            }
        }
        { $_ -in 'list','search','folders' } {
            $folder = Folder $request.folder
            $rows = New-Object 'System.Collections.Generic.List[object]'
            $position = [int]$request.offset
            $scanned = 0
            $watch = [Diagnostics.Stopwatch]::StartNew()
            if ($request.operation -eq 'folders') {
                $items = Keep ($folder.Folders)
            } else {
                if ($folder.DefaultItemType -ne 0) { throw 'This is not a mail folder.' }
                $items = Keep ($folder.Items)
                $items.Sort('[ReceivedTime]', $true)
            }
            $count = $items.Count
            while ($position -lt $count -and $rows.Count -lt [int]$request.limit -and $scanned -lt 1000 -and $watch.Elapsed.TotalSeconds -lt 30) {
                $position++; $scanned++
                $item = $null
                try {
                    $item = $items.Item($position)
                    if ($request.operation -eq 'folders') {
                        # Skip calendar/contact folders; listing remains a mail operation.
                        if ($item.DefaultItemType -ne 0) { continue }
                        $children = $null; $contents = $null
                        try {
                            $children = $item.Folders; $contents = $item.Items
                            $rows.Add(@{id=(Identifier $item.EntryID $item.StoreID); displayName=[string]$item.Name; childFolderCount=$children.Count; totalItemCount=$contents.Count; unreadItemCount=$item.UnReadItemCount})
                        } finally { Release $contents; Release $children }
                    } elseif ($item.Class -eq 43) {
                        if ($request.operation -eq 'search') {
                            $term = [string]$request.query
                            $comparison = [StringComparison]::OrdinalIgnoreCase
                            if (([string]$item.Subject).IndexOf($term, $comparison) -lt 0 -and ([string]$item.SenderName).IndexOf($term, $comparison) -lt 0 -and ([string]$item.SenderEmailAddress).IndexOf($term, $comparison) -lt 0) { continue }
                        }
                        $rows.Add((Message $item $folder.StoreID $false))
                    }
                } finally { Release $item }
            }
            $next = $null
            if ($position -lt $count) { $next = $position }
            $result = @{items=@($rows.ToArray()); next_offset=$next}
        }
        default { throw 'Unsupported bridge operation.' }
    }
    [Console]::Out.WriteLine(($result | ConvertTo-Json -Depth 12 -Compress))
} catch {
    $kind = 'desktop_error'
    $exception = $_.Exception
    while ($null -ne $exception) {
        if ($exception -is [ArgumentException]) { $kind = 'invalid_input' }
        if ($exception.HResult -eq -2147221233) { $kind = 'not_found' }
        $exception = $exception.InnerException
    }
    $message = $_.Exception.Message
    if ($kind -eq 'desktop_error') { $message += ' Check classic Outlook and its Windows profile for security prompts. A write may have completed; inspect Outlook before retrying.' }
    $failure = @{error=@{kind=$kind; message=$message}}
    [Console]::Out.WriteLine(($failure | ConvertTo-Json -Depth 4 -Compress))
    exit 1
} finally {
    for ($i = $script:refs.Count - 1; $i -ge 0; $i--) { Release $script:refs[$i] }
    # Never Quit Outlook: the running application belongs to the user.
}
