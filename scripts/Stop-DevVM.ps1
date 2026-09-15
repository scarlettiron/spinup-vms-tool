# Scarlett A. Scott
# Created 2026-09-15

#requires -Version 5.1

[CmdletBinding()]
param(
    [string]$Name,
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
    $targets = @(Get-DevVMTargets -RepoRoot $repoRoot -Name $Name)

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
    if ($Name) { $startArgs = " -Name $Name" }
    Write-Host "      .\vm.ps1 start$startArgs"
    Write-Host ''
}
catch {
    Write-BuildReport -Title 'stop report (FAILED)' -FailureMessage $_.Exception.Message
    if ($_.ScriptStackTrace) { Write-Verbose $_.ScriptStackTrace }
    exit 1
}
