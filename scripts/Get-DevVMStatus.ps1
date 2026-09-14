#requires -Version 5.1
<#
.SYNOPSIS
    Show what each dev VM is, where to reach it, and how it is doing.

.DESCRIPTION
    One place to answer 'what is this machine, is it up, what address do I browse
    to, and did bootstrap finish?'.

    Hyper-V is the source of truth for a VM that exists: CPU, memory and disk
    sizes are read back from the hypervisor rather than from config, so drift
    between the two shows up rather than hiding. Config fills in the blanks
    Hyper-V cannot know (blueprint, mount points, drive letters), and the guest
    fills in the rest over SSH when a key or agent lets us in: the OS it is
    actually running, real disk usage, and bootstrap state.

    Everything degrades: no SSH still gives you the full host-side picture, and a
    VM that has not been built yet is described from config so you can see what
    would be created.

.PARAMETER Name
    A Hyper-V VM name. Config for it is matched up automatically when one of the
    configured machines resolves to that name.

.PARAMETER Machine
    One machine from the Machines block, instead of all of them.

.PARAMETER Follow
    Redraw every 10 seconds.

.PARAMETER NoSsh
    Skip the guest probe and report only what the host knows.

.PARAMETER Report
    Also print the guest's own bootstrap report
    (/var/log/vm-bootstrap-report.txt), which lists every step with its status
    and timing. Needs SSH access.

.EXAMPLE
    .\scripts\Get-DevVMStatus.ps1

.EXAMPLE
    .\scripts\Get-DevVMStatus.ps1 -Name acme-fedora-workstation -Follow

.EXAMPLE
    .\scripts\Get-DevVMStatus.ps1 -Report
    Show which bootstrap steps passed and failed inside each VM.
#>
[CmdletBinding()]
param(
    [string]$Name,
    [string]$Machine,
    [switch]$Follow,
    [switch]$NoSsh,
    [switch]$Report
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$libDir = Join-Path $PSScriptRoot 'lib'
Import-Module (Join-Path $libDir 'Common.psm1') -Force -DisableNameChecking
Import-Module (Join-Path $libDir 'HyperV.psm1') -Force -DisableNameChecking

$repoRoot = Get-RepoRoot

# ------------------------------------------------------------------- targets

<#
    Every machine we know about, each carrying its config so the report can name
    the blueprint, the drive letters and the mount points that Hyper-V has no
    idea about.

    -Name takes a Hyper-V name rather than a config key, so match it back to a
    configured machine when one resolves to it. That is the difference between a
    full report and a bare one for the common case of naming a VM directly.
#>
function Resolve-Targets {
    # Status with no target named reports on everything, unlike the build and
    # lifecycle scripts which default to the single configured VM. -All gives
    # Get-DevVMTargets that behaviour, and it falls back to the one VM when there
    # is no Machines block to enumerate.
    $wantAll = -not $Machine
    return @(Get-DevVMTargets -RepoRoot $repoRoot -Machine $Machine -Name $Name -All:$wantAll)
}

# --------------------------------------------------------------- guest probe

<#
    One round trip collects everything the guest can tell us, as tab separated
    "key<TAB>field..." lines. One probe rather than several because -Follow runs
    this on a timer, and because a machine that is slow to answer should be slow
    once rather than five times.

    Written without single quotes on purpose: it lives in a single-quoted
    PowerShell here-string, so a quote of that kind would end the string.
#>
$script:GuestProbe = @'
if systemctl is-active --quiet vm-bootstrap.service; then
    printf "bootstrap\tprogress\t%s\n" "$(ls /var/lib/vm-bootstrap/*.done 2>/dev/null | wc -l)"
elif [ -f /var/lib/vm-bootstrap/complete ]; then
    printf "bootstrap\tcomplete\t\n"
elif [ -f /var/log/vm-bootstrap-report.txt ]; then
    printf "bootstrap\tfailed\t%s\n" "$(sed -n "s/^ FAILED: //p" /var/log/vm-bootstrap-report.txt | head -1)"
else
    printf "bootstrap\tprogress\t%s\n" "$(ls /var/lib/vm-bootstrap/*.done 2>/dev/null | wc -l)"
fi

. /etc/os-release 2>/dev/null
printf "os\t%s\n" "${PRETTY_NAME:-unknown}"
printf "kernel\t%s\n" "$(uname -r)"
printf "guestuptime\t%s\n" "$(uptime -p 2>/dev/null | sed "s/^up //")"

for u in $(getent passwd | awk -F: "\$3>=1000 && \$3<65534 {print \$1}"); do
    # GECOS is comma separated and usually only the first part is a name, so an
    # account with no real name gives ",,," rather than nothing. Take field one.
    printf "user\t%s\t%s\t%s\n" "$u" \
        "$(getent passwd "$u" | cut -d: -f5 | cut -d, -f1)" \
        "$(id -nG "$u" 2>/dev/null)"
done

for mp in / $(awk -F"\t" "!/^#/ && NF>2 {print \$3}" /etc/vm-bootstrap.disks 2>/dev/null); do
    [ -d "$mp" ] || continue
    set -- $(df -P -B1 "$mp" 2>/dev/null | tail -1)
    [ -n "$2" ] || continue
    printf "df\t%s\t%s\t%s\t%s\n" "$mp" "$2" "$3" "$5"
done

if [ -s /var/lib/vm-bootstrap/database ]; then
    printf "database\t%s\t%s\t%s\t%s\t%s\t%s\n" \
        "$(cut -f1 /var/lib/vm-bootstrap/database | head -1)" \
        "$(cut -f2 /var/lib/vm-bootstrap/database | head -1)" \
        "$(cut -f3 /var/lib/vm-bootstrap/database | head -1)" \
        "$(cut -f4 /var/lib/vm-bootstrap/database | head -1)" \
        "$(cut -f5 /var/lib/vm-bootstrap/database | head -1)" \
        "$(cut -f6 /var/lib/vm-bootstrap/database | head -1)"
fi

if [ -s /var/lib/vm-bootstrap/adminer ]; then
    printf "adminer\t%s\t%s\t%s\t%s\n" \
        "$(cut -f1 /var/lib/vm-bootstrap/adminer | head -1)" \
        "$(cut -f2 /var/lib/vm-bootstrap/adminer | head -1)" \
        "$(cut -f3 /var/lib/vm-bootstrap/adminer | head -1)" \
        "$(cut -f4 /var/lib/vm-bootstrap/adminer | head -1)"
fi

if [ -f /var/log/vm-post-start.log ]; then
    printf "hooks\t%s\t%s\n" \
        "$(wc -l < /var/log/vm-post-start.log | tr -d ' ')" \
        "$(systemctl is-enabled vm-post-start.service 2>/dev/null || echo once)"
fi

if [ -s /var/lib/vm-bootstrap/github-auth ]; then
    printf "github\t%s\t%s\n" \
        "$(cut -f1 /var/lib/vm-bootstrap/github-auth | head -1)" \
        "$(cut -f2 /var/lib/vm-bootstrap/github-auth | head -1)"
fi
'@

function Invoke-GuestCommand {
    param([string]$SshTarget, [string]$Command)

    if ($NoSsh -or -not $SshTarget) { return $null }
    if (-not (Get-Command ssh.exe -ErrorAction SilentlyContinue)) { return $null }

    # $ErrorActionPreference has to be dropped to 'Continue' around this call.
    #
    # This script runs with 'Stop', and in Windows PowerShell 5.1 a native
    # command that writes to stderr under 'Stop' throws a terminating
    # NativeCommandError - and '2>$null' does NOT prevent it, because the error
    # record is created before the redirection discards the text. So the single
    # most ordinary situation there is, a guest that is not reachable yet, took
    # the whole status command down with
    #
    #     STATUS FAILED: ssh: Could not resolve hostname <vm>.local
    #
    # which is exactly when you most want the rest of the status. Every guest
    # fact here is optional by design, so a failure must return $null and let
    # the caller print what it does know.
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        # BatchMode so a machine we have no key for fails fast instead of prompting.
        $out = & ssh.exe -o BatchMode=yes -o ConnectTimeout=4 -o StrictHostKeyChecking=accept-new `
            $SshTarget $Command 2>$null
    }
    catch { return $null }
    finally { $ErrorActionPreference = $previous }

    if ($LASTEXITCODE -ne 0) { return $null }
    return $out
}

function Get-GuestFacts {
    param([string]$SshTarget)

    $out = Invoke-GuestCommand -SshTarget $SshTarget -Command $script:GuestProbe
    if ($null -eq $out) { return $null }

    $facts = @{}
    foreach ($line in @($out)) {
        $parts = ([string]$line) -split "`t"
        if ($parts.Count -lt 2) { continue }
        $key = $parts[0]
        if (-not $facts.ContainsKey($key)) {
            $facts[$key] = New-Object System.Collections.Generic.List[object]
        }
        $facts[$key].Add(@($parts | Select-Object -Skip 1))
    }
    return $facts
}

# One field of the first row for a key, or $Default when the guest did not say.
function Get-Fact {
    param($Facts, [string]$Key, [int]$Field = 0, [string]$Default = '')

    if ($null -eq $Facts -or -not $Facts.ContainsKey($Key)) { return $Default }
    $row = @($Facts[$Key][0])
    if ($Field -ge $row.Count) { return $Default }
    $value = [string]$row[$Field]
    if ($value) { return $value }
    return $Default
}

function Get-FactRows {
    param($Facts, [string]$Key)

    if ($null -eq $Facts -or -not $Facts.ContainsKey($Key)) { return @() }
    return $Facts[$Key].ToArray()
}

# ------------------------------------------------------------------ printing

function Format-Size {
    param([double]$Bytes)

    if ($Bytes -le 0) { return '-' }
    if ($Bytes -ge 1TB) { return ('{0:N1} TB' -f ($Bytes / 1TB)) }
    if ($Bytes -ge 1GB) { return ('{0:N1} GB' -f ($Bytes / 1GB)) }
    if ($Bytes -ge 1MB) { return ('{0:N0} MB' -f ($Bytes / 1MB)) }
    return ('{0:N0} KB' -f ($Bytes / 1KB))
}

function Write-Field {
    param([string]$Label, [string]$Value, [string]$Colour = 'Gray')
    Write-Host ('    {0,-11} {1}' -f $Label, $Value) -ForegroundColor $Colour
}

<#
    What the OS is. The guest's own /etc/os-release is the honest answer once the
    machine is up, because it reflects what actually installed and any upgrade
    since. Config only says what was asked for, which is the right answer while
    the VM is still being built or does not exist yet.
#>
function Get-OsDescription {
    param($Config, $Facts)

    $fromGuest = Get-Fact -Facts $Facts -Key 'os'
    if ($fromGuest -and $fromGuest -ne 'unknown') { return $fromGuest }
    if ($null -eq $Config) { return 'unknown' }

    $os = $Config.Blueprint.Data['Os']
    $release = [string]$Config.Iso['Release']
    if ($os -and $os.Contains('Vendor')) { return ("{0} {1}  (from config)" -f $os['Vendor'], $release) }
    return "$release  (from config)"
}

<#
    The disks, joined across three places that each know part of the answer:
    Hyper-V for what is attached and how big the files really are, config for the
    drive letter and mount point behind each one, and the guest for how full it
    actually is.
#>
function Show-Disks {
    param([string]$VmName, $Config, $Facts)

    $attached = @(Get-VMHardDiskDrive -VMName $VmName -ErrorAction SilentlyContinue)
    if ($attached.Count -eq 0) {
        Write-Field 'disks' 'none attached'
        return
    }

    # Path to the config that describes it, so each VHD can be named the way the
    # team names it rather than by filename.
    $byPath = @{}
    if ($null -ne $Config) {
        $paths = Get-DevVMPaths -Config $Config
        $byPath[$paths.OsVhd.ToLowerInvariant()] = [pscustomobject]@{
            Letter = 'C:'; Label = 'system'; Mount = '/'; SizeGB = [int]$Config.VM['DiskSizeGB']
        }
        $byPath[$paths.SeedVhd.ToLowerInvariant()] = [pscustomobject]@{
            Letter = '-'; Label = 'seed'; Mount = '(install media)'; SizeGB = 0
        }
        foreach ($d in $paths.DataDisks) {
            $letter = if ($d.DriveLetter) { "$($d.DriveLetter):" } else { '-' }
            $byPath[$d.Path.ToLowerInvariant()] = [pscustomobject]@{
                Letter = $letter; Label = $d.Label; Mount = $d.MountPoint; SizeGB = [int]$d.SizeGB
            }
        }
    }

    # Mount point to guest usage.
    $usage = @{}
    foreach ($row in (Get-FactRows -Facts $Facts -Key 'df')) {
        if (@($row).Count -lt 4) { continue }
        $usage[[string]$row[0]] = [pscustomobject]@{
            Total = [double]$row[1]; Used = [double]$row[2]; Percent = [string]$row[3]
        }
    }

    Write-Host ('    {0,-11} {1,-7} {2,-14} {3,10} {4,10}  {5}' -f `
        'disks', 'DRIVE', 'LABEL', 'ALLOCATED', 'ON HOST', 'MOUNT') -ForegroundColor DarkGray

    foreach ($drive in ($attached | Sort-Object ControllerLocation)) {
        $key = ([string]$drive.Path).ToLowerInvariant()
        $meta = if ($byPath.ContainsKey($key)) { $byPath[$key] } else { $null }

        $letter = if ($meta) { $meta.Letter } else { '?' }
        $label = if ($meta) { $meta.Label } else { [System.IO.Path]::GetFileNameWithoutExtension($drive.Path) }
        $mount = if ($meta) { $meta.Mount } else { 'not in config' }
        # A disk attached by hand is worth showing, but not at the cost of
        # knocking every column out of line.
        if ($label.Length -gt 14) { $label = $label.Substring(0, 13) + '.' }

        # Size is the disk as the guest sees it; FileSize is what it costs on the
        # host today. A dynamic VHDX starts tiny and grows, so both matter.
        $allocated = '-'
        $onHost = '-'
        $vhd = Get-VHD -Path $drive.Path -ErrorAction SilentlyContinue
        if ($vhd) {
            $allocated = Format-Size -Bytes $vhd.Size
            $onHost = Format-Size -Bytes $vhd.FileSize
        }
        elseif ($meta -and $meta.SizeGB -gt 0) {
            $allocated = "$($meta.SizeGB) GB"
        }

        $line = '    {0,-11} {1,-7} {2,-14} {3,10} {4,10}  {5}' -f `
            '', $letter, $label, $allocated, $onHost, $mount
        Write-Host $line -ForegroundColor Gray

        if ($mount -and $usage.ContainsKey($mount)) {
            $u = $usage[$mount]
            Write-Host ('    {0,-11} {1,-7} {2}' -f '', '', `
                ("in use  {0} of {1}  ({2})" -f (Format-Size $u.Used), (Format-Size $u.Total), $u.Percent)) `
                -ForegroundColor DarkGray
        }
    }
}

function Show-NotCreated {
    param($Target)

    Write-Host ''
    Write-Host ("  {0,-30} {1}" -f $Target.Name, 'not created') -ForegroundColor DarkGray
    if ($null -eq $Target.Config) { return }

    $cfg = $Target.Config
    Write-Field 'would be' (Get-OsDescription -Config $cfg -Facts $null)
    Write-Field 'blueprint' $cfg.Blueprint.Name
    Write-Field 'hardware' ("{0} vCPU   {1} GB RAM   {2} GB system disk" -f `
        $cfg.VM['CpuCount'], $cfg.VM['MemoryStartupGB'], $cfg.VM['DiskSizeGB'])

    $disks = @(Get-DevVMDataDiskLayout -Config $cfg)
    if ($disks.Count -gt 0) {
        $text = ($disks | ForEach-Object {
            $l = if ($_.DriveLetter) { "$($_.DriveLetter):" } else { $_.Label }
            "$l $($_.SizeGB) GB at $($_.MountPoint)"
        }) -join ',  '
        Write-Field 'data disks' $text
    }
    Write-Field 'build it' (".\vm.ps1 create" + $(if ($Machine) { " -Machine $Machine" } else { '' }))
}

function Show-One {
    param($Target)

    $vm = Get-VM -Name $Target.Name -ErrorAction SilentlyContinue
    if (-not $vm) { Show-NotCreated -Target $Target; return }

    $colour = 'Yellow'
    if ($vm.State -eq 'Running') { $colour = 'Green' }
    elseif ($vm.State -eq 'Off') { $colour = 'DarkGray' }

    $uptime = ''
    if ($vm.State -eq 'Running') { $uptime = "  up {0:hh\:mm\:ss}" -f $vm.Uptime }

    Write-Host ''
    Write-Host ("  {0,-30} {1}{2}" -f $Target.Name, $vm.State, $uptime) -ForegroundColor $colour

    # Only talk to a machine that is running, and only when we know who to log in
    # as. Everything below tolerates $facts being $null.
    $facts = $null
    $sshTarget = $null
    if ($vm.State -eq 'Running' -and $Target.User) {
        $sshTarget = "{0}@{1}.local" -f $Target.User, $Target.Hostname
        $facts = Get-GuestFacts -SshTarget $sshTarget
    }

    Write-Field 'os' (Get-OsDescription -Config $Target.Config -Facts $facts)
    $kernel = Get-Fact -Facts $facts -Key 'kernel'
    if ($kernel) { Write-Field 'kernel' $kernel }

    if ($null -ne $Target.Config) {
        $chain = @($Target.Config.Blueprint.Chain)
        $bp = $Target.Config.Blueprint.Name
        if ($chain.Count -gt 1) {
            $bp += "  (inherits " + (($chain | Select-Object -Skip 1) -join ', ') + ")"
        }
        Write-Field 'blueprint' $bp
    }

    # The address someone actually browses to, and whether this laptop can
    # currently resolve it. An unresolvable name is the single most common reason
    # a working VM looks broken from the host.
    $resolved = Test-DevVMHostnameResolvable -Hostname $Target.Hostname
    $ip = Get-DevVMIPAddress -Name $Target.Name
    if ($resolved) {
        Write-Field 'address' ("http://{0}.local   (resolves to {1})" -f $Target.Hostname, $resolved) 'White'
    }
    elseif ($ip) {
        Write-Field 'address' ("http://{0}   ({1}.local does not resolve from here yet)" -f $ip, $Target.Hostname) 'Yellow'
    }
    elseif ($vm.State -ne 'Running') {
        Write-Field 'address' ("http://{0}.local   (once it is running)" -f $Target.Hostname) 'DarkGray'
    }
    else {
        Write-Field 'address' ("{0}.local   (no address yet, mDNS may still be starting)" -f $Target.Hostname) 'DarkGray'
    }
    if ($sshTarget) { Write-Field 'ssh' "ssh $sshTarget" }

    # Who you log in as. The guest is authoritative once it is up, because it
    # knows about accounts bootstrap created and any added since.
    $userRows = @(Get-FactRows -Facts $facts -Key 'user')
    if ($userRows.Count -gt 0) {
        foreach ($row in $userRows) {
            $u = @($row)
            $realName = if ($u.Count -gt 1 -and $u[1]) { " ($($u[1]))" } else { '' }
            $groups = if ($u.Count -gt 2 -and $u[2]) { "   groups: $($u[2])" } else { '' }
            Write-Field 'user' ("{0}{1}{2}" -f $u[0], $realName, $groups)
        }
    }
    elseif ($null -ne $Target.Config) {
        $pu = $Target.Config.Guest['PrimaryUser']
        $realName = if ($pu.Contains('RealName') -and $pu['RealName']) { " ($($pu['RealName']))" } else { '' }
        Write-Field 'user' ("{0}{1}  (from config)" -f $pu['Username'], $realName)
    }

    # Hyper-V, not config: this is what the machine was actually given.
    $memory = if ($vm.State -eq 'Running' -and $vm.MemoryAssigned -gt 0) {
        "$(Format-Size $vm.MemoryAssigned) RAM assigned"
    }
    else {
        "$(Format-Size $vm.MemoryStartup) RAM at startup"
    }
    if ($vm.DynamicMemoryEnabled) {
        $memory += ("   dynamic {0} to {1}" -f (Format-Size $vm.MemoryMinimum), (Format-Size $vm.MemoryMaximum))
    }
    Write-Field 'hardware' ("{0} vCPU   {1}" -f $vm.ProcessorCount, $memory)

    $guestUp = Get-Fact -Facts $facts -Key 'guestuptime'
    if ($guestUp) { Write-Field 'guest up' $guestUp }

    Show-Disks -VmName $Target.Name -Config $Target.Config -Facts $facts

    # What 35-database.sh ended up with. Reported from the guest when it is up,
    # and from config otherwise, so a machine that is off still says what database
    # it is meant to have.
    $dbEngine = Get-Fact -Facts $facts -Key 'database' -Field 0
    if ($dbEngine) {
        $dbStatus  = Get-Fact -Facts $facts -Key 'database' -Field 1
        $dbDetail  = Get-Fact -Facts $facts -Key 'database' -Field 2
        $dbVersion = Get-Fact -Facts $facts -Key 'database' -Field 3
        $dbPort    = Get-Fact -Facts $facts -Key 'database' -Field 4
        $dbListen  = Get-Fact -Facts $facts -Key 'database' -Field 5
        $where = if ($dbListen -eq '*') { 'reachable from other machines' } else { 'localhost only' }
        switch ($dbStatus) {
            'running'     { Write-Field 'database' "$dbEngine $dbVersion running on port $dbPort, $where" 'Green' }
            'client-only' { Write-Field 'database' "$dbEngine $dbVersion client tools only, no server" 'Gray' }
            'failed'      { Write-Field 'database' "$dbEngine $dbVersion FAILED - $dbDetail" 'Red' }
            default       { Write-Field 'database' "$dbEngine $dbVersion $dbStatus $dbDetail" 'Yellow' }
        }
    }
    elseif ($null -ne $Target.Config) {
        $dbPlan = Get-DevVMDatabasePlan -Config $Target.Config
        if ($dbPlan) {
            $what = if ($dbPlan.InstallServer) { "server on port $($dbPlan.Port)" } else { 'client tools only' }
            Write-Field 'database' ("{0} {1}, {2}  (from config)" -f `
                $dbPlan.DisplayName, $dbPlan.Version, $what)
        }
    }

    # The Adminer route. The reason it is worth its own line is that it is the one
    # thing here you paste into a browser, so it should never need looking up.
    $admStatus = Get-Fact -Facts $facts -Key 'adminer' -Field 0
    if ($admStatus) {
        $admDetail  = Get-Fact -Facts $facts -Key 'adminer' -Field 1
        $admVersion = Get-Fact -Facts $facts -Key 'adminer' -Field 2
        $admPort    = Get-Fact -Facts $facts -Key 'adminer' -Field 3
        switch ($admStatus) {
            'running' { Write-Field 'adminer' "$admDetail   (adminer $admVersion)" 'White' }
            'failed'  { Write-Field 'adminer' "FAILED - $admDetail" 'Red' }
            default   { Write-Field 'adminer' "$admStatus $admDetail" 'Yellow' }
        }
    }
    elseif ($null -ne $Target.Config) {
        $dbPlan2 = Get-DevVMDatabasePlan -Config $Target.Config
        if ($dbPlan2 -and $dbPlan2.AdminerInstall) {
            Write-Field 'adminer' ("http://{0}.local:{1}/  (from config)" -f `
                $Target.Hostname, $dbPlan2.AdminerPort)
        }
    }

    $hookLines = Get-Fact -Facts $facts -Key 'hooks' -Field 0
    if ($hookLines) {
        $hookWhen = Get-Fact -Facts $facts -Key 'hooks' -Field 1
        $when = if ($hookWhen -eq 'enabled') { 'runs on every boot' } else { 'ran once' }
        Write-Field 'hooks' "$when, $hookLines line(s) in /var/log/vm-post-start.log"
    }
    elseif ($null -ne $Target.Config) {
        $hookPlan = Get-DevVMHookPlan -Config $Target.Config
        if ($hookPlan) {
            $when = if ($hookPlan.RunAtEveryBoot) { 'on every boot' } else { 'once' }
            Write-Field 'hooks' ("{0} command(s), as {1}, {2}  (from config)" -f `
                $hookPlan.Commands.Count, $hookPlan.RunAs, $when)
        }
    }

    # Whatever 55-github.sh concluded when it ran `ssh -T git@github.com`, worded
    # the same way the guest's own login banner words it.
    $ghStatus = Get-Fact -Facts $facts -Key 'github' -Field 0
    $ghDetail = Get-Fact -Facts $facts -Key 'github' -Field 1
    if ($ghStatus) {
        switch ($ghStatus) {
            'ok'       { Write-Field 'github' "authenticated as $ghDetail" 'Green' }
            'mismatch' { Write-Field 'github' "authenticated as $ghDetail - NOT the account you configured" 'Yellow' }
            'denied'   { Write-Field 'github' "NOT authenticated - $ghDetail" 'Red' }
            'skipped'  { Write-Field 'github' 'no ssh key installed' 'DarkGray' }
            default    { Write-Field 'github' "unverified - $ghDetail" 'Yellow' }
        }
    }

    if ($vm.State -ne 'Running') { return }

    if ($null -eq $facts) {
        if ($NoSsh) { Write-Field 'bootstrap' '(not checked, -NoSsh)' 'DarkGray' }
        else { Write-Field 'bootstrap' '(no ssh access yet - add your key to Guest.PrimaryUser.AuthorizedKeys)' 'DarkGray' }
        return
    }

    $state = Get-Fact -Facts $facts -Key 'bootstrap' -Field 0
    $detail = Get-Fact -Facts $facts -Key 'bootstrap' -Field 1
    if ($state -eq 'complete') {
        Write-Field 'bootstrap' 'complete' 'Green'
    }
    elseif ($state -eq 'failed') {
        $which = if ($detail) { $detail } else { 'see the report' }
        Write-Field 'bootstrap' "finished with failed step(s): $which" 'Red'
        Write-Field '' "re-run this with -Report, or: ssh $sshTarget 'cat /var/log/vm-bootstrap-report.txt'" 'DarkGray'
    }
    else {
        Write-Field 'bootstrap' "in progress ($detail step(s) done)" 'Yellow'
    }

    if ($Report) {
        $text = Invoke-GuestCommand -SshTarget $sshTarget `
            -Command 'cat /var/log/vm-bootstrap-report.txt 2>/dev/null || echo "no report yet - bootstrap has not finished a run"'
        Write-Host ''
        foreach ($l in @($text)) { Write-Host "      $l" -ForegroundColor Gray }
    }
}

# ---------------------------------------------------------------------- main

try {
    Assert-Prerequisites -RequireHyperV
    $targets = Resolve-Targets

    do {
        if ($Follow) { Clear-Host }
        Write-Host ''
        Write-Host "  Dev VMs at $(Get-Date -Format 'HH:mm:ss')" -ForegroundColor Cyan
        foreach ($t in $targets) { Show-One -Target $t }
        Write-Host ''
        if ($Follow) {
            Write-Host '  Ctrl+C to stop following.' -ForegroundColor DarkGray
            Start-Sleep -Seconds 10
        }
    } while ($Follow)
}
catch {
    Write-Host "STATUS FAILED: $($_.Exception.Message)" -ForegroundColor Red
    if ($_.ScriptStackTrace) { Write-Verbose $_.ScriptStackTrace }
    exit 1
}

# Explicit, and not just tidiness. Probing a guest that is not reachable leaves
# $LASTEXITCODE at ssh's 255, and a script that ends without an exit statement
# hands that back as its own code - so a perfectly good status report came back
# as a failure, and vm.ps1 dutifully propagated 255. Reporting is the job here;
# an unreachable guest is information, not an error.
exit 0
