#requires -Version 5.1
<#
.SYNOPSIS
    Open a dev VM's desktop. Starts it first if it is not running.

.DESCRIPTION
    One command to get from a cold laptop to a desktop you can click around in.

    Starts the VM if needed, waits until the guest is actually up, and then opens
    the console window. The waiting is the point: Hyper-V decides between a basic
    session and an Enhanced Session at the moment the window opens, so a window
    opened while the guest is still booting stays stuck at a fixed size with no
    clipboard, and you have to close it and reopen it. Waiting first means the
    window you get is the good one.

    Nothing here provisions anything. If the VM does not exist yet this tells you
    to run New-DevVM.ps1 and stops.

.PARAMETER Machine
    One machine from the Machines block. Without it, the single VM that
    vm.config.yaml describes.

.PARAMETER Name
    A Hyper-V VM name, for a machine config does not describe.

.PARAMETER NoWait
    Open the window immediately instead of waiting for the guest to come up. You
    will probably get a basic session and have to reopen it.

.PARAMETER TimeoutSeconds
    How long to wait for the guest to be ready. Default 240, because a cold boot
    plus the display manager takes a while.

.EXAMPLE
    .\scripts\Open-DevVM.ps1 -Machine ubuntu-gui
    Start the Ubuntu desktop VM if needed, then open it.

.EXAMPLE
    .\scripts\Open-DevVM.ps1
    The same for the VM this config describes.
#>
[CmdletBinding()]
param(
    [string]$Machine,
    [string]$Name,
    [switch]$NoWait,
    [int]$TimeoutSeconds = 240
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$libDir = Join-Path $PSScriptRoot 'lib'
Import-Module (Join-Path $libDir 'Common.psm1') -Force -DisableNameChecking
Import-Module (Join-Path $libDir 'HyperV.psm1') -Force -DisableNameChecking

<#
    Is the guest far enough along to give us an Enhanced Session?

    Both signals come from the host, so this needs no SSH key and no network path
    into the guest. The heartbeat integration service reports OK once the guest OS
    is properly up rather than merely powered on, and an address means the network
    stack is live. xrdp comes up alongside the display manager, so the two together
    are a good enough proxy for "the desktop is ready".
#>
function Test-GuestReady {
    param([string]$VmName)

    $ip = Get-DevVMIPAddress -Name $VmName
    if (-not $ip) { return $false }

    $heartbeat = Get-VMIntegrationService -VMName $VmName -Name 'Heartbeat' -ErrorAction SilentlyContinue
    if (-not $heartbeat) {
        # No heartbeat service at all: an older guest, or the integration daemons
        # did not install. An address is the best signal available.
        return $true
    }
    if (-not $heartbeat.Enabled) { return $true }

    return ($heartbeat.PrimaryStatusDescription -eq 'OK')
}

try {
    Assert-Prerequisites -RequireHyperV
    $repoRoot = Get-RepoRoot
    $targets = @(Get-DevVMTargets -RepoRoot $repoRoot -Machine $Machine -Name $Name)
    $t = $targets[0]

    Reset-BuildSteps -Total 3

    # ------------------------------------------------------------------ 1. find it
    Start-BuildStep "Finding $($t.Name)"
    $vm = Get-VM -Name $t.Name -ErrorAction SilentlyContinue
    if (-not $vm) {
        $buildArgs = if ($t.Machine) { " -Machine $($t.Machine)" } else { '' }
        throw @"
No VM named '$($t.Name)' exists on this host.

This script only opens a machine that is already built. To build it:
    .\scripts\New-DevVM.ps1$buildArgs

That takes 30 to 40 minutes, most of it unattended.
"@
    }
    Write-Detail "state: $($vm.State)"

    # Enhanced Session needs the host transport set to HvSocket. The build does
    # this, but a VM created another way, or one whose config changed, will not
    # have it. It can only be changed while the machine is off.
    if ($null -ne $t.Config -and $t.Config.VM.Contains('EnhancedSession') -and $t.Config.VM['EnhancedSession']) {
        if ($vm.EnhancedSessionTransportType -ne 'HvSocket') {
            if ($vm.State -eq 'Off') {
                Write-Detail 'setting the Enhanced Session transport to HvSocket'
                Set-VM -Name $t.Name -EnhancedSessionTransportType HvSocket
            }
            else {
                Write-Warn 'Enhanced Session transport is not HvSocket, and it can only be changed while the VM is off'
                Write-Warn '  you will get a basic session. Stop it and re-run this to fix that:'
                Write-Warn "      .\vm.ps1 stop -Name $($t.Name)"
            }
        }
    }

    # ----------------------------------------------------------------- 2. start it
    Start-BuildStep "Starting $($t.Name)"
    if ($vm.State -eq 'Running') {
        Write-Detail 'already running'
    }
    else {
        Start-DevVMMachine -Name $t.Name -Hostname $t.Hostname | Out-Null
    }

    # ------------------------------------------------------------- 3. wait, open
    Start-BuildStep 'Waiting for the desktop, then opening it'

    if ($NoWait) {
        Write-Detail 'skipping the wait, as asked'
    }
    else {
        $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
        $start = Get-Date
        $ready = $false
        $nextTick = 20
        while ((Get-Date) -lt $deadline) {
            if (Test-GuestReady -VmName $t.Name) {
                Write-Detail ("guest is up after {0:N0}s" -f ((Get-Date) - $start).TotalSeconds)
                $ready = $true
                break
            }
            $elapsed = ((Get-Date) - $start).TotalSeconds
            if ($elapsed -ge $nextTick) {
                Write-Detail ("still booting, {0:N0}s elapsed" -f $elapsed)
                $nextTick += 20
            }
            Start-Sleep -Seconds 3
        }

        if ($ready) {
            # The display manager and xrdp finish a little after the heartbeat says
            # the OS is up. A few seconds here is the difference between landing on
            # the xrdp login prompt and landing on a black window.
            Write-Detail 'giving the display manager a few seconds to settle'
            Start-Sleep -Seconds 8
        }
        else {
            Write-Warn "the guest did not report ready within $TimeoutSeconds seconds"
            Write-Warn '  opening anyway. If the window is small and has no clipboard, close it and re-run this.'
        }
    }

    if (-not (Get-Command vmconnect.exe -ErrorAction SilentlyContinue)) {
        throw @"
vmconnect.exe is not on the PATH, so the Hyper-V console cannot be opened.

The management tools are a separate Windows feature from the hypervisor itself:
    Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-Management-Clients -All
"@
    }

    Write-Detail "opening the console for $($t.Name)"
    Start-Process -FilePath 'vmconnect.exe' -ArgumentList 'localhost', $t.Name

    Write-BuildReport -Title "opened $($t.Name)"

    Write-Host ''
    if ($t.User) {
        Write-Host "  Log in as:  $($t.User)" -ForegroundColor White
        Write-Host '  The password is the one from Guest.PrimaryUser in your config.' -ForegroundColor Gray
    }
    Write-Host ''
    Write-Host '  In an Enhanced Session you get an xrdp login prompt, a resizable window,' -ForegroundColor Gray
    Write-Host '  and a shared clipboard. If instead the window is a fixed size with no' -ForegroundColor Gray
    Write-Host '  clipboard, it opened too early: close it and run this again.' -ForegroundColor Gray
    Write-Host ''
    Write-Host '  Shut it down when you are done, to get the RAM back:' -ForegroundColor Gray
    $stopArgs = if ($Machine) { " -Machine $Machine" } elseif ($Name) { " -Name $Name" } else { '' }
    Write-Host "      .\vm.ps1 stop$stopArgs"
    Write-Host ''
}
catch {
    Write-BuildReport -Title 'open failed' -FailureMessage $_.Exception.Message
    if ($_.ScriptStackTrace) { Write-Verbose $_.ScriptStackTrace }
    exit 1
}
