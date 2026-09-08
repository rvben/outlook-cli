# Test-only Outlook object model. This code never creates a COM object.
class MockItems {
    [object[]] $Rows
    [int] $Count
    MockItems([object[]] $rows) { $this.Rows = $rows; $this.Count = $rows.Count }
    [object] Item([int] $index) { return $this.Rows[$index - 1] }
    [void] Sort([string] $field, [bool] $descending) {
        $this.Rows = @($this.Rows | Sort-Object ReceivedTime -Descending)
    }
}
class MockFolder {
    [string] $EntryID = 'F001'
    [string] $StoreID = 'AABB'
    [string] $Name = 'Inbox'
    [int] $DefaultItemType = 0
    [int] $UnReadItemCount = 1
    [MockItems] $Items
    [MockItems] $Folders
}
class MockRecipient {
    [string] $Address
    [int] $Type = 1
    MockRecipient([string] $address) { $this.Address = $address }
}
class MockRecipients {
    [System.Collections.Generic.List[object]] $Rows = [System.Collections.Generic.List[object]]::new()
    [int] $Count = 0
    [object] Item([int] $index) { return $this.Rows[$index - 1] }
    [void] Remove([int] $index) { $this.Rows.RemoveAt($index - 1); $this.Count-- }
    [object] Add([string] $address) {
        $recipient = [MockRecipient]::new($address)
        $this.Rows.Add($recipient); $this.Count++
        return $recipient
    }
    [bool] ResolveAll() { return -not ($this.Rows.Address -contains 'unresolved@example.com') }
}
class MockMail {
    [int] $Class = 43
    [string] $EntryID = 'AB01'
    [string] $Subject = "O'Brien [report]"
    [string] $SenderName = 'Test sender'
    [string] $SenderEmailAddress = 'sender@example.com'
    [datetime] $ReceivedTime = [datetime]'2026-09-01T12:00:00Z'
    [bool] $UnRead = $true
    [bool] $Sent = $true
    [bool] $Submitted = $false
    [int] $Importance = 1
    [object] $Attachments = [pscustomobject]@{Count=0}
    [string] $Body = 'Full message body'
    [int] $BodyFormat = 1
    [MockRecipients] $Recipients = [MockRecipients]::new()
    [MockFolder] $Parent = [MockFolder]::new()
    [string] $ReplyMode = ''
    [void] Record([string] $action) {
        $state = @{action=$action; subject=$this.Subject; body=$this.Body; unread=$this.UnRead;
            recipients=@($this.Recipients.Rows.ToArray()); reply=$this.ReplyMode; entry=$this.EntryID; store=$this.Parent.StoreID}
        [IO.File]::WriteAllText($env:OUTLOOK_MOCK_STATE, ($state | ConvertTo-Json -Depth 8))
    }
    [void] Save() { $this.Record('save') }
    [void] Send() { $this.Record('send'); $this.Sent=$true }
    [void] Delete() { $this.Record('delete') }
    [object] Move([MockFolder] $folder) {
        $this.EntryID='AB99'; $this.Parent=$folder; $this.Record('move'); return $this
    }
    [object] Reply() {
        $reply = [MockMail]::new(); $reply.Sent=$false; $reply.ReplyMode='reply'
        [void]$reply.Recipients.Add('sender@example.com'); return $reply
    }
    [object] ReplyAll() {
        $reply = $this.Reply(); $reply.ReplyMode='all'
        [void]$reply.Recipients.Add('copy@example.com'); return $reply
    }
}
class MockStore {
    [string] $DisplayName = 'Test mailbox'
    [MockFolder] $Root
    [object] GetRootFolder() { return $this.Root }
}
class MockNamespace {
    [string] $CurrentProfileName = 'Test profile'
    [MockStore] $DefaultStore
    [MockFolder] $Inbox
    [object] GetDefaultFolder([int] $id) {
        if ($id -eq 6) { return $this.Inbox }
        $folder = [MockFolder]::new(); $folder.EntryID='F002'; return $folder
    }
    [object] GetFolderFromID([string] $entry, [string] $store) {
        if ($entry -eq 'F003' -and $store -eq 'CCDD') {
            $folder = [MockFolder]::new(); $folder.EntryID=$entry; $folder.StoreID=$store; return $folder
        }
        if ($entry -eq 'F004') { $folder = [MockFolder]::new(); $folder.DefaultItemType=1; return $folder }
        if ($entry -ne 'F001' -or $store -ne 'AABB') { throw 'Folder not found' }
        return $this.Inbox
    }
    [object] GetItemFromID([string] $entry, [string] $store) {
        if ($store -eq 'AABB' -and $entry -in 'DA01','DA02','DA03') {
            $draft = [MockMail]::new(); $draft.Sent=$false; $draft.EntryID=$entry
            if ($entry -eq 'DA02') { $draft.Submitted=$true }
            if ($entry -ne 'DA03') {
                [void]$draft.Recipients.Add('old@example.com')
                $cc = $draft.Recipients.Add('copy@example.com'); $cc.Type=2
                $bcc = $draft.Recipients.Add('hidden@example.com'); $bcc.Type=3
            }
            return $draft
        }
        if ($entry -eq 'CA01') { $item = [MockMail]::new(); $item.Class=26; return $item }
        if ($store -eq 'AABB') {
            foreach ($mail in $this.Inbox.Items.Rows) {
                if ($mail.EntryID -eq $entry) { return $mail }
            }
        }
        throw [Runtime.InteropServices.COMException]::new('Item not found', -2147221233)
    }
}
class MockApplication {
    [MockNamespace] $Namespace
    [object] GetNamespace([string] $name) { return $this.Namespace }
    [object] CreateItem([int] $type) {
        $draft = [MockMail]::new(); $draft.Sent=$false; $draft.EntryID='DA01'; return $draft
    }
}
function New-MockApplication {
    $mail = [MockMail]::new()
    $other = [pscustomobject]@{
        Class=43; EntryID='AB02'; Subject='Older message'; SenderName='Another sender';
        SenderEmailAddress='another@example.com'; ReceivedTime=[datetime]'2026-08-01T12:00:00Z';
        UnRead=$false; Sent=$true; Importance=2; Attachments=[pscustomobject]@{Count=2}; Body='Other body'
    }
    $rows = @($mail, $other)
    # Non-mail items exercise skipping and the scan budget.
    for ($i=0; $i -lt 1000; $i++) {
        $rows += [pscustomobject]@{Class=26; ReceivedTime=[datetime]'2026-07-01T00:00:00Z'}
    }
    $inbox = [MockFolder]::new()
    $inbox.Items = [MockItems]::new($rows)
    $inbox.Folders = [MockItems]::new(@())
    $root = [MockFolder]::new()
    $root.EntryID = 'F000'; $root.Name = 'Root'
    $root.Folders = [MockItems]::new(@($inbox))
    $store = [MockStore]::new(); $store.Root = $root
    $namespace = [MockNamespace]::new(); $namespace.DefaultStore = $store; $namespace.Inbox = $inbox
    $app = [MockApplication]::new(); $app.Namespace = $namespace
    return $app
}
