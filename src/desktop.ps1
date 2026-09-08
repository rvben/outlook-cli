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
            $mail = Keep ($script:ns.GetItemFromID([string]$request.id.entry, [string]$request.id.store))
            if ($mail.Class -ne 43) { throw 'The requested item is not an email message.' }
            $result = Message $mail $request.id.store $true
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
        if ($exception.HResult -eq -2147221233) { $kind = 'not_found' }
        $exception = $exception.InnerException
    }
    $failure = @{error=@{kind=$kind; message=($_.Exception.Message + ' Check that classic Outlook is installed and its Windows profile is signed in; new Outlook is unsupported.')}}
    [Console]::Out.WriteLine(($failure | ConvertTo-Json -Depth 4 -Compress))
    exit 1
} finally {
    for ($i = $script:refs.Count - 1; $i -ge 0; $i--) { Release $script:refs[$i] }
    # Never Quit Outlook: the running application belongs to the user.
}
