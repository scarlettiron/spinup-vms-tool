# Scarlett A. Scott
# Created 2026-09-15

#requires -Version 5.1
Set-StrictMode -Version Latest

Import-Module (Join-Path $PSScriptRoot 'Common.psm1') -DisableNameChecking

function Get-DevVMPaths {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Config)

    $name = [string]$Config.VM['Name']
    $root = [string]$Config.VM['Path']
    $dir = Join-Path $root $name

    $dataDisks = New-Object System.Collections.Generic.List[object]
    foreach ($disk in (Get-DevVMDataDiskLayout -Config $Config)) {
        $dataDisks.Add([pscustomobject]@{
            Label              = $disk.Label
            DriveLetter        = $disk.DriveLetter
            SizeGB             = $disk.SizeGB
            MountPoint         = $disk.MountPoint
            Filesystem         = $disk.Filesystem
            ControllerLocation = $disk.ControllerLocation
            Path               = Join-Path $dir ("{0}-{1}.vhdx" -f $name, $disk.Label)
        })
    }

    return [pscustomobject]@{
        Name      = $name

        Hostname  = [string]$Config.Guest['Hostname']
        Dir       = $dir
        OsVhd     = Join-Path $dir "$name.vhdx"
        SeedVhd   = Join-Path $dir "$name-seed.vhdx"

        DataDisks = $dataDisks.ToArray()
    }
}

function Test-DevVMExists {
    [CmdletBinding()] param([Parameter(Mandatory)][string]$Name)
    [bool](Get-VM -Name $Name -ErrorAction SilentlyContinue)
}

function Assert-SwitchExists {
    [CmdletBinding()] param([Parameter(Mandatory)][string]$Name)

    if (Get-VMSwitch -Name $Name -ErrorAction SilentlyContinue) { return }

    $available = (Get-VMSwitch -ErrorAction SilentlyContinue | ForEach-Object { "$($_.Name) ($($_.SwitchType))" }) -join ', '
    if (-not $available) { $available = '(none)' }
    throw @"
Virtual switch '$Name' does not exist. Available switches: $available

'Default Switch' is Hyper-V's built-in NAT switch and exists on Windows Pro by
default. If it is missing, either create an external switch in Hyper-V Manager
and set VM.SwitchName to its name, or create one here:

    New-VMSwitch -Name 'dev-nat' -SwitchType Internal

Every machine in a fleet must share one switch for mDNS (<host>.local) to work
between them.
"@
}

function Assert-DevVMNameAvailable {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]$Config,
        [switch]$Force
    )

    Import-Module Hyper-V -ErrorAction Stop

    $name = [string]$Config.VM['Name']
    $hostname = [string]$Config.Guest['Hostname']

    $existing = Get-VM -Name $name -ErrorAction SilentlyContinue
    if ($existing -and -not $Force) {
        throw @"
A VM named '$name' already exists on this host (state: $($existing.State)).

Pick another name, or replace it:
    .\scripts\New-DevVM.ps1 -Force            rebuild it from scratch
    .\scripts\Remove-DevVM.ps1 -DeleteDisks   remove it first

The name comes from ProjectName plus the blueprint, so changing ProjectName in
config gives every VM in this project a new name.
"@
    }

    if ($hostname -ne $name) {
        $sameHost = Get-VM -Name $hostname -ErrorAction SilentlyContinue
        if ($sameHost) {
            Write-Warn "another VM is named '$hostname', which is this machine's hostname. Not fatal, but confusing."
        }
    }

    if ($existing -and $Force) {
        Write-Detail "'$name' exists and will be replaced (-Force)"
        return
    }

    $address = Test-DevVMHostnameResolvable -Hostname $hostname
    if ($address) {
        throw @"
The hostname '$hostname' already resolves on this network, to $address.

Something is already using that name: another VM on this switch, a teammate's
machine, or a previous build of this one. Two machines claiming one mDNS name
means avahi renames the loser to '$hostname-2.local', so the address you hand
people would reach the wrong machine.

Fix it by changing ProjectName (or Hostname) in config, or by removing whatever
holds the name. To check what answers:
    ping $hostname.local
    Resolve-DnsName $hostname.local -ErrorAction SilentlyContinue
"@
    }
    Write-Detail "'$name' is free, and $hostname.local resolves to nothing yet"
}

function New-DevVMMachine {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]$Config,
        [Parameter(Mandatory)][string]$IsoPath,
        [Parameter(Mandatory)][string]$SeedVhdPath,
        [switch]$Force
    )

    Import-Module Hyper-V -ErrorAction Stop

    $p = Get-DevVMPaths -Config $Config
    $vm = $Config.VM

    Assert-SwitchExists -Name ([string]$vm['SwitchName'])

    if (Test-DevVMExists -Name $p.Name) {
        if (-not $Force) {
            throw "A VM named '$($p.Name)' already exists. Re-run with -Force to delete and rebuild it, or pick another VM.Name."
        }
        Write-Warn "removing the existing VM '$($p.Name)' because -Force was given"
        Remove-DevVMMachine -Name $p.Name -Directory $p.Dir -DeleteDisks
    }

    $stale = @(@($p.OsVhd) + @($p.DataDisks | ForEach-Object { $_.Path }) |
               Where-Object { Test-Path -LiteralPath $_ })
    if ($stale.Count -gt 0) {
        if (-not $Force) {
            throw "These disks already exist but no VM uses them: $($stale -join ', '). Delete them, or re-run with -Force."
        }
        foreach ($orphan in $stale) {
            Write-Detail "deleting orphaned $orphan"
            Remove-Item -LiteralPath $orphan -Force
        }
    }

    if (-not (Test-Path -LiteralPath $p.Dir)) {
        New-Item -ItemType Directory -Path $p.Dir -Force | Out-Null
    }

    $generation = 2
    $firmware = @{}
    if ($Config.Blueprint.Data.Contains('Firmware') -and $Config.Blueprint.Data['Firmware']) {
        $firmware = $Config.Blueprint.Data['Firmware']
        if ($firmware.Contains('Generation')) { $generation = [int]$firmware['Generation'] }
    }

    $startupBytes = ([long]$vm['MemoryStartupGB']) * 1GB
    $diskBytes = ([long]$vm['DiskSizeGB']) * 1GB

    Write-Detail "creating VM '$($p.Name)' (gen $generation, $($vm['CpuCount']) vCPU, $($vm['MemoryStartupGB']) GB RAM, $($vm['DiskSizeGB']) GB disk)"
    $machine = New-VM -Name $p.Name `
        -Generation $generation `
        -MemoryStartupBytes $startupBytes `
        -NewVHDPath $p.OsVhd `
        -NewVHDSizeBytes $diskBytes `
        -SwitchName ([string]$vm['SwitchName']) `
        -Path $p.Dir `
        -ErrorAction Stop

    Set-VMProcessor -VM $machine -Count ([int]$vm['CpuCount'])

    $nested = $false
    if ($vm.Contains('NestedVirtualization')) { $nested = [bool]$vm['NestedVirtualization'] }
    $dynamic = $true
    if ($vm.Contains('DynamicMemory')) { $dynamic = [bool]$vm['DynamicMemory'] }

    if ($nested) {
        if ($dynamic) {
            Write-Warn 'NestedVirtualization forces static memory; ignoring DynamicMemory.'
            $dynamic = $false
        }
        Set-VMProcessor -VM $machine -ExposeVirtualizationExtensions $true
        Set-VMNetworkAdapter -VM $machine -MacAddressSpoofing On
    }

    if ($dynamic) {
        $minBytes = ([long]$vm['MemoryMinimumGB']) * 1GB
        $maxBytes = ([long]$vm['MemoryMaximumGB']) * 1GB
        if ($minBytes -gt $startupBytes) { throw 'VM.MemoryMinimumGB cannot exceed VM.MemoryStartupGB.' }
        if ($maxBytes -lt $startupBytes) { throw 'VM.MemoryMaximumGB cannot be below VM.MemoryStartupGB.' }
        Set-VMMemory -VM $machine -DynamicMemoryEnabled $true `
            -MinimumBytes $minBytes -StartupBytes $startupBytes -MaximumBytes $maxBytes
    }
    else {
        Set-VMMemory -VM $machine -DynamicMemoryEnabled $false -StartupBytes $startupBytes
    }

    $loc = Get-DevVMDiskLocations

    Write-Detail "attaching install media: $IsoPath"
    Add-VMDvdDrive -VM $machine -ControllerNumber 0 -ControllerLocation $loc.Dvd -Path $IsoPath

    Write-Detail "attaching the seed disk (scsi 0:$($loc.Seed))"
    Add-VMHardDiskDrive -VM $machine -Path $SeedVhdPath `
        -ControllerNumber 0 -ControllerLocation $loc.Seed

    Write-Detail "data disks attach after the install ($($p.DataDisks.Count) to come)"

    if ($generation -eq 2) {
        $secureBoot = $true
        if ($vm.Contains('SecureBoot')) { $secureBoot = [bool]$vm['SecureBoot'] }

        if ($secureBoot) {
            $template = 'MicrosoftUEFICertificateAuthority'
            if ($firmware.Contains('SecureBootTemplate') -and $firmware['SecureBootTemplate']) {
                $template = [string]$firmware['SecureBootTemplate']
            }

            Set-VMFirmware -VM $machine -EnableSecureBoot On -SecureBootTemplate $template
        }
        else {
            Set-VMFirmware -VM $machine -EnableSecureBoot Off
        }

        if ($firmware.Contains('EnableTpm') -and $firmware['EnableTpm']) {
            Write-Detail 'enabling the virtual TPM'
            Set-VMKeyProtector -VM $machine -NewLocalKeyProtector
            Enable-VMTPM -VM $machine
        }

        $dvd = Get-VMDvdDrive -VM $machine
        $hdd = Get-VMHardDiskDrive -VM $machine | Where-Object { $_.Path -eq $p.OsVhd }
        Set-VMFirmware -VM $machine -BootOrder $dvd, $hdd
    }

    $autoCheckpoints = $false
    if ($vm.Contains('AutomaticCheckpoints')) { $autoCheckpoints = [bool]$vm['AutomaticCheckpoints'] }
    Set-VM -VM $machine -AutomaticCheckpointsEnabled $autoCheckpoints -CheckpointType Standard
    Set-VM -VM $machine -AutomaticStartAction Nothing -AutomaticStopAction ShutDown

    foreach ($svc in @('Guest Service Interface', 'Heartbeat', 'Time Synchronization')) {
        try { Enable-VMIntegrationService -VM $machine -Name $svc -ErrorAction Stop }
        catch { Write-Warn "could not enable integration service '$svc': $($_.Exception.Message)" }
    }

    if ($vm.Contains('EnhancedSession') -and $vm['EnhancedSession']) {
        try {

            Set-VM -VM $machine -EnhancedSessionTransportType HvSocket -ErrorAction Stop
            Write-Detail 'enhanced session transport set to HvSocket'
        }
        catch {
            Write-Warn "could not set the enhanced session transport: $($_.Exception.Message)"
        }
    }

    return (Get-VM -Name $p.Name)
}

function Wait-DevVMInstall {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Name,
        [int]$TimeoutMinutes = 60
    )

    $deadline = (Get-Date).AddMinutes($TimeoutMinutes)
    $start = Get-Date

    $nextTick = 2
    $tickMinutes = 2

    $systemDisk = $null
    try {
        $systemDisk = (Get-VMHardDiskDrive -VMName $Name -ErrorAction Stop |
            Where-Object { $_.ControllerLocation -eq 0 } | Select-Object -First 1).Path
    }
    catch { $systemDisk = $null }
    $lastBytes = 0

    while ((Get-Date) -lt $deadline) {
        $vm = Get-VM -Name $Name -ErrorAction Stop
        if ($vm.State -eq 'Off') {
            Write-Progress -Activity 'Unattended install' -Completed
            Write-Detail ("installer finished after {0:N0} minutes" -f ((Get-Date) - $start).TotalMinutes)
            return $true
        }

        $elapsed = ((Get-Date) - $start)
        Write-Progress -Activity 'Unattended install' `
            -Status ("{0} - elapsed {1:hh\:mm\:ss} (watch it live: .\vm.ps1 open -NoWait)" -f $vm.State, $elapsed) `
            -PercentComplete ([Math]::Min(99, [int](100 * $elapsed.TotalMinutes / $TimeoutMinutes)))

        if ($elapsed.TotalMinutes -ge $nextTick) {
            $line = "still installing, {0:N0} of {1} min allowed" -f $elapsed.TotalMinutes, $TimeoutMinutes

            if ($systemDisk) {
                try {
                    $bytes = (Get-Item -LiteralPath $systemDisk -ErrorAction Stop).Length
                    $grew = $bytes - $lastBytes
                    $line += " - system disk {0:N1} GB" -f ($bytes / 1GB)
                    if ($lastBytes -gt 0) {
                        if ($grew -gt 0) { $line += " (+{0:N0} MB)" -f ($grew / 1MB) }
                        else             { $line += " (no growth this tick)" }
                    }
                    $lastBytes = $bytes
                }
                catch {

                    $systemDisk = $null
                }
            }

            $line += ", cpu {0}%" -f $vm.CPUUsage
            Write-Detail $line
            $nextTick += $tickMinutes
        }

        Start-Sleep -Seconds 5
    }

    Write-Progress -Activity 'Unattended install' -Completed
    throw "The install did not finish within $TimeoutMinutes minutes. Open the console with '.\vm.ps1 open -NoWait' to see where it stopped; if it is sitting at an installer prompt, the seed disk was not picked up."
}

function Stop-DevVMMachine {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Name,
        [switch]$Save,
        [switch]$TurnOff,
        [int]$TimeoutSeconds = 120
    )

    Import-Module Hyper-V -ErrorAction Stop

    $vm = Get-VM -Name $Name -ErrorAction SilentlyContinue
    if (-not $vm) { return 'not-created' }
    if ($vm.State -eq 'Off') { return 'already-off' }

    if ($Save) {
        Write-Detail 'saving state, so it resumes where it left off'
        Save-VM -Name $Name
        return 'saved'
    }

    if ($TurnOff) {
        Write-Warn 'turning the power off without a clean shutdown'
        Stop-VM -Name $Name -TurnOff -Force
        return 'turned-off'
    }

    if ($vm.State -in @('Saved', 'Paused')) {
        Write-Detail "state is $($vm.State), so resuming it first to shut down cleanly"
        Start-VM -Name $Name | Out-Null
        Start-Sleep -Seconds 5
    }

    Write-Detail 'asking the guest to shut down'
    Stop-VM -Name $Name -Force -AsJob | Out-Null

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    $start = Get-Date
    $nextTick = 15
    while ((Get-Date) -lt $deadline) {
        $state = (Get-VM -Name $Name -ErrorAction Stop).State
        if ($state -eq 'Off') {
            Write-Detail ("shut down after {0:N0}s" -f ((Get-Date) - $start).TotalSeconds)
            return 'off'
        }
        $elapsed = ((Get-Date) - $start).TotalSeconds
        if ($elapsed -ge $nextTick) {
            Write-Detail ("still shutting down, {0:N0}s elapsed (state: {1})" -f $elapsed, $state)
            $nextTick += 15
        }
        Start-Sleep -Seconds 2
    }

    Write-Warn "still not off after $TimeoutSeconds seconds"
    Write-Warn '  the guest is ignoring the shutdown request. Check the console, or force it:'
    Write-Warn "      .\scripts\Stop-DevVM.ps1 -Name $Name -TurnOff"
    return 'timeout'
}

function Start-DevVMMachine {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Name,
        [string]$Hostname,
        [switch]$WaitForNetwork,
        [int]$TimeoutSeconds = 180
    )

    Import-Module Hyper-V -ErrorAction Stop

    $vm = Get-VM -Name $Name -ErrorAction SilentlyContinue
    if (-not $vm) { return 'not-created' }

    if ($vm.State -eq 'Running') {
        Write-Detail 'already running'
    }
    else {
        $from = $vm.State
        Start-VM -Name $Name | Out-Null
        if ($from -eq 'Saved') { Write-Detail 'resumed from saved state' }
        else { Write-Detail 'started' }
    }

    if (-not $WaitForNetwork) { return 'started' }

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    $start = Get-Date
    $nextTick = 20
    while ((Get-Date) -lt $deadline) {
        $ip = Get-DevVMIPAddress -Name $Name
        if ($ip) {
            Write-Detail ("network up after {0:N0}s, ip {1}" -f ((Get-Date) - $start).TotalSeconds, $ip)
            return 'ready'
        }
        $elapsed = ((Get-Date) - $start).TotalSeconds
        if ($elapsed -ge $nextTick) {
            Write-Detail ("waiting for an address, {0:N0}s elapsed" -f $elapsed)
            $nextTick += 20
        }
        Start-Sleep -Seconds 3
    }

    Write-Warn "no address after $TimeoutSeconds seconds"
    Write-Warn '  it may still be booting. Check the console, or run Get-DevVMStatus.ps1.'
    return 'no-network'
}

function Add-DevVMDataDisks {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Config)

    Import-Module Hyper-V -ErrorAction Stop

    $p = Get-DevVMPaths -Config $Config
    if ($p.DataDisks.Count -eq 0) { return }

    $attached = @(Get-VMHardDiskDrive -VMName $p.Name | Select-Object -ExpandProperty Path)

    foreach ($disk in $p.DataDisks) {
        if ($attached -contains $disk.Path) {
            Write-Detail "data disk '$($disk.Label)' is already attached"
            continue
        }

        if (-not (Test-Path -LiteralPath $disk.Path)) {
            Write-Detail ("creating data disk {0} '{1}' - {2} GB, mounts at {3}" -f `
                $(if ($disk.DriveLetter) { "$($disk.DriveLetter):" } else { '' }),
                $disk.Label, $disk.SizeGB, $disk.MountPoint)

            New-VHD -Path $disk.Path -SizeBytes ([long]$disk.SizeGB * 1GB) -Dynamic | Out-Null
        }

        Add-VMHardDiskDrive -VMName $p.Name -Path $disk.Path `
            -ControllerNumber 0 -ControllerLocation $disk.ControllerLocation
        Write-Detail "attached '$($disk.Label)' at scsi 0:$($disk.ControllerLocation)"
    }
}

function Complete-DevVMInstall {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Config)

    $p = Get-DevVMPaths -Config $Config
    $vm = Get-VM -Name $p.Name -ErrorAction Stop

    if ($vm.State -ne 'Off') {
        throw "Expected '$($p.Name)' to be powered off before finishing the install; it is $($vm.State)."
    }

    Write-Detail 'ejecting the install media'
    foreach ($drive in @(Get-VMDvdDrive -VM $vm)) {
        Remove-VMDvdDrive -VMDvdDrive $drive
    }

    Add-DevVMDataDisks -Config $Config

    if ($vm.Generation -eq 2) {
        $hdd = Get-VMHardDiskDrive -VM $vm | Where-Object { $_.Path -eq $p.OsVhd }
        if ($hdd) {
            Write-Detail 'setting the boot order to the system disk'
            Set-VMFirmware -VM $vm -FirstBootDevice $hdd
        }
    }

    Write-Detail 'starting the VM for first boot + bootstrap'
    Start-VM -Name $p.Name | Out-Null
}

function New-DevVMCheckpoint {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Name,
        [string]$SnapshotName
    )
    if (-not $SnapshotName) {
        $SnapshotName = 'clean-install {0}' -f (Get-Date -Format 'yyyy-MM-dd HH:mm')
    }
    Write-Detail "checkpoint: $SnapshotName"
    Checkpoint-VM -Name $Name -SnapshotName $SnapshotName -ErrorAction Stop
}

function Get-DevVMIPAddress {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Name,
        [int]$TimeoutSeconds = 0
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $addresses = @(Get-VMNetworkAdapter -VMName $Name -ErrorAction SilentlyContinue |
                       Select-Object -ExpandProperty IPAddresses -ErrorAction SilentlyContinue |
                       Where-Object { $_ -and $_ -notmatch ':' })
        if ($addresses.Count -gt 0) { return $addresses[0] }
        if ($TimeoutSeconds -gt 0) { Start-Sleep -Seconds 5 }
    } while ((Get-Date) -lt $deadline)

    return $null
}

function Remove-DevVMMachine {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Name,
        [string]$Directory,
        [switch]$DeleteDisks
    )

    Import-Module Hyper-V -ErrorAction Stop

    $vm = Get-VM -Name $Name -ErrorAction SilentlyContinue
    if ($vm) {
        if ($vm.State -ne 'Off') {
            Write-Detail "stopping '$Name'"
            Stop-VM -Name $Name -TurnOff -Force -ErrorAction SilentlyContinue

            Start-Sleep -Seconds 2
        }

        $disks = @(Get-VMHardDiskDrive -VMName $Name | Select-Object -ExpandProperty Path)

        Get-VMSnapshot -VMName $Name -ErrorAction SilentlyContinue |
            Remove-VMSnapshot -ErrorAction SilentlyContinue

        Write-Detail "removing the VM '$Name'"
        Remove-VM -Name $Name -Force -ErrorAction Stop

        if ($DeleteDisks) {
            foreach ($disk in $disks) {
                if ($disk -and (Test-Path -LiteralPath $disk)) {
                    Write-Detail "deleting $disk"
                    Remove-Item -LiteralPath $disk -Force -ErrorAction SilentlyContinue
                }
            }
        }
    }
    else {
        Write-Detail "no VM named '$Name' exists"
    }

    if ($DeleteDisks -and $Directory -and (Test-Path -LiteralPath $Directory)) {

        $remaining = @(Get-ChildItem -LiteralPath $Directory -Recurse -File |
                       Where-Object { $_.Extension -notin @('.vhdx', '.vhd', '.avhdx', '.vmcx', '.vmrs', '.vmgs', '.bin', '.vsv') })
        if ($remaining.Count -eq 0) {
            Write-Detail "removing $Directory"
            Remove-Item -LiteralPath $Directory -Recurse -Force -ErrorAction SilentlyContinue
        }
        else {
            Write-Warn "left $Directory in place - it still holds $($remaining.Count) unrecognised file(s)."
        }
    }
}

Export-ModuleMember -Function Get-DevVMPaths, Test-DevVMExists, Assert-SwitchExists, Add-DevVMDataDisks,
    Stop-DevVMMachine, Start-DevVMMachine,
    Assert-DevVMNameAvailable,
    New-DevVMMachine, Wait-DevVMInstall, Complete-DevVMInstall, New-DevVMCheckpoint,
    Get-DevVMIPAddress, Remove-DevVMMachine
