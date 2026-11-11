param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Register', 'Unregister')]
    [string]$Action
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$progId = 'iCatalog.Catalogue'
$exe = Join-Path $PSScriptRoot 'icatalog-ui.exe'
$classes = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software\Classes')
$software = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software')
if ($null -eq $classes -or $null -eq $software) {
    throw 'Could not open the current user registry.'
}
try {
    if ($Action -eq 'Register') {
        if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) {
            throw "Place associate.ps1 beside icatalog-ui.exe before registering. Missing: $exe"
        }
        $exe = (Resolve-Path -LiteralPath $exe).ProviderPath
        $command = '"{0}" "%1"' -f $exe
        $type = $classes.CreateSubKey($progId)
        $type.SetValue('', 'iCatalog Catalogue')
        $icon = $type.CreateSubKey('DefaultIcon')
        $icon.SetValue('', '"{0}",0' -f $exe)
        $icon.Close()
        $open = $type.CreateSubKey('shell\open\command')
        $open.SetValue('', $command)
        $open.Close()
        $type.Close()

        $openWith = $classes.CreateSubKey('.icatalog\OpenWithProgids')
        $openWith.SetValue($progId, [byte[]]@(), [Microsoft.Win32.RegistryValueKind]::None)
        $openWith.Close()

        $capabilities = $software.CreateSubKey('iCatalog\Capabilities')
        $capabilities.SetValue('ApplicationName', 'iCatalog')
        $capabilities.SetValue('ApplicationDescription', 'Native media catalogue and organiser')
        $capabilities.SetValue('InstallPath', $exe)
        $fileTypes = $capabilities.CreateSubKey('FileAssociations')
        $fileTypes.SetValue('.icatalog', $progId)
        $fileTypes.Close()
        $capabilities.Close()
        $registered = $software.CreateSubKey('RegisteredApplications')
        $registered.SetValue('iCatalog', 'Software\iCatalog\Capabilities')
        $registered.Close()
        Write-Output 'iCatalog is available for .icatalog in Open with and Settings > Apps > Default apps. Choose it there to make it the default.'
    } else {
        $capabilities = $software.OpenSubKey('iCatalog\Capabilities')
        if ($null -ne $capabilities) {
            $owner = [string]$capabilities.GetValue('InstallPath', '')
            $capabilities.Close()
            if (-not [string]::Equals($owner, $exe, [System.StringComparison]::OrdinalIgnoreCase)) {
                throw 'This iCatalog registration belongs to another folder. Run its associate.ps1 to unregister it.'
            }
        } else {
            $type = $classes.OpenSubKey($progId)
            if ($null -ne $type) {
                $type.Close()
                throw 'Found an iCatalog file type without an install path; refusing to remove it.'
            }
        }
        $registered = $software.OpenSubKey('RegisteredApplications', $true)
        if ($null -ne $registered) {
            if ($registered.GetValue('iCatalog') -eq 'Software\iCatalog\Capabilities') {
                $registered.DeleteValue('iCatalog', $false)
            }
            $registered.Close()
        }
        $software.DeleteSubKeyTree('iCatalog\Capabilities', $false)
        $app = $software.OpenSubKey('iCatalog', $true)
        if ($null -ne $app) {
            $empty = $app.SubKeyCount -eq 0 -and $app.ValueCount -eq 0
            $app.Close()
            if ($empty) {
                $software.DeleteSubKey('iCatalog', $false)
            }
        }
        $openWith = $classes.OpenSubKey('.icatalog\OpenWithProgids', $true)
        if ($null -ne $openWith) {
            $openWith.DeleteValue($progId, $false)
            $openWith.Close()
        }
        $classes.DeleteSubKeyTree($progId, $false)
        Write-Output 'Removed the iCatalog association registration. Windows may retain a default you selected; choose another app in Settings if needed.'
    }
} finally {
    $classes.Close()
    $software.Close()
}
