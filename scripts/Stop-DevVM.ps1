#requires -Version 5.1
<#
.SYNOPSIS
    Shut a dev VM down so it stops using CPU and memory. Nothing is deleted.

.DESCRIPTION
    A stopped VM releases its CPU and RAM back to the host and keeps everything
    else: the disks, your files, your repos, installed packages, and the whole
    provisioned state. Start it again with Start-DevVM.ps1 and it comes back as it
    was, without re-provisioning.

    Nothing here removes a disk. Only Remove-DevVM.ps1 -DeleteDisks does that.

    Three ways down:

      (default)  ask the guest to shut down through the integration services, so
                 filesystems are flushed and services stop cleanly.
      -Save      write the guest's memory to disk and release it, so it resumes
                 exactly where it was. Costs disk equal to the assigned RAM.
      -TurnOff   cut the power. Fast, and risks the filesystem damage you would
                 expect from pulling the plug on a real machine.

    Names resolve exactly as they do for New-DevVM.ps1, so -Machine and the config
    layers behave identically.

.PARAMETER Machine
    One machine from the Machines block. Without it, the single VM that
    vm.config.yaml describes.

.PARAMETER Name
    A Hyper-V VM name, for a machine config does not describe.

.PARAMETER All
    Every machine in the Machines block.

.PARAMETER Save
    Save state instead of shutting down, so it resumes where it left off.

.PARAMETER TurnOff
    Cut the power without asking the guest. For a machine that will not shut down.

.PARAMETER TimeoutSeconds
    How long to wait for a graceful shutdown before reporting that the guest is
    ignoring the request. Default 120.

.EXAMPLE
    .\scripts\Stop-DevVM.ps1
    Shut down the VM this config describes.

.EXAMPLE
    .\scripts\Stop-DevVM.ps1 -Save
    Hibernate it, so the next start is a few seconds instead of a boot.

.EXAMPLE
    .\scripts\Stop-DevVM.ps1 -All
    Shut down the whole fleet.
#>
[CmdletBinding()]
param(
    [string]$Machine,
    [string]$Name,
    [switch]$All,
    [switch]$Save,
    [switch]$TurnOff,
    [int]$TimeoutSeconds = 120
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$libDir = Join-Path $PSScriptRoot 'lib'
Import-Module (Join-Path $libDir 'Common.psm1') -Force -DisableNameChecking
Import-Module (Join-Path $libDir 'HyperV.psm1') -Force -DisableNameChecking

try {
    if ($Save -and $TurnOff) {
        throw '-Save and -TurnOff ask for opposite things. Pick one.'
    }

    Assert-Prerequisites -RequireHyperV
    $repoRoot = Get-RepoRoot
    $targets = @(Get-DevVMTargets -RepoRoot $repoRoot -Machine $Machine -Name $Name -All:$All)

    Reset-BuildSteps -Total $targets.Count
    $results = New-Object System.Collections.Generic.List[object]

    foreach ($t in $targets) {
        Start-BuildStep "Stopping $($t.Name)"

        $outcome = Stop-DevVMMachine -Name $t.Name -Save:$Save -TurnOff:$TurnOff `
            -TimeoutSeconds $TimeoutSeconds

        switch ($outcome) {
            'not-created' { Write-Detail 'no such VM on this host, so nothing to stop' }
            'already-off' { Write-Detail 'already off' }
        }
        $results.Add([pscustomobject]@{ Name = $t.Name; Outcome = $outcome })
    }

    Write-BuildReport -Title 'stop report'

    Write-Host ''
    foreach ($r in $results) {
        $note = switch ($r.Outcome) {
            'off'         { 'off, disks untouched' }
            'already-off' { 'was already off' }
            'saved'       { 'saved, resumes where it left off' }
            'turned-off'  { 'powered off without a clean shutdown' }
            'timeout'     { 'STILL RUNNING, the guest ignored the request' }
            'not-created' { 'not created' }
            default       { $r.Outcome }
        }
        $colour = if ($r.Outcome -eq 'timeout') { 'Yellow' } else { 'Gray' }
        Write-Host ("  {0,-30} {1}" -f $r.Name, $note) -ForegroundColor $colour
    }

    Write-Host ''
    Write-Host '  Nothing was deleted. Start it again with:' -ForegroundColor Gray
    $startArgs = ''
    if ($Machine) { $startArgs = " -Machine $Machine" }
    elseif ($All) { $startArgs = ' -All' }
    elseif ($Name) { $startArgs = " -Name $Name" }
    Write-Host "      .\vm.ps1 start$startArgs"
    Write-Host ''
}
catch {
    Write-BuildReport -Title 'stop report (FAILED)' -FailureMessage $_.Exception.Message
    if ($_.ScriptStackTrace) { Write-Verbose $_.ScriptStackTrace }
    exit 1
}
