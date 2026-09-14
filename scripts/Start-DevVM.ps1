#requires -Version 5.1
<#
.SYNOPSIS
    Start a dev VM that already exists. Does not provision anything.

.DESCRIPTION
    An ordinary boot of a machine that is already built. Provisioning cannot run
    again: vm-bootstrap.service carries
    ConditionPathExists=!/var/lib/vm-bootstrap/complete, so once a clean run has
    happened the unit is skipped on every subsequent boot. Your files, repos,
    packages and configuration come back exactly as you left them.

    If you want to rebuild from scratch, that is New-DevVM.ps1 -Force, and it is a
    different script on purpose.

    A machine that was saved with Stop-DevVM.ps1 -Save resumes in a few seconds. A
    machine that was shut down boots normally, which takes about half a minute.

.PARAMETER Machine
    One machine from the Machines block. Without it, the single VM that
    vm.config.yaml describes.

.PARAMETER Name
    A Hyper-V VM name, for a machine config does not describe.

.PARAMETER All
    Every machine in the Machines block.

.PARAMETER Wait
    Wait until the guest has a network address, which is the point at which it is
    actually usable rather than merely powered on.

.PARAMETER Console
    Open the Hyper-V console window once it is started.

.PARAMETER TimeoutSeconds
    How long -Wait waits for an address. Default 180.

.EXAMPLE
    .\scripts\Start-DevVM.ps1
    Start the VM this config describes.

.EXAMPLE
    .\scripts\Start-DevVM.ps1 -Wait
    Start it and wait until it is reachable, then print how to connect.

.EXAMPLE
    .\scripts\Start-DevVM.ps1 -All
    Bring the whole fleet up.
#>
[CmdletBinding()]
param(
    [string]$Machine,
    [string]$Name,
    [switch]$All,
    [switch]$Wait,
    [switch]$Console,
    [int]$TimeoutSeconds = 180
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$libDir = Join-Path $PSScriptRoot 'lib'
Import-Module (Join-Path $libDir 'Common.psm1') -Force -DisableNameChecking
Import-Module (Join-Path $libDir 'HyperV.psm1') -Force -DisableNameChecking

try {
    Assert-Prerequisites -RequireHyperV
    $repoRoot = Get-RepoRoot
    $targets = @(Get-DevVMTargets -RepoRoot $repoRoot -Machine $Machine -Name $Name -All:$All)

    Reset-BuildSteps -Total $targets.Count
    $results = New-Object System.Collections.Generic.List[object]

    foreach ($t in $targets) {
        Start-BuildStep "Starting $($t.Name)"

        $outcome = Start-DevVMMachine -Name $t.Name -Hostname $t.Hostname `
            -WaitForNetwork:$Wait -TimeoutSeconds $TimeoutSeconds

        if ($outcome -eq 'not-created') {
            # Not an error worth aborting a fleet for, but say plainly that this is
            # the one case where you do need the build script.
            Write-Warn "no VM named '$($t.Name)' exists on this host"
            Write-Warn '  this script only starts a machine that is already built. To build it:'
            $buildArgs = if ($t.Machine) { " -Machine $($t.Machine)" } else { '' }
            Write-Warn "      .\vm.ps1 create$buildArgs"
        }
        elseif ($Console) {
            Write-Detail "opening the console"
            Start-Process -FilePath 'vmconnect.exe' -ArgumentList 'localhost', $t.Name -ErrorAction SilentlyContinue
        }

        $results.Add([pscustomobject]@{
            Name = $t.Name; Hostname = $t.Hostname; User = $t.User; Outcome = $outcome
        })
    }

    Write-BuildReport -Title 'start report'

    Write-Host ''
    foreach ($r in $results) {
        if ($r.Outcome -eq 'not-created') {
            Write-Host ("  {0,-30} not created" -f $r.Name) -ForegroundColor DarkGray
            continue
        }

        $colour = if ($r.Outcome -eq 'no-network') { 'Yellow' } else { 'Green' }
        Write-Host ("  {0,-30} {1}" -f $r.Name, $r.Outcome) -ForegroundColor $colour

        # The address is the useful part, and it is worth resolving now rather than
        # telling someone to go and look it up.
        $resolved = Test-DevVMHostnameResolvable -Hostname $r.Hostname
        if ($resolved) {
            Write-Detail "http://$($r.Hostname).local   (resolves to $resolved)"
        }
        else {
            Write-Detail "http://$($r.Hostname).local   (not resolving yet, mDNS takes a moment)"
        }
        if ($r.User) { Write-Detail "ssh $($r.User)@$($r.Hostname).local" }
    }

    Write-Host ''
    Write-Host '  Provisioning did not re-run: bootstrap is skipped once a clean run has happened.' -ForegroundColor Gray
    Write-Host '  Check what it is doing with:' -ForegroundColor Gray
    Write-Host '      .\vm.ps1 status'
    Write-Host ''
}
catch {
    Write-BuildReport -Title 'start report (FAILED)' -FailureMessage $_.Exception.Message
    if ($_.ScriptStackTrace) { Write-Verbose $_.ScriptStackTrace }
    exit 1
}
