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
class MockStore {
    [string] $DisplayName = 'Test mailbox'
    [MockFolder] $Root
    [object] GetRootFolder() { return $this.Root }
}
class MockNamespace {
    [string] $CurrentProfileName = 'Test profile'
    [MockStore] $DefaultStore
    [MockFolder] $Inbox
    [object] GetDefaultFolder([int] $id) { return $this.Inbox }
    [object] GetFolderFromID([string] $entry, [string] $store) {
        if ($entry -ne 'F001' -or $store -ne 'AABB') { throw 'Folder not found' }
        return $this.Inbox
    }
    [object] GetItemFromID([string] $entry, [string] $store) {
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
}
function New-MockApplication {
    $mail = [pscustomobject]@{
        Class=43; EntryID='AB01'; Subject="O'Brien [report]"; SenderName='Test sender';
        SenderEmailAddress='sender@example.com'; ReceivedTime=[datetime]'2026-09-01T12:00:00Z';
        UnRead=$true; Sent=$true; Importance=1; Attachments=[pscustomobject]@{Count=0}; Body='Full message body'
    }
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
