#requires -Version 5.1
<#
.SYNOPSIS
    Delete a dev VM and, optionally, its disks.

.DESCRIPTION
    Resolves the VM name the same way New-DevVM.ps1 does, so -Machine and the
    config layers behave identically. Prompts before destroying anything unless
    -Confirm:$false is given.

.EXAMPLE
    .\scripts\Remove-DevVM.ps1 -Machine ubuntu-gui -DeleteDisks

.EXAMPLE
    .\scripts\Remove-DevVM.ps1 -All -DeleteDisks -Confirm:$false
#>
[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
param(
    [string]$Machine,
    [string]$Name,
    [switch]$All,
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

    if ($All) {
        $machines = Get-MachineNames -RepoRoot $repoRoot
        if ($machines.Count -eq 0) { throw '-All was given but vm.config.yaml defines no Machines block.' }
        foreach ($m in $machines) {
            $targets.Add((Get-DevVMPaths -Config (Get-DevVMConfig -RepoRoot $repoRoot -Machine $m)))
        }
    }
    elseif ($Name) {
        # An explicit name still needs a directory to tidy up, so fall back to
        # the config's VM.Path.
        $config = Get-DevVMConfig -RepoRoot $repoRoot -Machine $Machine -Override @{ VM = @{ Name = $Name } }
        $targets.Add((Get-DevVMPaths -Config $config))
    }
    else {
        $targets.Add((Get-DevVMPaths -Config (Get-DevVMConfig -RepoRoot $repoRoot -Machine $Machine)))
    }

    foreach ($t in $targets) {
        $what = "VM '$($t.Name)'"
        if ($DeleteDisks) { $what += " and its disks under $($t.Dir)" }

        if ($PSCmdlet.ShouldProcess($what, 'Delete')) {
            Write-Step "Removing $($t.Name)"
            Remove-DevVMMachine -Name $t.Name -Directory $t.Dir -DeleteDisks:$DeleteDisks

            # The machine is gone, so a remembered host key for its name can only
            # get in the way of the next build under the same name.
            #
            # The property is checked for existence rather than just read:
            # StrictMode turns a missing property into a terminating error, so
            # `if ($t.Hostname)` on an object without one does not evaluate false,
            # it throws - and it threw here, AFTER the VM and its disks were
            # already gone, turning a completed removal into "REMOVE FAILED".
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
