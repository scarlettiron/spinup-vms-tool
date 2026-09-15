# Scarlett A. Scott
# Created 2026-09-15

#requires -Version 5.1

param([string]$Command)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$Rest = @($args)

$commands = [ordered]@{
    'create'  = @{ Script = 'New-DevVM.ps1';       Help = 'build a VM from config' }
    'new'     = @{ Script = 'New-DevVM.ps1';       Help = $null }
    'build'   = @{ Script = 'New-DevVM.ps1';       Help = $null }
    'open'    = @{ Script = 'Open-DevVM.ps1';      Help = 'start if needed, then open the desktop' }
    'desktop' = @{ Script = 'Open-DevVM.ps1';      Help = $null }
    'console' = @{ Script = 'Open-DevVM.ps1';      Help = $null }
    'start'   = @{ Script = 'Start-DevVM.ps1';     Help = 'start or resume; never provisions' }
    'up'      = @{ Script = 'Start-DevVM.ps1';     Help = $null }
    'stop'    = @{ Script = 'Stop-DevVM.ps1';      Help = 'shut down and free the RAM; keeps the disks' }
    'halt'    = @{ Script = 'Stop-DevVM.ps1';      Help = $null }
    'down'    = @{ Script = 'Stop-DevVM.ps1';      Help = $null }
    'status'  = @{ Script = 'Get-DevVMStatus.ps1'; Help = 'what the VM is, and how it is doing' }
    'info'    = @{ Script = 'Get-DevVMStatus.ps1'; Help = $null }
    'remove'  = @{ Script = 'Remove-DevVM.ps1';    Help = 'delete the VM; needs -DeleteDisks to touch the disks' }
    'destroy' = @{ Script = 'Remove-DevVM.ps1';    Help = $null }
    'rm'      = @{ Script = 'Remove-DevVM.ps1';    Help = $null }
    'hash'    = @{ Script = 'New-PasswordHash.ps1';Help = 'make a SHA-512 crypt hash for PasswordHash' }
}

function Show-Usage {
    Write-Host ''
    Write-Host '  dev-vm' -ForegroundColor White
    Write-Host ''
    Write-Host '  Usage:  .\vm.ps1 <command> [arguments]' -ForegroundColor Gray
    Write-Host ''

    foreach ($key in $commands.Keys) {
        $entry = $commands[$key]
        if (-not $entry.Help) { continue }

        $aliases = New-Object System.Collections.Generic.List[string]
        foreach ($other in $commands.Keys) {
            if ($other -eq $key) { continue }
            $candidate = $commands[$other]
            if ($candidate.Help) { continue }
            if ($candidate.Script -ne $entry.Script) { continue }
            $aliases.Add($other)
        }

        $line = '    {0,-9} {1}' -f $key, $entry.Help
        Write-Host $line -NoNewline
        if ($aliases.Count -gt 0) {
            Write-Host ("   (or {0})" -f (($aliases.ToArray()) -join ', ')) -ForegroundColor DarkGray
        }
        else {
            Write-Host ''
        }
    }

    Write-Host ''
    Write-Host '  Arguments are passed straight through, so anything the underlying' -ForegroundColor Gray
    Write-Host '  script accepts works here:' -ForegroundColor Gray
    Write-Host ''
    Write-Host '    .\vm.ps1 open'
    Write-Host '    .\vm.ps1 stop -Save'
    Write-Host '    .\vm.ps1 status -Report'
    Write-Host '    .\vm.ps1 remove -DeleteDisks'
    Write-Host ''
    Write-Host '  Most commands need an elevated PowerShell, because the Hyper-V' -ForegroundColor Gray
    Write-Host '  cmdlets do. "hash" does not.' -ForegroundColor Gray
    Write-Host ''
}

if (-not $Command -or $Command -in @('help', '-h', '--help', '/?', '-?')) {
    Show-Usage
    exit 0
}

$key = $Command.ToLowerInvariant()
if (-not $commands.Contains($key)) {
    Write-Host ''
    Write-Host "  Unknown command '$Command'." -ForegroundColor Red
    Write-Host ''
    Write-Host ('  Valid commands: {0}' -f (($commands.Keys) -join ', ')) -ForegroundColor Gray
    Write-Host '  Run .\vm.ps1 with no arguments for what each one does.' -ForegroundColor Gray
    Write-Host ''
    exit 2
}

$entry = $commands[$key]
$target = Join-Path (Join-Path $PSScriptRoot 'scripts') $entry.Script
if (-not (Test-Path -LiteralPath $target)) {
    Write-Host ''
    Write-Host "  '$key' maps to $($entry.Script), which is missing from scripts\." -ForegroundColor Red
    Write-Host ''
    exit 1
}

& $target @args

if (Test-Path variable:LASTEXITCODE) {
    if ($null -ne $LASTEXITCODE -and $LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
