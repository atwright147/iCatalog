$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$scratch = Join-Path $env:RUNNER_TEMP 'iCatalog association test with spaces'
$other = Join-Path $env:RUNNER_TEMP 'iCatalog other association folder'
New-Item -ItemType Directory -Force -Path $scratch | Out-Null
New-Item -ItemType Directory -Force -Path $other | Out-Null
$exe = Join-Path $scratch 'icatalog-ui.exe'
$helper = Join-Path $scratch 'associate.ps1'
Copy-Item -LiteralPath 'packaging/windows/associate.ps1' -Destination $helper
Copy-Item -LiteralPath 'packaging/windows/associate.ps1' -Destination (Join-Path $other 'associate.ps1')
[System.IO.File]::WriteAllBytes($exe, [byte[]]@(0))
$classes = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software\Classes')
$software = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software')
$extension = $classes.CreateSubKey('.icatalog')
$defaultBefore = $extension.GetValue('', $null)
$extension.Close()
try {
    & $helper Register
    $type = $classes.OpenSubKey('iCatalog.Catalogue\shell\open\command')
    if ($null -eq $type) { throw 'Missing iCatalog open command' }
    $expected = '"{0}" "%1"' -f $exe
    if ($type.GetValue('') -cne $expected) { throw "Unexpected open command: $($type.GetValue(''))" }
    $type.Close()
    $openWith = $classes.OpenSubKey('.icatalog\OpenWithProgids')
    if ($null -eq $openWith -or -not ($openWith.GetValueNames() -ccontains 'iCatalog.Catalogue')) {
        throw 'Missing Open with registration'
    }
    $openWith.Close()
    $capabilities = $software.OpenSubKey('iCatalog\Capabilities\FileAssociations')
    if ($null -eq $capabilities -or $capabilities.GetValue('.icatalog') -cne 'iCatalog.Catalogue') {
        throw 'Missing Default apps registration'
    }
    $capabilities.Close()
    $registered = $software.OpenSubKey('RegisteredApplications')
    if ($null -eq $registered -or $registered.GetValue('iCatalog') -cne 'Software\iCatalog\Capabilities') {
        throw 'Missing RegisteredApplications entry'
    }
    $registered.Close()
    $extension = $classes.OpenSubKey('.icatalog')
    if ($extension.GetValue('', $null) -cne $defaultBefore) {
        throw 'Registration changed the default file handler'
    }
    $extension.Close()
    $refused = $false
    try { & (Join-Path $other 'associate.ps1') Unregister } catch {
        $refused = $_.Exception.Message -like '*belongs to another folder*'
    }
    if (-not $refused) { throw 'Unregister from another folder removed this installation' }
    if ($null -eq $classes.OpenSubKey('iCatalog.Catalogue')) {
        throw 'Another folder removed the registered file type'
    }
} finally {
    & $helper Unregister
    $classes.Close()
    $software.Close()
    Remove-Item -LiteralPath $scratch -Recurse -Force
    Remove-Item -LiteralPath $other -Recurse -Force
}
$classes = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Classes')
$software = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software')
try {
    if ($null -ne $classes.OpenSubKey('iCatalog.Catalogue')) { throw 'Unregister left the ProgID behind' }
    $openWith = $classes.OpenSubKey('.icatalog\OpenWithProgids')
    if ($null -ne $openWith) {
        if ($openWith.GetValueNames() -ccontains 'iCatalog.Catalogue') { throw 'Unregister left Open with behind' }
        $openWith.Close()
    }
    if ($null -ne $software.OpenSubKey('iCatalog\Capabilities')) { throw 'Unregister left capabilities behind' }
} finally {
    $classes.Close()
    $software.Close()
}
