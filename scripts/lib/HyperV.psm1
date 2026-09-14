#requires -Version 5.1
Set-StrictMode -Version Latest

<#
    Everything that touches Hyper-V.

    Keeping the hypervisor calls in one module is what makes a different OS a
    config change rather than a rewrite: nothing in here knows or cares that the
    guest is Ubuntu, apart from the Secure Boot template the blueprint names.
#>

# No -Force here: re-importing Common would unload it from the caller's
# session state, taking its functions with it.
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
        # The guest's own name, which is not always the VM's: it is what mDNS
        # publishes and what ssh and known_hosts key off, so anything tidying up
        # after a machine needs it alongside the paths.
        Hostname  = [string]$Config.Guest['Hostname']
        Dir       = $dir
        OsVhd     = Join-Path $dir "$name.vhdx"
        SeedVhd   = Join-Path $dir "$name-seed.vhdx"
        # .ToArray(), not @(): @() throws on an empty List[object] in PS 5.1.
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

<#
    Refuse to build over a name that is already taken.

    Two distinct clashes, both worth catching before anything is created:

      the VM name    another Hyper-V VM already has it. -Force replaces it; that
                     is the only way past this.
      the hostname   <hostname>.local already answers on this network. That is
                     not a Hyper-V question - it could be a teammate's machine on
                     the same switch - and it matters because two machines
                     claiming one mDNS name means avahi renames one to
                     <hostname>-2.local and the address you were told to use
                     reaches the wrong box.

    The hostname check is skipped when -Force is replacing a VM of this name,
    since the thing answering is almost certainly the VM being replaced.
#>
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

    # A different VM using our hostname as its own name is a coming collision.
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

<#
    Create the VM, its disks, firmware, and integration settings.
#>
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

    # Nested virtualisation and dynamic memory are mutually exclusive, so the
    # flag has to win rather than silently failing later.
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

    # Explicit SCSI locations throughout. The guest identifies its data disks by
    # LUN, so letting Hyper-V place them automatically - and renumber when the
    # DVD is ejected - would break 00-disks.sh.
    $loc = Get-DevVMDiskLocations

    Write-Detail "attaching install media: $IsoPath"
    Add-VMDvdDrive -VM $machine -ControllerNumber 0 -ControllerLocation $loc.Dvd -Path $IsoPath

    Write-Detail "attaching the seed disk (scsi 0:$($loc.Seed))"
    Add-VMHardDiskDrive -VM $machine -Path $SeedVhdPath `
        -ControllerNumber 0 -ControllerLocation $loc.Seed

    # Data disks are deliberately NOT attached yet. Add-DevVMDataDisks runs after
    # the install, so during it the only disks present are the system disk and
    # the tiny seed volume.
    #
    # This is not tidiness, it is the only reliable protection. An OS installer
    # picks its target from whatever it can see: Ubuntu's autoinstall uses
    # 'match: size: largest', and with a 25 GB system disk beside a 50 GB data
    # disk that resolves to the wrong one; Fedora's kickstart 'clearpart --all'
    # would happily wipe every disk attached. A disk that is not plugged in
    # cannot be chosen or erased.
    Write-Detail "data disks attach after the install ($($p.DataDisks.Count) to come)"

    if ($generation -eq 2) {
        $secureBoot = $true
        if ($vm.Contains('SecureBoot')) { $secureBoot = [bool]$vm['SecureBoot'] }

        if ($secureBoot) {
            $template = 'MicrosoftUEFICertificateAuthority'
            if ($firmware.Contains('SecureBootTemplate') -and $firmware['SecureBootTemplate']) {
                $template = [string]$firmware['SecureBootTemplate']
            }
            # Ubuntu's shim is signed by the Microsoft UEFI CA, not the Windows
            # production CA, so the default template will not boot it.
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

        # Boot the installer first; Complete-DevVMInstall flips this afterwards.
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
            # Routes the VM window's RDP session over an hvsocket, so Enhanced
            # Session works with no network path to the guest. The guest half is
            # bootstrap step 20-hyperv.sh.
            Set-VM -VM $machine -EnhancedSessionTransportType HvSocket -ErrorAction Stop
            Write-Detail 'enhanced session transport set to HvSocket'
        }
        catch {
            Write-Warn "could not set the enhanced session transport: $($_.Exception.Message)"
        }
    }

    return (Get-VM -Name $p.Name)
}

<#
    Block until the unattended install powers the VM off.
#>
function Wait-DevVMInstall {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Name,
        [int]$TimeoutMinutes = 60
    )

    $deadline = (Get-Date).AddMinutes($TimeoutMinutes)
    $start = Get-Date
    # Write-Progress draws a bar that leaves nothing behind, so also print a line
    # every couple of minutes. Someone who redirected the build to a file, or who
    # comes back to a scrolled-away console, still gets to see it was making
    # progress rather than wondering whether it hung.
    $nextTick = 2
    $tickMinutes = 2

    # "still installing, 4 min elapsed" on its own says only that time passed. On
    # a netinst blueprint most of that time is spent pulling packages from a
    # mirror, and the honest question - is this working or has it wedged? - needs
    # something that MOVES.
    #
    # Two signals, both from the hypervisor, because nothing inside the installer
    # is reachable: no SSH, no mDNS, and the integration daemons are not running
    # yet. The system disk grows as packages land, and CPU is non-zero while it
    # is unpacking them. A tick where the disk has not grown and the CPU is idle
    # is the shape of a machine genuinely stuck, usually at a prompt the seed
    # disk failed to answer.
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

            # Size of the file on the host, not the guest's view: it is a
            # dynamically expanding VHDX, so it grows as the install writes.
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
                    # A locked or vanished file is not worth failing a build over.
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
    throw "The install did not finish within $TimeoutMinutes minutes. Open the console with '.\vm.ps1 open -NoWait' to see where it stopped; if it is sitting at an installer prompt, the seed disk was not picked up - see docs/troubleshooting.md."
}

<#
    Stop a VM without touching its disks.

    Three ways down, and the difference matters:

      shutdown  the default. Asks the guest to shut down through the integration
                services, so filesystems are flushed and services stop cleanly.
      save      writes the guest's memory to disk and releases it, so the machine
                resumes exactly where it was. Costs disk equal to the assigned RAM.
      turn off  cuts the power. Fast, and risks the filesystem damage you would
                expect from pulling the plug on a real machine.

    A graceful shutdown depends on the guest cooperating, so it is polled with a
    deadline rather than waited on indefinitely: a guest sitting on an unsaved
    file dialog or a hung service will never go down on its own, and a script that
    blocks forever is worse than one that says so.
#>
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

    # A saved or paused machine has no running OS to ask, so there is nothing to
    # request politely. Bring it back first, then it can shut itself down.
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

<#
    Start a VM that already exists. Provisioning never re-runs: bootstrap is gated
    on /var/lib/vm-bootstrap/complete, and the unit is skipped once a clean run has
    happened, so this is an ordinary boot of a machine that is already built.

    -WaitForNetwork polls until the guest has an address, which is the point at
    which it is actually usable rather than merely powered on.
#>
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

<#
    Create and attach the data disks, at their fixed SCSI locations.

    Split out of VM creation on purpose: this must not run until the OS installer
    has finished, or the installer may choose one of these as its target. Safe to
    re-run - a disk already attached is left alone.
#>
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
            # Dynamic, so a 50 GB drive costs only what is written to it.
            New-VHD -Path $disk.Path -SizeBytes ([long]$disk.SizeGB * 1GB) -Dynamic | Out-Null
        }

        Add-VMHardDiskDrive -VMName $p.Name -Path $disk.Path `
            -ControllerNumber 0 -ControllerLocation $disk.ControllerLocation
        Write-Detail "attached '$($disk.Label)' at scsi 0:$($disk.ControllerLocation)"
    }
}

<#
    Post-install: eject the ISO, attach the data disks, boot from disk, and start.
#>
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

    # Now that the installer is done and cannot target them, the data disks go on.
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

<#
    Best-effort guest address, once the integration services are up.
#>
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
            # Give the worker process a moment to release the VHDX handles.
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
        # Only clear the directory if nothing but Hyper-V's own scaffolding is
        # left - never blow away a folder that still has someone's data in it.
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
