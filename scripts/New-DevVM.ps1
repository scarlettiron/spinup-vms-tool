#requires -Version 5.1
<#
.SYNOPSIS
    Build a reproducible Ubuntu desktop VM on Hyper-V from config.

.DESCRIPTION
    Creates the VM, renders an unattended-install seed, waits for the install to
    finish, ejects the media, and boots the machine so its first-boot bootstrap
    can turn it into a developer workstation.

    Configuration comes from vm.config.yaml, optionally overridden by
    vm.config.local.yaml, a Machines block, and the parameters below.

.PARAMETER Machine
    Build one machine from the Machines block in config.

.PARAMETER All
    Build every machine in the Machines block, one after another.

.PARAMETER Blueprint
    Override the blueprint (a directory under blueprints/), for example ubuntu-server.

.PARAMETER RenderOnly
    Render the cloud-init seed and generated bootstrap files into build/ and
    stop. Touches no Hyper-V state and needs no elevation - the fast way to
    review exactly what a config change will do to a guest.

.PARAMETER Force
    Delete and rebuild a VM of the same name if one already exists.

.EXAMPLE
    .\scripts\New-DevVM.ps1
    Build the single VM described by vm.config.yaml.

.EXAMPLE
    .\scripts\New-DevVM.ps1 -Machine ubuntu-middleware-db
    Build one machine from the fleet.

.EXAMPLE
    .\scripts\New-DevVM.ps1 -All
    Build the whole fleet on a shared switch, each reachable as <host>.local.

.EXAMPLE
    .\scripts\New-DevVM.ps1 -RenderOnly
    Show what would be provisioned, without creating anything.
#>
[CmdletBinding(DefaultParameterSetName = 'Single')]
param(
    [Parameter(ParameterSetName = 'Single')]
    [string]$Machine,

    [Parameter(ParameterSetName = 'Fleet')]
    [switch]$All,

    # Plainly -Blueprint: unlike -Profile, it shadows no automatic variable.
    [string]$Blueprint,

    [string]$Name,
    [string]$Hostname,
    [string]$Username,
    [string]$SwitchName,
    [int]$CpuCount,
    [int]$MemoryStartupGB,
    [int]$DiskSizeGB,
    [string]$IsoPath,

    [switch]$RenderOnly,
    [switch]$Force,
    [switch]$SkipCheckpoint
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$libDir = Join-Path $PSScriptRoot 'lib'
Import-Module (Join-Path $libDir 'Common.psm1') -Force -DisableNameChecking
Import-Module (Join-Path $libDir 'Iso.psm1')    -Force -DisableNameChecking
Import-Module (Join-Path $libDir 'Seed.psm1')   -Force -DisableNameChecking
Import-Module (Join-Path $libDir 'HyperV.psm1') -Force -DisableNameChecking

$repoRoot = Get-RepoRoot

# Command-line overrides, shaped like the config tree so they merge the same way.
function Get-CliOverride {
    $o = @{}
    $vm = @{}
    if ($Name)            { $vm['Name'] = $Name }
    if ($SwitchName)      { $vm['SwitchName'] = $SwitchName }
    if ($CpuCount)        { $vm['CpuCount'] = $CpuCount }
    if ($MemoryStartupGB) { $vm['MemoryStartupGB'] = $MemoryStartupGB }
    if ($DiskSizeGB)      { $vm['DiskSizeGB'] = $DiskSizeGB }
    if ($vm.Count)        { $o['VM'] = $vm }

    $guest = @{}
    if ($Hostname) { $guest['Hostname'] = $Hostname }
    if ($Username) { $guest['PrimaryUser'] = @{ Username = $Username } }
    if ($guest.Count) { $o['Guest'] = $guest }

    if ($IsoPath) { $o['Iso'] = @{ Path = $IsoPath } }
    return $o
}

function Show-Plan {
    param($Config)

    $vm = $Config.VM
    $g = $Config.Guest
    $b = $Config.Bootstrap

    Write-Host ''
    $osBits = $Config.Blueprint.Name
    $osData = $Config.Blueprint.Data['Os']
    if ($osData -and $osData.Contains('Vendor')) {
        $osBits = "$($osData['Vendor']) $($Config.Iso['Release'])"
    }
    Write-Host "  VM          $($vm['Name'])  ($osBits)" -ForegroundColor White
    Write-Detail "blueprint      $($Config.Blueprint.Name)$(if (@($Config.Blueprint.Chain).Count -gt 1) { ', inherits ' + ((@($Config.Blueprint.Chain) | Select-Object -Skip 1) -join ', ') })"
    Write-Detail "hardware       $($vm['CpuCount']) vCPU / $($vm['MemoryStartupGB']) GB RAM / $($vm['DiskSizeGB']) GB system disk"

    $dataDisks = @(Get-DevVMDataDiskLayout -Config $Config)
    foreach ($disk in $dataDisks) {
        $drive = 'disk'
        if ($disk.DriveLetter) { $drive = "$($disk.DriveLetter):" }
        Write-Detail ("data disk      {0,-4} {1,-10} {2,4} GB  at {3}" -f `
            $drive, $disk.Label, $disk.SizeGB, $disk.MountPoint)
    }

    Write-Detail "switch         $($vm['SwitchName'])"
    Write-Detail "guest          user '$($g['PrimaryUser']['Username'])'  tz $($g['Timezone'])"
    if ($b.Contains('EnableMdns') -and $b['EnableMdns']) {
        Write-Detail "address        http://$($g['Hostname']).local"
    }
    else {
        Write-Detail "hostname       $($g['Hostname'])  (mDNS off, so no .local address)"
    }

    $extras = New-Object System.Collections.Generic.List[string]
    foreach ($pair in @(
        @('desktop', 'InstallDesktop'), @('docker', 'InstallDocker'), @('ddev', 'InstallDdev'),
        @('node', 'InstallNode'), @('python', 'InstallPython'), @('vscode', 'InstallVSCode'),
        @('chrome', 'InstallChrome'), @('postman', 'InstallPostman'),
        @('remote-access', 'InstallRemoteAccess'),
        @('mdns', 'EnableMdns'))) {
        if ($b.Contains($pair[1]) -and $b[$pair[1]]) { $extras.Add($pair[0]) }
    }
    Write-Detail "bootstrap      $((@($extras)) -join ', ')"

    $node = 'off'
    if ($b.Contains('InstallNode') -and $b['InstallNode']) {
        $node = "nvm $($b['NvmVersion']) / node $($b['NodeVersion'])"
    }
    Write-Detail "node           $node"

    # The resolved plan, so the version and port shown are the ones the guest will
    # actually use rather than the blanks that may be in config.
    $db = Get-DevVMDatabasePlan -Config $Config
    if ($db) {
        $what = if ($db.InstallServer) { 'client + server' } else { 'client only' }
        Write-Detail "database       $($db.DisplayName) $($db.Version), $what"
        if ($db.InstallServer) {
            $reach = if ($db.Remote) {
                "port $($db.Port), reachable as $($g['Hostname']).local (firewall opened)"
            }
            else {
                "port $($db.Port), localhost only"
            }
            Write-Detail "               $reach"
            if ($db.Databases.Count -gt 0) {
                Write-Detail "               creates $(($db.Databases) -join ', ')"
            }
        }
        if ($db.AdminerInstall) {
            Write-Detail "adminer        $($db.AdminerVersion) at http://$($g['Hostname']).local:$($db.AdminerPort)/"
        }
    }

    $copies = @(Get-DevVMFileCopyLayout -Config $Config)
    if ($copies.Count -gt 0) {
        # Counted in a loop: Measure-Object -Property takes a name, not a
        # scriptblock, in Windows PowerShell 5.1.
        $totalFiles = 0
        foreach ($copy in $copies) { $totalFiles += $copy.Files.Count }
        $totalMb = (($copies | Measure-Object -Property SizeBytes -Sum).Sum) / 1MB
        Write-Detail ("files          {0} file(s), {1:N1} MB, from {2} director(ies)" -f `
            $totalFiles, $totalMb, $copies.Count)
        foreach ($copy in $copies) {
            Write-Detail ("               {0}  to {1}  (owner {2})" -f `
                (Split-Path -Leaf $copy.From), $copy.To, $copy.Owner)
        }
    }

    $hooks = Get-DevVMHookPlan -Config $Config
    if ($hooks) {
        $when = if ($hooks.RunAtEveryBoot) { 'on every boot' } else { 'once, after provisioning' }
        $what = "$($hooks.Commands.Count) command(s)"
        if ($hooks.ScriptFile) { $what += " + $(Split-Path -Leaf $hooks.ScriptFile)" }
        Write-Detail "hooks          $what, as $($hooks.RunAs), $when"
    }

    # Only worth a line on the one blueprint that needs it. An unregistered RHEL
    # is the single case where a build can succeed and still leave a machine that
    # cannot install anything, so it is called out here rather than left to be
    # discovered in the bootstrap log.
    $osVendor = ''
    if ($Config.Blueprint.Data.Contains('Os') -and $Config.Blueprint.Data['Os'] -and
        $Config.Blueprint.Data['Os'].Contains('Vendor')) {
        $osVendor = [string]$Config.Blueprint.Data['Os']['Vendor']
    }
    if ($osVendor -eq 'rhel') {
        $rh = Get-DevVMRedHatPlan -Config $Config
        switch ($rh.Mode) {
            'activation-key' {
                $pin = if ($rh.PinRelease) { ", pinned to $($Config.Iso['Release'])" } else { '' }
                Write-Detail "subscription   activation key, org $($rh.OrganizationId)$pin"
            }
            'credentials' {
                Write-Detail "subscription   account $($rh.Username) (an activation key would avoid putting a password on the seed disk)"
            }
            default {
                Write-Warn 'no Red Hat subscription configured, so this VM will have NO package repositories.'
                Write-Warn '  It will install and boot with everything on the DVD, but bootstrap can add nothing:'
                Write-Warn '  no EPEL, so no xrdp, so no Enhanced Session Mode.'
                Write-Warn '  Set redHat.activationKey and redHat.organizationId in vm.config.local.yaml.'
            }
        }
    }

    $gh = $Config.GitHub
    if ($gh.Count -gt 0 -and $gh.Contains('SshKey')) {
        # The resolved source, so 'auto' reports what it actually found.
        $keySource = Resolve-SshKeySource -Config $Config
        $mode = $keySource.Mode
        $bits = New-Object System.Collections.Generic.List[string]
        if ($gh.Contains('Username') -and $gh['Username']) { $bits.Add("account: $($gh['Username'])") }
        $bits.Add("ssh key: $($keySource.Source)")
        if ($gh.Contains('Cli') -and $gh['Cli']['Install']) { $bits.Add('gh cli') }
        $hasToken = ($gh.Contains('Cli') -and ($gh['Cli']['Token'] -or $gh['Cli']['TokenFile']))
        if ($hasToken) { $bits.Add('token supplied') }
        if ($hasToken -and $gh.Contains('UploadSshKey') -and $gh['UploadSshKey']) { $bits.Add('upload key') }
        Write-Detail "github         $((@($bits)) -join ', ')"
    }

    $gitCfg = $Config.Git
    if ($gitCfg.Count -gt 0 -and $gitCfg.Contains('UserEmail') -and $gitCfg['UserEmail']) {
        Write-Detail "git identity   $($gitCfg['UserName']) <$($gitCfg['UserEmail'])>"
    }

    $additional = @()
    if ($g.Contains('AdditionalUsers') -and $g['AdditionalUsers']) { $additional = @($g['AdditionalUsers']) }
    if ($additional.Count) {
        Write-Detail "extra users    $((@($additional | ForEach-Object { $_['Username'] })) -join ', ')"
    }

    $repos = @(Get-DevVMRepoLayout -Config $Config)
    if ($repos.Count -gt 0) {
        Write-Detail "repos          $($repos.Count) into ~/$(Get-DevVMRepoRoot -Config $Config)"
        foreach ($repo in $repos) {
            $suffix = ''
            if ($repo.Branch) { $suffix = " (branch $($repo.Branch))" }
            Write-Detail ("               {0,-22} from {1}{2}" -f $repo.Directory, $repo.Url, $suffix)
        }

        # An ssh remote with no usable key cannot clone. Say so now rather than
        # letting it fail twenty minutes into a first boot.
        $sshMode = 'none'
        if ($gh.Count -gt 0 -and $gh.Contains('SshKey')) { $sshMode = [string]$gh['SshKey']['Mode'] }
        $needsSsh = @($repos | Where-Object { $_.NeedsSsh })

        if ($needsSsh.Count -gt 0 -and $sshMode -eq 'none') {
            Write-Warn "$($needsSsh.Count) repo(s) use an ssh remote but GitHub.SshKey.Mode is 'none' - those clones will fail."
        }
        elseif ($needsSsh.Count -gt 0 -and $sshMode -eq 'generate') {
            $uploads = ($gh.Contains('UploadSshKey') -and $gh['UploadSshKey'] -and
                        $gh.Contains('Cli') -and ($gh['Cli']['Token'] -or $gh['Cli']['TokenFile']))
            if (-not $uploads) {
                Write-Warn "$($needsSsh.Count) repo(s) use an ssh remote and the key is generated inside the guest, so it is not on GitHub yet - private clones will fail on the first boot. Supply GitHub.Cli.TokenFile with UploadSshKey, or use an https:// remote. See docs/repos.md."
            }
        }
    }
}

function Show-Summary {
    param($Config, $Seed)

    $vmName = [string]$Config.VM['Name']
    $hostName = [string]$Config.Guest['Hostname']
    $user = [string]$Config.Guest['PrimaryUser']['Username']

    # Hand back commands through vm.ps1, the same front door the README teaches,
    # rather than the raw vmconnect.exe and scripts\ paths this used to print.
    # Someone who has just run '.\vm.ps1 create' should not have to learn a
    # second vocabulary to look at the thing they built.
    #
    # A machine built out of a Machines block needs -Machine to be found again,
    # so carry it; the single default VM needs nothing.
    $for = ''
    if ($Config.Machine) { $for = " -Machine $($Config.Machine)" }

    Write-Host ''
    Write-Host '--------------------------------------------------------------' -ForegroundColor DarkGray
    Write-Host " $vmName is installing its desktop and toolchain now." -ForegroundColor Green
    Write-Host '--------------------------------------------------------------' -ForegroundColor DarkGray
    Write-Host ''
    Write-Host '  The guest reboots itself into the GNOME login screen once' -ForegroundColor Gray
    Write-Host '  bootstrap finishes. Expect 15-30 minutes on a fresh build.' -ForegroundColor Gray
    Write-Host ''
    Write-Host '  Open the desktop:' -ForegroundColor White
    Write-Host "      .\vm.ps1 open$for"
    Write-Host '        (waits for the guest, so you get an Enhanced Session:' -ForegroundColor DarkGray
    Write-Host '         resizable, shared clipboard. Add -NoWait to look now.)' -ForegroundColor DarkGray
    Write-Host '  Check its state:' -ForegroundColor White
    Write-Host "      .\vm.ps1 status$for"
    Write-Host '  Watch bootstrap:' -ForegroundColor White
    Write-Host "      .\vm.ps1 status$for -Report"
    Write-Host '        (or, once mDNS is up and your key is installed:' -ForegroundColor DarkGray
    Write-Host "         ssh $user@$hostName.local 'tail -f /var/log/vm-bootstrap.log')" -ForegroundColor DarkGray
    Write-Host ''

    # Name the format this blueprint actually rendered. Saying "cloud-init" on a
    # kickstart build was simply wrong, and it is the one line pointing at the
    # artefacts someone would open when a build goes wrong.
    $method = 'seed'
    $prov = $Config.Blueprint.Data['Provisioning']
    if ($prov -and $prov.Contains('Method') -and $prov['Method']) { $method = [string]$prov['Method'] }
    Write-Detail "seed + rendered $method files: $($Seed.BuildDir)"
}

function Invoke-Build {
    param([string]$MachineName)

    $config = Get-DevVMConfig -RepoRoot $repoRoot -Machine $MachineName `
        -Override (Get-CliOverride) -BlueprintName $Blueprint

    # Counted up front so each step can announce itself as [3/7]. A dry run stops
    # after rendering, and the checkpoint is optional, so the total is not fixed.
    if ($RenderOnly) {
        $stepCount = 2
    }
    else {
        $stepCount = 7
        if (-not $SkipCheckpoint -and $config.VM.Contains('CheckpointAfterInstall') `
            -and $config.VM['CheckpointAfterInstall']) { $stepCount++ }
    }
    Reset-BuildSteps -Total $stepCount

    Start-BuildStep "Planning $($config.VM['Name'])"
    Show-Plan -Config $config

    if ($RenderOnly) {
        # The DNS half of the name check needs neither elevation nor Hyper-V, so
        # it is worth reporting even in a dry run. A warning, not a failure:
        # nothing is being created.
        $taken = Test-DevVMHostnameResolvable -Hostname ([string]$config.Guest['Hostname'])
        if ($taken) {
            Write-Warn "$($config.Guest['Hostname']).local already resolves to $taken - a real build would refuse this name."
        }

        Start-BuildStep 'Rendering the seed (no Hyper-V changes)'
        $seed = New-DevVMSeed -Config $config -SkipDisk
        Write-Detail "seed:        $($seed.SeedPath) ($($seed.Method), label $($seed.SeedLabel))"
        Write-Detail "bootstrap:   $($seed.Files.Count) file(s) embedded"
        Write-BuildReport -Title "render report: $($config.VM['Name'])"
        Write-Host ''
        Write-Host '  Render complete. Review the files above, then re-run without -RenderOnly.' -ForegroundColor Green
        return
    }

    Start-BuildStep 'Checking the name is free'
    Assert-DevVMNameAvailable -Config $config -Force:$Force

    Start-BuildStep 'Resolving install media'
    $iso = Resolve-DevVMIso -Config $config

    Start-BuildStep 'Building the seed disk'
    $seed = New-DevVMSeed -Config $config

    Start-BuildStep "Creating the Hyper-V VM '$($config.VM['Name'])'"
    New-DevVMMachine -Config $config -IsoPath $iso -SeedVhdPath $seed.SeedVhdPath -Force:$Force | Out-Null

    # A fresh install means a fresh host key. Anything this laptop remembers for
    # that name is now wrong, and ssh would refuse every later connection with
    # REMOTE HOST IDENTIFICATION HAS CHANGED - including the status command's own
    # probe. Forget it here rather than making someone run ssh-keygen -R to get
    # into a VM this tool just built for them.
    if (Remove-KnownHostEntry -HostName ([string]$config.Guest['Hostname'])) {
        Write-Detail "dropped the old $([string]$config.Guest['Hostname']) entry from known_hosts (this is a fresh install, so its host key changed)"
    }

    $prov = $config.Blueprint.Data['Provisioning']
    $timeout = 60
    if ($prov.Contains('InstallTimeoutMinutes') -and $prov['InstallTimeoutMinutes']) {
        $timeout = [int]$prov['InstallTimeoutMinutes']
    }

    Start-BuildStep 'Running the unattended install'
    # -NoWait on purpose: the installer is running, so there is no desktop to
    # wait for, and the point is to watch it happen.
    $watchFor = ''
    if ($config.Machine) { $watchFor = " -Machine $($config.Machine)" }

    # Say where the packages come from, because that is what the wait actually
    # is. Three different answers, and guessing one of them wrongly is worse
    # than saying nothing: Ubuntu declares no InstallSource at all, so anything
    # that treats "not cdrom" as "netinst" tells a Ubuntu build it is
    # downloading a package set it is in fact reading off the ISO.
    $source = ''
    if ($prov.Contains('InstallSource')) { $source = [string]$prov['InstallSource'] }

    if (-not $source) {
        # cloud-init autoinstall: the base system is on the ISO. The desktop and
        # the toolchain arrive later, during bootstrap, not now.
        Write-Detail 'installing the base system from the ISO; the desktop and tools come later, during bootstrap'
    }
    elseif ($source -match '^\s*cdrom') {
        Write-Detail 'installing from the DVD in the drive, so no packages are downloaded'
    }
    else {
        Write-Detail 'the image carries only the installer, so the packages come from the mirror now'
        Write-Detail '  most of the wait is that download; it follows your connection, not this laptop'
    }
    Write-Detail "roughly 10-20 minutes. Watch the installer's own progress with: .\vm.ps1 open$watchFor -NoWait"
    Start-VM -Name ([string]$config.VM['Name']) | Out-Null
    Wait-DevVMInstall -Name ([string]$config.VM['Name']) -TimeoutMinutes $timeout | Out-Null

    if (-not $SkipCheckpoint -and $config.VM.Contains('CheckpointAfterInstall') -and $config.VM['CheckpointAfterInstall']) {
        Start-BuildStep 'Checkpointing the clean install'
        # Taken before bootstrap so a broken tool install can be rolled back
        # without repeating the OS install.
        New-DevVMCheckpoint -Name ([string]$config.VM['Name']) -SnapshotName 'clean-install (pre-bootstrap)'
    }

    Start-BuildStep 'Finishing the install'
    Complete-DevVMInstall -Config $config

    Write-BuildReport -Title "host build report: $($config.VM['Name'])"
    Show-Summary -Config $config -Seed $seed
}

# ---------------------------------------------------------------------- main

# Machines finished, for the fleet-level line and for the failure path.
$script:BuiltMachines = New-Object System.Collections.Generic.List[string]

try {
    if (-not $RenderOnly) {
        Assert-Prerequisites -RequireHyperV
    }

    if ($All) {
        $machines = Get-MachineNames -RepoRoot $repoRoot
        if ($machines.Count -eq 0) {
            throw '-All was given but vm.config.yaml defines no Machines block.'
        }
        # Resolve every machine's identity up front. Two machines resolving to one
        # VM name would have the second silently overwrite the first, and two
        # sharing a hostname would fight over the same mDNS name, so refuse before
        # anything is created.
        $seenNames = @{}
        $seenHosts = @{}
        foreach ($m in $machines) {
            $peek = Get-DevVMConfig -RepoRoot $repoRoot -Machine $m -Override (Get-CliOverride) -BlueprintName $Blueprint

            $vmName = [string]$peek.VM['Name']
            if ($seenNames.ContainsKey($vmName)) {
                throw "Machines '$($seenNames[$vmName])' and '$m' both resolve to the VM name '$vmName'. Give each machine its own VmName inside its Machines block."
            }
            $seenNames[$vmName] = $m

            # NOT $hostName. PowerShell variable names are case-insensitive, so
            # $hostName IS the -Hostname parameter: assigning it here overwrote
            # the parameter, Get-CliOverride then read it back as if the user had
            # passed -Hostname, and every machine after the first was forced onto
            # the FIRST machine's hostname. -All then refused to build, claiming
            # two machines collided when the config was fine.
            #
            # This loop runs at script scope, which is what makes it bite; the
            # same name inside a function would only shadow it locally.
            $machineHostname = [string]$peek.Guest['Hostname']
            if ($seenHosts.ContainsKey($machineHostname)) {
                throw "Machines '$($seenHosts[$machineHostname])' and '$m' both resolve to the hostname '$machineHostname'. They would fight over $machineHostname.local - give each its own Hostname."
            }
            $seenHosts[$machineHostname] = $m
        }

        Write-Step "Building $($machines.Count) machine(s): $($machines -join ', ')"
        # Each machine keeps its own step report, so track them at the fleet level
        # too: if the third of three fails, the report has to say the first two
        # stand and were not rolled back.
        foreach ($m in $machines) {
            Invoke-Build -MachineName $m
            $script:BuiltMachines.Add($m)
        }

        Write-Host ''
        Write-Host "  Built $($script:BuiltMachines.Count) of $($machines.Count) machine(s): $($script:BuiltMachines -join ', ')" -ForegroundColor Green
        Write-Host ''
        Write-Host '  All machines share one virtual switch, so once both are up they' -ForegroundColor Gray
        Write-Host '  resolve each other by name, for example  ping ubuntu-middleware-db.local' -ForegroundColor Gray
    }
    else {
        Invoke-Build -MachineName $Machine
    }
}
catch {
    # The report comes first: which step stopped it is more use than the exception
    # text on its own, and a build that threw is exactly the one someone needs a
    # report for.
    Write-BuildReport -Title 'host build report (FAILED)' -FailureMessage $_.Exception.Message
    if ($script:BuiltMachines.Count -gt 0) {
        Write-Host ''
        Write-Host "  Already built before this failure: $($script:BuiltMachines -join ', ')" -ForegroundColor Yellow
        Write-Host '  Those machines are left in place. Re-run with -Machine to build the rest.' -ForegroundColor Yellow
    }
    if ($_.ScriptStackTrace) {
        Write-Verbose $_.ScriptStackTrace
    }
    Write-Host ''
    Write-Host 'See docs/troubleshooting.md, or re-run with -Verbose for a stack trace.' -ForegroundColor Yellow
    exit 1
}
