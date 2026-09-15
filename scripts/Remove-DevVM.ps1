# Scarlett A. Scott
# Created 2026-09-15

#requires -Version 5.1

[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
param(
    [string]$Name,
    [switch]$DeleteDisks
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$libDir = Join-Path $PSScriptRoot 'lib'
Import-Module (Join-Path $libDir 'Common.psm1') -Force -DisableNameChecking
Import-Module (Join-Path $libDir 'HyperV.psm1') -Force -DisableNameChecking

try {
    Assert-Prerequisites -RequireHyperV
    $repoRoot = Get-RepoRoot

    $targets = New-Object System.Collections.Generic.List[object]

    if ($Name) {

        $config = Get-DevVMConfig -RepoRoot $repoRoot -Override @{ VM = @{ Name = $Name } }
        $targets.Add((Get-DevVMPaths -Config $config))
    }
    else {
        $targets.Add((Get-DevVMPaths -Config (Get-DevVMConfig -RepoRoot $repoRoot)))
    }

    foreach ($t in $targets) {
        $what = "VM '$($t.Name)'"
        if ($DeleteDisks) { $what += " and its disks under $($t.Dir)" }

        if ($PSCmdlet.ShouldProcess($what, 'Delete')) {
            Write-Step "Removing $($t.Name)"
            Remove-DevVMMachine -Name $t.Name -Directory $t.Dir -DeleteDisks:$DeleteDisks

            $tidyHost = ''
            if ($t.PSObject.Properties.Name -contains 'Hostname') { $tidyHost = [string]$t.Hostname }
            if ($tidyHost -and (Remove-KnownHostEntry -HostName $tidyHost)) {
                Write-Detail "removed $tidyHost from known_hosts"
            }

            $buildDir = Join-Path (Join-Path $repoRoot 'build') $t.Name
            if ($DeleteDisks -and (Test-Path -LiteralPath $buildDir)) {
                Write-Detail "removing $buildDir"
                Remove-Item -LiteralPath $buildDir -Recurse -Force
            }
        }
    }

    Write-Host ''
    Write-Host '  Done.' -ForegroundColor Green
}
catch {
    Write-Host ''
    Write-Host "REMOVE FAILED: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
