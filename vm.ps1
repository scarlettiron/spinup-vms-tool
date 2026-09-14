#requires -Version 5.1
<#
.SYNOPSIS
    One entry point for everything. Dispatches to the scripts under scripts/.

.DESCRIPTION
    A thin front door, so there is one command to remember instead of seven paths:

        .\vm.ps1 create -Machine ubuntu-gui
        .\vm.ps1 open   -Machine ubuntu-gui
        .\vm.ps1 stop

    Every argument after the command is passed straight through to the script that
    handles it, so anything documented for those scripts works here unchanged. This
    file deliberately does no work of its own: it holds no defaults, validates no
    config, and requires no elevation. Whatever the target script does, it does.

.PARAMETER Command
    What to do. Run .\vm.ps1 with no arguments for the list.

    Everything after the command is collected by $args and forwarded verbatim.
    Note the deliberate absence of [CmdletBinding()] and [Parameter()] here: either
    one makes this an advanced script, and an advanced script tries to bind a
    leading-dash token like -RenderOnly to a parameter of its own and fails.
    ValueFromRemainingArguments does not help, since it only gathers positional
    leftovers. A plain script puts every unbound token in $args, dashes and all,
    which is exactly what a pass-through wrapper needs.

.EXAMPLE
    .\vm.ps1
    List the commands.

.EXAMPLE
    .\vm.ps1 create -Machine ubuntu-gui
    Build the Ubuntu desktop VM.

.EXAMPLE
    .\vm.ps1 render
    Show what a build would produce, without touching Hyper-V. Needs no elevation.

.EXAMPLE
    .\vm.ps1 open
    Start the VM if needed and open its desktop.

.EXAMPLE
    .\vm.ps1 stop -Save
    Hibernate it.
#>
param([string]$Command)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Everything after the command, as written, dashes intact. See the note in the
# help above for why this is $args rather than a declared parameter.
$Rest = @($args)

# command -> script, plus any parameters the command itself implies. Aliases sit
# beside their real name rather than in a second table, so adding one is one line.
#
# Implied is a hashtable, not a list of strings, because it is splatted: splatting
# a hashtable binds by name, while splatting an array of strings passes everything
# positionally and '-RenderOnly' would arrive as a *value*.
$commands = [ordered]@{
    'create'  = @{ Script = 'New-DevVM.ps1';       Implied = @{};                     Help = 'build a VM from config' }
    'new'     = @{ Script = 'New-DevVM.ps1';       Implied = @{};                     Help = $null }
    'build'   = @{ Script = 'New-DevVM.ps1';       Implied = @{};                     Help = $null }
    'render'  = @{ Script = 'New-DevVM.ps1';       Implied = @{ RenderOnly = $true }; Help = 'show what a build would do, no Hyper-V, no elevation' }
    'open'    = @{ Script = 'Open-DevVM.ps1';      Implied = @{};                     Help = 'start if needed, then open the desktop' }
    'desktop' = @{ Script = 'Open-DevVM.ps1';      Implied = @{};                     Help = $null }
    'console' = @{ Script = 'Open-DevVM.ps1';      Implied = @{};                     Help = $null }
    'start'   = @{ Script = 'Start-DevVM.ps1';     Implied = @{};                     Help = 'start or resume; never provisions' }
    'up'      = @{ Script = 'Start-DevVM.ps1';     Implied = @{};                     Help = $null }
    'stop'    = @{ Script = 'Stop-DevVM.ps1';      Implied = @{};                     Help = 'shut down and free the RAM; keeps the disks' }
    'halt'    = @{ Script = 'Stop-DevVM.ps1';      Implied = @{};                     Help = $null }
    'down'    = @{ Script = 'Stop-DevVM.ps1';      Implied = @{};                     Help = $null }
    'status'  = @{ Script = 'Get-DevVMStatus.ps1'; Implied = @{};                     Help = 'what each machine is, and how it is doing' }
    'info'    = @{ Script = 'Get-DevVMStatus.ps1'; Implied = @{};                     Help = $null }
    'remove'  = @{ Script = 'Remove-DevVM.ps1';    Implied = @{};                     Help = 'delete the VM; needs -DeleteDisks to touch the disks' }
    'destroy' = @{ Script = 'Remove-DevVM.ps1';    Implied = @{};                     Help = $null }
    'rm'      = @{ Script = 'Remove-DevVM.ps1';    Implied = @{};                     Help = $null }
    'hash'    = @{ Script = 'New-PasswordHash.ps1';Implied = @{};                     Help = 'make a SHA-512 crypt hash for PasswordHash' }
}

function Show-Usage {
    Write-Host ''
    Write-Host '  dev-vm' -ForegroundColor White
    Write-Host ''
    Write-Host '  Usage:  .\vm.ps1 <command> [arguments]' -ForegroundColor Gray
    Write-Host ''

    # An alias is an entry with no help text. It belongs to the command with the
    # same script AND the same implied arguments: grouping on the script alone
    # would file 'render' as an alias of 'create', which it is not, since it adds
    # -RenderOnly.
    foreach ($key in $commands.Keys) {
        $entry = $commands[$key]
        if (-not $entry.Help) { continue }

        $aliases = New-Object System.Collections.Generic.List[string]
        foreach ($other in $commands.Keys) {
            if ($other -eq $key) { continue }
            $candidate = $commands[$other]
            if ($candidate.Help) { continue }
            if ($candidate.Script -ne $entry.Script) { continue }
            $a = (($candidate.Implied.Keys | Sort-Object) -join ',')
            $b = (($entry.Implied.Keys | Sort-Object) -join ',')
            if ($a -ne $b) { continue }
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
    Write-Host '    .\vm.ps1 create -Machine ubuntu-gui'
    Write-Host '    .\vm.ps1 create -All'
    Write-Host '    .\vm.ps1 render'
    Write-Host '    .\vm.ps1 open -Machine ubuntu-gui'
    Write-Host '    .\vm.ps1 stop -Save'
    Write-Host '    .\vm.ps1 status -Report'
    Write-Host '    .\vm.ps1 remove -Machine ubuntu-gui -DeleteDisks'
    Write-Host ''
    Write-Host '  Most commands need an elevated PowerShell, because the Hyper-V' -ForegroundColor Gray
    Write-Host '  cmdlets do. "render" and "hash" do not.' -ForegroundColor Gray
    Write-Host ''
    Write-Host '  Full documentation: README.md and docs/' -ForegroundColor DarkGray
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

# Two splats: the command's implied parameters by name, then the user's arguments
# exactly as typed.
#
# @args MUST be forwarded untouched. It is not interchangeable with an array that
# happens to hold the same strings: splatting any other array passes its elements
# positionally, so '-DeleteDisks' arrives as a VALUE and the target says
# "A positional parameter cannot be found that accepts argument '-DeleteDisks'".
# Copying $args into a list, editing it, and assigning it back to $args loses it
# too - all three were measured, and only the original survives. So do not
# "tidy up" the arguments on the way through; there is nowhere to stand.
#
# The known cost of that, and of this script deliberately not being
# [CmdletBinding()] (an advanced script would try to bind -RenderOnly and friends
# as its own parameters):
#
#     .\vm.ps1 remove -Name web -Confirm:$false
#
# A plain script does not get colon-syntax parsed for it, so that arrives as
# '-Confirm:' and 'False' in separate slots, and the stray 'False' binds to the
# target's first positional parameter. Call the script under scripts\ directly
# when you need -Confirm:$false or -Verbose:$false; everything else forwards fine.
$implied = $entry.Implied
& $target @implied @args

# Propagate the child's exit code, so a wrapper in CI or a shell chain still sees
# a failure. `exit` inside the target sets this in our scope.
if (Test-Path variable:LASTEXITCODE) {
    if ($null -ne $LASTEXITCODE -and $LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
