# Scarlett A. Scott
# Created 2026-09-15

#requires -Version 5.1

[CmdletBinding()]
param(
    [string]$Name,
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
    $targets = @(Get-DevVMTargets -RepoRoot $repoRoot -Name $Name)

    Reset-BuildSteps -Total $targets.Count
    $results = New-Object System.Collections.Generic.List[object]

    foreach ($t in $targets) {
        Start-BuildStep "Starting $($t.Name)"

        $outcome = Start-DevVMMachine -Name $t.Name -Hostname $t.Hostname `
            -WaitForNetwork:$Wait -TimeoutSeconds $TimeoutSeconds

        if ($outcome -eq 'not-created') {

            Write-Warn "no VM named '$($t.Name)' exists on this host"
            Write-Warn '  this script only starts a machine that is already built. To build it:'
            Write-Warn "      .\vm.ps1 create''"
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
