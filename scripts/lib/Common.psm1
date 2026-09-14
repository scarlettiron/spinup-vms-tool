#requires -Version 5.1
Set-StrictMode -Version Latest

<#
    Config loading, layered merging, and small shared helpers.

    Precedence, lowest first:
        1. blueprint.Defaults      what this OS suggests (sizing, nice-to-haves)
        2. vm.config.*           the team standard
        3. blueprint.Overrides     what this OS *is* (a server blueprint has no desktop)
        4. vm.config.local.*     per-developer overrides (git-ignored)
        5. Machines.<name>       the per-machine block, with -Machine
        6. -Parameter            one-off command line overrides

    Defaults and Overrides sitting on opposite sides of the team standard is the
    point: the team gets to pick how big a VM is, but choosing the ubuntu-server
    blueprint has to actually make it headless rather than being quietly undone by
    an InstallDesktop = $true two layers down. A human naming a key in their own
    local config or on the command line still wins over both.
#>


function Write-Step {
    [CmdletBinding()] param([Parameter(Mandatory)][string]$Message)
    Write-Host ''
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Write-Detail {
    [CmdletBinding()] param([Parameter(Mandatory)][AllowEmptyString()][string]$Message)
    Write-Host "    $Message" -ForegroundColor Gray
}

function Write-Warn {
    [CmdletBinding()] param([Parameter(Mandatory)][string]$Message)
    Write-Host "    ! $Message" -ForegroundColor Yellow
}

<#
    ---------------------------------------------------------------- step tracking

    Announces each step as it starts, times it, and prints a report of what passed
    and what did not when the build ends.

    Host steps are not like the guest bootstrap steps: they are strictly
    dependent, so a failure aborts rather than carrying on. There is no point
    creating a VM whose seed disk failed to build. That means the report has three
    outcomes rather than two - ok, FAILED, and 'not reached' for the steps after
    the failure - and it has to be printed from the catch block, since a build that
    threw is exactly the build someone needs the report for.

    Starting a step implicitly finishes the previous one, so the caller only has to
    name its steps and does not have to remember to close each one.
#>
$script:BuildSteps = [System.Collections.Generic.List[object]]::new()
$script:BuildStepTotal = 0
$script:BuildStepIndex = 0
$script:BuildStepOpen = $null
$script:BuildStepWatch = $null

function Reset-BuildSteps {
    [CmdletBinding()] param([int]$Total = 0)
    $script:BuildSteps = [System.Collections.Generic.List[object]]::new()
    $script:BuildStepTotal = $Total
    $script:BuildStepIndex = 0
    $script:BuildStepOpen = $null
    $script:BuildStepWatch = [System.Diagnostics.Stopwatch]::StartNew()
}

function Complete-BuildStep {
    [CmdletBinding()] param([string]$Status = 'ok')
    if (-not $script:BuildStepOpen) { return }
    $secs = 0
    if ($script:BuildStepOpen.Watch) {
        $script:BuildStepOpen.Watch.Stop()
        $secs = [int][math]::Round($script:BuildStepOpen.Watch.Elapsed.TotalSeconds)
    }
    $script:BuildSteps.Add([pscustomobject]@{
        Name    = $script:BuildStepOpen.Name
        Status  = $Status
        Seconds = $secs
    })
    if ($Status -eq 'ok') {
        Write-Host "    ok ($($secs)s)" -ForegroundColor DarkGreen
    }
    else {
        Write-Host "    $Status ($($secs)s)" -ForegroundColor Red
    }
    $script:BuildStepOpen = $null
}

function Start-BuildStep {
    [CmdletBinding()] param([Parameter(Mandatory)][string]$Message)
    Complete-BuildStep
    $script:BuildStepIndex++
    $label = if ($script:BuildStepTotal -gt 0) {
        "[$($script:BuildStepIndex)/$($script:BuildStepTotal)] $Message"
    }
    else {
        $Message
    }
    Write-Host ''
    Write-Host "==> $label" -ForegroundColor Cyan
    $script:BuildStepOpen = [pscustomobject]@{
        Name  = $Message
        Watch = [System.Diagnostics.Stopwatch]::StartNew()
    }
}

<#
    The end-of-run report. Pass -FailureMessage from a catch block: the step that
    was in flight is recorded as FAILED and every step that never ran is listed as
    'not reached', so the first thing on screen after a failed build is where it
    stopped rather than a bare exception.
#>
function Write-BuildReport {
    [CmdletBinding()] param(
        [string]$Title = 'build report',
        [string]$FailureMessage
    )

    if ($FailureMessage) { Complete-BuildStep -Status 'FAILED' } else { Complete-BuildStep }

    # A config error throws before the first step starts, so there is nothing to
    # tabulate. Print the failure on its own rather than an empty table. The title
    # carries the caller's own wording, so a stop or start script does not announce
    # a failed build.
    if ($script:BuildSteps.Count -eq 0) {
        if ($FailureMessage) {
            Write-Host ''
            Write-Host "FAILED before the first step: $FailureMessage" -ForegroundColor Red
            Write-Host "  ($Title)" -ForegroundColor DarkGray
        }
        return
    }

    $total = 0
    if ($script:BuildStepWatch) {
        $script:BuildStepWatch.Stop()
        $total = [int][math]::Round($script:BuildStepWatch.Elapsed.TotalSeconds)
    }

    $failedNames = @()
    foreach ($s in $script:BuildSteps) { if ($s.Status -ne 'ok') { $failedNames += $s.Name } }

    Write-Host ''
    Write-Host '==============================================================' -ForegroundColor DarkGray
    Write-Host " $Title" -ForegroundColor White
    Write-Host '==============================================================' -ForegroundColor DarkGray
    Write-Host ''
    Write-Host ('  {0,-3} {1,-40} {2,-10} {3}' -f '#', 'STEP', 'STATUS', 'TIME') -ForegroundColor DarkGray
    Write-Host ('  {0,-3} {1,-40} {2,-10} {3}' -f '---', ('-' * 40), '----------', '-----') -ForegroundColor DarkGray

    $n = 0
    foreach ($s in $script:BuildSteps) {
        $n++
        $colour = if ($s.Status -eq 'ok') { 'Gray' } else { 'Red' }
        Write-Host ('  {0,-3} {1,-40} {2,-10} {3}s' -f $n, $s.Name, $s.Status, $s.Seconds) -ForegroundColor $colour
    }

    # Steps that were planned but never started, because an earlier one aborted.
    # One line for the lot: which ones they are matters less than the fact that
    # the build stopped before reaching them.
    $notReached = 0
    if ($FailureMessage -and $script:BuildStepTotal -gt $script:BuildStepIndex) {
        $notReached = $script:BuildStepTotal - $script:BuildStepIndex
        $range = if ($notReached -eq 1) { [string]$script:BuildStepTotal }
                 else { "$($script:BuildStepIndex + 1)-$($script:BuildStepTotal)" }
        Write-Host ('  {0,-3} {1,-40} {2,-10}' -f $range, "$notReached remaining step(s)", 'not reached') `
            -ForegroundColor DarkGray
    }

    Write-Host ''
    $okCount = $script:BuildSteps.Count - $failedNames.Count
    $counts = "$okCount ok, $($failedNames.Count) failed"
    if ($notReached -gt 0) { $counts += ", $notReached not reached" }
    Write-Host ("  $($script:BuildSteps.Count) step(s) attempted: $counts   total $($total)s") -ForegroundColor White

    if ($FailureMessage) {
        Write-Host ''
        Write-Host "  FAILED at: $($failedNames -join ', ')" -ForegroundColor Red
        Write-Host "  $FailureMessage" -ForegroundColor Red
    }
    Write-Host '==============================================================' -ForegroundColor DarkGray
}

function Get-RepoRoot {
    [CmdletBinding()] param()
    Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
}

<#
    Which machines a command should act on, resolved the same way for every script
    that takes -Machine, -Name and -All.

    Each target carries its own config, so a caller can reach the blueprint, the
    hostname and the primary user without resolving anything a second time and
    risking a different answer.

    -Name takes a Hyper-V name rather than a config key, so it is matched back to a
    configured machine when one resolves to it. That is the difference between a
    full answer and a bare one for the common case of naming a VM directly. A name
    config knows nothing about still comes back, with a null Config, because acting
    on a VM that config does not describe is legitimate.

    -All with no Machines block falls back to the single configured VM rather than
    failing. A caller that wants "you asked for a fleet and there is no fleet" to be
    an error should check for itself: for a destructive command that is the right
    call, and for a status read it is not.
#>
function Get-DevVMTargets {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$RepoRoot,
        [string]$Machine,
        [string]$Name,
        [switch]$All
    )

    $machineNames = @()
    if ($All) {
        $machineNames = @(Get-MachineNames -RepoRoot $RepoRoot)
    }
    elseif ($Machine) {
        $machineNames = @($Machine)
    }
    elseif ($Name) {
        # A name has to be searched for across everything config can produce, or a
        # VM that is described by a Machines block comes back without its config
        # and the caller loses the hostname and the user. $null first, so the
        # no-machine default is a candidate too.
        $machineNames = @($null) + @(Get-MachineNames -RepoRoot $RepoRoot)
    }
    # Neither, or -All with nothing to enumerate: the single default VM.
    if ($machineNames.Count -eq 0) { $machineNames = @($null) }

    $out = New-Object System.Collections.Generic.List[object]
    foreach ($m in $machineNames) {
        $cfg = Get-DevVMConfig -RepoRoot $RepoRoot -Machine $m
        $out.Add([pscustomobject]@{
            Machine  = $m
            Name     = [string]$cfg.VM['Name']
            Hostname = [string]$cfg.Guest['Hostname']
            User     = [string]$cfg.Guest['PrimaryUser']['Username']
            Config   = $cfg
        })
    }

    if (-not $Name) { return $out.ToArray() }

    $match = @($out.ToArray() | Where-Object { $_.Name -eq $Name })
    if ($match.Count -gt 0) { return @($match[0]) }

    return @([pscustomobject]@{
        Machine = $null; Name = $Name; Hostname = $Name; User = $null; Config = $null
    })
}

<#
    Windows PowerShell 5.1 throws 'Argument types do not match' when @() wraps an
    EMPTY System.Collections.Generic.List[object]. An empty List[string] is fine,
    and so is any non-empty list, which is exactly why the bug hides until the
    one run where a collection happens to come back empty.

    So: never write @($someList). Call .ToArray() on it, or pipe it. This helper
    is for the cases where the value might be a list, an array, or $null.
#>
function ConvertTo-Array {
    param($Value)

    if ($null -eq $Value) { return @() }
    if ($Value -is [string]) { return , ([object[]]@($Value)) }
    if ($Value -is [System.Array]) { return , ([object[]]$Value) }
    if ($Value -is [System.Collections.IEnumerable]) {
        $out = New-Object System.Collections.ArrayList
        foreach ($item in $Value) { [void]$out.Add($item) }
        return , ([object[]]$out.ToArray())
    }
    return , ([object[]]@($Value))
}

function Copy-Deep {
    param($Value)
    if ($Value -is [System.Collections.IDictionary]) {
        $copy = @{}
        foreach ($k in @($Value.Keys)) { $copy[$k] = Copy-Deep $Value[$k] }
        return $copy
    }
    if ($Value -is [System.Array]) {
        $items = New-Object System.Collections.Generic.List[object]
        foreach ($item in $Value) { $items.Add((Copy-Deep $item)) }
        # The leading comma matters: PowerShell unrolls a returned array, which
        # would silently turn an empty @() into $null and an @('one') into a
        # bare string. Both bite hard in config merging.
        return , ([object[]]$items.ToArray())
    }
    return $Value
}

<#
    Deep-merge $Override onto $Base. Nested dictionaries merge key by key; arrays
    and scalars replace wholesale, so an override can shorten a package list
    rather than only ever growing it.
#>
function Merge-Config {
    [CmdletBinding()]
    param($Base, $Override)

    if ($null -eq $Base) { $Base = @{} }
    if ($null -eq $Override) { return (Copy-Deep $Base) }

    $result = Copy-Deep $Base
    foreach ($key in @($Override.Keys)) {
        $new = $Override[$key]
        if ($new -is [System.Collections.IDictionary] -and
            $result.Contains($key) -and
            $result[$key] -is [System.Collections.IDictionary]) {
            $result[$key] = Merge-Config -Base $result[$key] -Override $new
        }
        else {
            $result[$key] = Copy-Deep $new
        }
    }
    return $result
}

function Import-Psd {
    [CmdletBinding()] param([Parameter(Mandatory)][string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { throw "Config file not found: $Path" }
    $data = Import-PowerShellDataFile -LiteralPath $Path
    if ($null -eq $data) { throw "Config file is empty or invalid: $Path" }
    return $data
}

# ------------------------------------------------------------ config layers

# The sections a config file may contain. Anything else at the top level is
# either a quick setting (below) or a typo - and a silently ignored typo in a
# config file is exactly the kind of misconfiguration this repo exists to stop.
$script:ConfigSections = @(
    'Blueprint', 'ProjectName', 'VM', 'Guest', 'Bootstrap', 'Database', 'Files', 'Hooks',
    'Git', 'GitHub', 'Repos', 'Iso', 'Machines', 'RedHat'
)

<#
    Quick settings: flat names that map onto a nested setting, so the handful of
    things people actually change can live in one block at the top of the file
    instead of being scattered four levels deep.

        cpus: 8        is exactly       VM: { CpuCount: 8 }

    Resolved per config layer, before the layers are merged, so precedence still
    works the way it reads: a quick setting in your local file beats a long-hand
    setting in the shared file, and vice versa.
#>
$script:ConfigAliases = @{
    VmName            = 'VM.Name'
    VmPath            = 'VM.Path'
    Cpus              = 'VM.CpuCount'
    MemoryGB          = 'VM.MemoryStartupGB'
    MaxMemoryGB       = 'VM.MemoryMaximumGB'
    DiskGB            = 'VM.DiskSizeGB'
    Switch            = 'VM.SwitchName'
    EnhancedSession   = 'VM.EnhancedSession'

    Hostname          = 'Guest.Hostname'
    Username          = 'Guest.PrimaryUser.Username'
    RealName          = 'Guest.PrimaryUser.RealName'
    Password          = 'Guest.PrimaryUser.Password'
    PasswordHash      = 'Guest.PrimaryUser.PasswordHash'
    Timezone          = 'Guest.Timezone'
    Locale            = 'Guest.Locale'
    KeyboardLayout    = 'Guest.KeyboardLayout'

    GitName           = 'Git.UserName'
    GitEmail          = 'Git.UserEmail'
    GithubUsername    = 'GitHub.Username'
    GithubToken       = 'GitHub.Cli.Token'
    GithubTokenFile   = 'GitHub.Cli.TokenFile'
    SshKeyMode        = 'GitHub.SshKey.Mode'

    ReposRoot         = 'Repos.Root'

    OsVersion         = 'Iso.Release'
    Release           = 'Iso.Release'   # synonym
    UbuntuRelease     = 'Iso.Release'   # synonym, kept so existing configs work
    IsoPath           = 'Iso.Path'

    # Red Hat subscription. Only the RHEL blueprint uses these: its rebuilds
    # (Rocky, Alma) need no subscription at all. Put the key in your LOCAL file.
    RhsmActivationKey = 'RedHat.ActivationKey'
    RhsmOrgId         = 'RedHat.OrganizationId'
    RhsmUsername      = 'RedHat.Username'
    RhsmPassword      = 'RedHat.Password'

    InstallDesktop    = 'Bootstrap.InstallDesktop'
    InstallDocker     = 'Bootstrap.InstallDocker'
    InstallDdev       = 'Bootstrap.InstallDdev'
    InstallNode       = 'Bootstrap.InstallNode'
    InstallPython     = 'Bootstrap.InstallPython'
    InstallVSCode     = 'Bootstrap.InstallVSCode'
    InstallChrome     = 'Bootstrap.InstallChrome'
    InstallPostman    = 'Bootstrap.InstallPostman'
    VSCodeExtensions  = 'Bootstrap.VSCodeExtensions'
    ExtraVSCodeExtensions = 'Bootstrap.ExtraVSCodeExtensions'
    InstallRemoteAccess = 'Bootstrap.InstallRemoteAccess'
    NodeVersion       = 'Bootstrap.NodeVersion'
    PythonVersion     = 'Bootstrap.PythonVersion'
    Resolution        = 'Bootstrap.DisplayResolution'
    ExtraPackages     = 'Bootstrap.ExtraAptPackages'
    DotfilesRepo      = 'Bootstrap.DotfilesRepo'
    EnableMdns        = 'Bootstrap.EnableMdns'
    RebootWhenDone    = 'Bootstrap.RebootWhenDone'

    DbEngine          = 'Database.Engine'
    DbVersion         = 'Database.Version'
    DbClient          = 'Database.InstallClient'
    DbServer          = 'Database.InstallServer'
    DbPort            = 'Database.Port'
    DbListenAddress   = 'Database.ListenAddress'
    DbAdminPassword   = 'Database.AdminPassword'
    DbDatabases       = 'Database.Databases'

    DbAdminer         = 'Database.Adminer.Install'
    DbAdminerPort     = 'Database.Adminer.Port'

    CopyFrom          = 'Files.Source'
    CopyTo            = 'Files.Destination'

    PostStart         = 'Hooks.PostStart'
    PostStartFile     = 'Hooks.PostStartFile'
    PostStartRunAs    = 'Hooks.RunAs'
    PostStartEveryBoot = 'Hooks.RunAtEveryBoot'
}

function Get-ConfigAliasTable {
    [CmdletBinding()] param()
    return $script:ConfigAliases
}

function Set-ConfigPath {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]$Root,
        [Parameter(Mandatory)][string]$Path,
        $Value
    )

    $parts = $Path -split '\.'
    $node = $Root
    for ($i = 0; $i -lt $parts.Count - 1; $i++) {
        $part = $parts[$i]
        if (-not $node.Contains($part) -or -not ($node[$part] -is [System.Collections.IDictionary])) {
            $node[$part] = @{}
        }
        $node = $node[$part]
    }
    $node[$parts[$parts.Count - 1]] = $Value
}

<#
    Turn one config layer's quick settings into the nested shape, and reject
    top-level keys that are neither a section nor a known quick setting.
#>
function Expand-ConfigAliases {
    [CmdletBinding()]
    param($Layer, [string]$Source = 'config')

    if ($null -eq $Layer) { return @{} }

    $sections = @{}
    $quick = @{}

    foreach ($key in @($Layer.Keys)) {
        $name = [string]$key
        $value = $Layer[$key]

        if ($script:ConfigSections -contains $name) {
            # 'Repos' is both a section and, when given a list, a shorthand for
            # Repos.Items - which is what someone naturally writes.
            if ($name -eq 'Repos' -and $value -is [System.Array]) {
                $sections['Repos'] = @{ Items = $value }
            }
            else { $sections[$name] = $value }
            continue
        }

        if ($script:ConfigAliases.Contains($name)) {
            $quick[$name] = $value
            continue
        }

        $valid = (@($script:ConfigSections) + @($script:ConfigAliases.Keys) | Sort-Object) -join ', '
        throw "Unknown setting '$name' in $Source. Valid top-level settings are: $valid"
    }

    $result = Copy-Deep $sections

    foreach ($name in @($quick.Keys)) {
        $target = [string]$script:ConfigAliases[$name]
        Set-ConfigPath -Root $result -Path $target -Value (Copy-Deep $quick[$name])
    }

    # Machine blocks accept the same shorthand.
    if ($result.Contains('Machines') -and $result['Machines'] -is [System.Collections.IDictionary]) {
        $machines = @{}
        foreach ($machineName in @($result['Machines'].Keys)) {
            $machines[$machineName] = Expand-ConfigAliases -Layer $result['Machines'][$machineName] `
                -Source "$Source (machine '$machineName')"
        }
        $result['Machines'] = $machines
    }

    return $result
}

<#
    Find the file backing one config layer. Both formats are accepted, but only
    one file per layer - two files claiming the same layer is ambiguous, and
    guessing which wins is how people lose an afternoon.
#>
function Resolve-ConfigLayerPath {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$RepoRoot,
        [Parameter(Mandatory)][string]$BaseName
    )

    $candidates = @('psd1', 'yaml', 'yml') | ForEach-Object { Join-Path $RepoRoot "$BaseName.$_" }
    $found = @($candidates | Where-Object { Test-Path -LiteralPath $_ })

    if ($found.Count -gt 1) {
        $names = (@($found | ForEach-Object { Split-Path -Leaf $_ })) -join ' and '
        throw "Found $names. Keep only one file per config layer - delete or rename the other."
    }
    if ($found.Count -eq 0) { return $null }
    return $found[0]
}

<#
    Load one config layer from either format, expand its quick settings, and
    validate its top-level keys.
#>
function Import-ConfigLayer {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Path)

    $leaf = Split-Path -Leaf $Path
    $extension = [System.IO.Path]::GetExtension($Path).ToLowerInvariant()

    if ($extension -eq '.psd1') { $raw = Import-Psd $Path }
    elseif ($extension -eq '.yaml' -or $extension -eq '.yml') { $raw = Import-Yaml -Path $Path }
    else { throw "Unsupported config format '$extension' ($leaf). Use .psd1, .yaml, or .yml." }

    if ($null -eq $raw) { $raw = @{} }
    if (-not ($raw -is [System.Collections.IDictionary])) {
        throw "$leaf must contain a mapping of settings at the top level."
    }

    return (Expand-ConfigAliases -Layer $raw -Source $leaf)
}

<#
    Resolve a blueprint plus its inheritance chain into one object, with template
    and bootstrap paths pointing at the nearest ancestor that actually has them.
#>
function Resolve-Blueprint {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$RepoRoot,
        [Parameter(Mandatory)][string]$Name
    )

    $chain = New-Object System.Collections.Generic.List[object]   # child first
    $seen = @{}
    $current = $Name
    while ($current) {
        if ($seen.Contains($current)) {
            throw "Circular blueprint inheritance detected at '$current'."
        }
        $seen[$current] = $true

        $dir = Join-Path (Join-Path $RepoRoot 'blueprints') $current
        $psd = Join-Path $dir 'blueprint.psd1'
        if (-not (Test-Path -LiteralPath $psd)) {
            # Only directories that actually carry a blueprint.psd1 are selectable,
            # which leaves out blueprints/bootstrap: that holds the steps every
            # blueprint shares, not a blueprint.
            $available = (Get-ChildItem (Join-Path $RepoRoot 'blueprints') -Directory -ErrorAction SilentlyContinue |
                          Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'blueprint.psd1') } |
                          Select-Object -ExpandProperty Name) -join ', '
            throw "Unknown blueprint '$current'. Available: $available"
        }

        $data = Import-Psd $psd
        $chain.Add([pscustomobject]@{ Name = $current; Dir = $dir; Data = $data })

        if ($data.Contains('Inherits') -and $data['Inherits']) { $current = [string]$data['Inherits'] }
        else { $current = $null }
    }

    # Merge ancestors first so the child wins.
    $merged = @{}
    for ($i = $chain.Count - 1; $i -ge 0; $i--) {
        $merged = Merge-Config -Base $merged -Override $chain[$i].Data
    }
    if ($merged.Contains('Inherits')) { $merged.Remove('Inherits') }

    $prov = @{}
    if ($merged.Contains('Provisioning') -and $merged['Provisioning']) { $prov = $merged['Provisioning'] }

    # Locate an asset along the chain, child first.
    $findInChain = {
        param([string]$relative)
        if (-not $relative) { return $null }
        foreach ($link in $chain) {
            $candidate = Join-Path $link.Dir $relative
            if (Test-Path -LiteralPath $candidate) { return (Resolve-Path -LiteralPath $candidate).Path }
        }
        return $null
    }

    $paths = @{}

    # SeedTemplate is the unattended-install file (cloud-init user-data, or a
    # kickstart). MetaTemplate is optional: only cloud-init needs one.
    foreach ($key in @('SeedTemplate')) {
        $rel = ''
        if ($prov.Contains($key)) { $rel = [string]$prov[$key] }
        $paths[$key] = & $findInChain $rel
        if (-not $paths[$key]) {
            throw "Blueprint '$Name' (or an ancestor) does not provide Provisioning.$key."
        }
    }
    $paths['MetaTemplate'] = $null
    if ($prov.Contains('MetaTemplate') -and $prov['MetaTemplate']) {
        $paths['MetaTemplate'] = & $findInChain ([string]$prov['MetaTemplate'])
        if (-not $paths['MetaTemplate']) {
            throw "Blueprint '$Name' names Provisioning.MetaTemplate but the file was not found."
        }
    }

    # Bootstrap trees layer: BootstrapDirs lists them lowest-priority first, so a
    # blueprint can share the steps in blueprints/bootstrap and
    # add only the ones whose package names differ. A file in a later directory
    # replaces the same relative path from an earlier one.
    $bootstrapRelatives = @()
    if ($prov.Contains('BootstrapDirs') -and $prov['BootstrapDirs']) {
        $bootstrapRelatives = @($prov['BootstrapDirs'])
    }
    elseif ($prov.Contains('BootstrapDir') -and $prov['BootstrapDir']) {
        $bootstrapRelatives = @([string]$prov['BootstrapDir'])
    }
    if ($bootstrapRelatives.Count -eq 0) {
        throw "Blueprint '$Name' (or an ancestor) does not provide Provisioning.BootstrapDirs."
    }

    $bootstrapDirs = New-Object System.Collections.Generic.List[string]
    foreach ($relative in $bootstrapRelatives) {
        $resolved = & $findInChain ([string]$relative)
        if (-not $resolved) {
            throw "Blueprint '$Name': bootstrap directory '$relative' was not found."
        }
        $bootstrapDirs.Add($resolved)
    }
    $paths['BootstrapDirs'] = $bootstrapDirs.ToArray()
    # Kept for anything still asking for a single directory: the last one wins.
    $paths['BootstrapDir'] = $bootstrapDirs[$bootstrapDirs.Count - 1]

    return [pscustomobject]@{
        Name  = $Name
        Chain = @($chain | Select-Object -ExpandProperty Name)
        Dir   = $chain[0].Dir
        Data  = $merged
        Paths = $paths
    }
}

<#
    Build the effective configuration for one machine.
#>
function Get-DevVMConfig {
    [CmdletBinding()]
    param(
        [string]$RepoRoot,
        [string]$Machine,
        $Override = @{},
        [string]$BlueprintName
    )

    if (-not $RepoRoot) { $RepoRoot = Get-RepoRoot }

    # The layers stay separate until the very end, so blueprint.Overrides can be
    # slotted in between the team standard and the developer's own config.
    # Either layer may be .psd1 or .yaml.
    $basePath = Resolve-ConfigLayerPath -RepoRoot $RepoRoot -BaseName 'vm.config'
    if (-not $basePath) {
        throw "No shared config found. Expected vm.config.yaml in $RepoRoot."
    }
    $base = Import-ConfigLayer -Path $basePath

    $local = @{}
    $localPath = Resolve-ConfigLayerPath -RepoRoot $RepoRoot -BaseName 'vm.config.local'
    if ($localPath) { $local = Import-ConfigLayer -Path $localPath }

    if ($null -eq $Override) { $Override = @{} }

    # A fleet may be declared in either the shared or the local config.
    $machines = @{}
    $withLocal = Merge-Config -Base $base -Override $local
    if ($withLocal.Contains('Machines') -and $withLocal['Machines']) { $machines = $withLocal['Machines'] }

    $block = @{}
    if ($Machine) {
        if (-not $machines.Contains($Machine)) {
            $known = (@($machines.Keys) | Sort-Object) -join ', '
            if (-not $known) { $known = '(none defined)' }
            throw "Unknown machine '$Machine'. Defined machines: $known"
        }
        $block = Copy-Deep $machines[$Machine]
    }

    # Which blueprint? The highest layer that names one wins.
    $probe = Merge-Config -Base $withLocal -Override $block
    $probe = Merge-Config -Base $probe -Override $Override
    $selected = $BlueprintName
    if (-not $selected -and $probe.Contains('Blueprint')) { $selected = [string]$probe['Blueprint'] }
    if (-not $selected) {
        throw "No Blueprint selected. Set 'Blueprint' in vm.config.yaml."
    }

    $blueprint = Resolve-Blueprint -RepoRoot $RepoRoot -Name $selected

    $blueprintDefaults = @{}
    if ($blueprint.Data.Contains('Defaults') -and $blueprint.Data['Defaults']) {
        $blueprintDefaults = $blueprint.Data['Defaults']
    }
    $blueprintOverrides = @{}
    if ($blueprint.Data.Contains('Overrides') -and $blueprint.Data['Overrides']) {
        $blueprintOverrides = $blueprint.Data['Overrides']
    }

    $effective = Copy-Deep $blueprintDefaults
    $effective = Merge-Config -Base $effective -Override $base
    $effective = Merge-Config -Base $effective -Override $blueprintOverrides
    $effective = Merge-Config -Base $effective -Override $local
    $effective = Merge-Config -Base $effective -Override $block
    $effective = Merge-Config -Base $effective -Override $Override
    $effective['Blueprint'] = $selected

    if ($Machine) {
        # A machine's key is its identity unless the machine's own block or an
        # explicit command-line parameter says otherwise.
        #
        # vm.config.local.yaml is deliberately NOT consulted here. A developer
        # setting a single VM.Name locally means 'name my one VM this'; letting
        # that win would collapse every machine in a fleet onto the same name and
        # have them overwrite each other on -All.
        $named = {
            param($section, $key)
            foreach ($layer in @($block, $Override)) {
                if ($layer.Contains($section) -and $layer[$section].Contains($key)) { return $true }
            }
            return $false
        }
        if (-not (& $named 'VM' 'Name')) { $effective['VM']['Name'] = $Machine }
        if (-not (& $named 'Guest' 'Hostname')) { $effective['Guest']['Hostname'] = $Machine }
    }

    # Every hostname in the fleet, so a guest can sanity-check its mDNS peers.
    $peers = New-Object System.Collections.Generic.List[string]
    foreach ($key in (@($machines.Keys) | Sort-Object)) {
        $mBlock = $machines[$key]
        if ($mBlock -is [System.Collections.IDictionary] -and
            $mBlock.Contains('Guest') -and
            $mBlock['Guest'].Contains('Hostname')) {
            $peers.Add([string]$mBlock['Guest']['Hostname'])
        }
        else { $peers.Add([string]$key) }
    }
    if ($peers.Count -eq 0) { $peers.Add([string]$effective['Guest']['Hostname']) }

    # OsVersion: configured explicitly, or the version the blueprint declares.
    if (-not $effective.Contains('Iso') -or -not $effective['Iso']) { $effective['Iso'] = @{} }

    $os = @{}
    if ($blueprint.Data.Contains('Os') -and $blueprint.Data['Os']) { $os = $blueprint.Data['Os'] }

    if (-not $effective['Iso'].Contains('Release') -or -not $effective['Iso']['Release']) {
        if ($os.Contains('Version') -and $os['Version']) {
            $effective['Iso']['Release'] = [string]$os['Version']
        }
        else {
            throw "Blueprint '$($blueprint.Name)' declares no Os.Version, and no OsVersion was configured. Set one or the other."
        }
    }

    # One OsVersion setting has to serve every blueprint, and '42' means nothing
    # to Ubuntu while '24.04' means nothing to Fedora. Each blueprint says what
    # shape its versions take, so switching blueprint without updating the
    # version fails here with a readable message rather than 404ing on a
    # download twenty seconds later.
    $version = [string]$effective['Iso']['Release']
    if ($os.Contains('VersionPattern') -and $os['VersionPattern']) {
        if ($version -notmatch [string]$os['VersionPattern']) {
            $example = ''
            if ($os.Contains('Version')) { $example = " Its own version is $($os['Version'])." }
            throw "OsVersion '$version' does not look like a $($blueprint.Name) release.$example Set OsVersion to match the blueprint, or blank it to use the blueprint's own."
        }
    }

    # Names. Unless something above already set them, the VM and the guest are
    # called <ProjectName>-<Blueprint>, so the hostname says what the machine is
    # for and what it runs. That name is what avahi publishes, so it is also the
    # address a browser uses: http://myproject-fedora-workstation.local
    #
    # Precedence, highest first: an explicit VmName or Hostname, then a Machines
    # key (applied just above), then this.
    $projectName = 'dev'
    if ($effective.Contains('ProjectName') -and $effective['ProjectName']) {
        $projectName = ([string]$effective['ProjectName']).Trim()
    }
    if ($projectName -notmatch '^[A-Za-z0-9][A-Za-z0-9-]*$') {
        throw "ProjectName '$projectName' must be letters, digits and hyphens, starting with a letter or digit. It becomes part of a hostname."
    }

    $derivedName = ("$projectName-$($blueprint.Name)").ToLowerInvariant()

    if (-not $effective['VM'].Contains('Name') -or -not $effective['VM']['Name']) {
        $effective['VM']['Name'] = $derivedName
    }
    if (-not $effective['Guest'].Contains('Hostname') -or -not $effective['Guest']['Hostname']) {
        $effective['Guest']['Hostname'] = $derivedName
    }

    # A hostname that is not a valid DNS label will not resolve over mDNS, which
    # is the whole point of publishing it, so reject it here rather than after a
    # 30-minute build.
    $hostname = [string]$effective['Guest']['Hostname']
    if ($hostname.Length -gt 63) {
        throw "Hostname '$hostname' is $($hostname.Length) characters; DNS labels stop at 63. Shorten ProjectName, or set Hostname explicitly."
    }
    if ($hostname -notmatch '^[a-zA-Z0-9]([a-zA-Z0-9-]*[a-zA-Z0-9])?$') {
        throw "Hostname '$hostname' is not a valid DNS label. Use letters, digits and hyphens only, not starting or ending with a hyphen."
    }

    # Optional sections, so an older or trimmed-down config still loads.
    $optional = @{}
    foreach ($section in @('Git', 'GitHub', 'Database', 'RedHat')) {
        if ($effective.Contains($section) -and $effective[$section]) { $optional[$section] = $effective[$section] }
        else { $optional[$section] = @{} }
    }

    $config = [pscustomobject]@{
        RepoRoot  = $RepoRoot
        Machine   = $Machine
        Blueprint   = $blueprint
        VM        = $effective['VM']
        Guest     = $effective['Guest']
        Bootstrap = $effective['Bootstrap']
        Database  = $optional['Database']
        Iso       = $effective['Iso']
        Git       = $optional['Git']
        GitHub    = $optional['GitHub']
        RedHat    = $optional['RedHat']
        Peers     = @($peers)
        Raw       = $effective
    }

    # Validate now rather than twenty minutes into a first boot: an unknown
    # database engine, a missing directory to copy, a hook script that is not
    # there. All three are cheap to check and expensive to discover late.
    Get-DevVMDatabasePlan -Config $config | Out-Null
    Get-DevVMFileCopyLayout -Config $config | Out-Null
    Get-DevVMHookPlan -Config $config | Out-Null
    Get-DevVMRedHatPlan -Config $config | Out-Null

    return $config
}

<#
    SCSI controller locations. Hyper-V's synthetic SCSI controller reports the
    ControllerLocation as the LUN inside the guest, which is what lets a
    bootstrap step identify 'the logs disk' as /dev/disk/by-path/...scsi-0:0:0:3
    rather than guessing at /dev/sdc. Assigned explicitly here - relying on
    Hyper-V's automatic placement would renumber things when the DVD is ejected.

    Both the host (which attaches the disks) and the seed generator (which tells
    the guest where to look) read this one function, so they cannot drift.
#>
$script:DiskLocationOs   = 0
$script:DiskLocationDvd  = 1
$script:DiskLocationSeed = 2
$script:DiskLocationData = 3

function Get-DevVMDiskLocations {
    [CmdletBinding()] param()
    return [pscustomobject]@{
        Os       = $script:DiskLocationOs
        Dvd      = $script:DiskLocationDvd
        Seed     = $script:DiskLocationSeed
        DataFrom = $script:DiskLocationData
    }
}

<#
    Normalise VM.DataDisks into a validated layout, one object per extra disk.
#>
function Get-DevVMDataDiskLayout {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Config)

    $layout = New-Object System.Collections.Generic.List[object]

    $disks = @()
    if ($Config.VM.Contains('DataDisks') -and $Config.VM['DataDisks']) {
        $disks = @($Config.VM['DataDisks'])
    }
    if ($disks.Count -eq 0) { return @() }

    $location = $script:DiskLocationData
    $seenLabels = @{}
    $seenMounts = @{}
    $seenLetters = @{}

    foreach ($disk in $disks) {
        if (-not ($disk -is [System.Collections.IDictionary])) {
            throw 'Each entry in VM.DataDisks must be a hashtable, for example @{ Label = "data-e"; DriveLetter = "E"; SizeGB = 50 }.'
        }
        foreach ($required in @('Label', 'SizeGB')) {
            if (-not $disk.Contains($required) -or -not $disk[$required]) {
                throw "A VM.DataDisks entry is missing $required."
            }
        }

        $label = [string]$disk['Label']
        # ext4 labels cap at 16 characters, and the label is how the guest finds
        # the volume, so a silently truncated one would break the fstab entry.
        if ($label.Length -gt 16) {
            throw "DataDisk label '$label' is longer than 16 characters, which ext4 cannot store."
        }
        if ($label -notmatch '^[A-Za-z0-9_-]+$') {
            throw "DataDisk label '$label' must be letters, digits, hyphen, or underscore."
        }
        if ($seenLabels.ContainsKey($label)) { throw "Two VM.DataDisks entries share the label '$label'." }
        $seenLabels[$label] = $true

        # A drive letter keeps the production naming, which is how the team talks
        # about these disks. Linux still needs somewhere to attach the
        # filesystem, so an unspecified MountPoint becomes /mnt/<letter> rather
        # than taking over a system directory such as /var/log or /opt.
        $letter = ''
        if ($disk.Contains('DriveLetter') -and $disk['DriveLetter']) {
            $letter = ([string]$disk['DriveLetter']).Trim().TrimEnd(':').ToUpperInvariant()
            if ($letter -notmatch '^[A-Z]$') {
                throw "DataDisk '$label' DriveLetter must be a single letter (got '$($disk['DriveLetter'])')."
            }
            if ($letter -eq 'C') {
                throw "DataDisk '$label' cannot use drive letter C - that is the system disk (set DiskGB for its size)."
            }
            if ($seenLetters.ContainsKey($letter)) { throw "Two VM.DataDisks entries use drive letter $letter." }
            $seenLetters[$letter] = $true
        }

        $mount = ''
        if ($disk.Contains('MountPoint') -and $disk['MountPoint']) {
            $mount = ([string]$disk['MountPoint']).TrimEnd('/')
        }
        elseif ($letter) {
            $mount = '/mnt/' + $letter.ToLowerInvariant()
        }
        else {
            throw "DataDisk '$label' needs either a DriveLetter or a MountPoint."
        }

        if (-not $mount.StartsWith('/')) {
            throw "DataDisk '$label' MountPoint must be an absolute Linux path (got '$mount')."
        }
        if ($mount -eq '') { throw "DataDisk '$label' cannot mount at / - that is the system disk." }
        if ($seenMounts.ContainsKey($mount)) { throw "Two VM.DataDisks entries mount at '$mount'." }
        $seenMounts[$mount] = $true

        $fs = 'ext4'
        if ($disk.Contains('Filesystem') -and $disk['Filesystem']) { $fs = [string]$disk['Filesystem'] }
        if ($fs -notin @('ext4', 'xfs')) {
            throw "DataDisk '$label' Filesystem must be ext4 or xfs (got '$fs')."
        }

        $layout.Add([pscustomobject]@{
            Label              = $label
            SizeGB             = [int]$disk['SizeGB']
            MountPoint         = $mount
            Filesystem         = $fs
            ControllerLocation = $location
            DriveLetter        = $letter
        })
        $location++
    }

    # Emitted as individual objects; every caller wraps the call in @(...).
    return $layout.ToArray()
}

<#
    ------------------------------------------------------------------ databases

    What each engine is, in one table, so adding a third is an entry here plus a
    case in 35-database.sh rather than a hunt through both.

    Source says where the packages come from, and it is not a style choice:

      'pgdg'    the PostgreSQL project's own repository. Fedora and Ubuntu each
                ship whichever major version suited their release, so pinning a
                version means using the upstream repo. PGDG publishes for both.
      'distro'  the distribution's own packages. MySQL has no equivalent
                cross-distro repo that installs unattended without debconf
                juggling, so the requested major version is verified in the guest
                after install and reported if it differs.

    Versions lists what we have actually accounted for. It is deliberately a list
    rather than a range: 'any integer' would accept 99 and fail in the guest.
#>
$script:DatabaseEngines = @{
    postgres = @{
        DisplayName    = 'PostgreSQL'
        DefaultVersion = '17'
        DefaultPort    = 5432
        Versions       = @('13', '14', '15', '16', '17', '18')
        Source         = 'pgdg'
        Client         = 'psql'
        # SELinux confines the server to labelled ports, so a custom port has to
        # be added to this type or the daemon is denied the bind.
        SELinuxType    = 'postgresql_port_t'
        # The PHP extension Adminer needs to talk to this engine. Named the same
        # on both distros for Postgres; MySQL differs, hence the two below.
        PhpDriverApt   = 'php-pgsql'
        PhpDriverDnf   = 'php-pgsql'
        AdminerDriver  = 'pgsql'
    }
    mysql = @{
        DisplayName    = 'MySQL'
        DefaultVersion = '8'
        DefaultPort    = 3306
        Versions       = @('8', '8.0', '8.4')
        Source         = 'distro'
        Client         = 'mysql'
        SELinuxType    = 'mysqld_port_t'
        PhpDriverApt   = 'php-mysql'
        PhpDriverDnf   = 'php-mysqlnd'
        AdminerDriver  = 'server'
    }
}

# Adminer is a single PHP file, published per release. Pinned so every build gets
# the same one rather than whatever is current on the day.
$script:AdminerDefaultVersion = '4.8.1'
$script:AdminerDefaultPort = 3800

function Get-DevVMDatabaseEngineNames {
    [CmdletBinding()] param()
    return ($script:DatabaseEngines.Keys | Sort-Object)
}

<#
    Resolve the Database section into a single validated plan, with the engine's
    defaults filled in. Returns $null when no database was asked for.

    Called from Get-DevVMConfig so a bad engine or version fails at load time,
    and again by the seed generator and the plan printer, which both need the
    resolved version and port rather than the raw config.
#>
function Get-DevVMDatabasePlan {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Config)

    $db = @{}
    if ($Config.PSObject.Properties.Name -contains 'Database' -and $Config.Database) {
        $db = $Config.Database
    }

    $engine = ''
    if ($db.Contains('Engine') -and $db['Engine']) { $engine = ([string]$db['Engine']).Trim().ToLowerInvariant() }

    $wantClient = $db.Contains('InstallClient') -and $db['InstallClient']
    $wantServer = $db.Contains('InstallServer') -and $db['InstallServer']

    if (-not $engine) {
        # Asking for a client or server without naming an engine is a mistake
        # worth reporting: silently installing nothing looks like a broken build.
        if ($wantClient -or $wantServer) {
            throw "Database.InstallClient or Database.InstallServer is set but Database.Engine is blank. Set DbEngine to one of: $((Get-DevVMDatabaseEngineNames) -join ', ')."
        }
        # Adminer needs to know which driver to load, so it cannot stand alone.
        if ($db.Contains('Adminer') -and $db['Adminer'] -and `
            $db['Adminer'].Contains('Install') -and $db['Adminer']['Install']) {
            throw "Database.Adminer.Install is set but Database.Engine is blank. Adminer needs an engine to know which driver to load: set DbEngine to one of: $((Get-DevVMDatabaseEngineNames) -join ', ')."
        }
        return $null
    }

    # Accept the obvious synonyms rather than failing on them.
    $synonyms = @{ postgresql = 'postgres'; pgsql = 'postgres'; pg = 'postgres'; mariadb = 'mysql' }
    if ($synonyms.ContainsKey($engine)) { $engine = $synonyms[$engine] }

    if (-not $script:DatabaseEngines.ContainsKey($engine)) {
        throw "Unknown Database.Engine '$engine'. Supported engines are: $((Get-DevVMDatabaseEngineNames) -join ', ')."
    }
    $spec = $script:DatabaseEngines[$engine]

    $adminer = @{}
    if ($db.Contains('Adminer') -and $db['Adminer']) { $adminer = $db['Adminer'] }
    $adminerInstall = $adminer.Contains('Install') -and $adminer['Install']

    # Adminer on its own is a legitimate ask: it can browse a database on another
    # machine without this one running a server or even holding the client tools.
    if (-not $wantClient -and -not $wantServer -and -not $adminerInstall) {
        throw "Database.Engine is '$engine' but none of Database.InstallClient, Database.InstallServer or Database.Adminer.Install is set, so nothing would be installed. Set DbClient, DbServer or DbAdminer to `$true, or blank DbEngine."
    }

    $version = [string]$spec['DefaultVersion']
    if ($db.Contains('Version') -and $db['Version']) { $version = ([string]$db['Version']).Trim() }
    if ($spec['Versions'] -notcontains $version) {
        throw "Database.Version '$version' is not a supported $($spec['DisplayName']) version. Choose one of: $($spec['Versions'] -join ', ')."
    }

    $port = [int]$spec['DefaultPort']
    if ($db.Contains('Port') -and [int]$db['Port'] -gt 0) { $port = [int]$db['Port'] }
    if ($port -lt 1 -or $port -gt 65535) {
        throw "Database.Port '$port' is outside 1-65535."
    }

    $listen = 'localhost'
    if ($db.Contains('ListenAddress') -and $db['ListenAddress']) { $listen = ([string]$db['ListenAddress']).Trim() }
    if ($listen -notin @('localhost', '*')) {
        throw "Database.ListenAddress must be 'localhost' or '*', not '$listen'."
    }

    $databases = New-Object System.Collections.Generic.List[string]
    if ($db.Contains('Databases') -and $db['Databases']) {
        foreach ($name in (ConvertTo-Array $db['Databases'])) {
            $trimmed = ([string]$name).Trim()
            if (-not $trimmed) { continue }
            # Both engines accept far more than this, but anything outside it
            # needs quoting in SQL, and a dev database does not need the freedom.
            if ($trimmed -notmatch '^[A-Za-z_][A-Za-z0-9_]*$') {
                throw "Database name '$trimmed' must start with a letter or underscore and contain only letters, digits and underscores."
            }
            $databases.Add($trimmed)
        }
    }

    $adminPassword = ''
    if ($db.Contains('AdminPassword') -and $db['AdminPassword']) { $adminPassword = [string]$db['AdminPassword'] }

    # A server on '*' with no password would accept connections from every other
    # VM on the switch as the superuser. Refuse rather than build it.
    if ($wantServer -and $listen -eq '*' -and -not $adminPassword) {
        throw "Database.ListenAddress is '*' but Database.AdminPassword is blank, which would expose the $($spec['DisplayName']) superuser to every machine on the switch. Set AdminPassword, or leave ListenAddress as 'localhost'."
    }

    # ---------------------------------------------------------------- Adminer
    # A database browser in a single PHP file. Parsed above, because whether it
    # was asked for decides whether this section has anything to do at all.
    $adminerPort = [int]$script:AdminerDefaultPort
    if ($adminer.Contains('Port') -and [int]$adminer['Port'] -gt 0) { $adminerPort = [int]$adminer['Port'] }
    $adminerVersion = [string]$script:AdminerDefaultVersion
    if ($adminer.Contains('Version') -and $adminer['Version']) { $adminerVersion = ([string]$adminer['Version']).Trim() }

    if ($adminerInstall) {
        if ($adminerPort -lt 1 -or $adminerPort -gt 65535) {
            throw "Database.Adminer.Port '$adminerPort' is outside 1-65535."
        }
        if ($adminerPort -eq $port) {
            throw "Database.Adminer.Port and Database.Port are both $port. Adminer needs a port of its own."
        }
        if ($adminerVersion -notmatch '^\d+\.\d+(\.\d+)?$') {
            throw "Database.Adminer.Version '$adminerVersion' does not look like a release number, for example 4.8.1."
        }
    }

    $phpDriverApt = [string]$spec['PhpDriverApt']
    $phpDriverDnf = [string]$spec['PhpDriverDnf']

    return [pscustomobject]@{
        Engine        = $engine
        DisplayName   = [string]$spec['DisplayName']
        Version       = $version
        Port          = $port
        ListenAddress = $listen
        Source        = [string]$spec['Source']
        Client        = [string]$spec['Client']
        SELinuxType   = [string]$spec['SELinuxType']
        InstallClient = [bool]($wantClient -or $wantServer)   # a server needs its client
        InstallServer = [bool]$wantServer
        AdminPassword = $adminPassword
        Databases     = $databases.ToArray()
        Remote        = [bool]($listen -eq '*')

        AdminerInstall = [bool]$adminerInstall
        AdminerPort    = $adminerPort
        AdminerVersion = $adminerVersion
        AdminerDriver  = [string]$spec['AdminerDriver']
        PhpDriverApt   = $phpDriverApt
        PhpDriverDnf   = $phpDriverDnf
    }
}

<#
    ------------------------------------------------------- files copied from here

    Directories on this laptop whose contents are carried into the guest.

    Everything is embedded in the seed disk, so this is for configuration,
    certificates, licence files and small fixtures, not for bulk data. Two limits
    guard that: a warning at 10 MB and a hard failure at 64 MB, because base64
    inflates by a third and the seed volume is 256 MB.

    Files land staged under /var/lib/vm-bootstrap/files/<n>/ and are moved into
    place by 06-files.sh. That indirection is what lets a destination be '~':
    where a user's home actually is, and which uid owns it, are things only the
    guest knows.
#>
$script:FileCopyWarnBytes = 10MB
$script:FileCopyMaxBytes = 64MB

function Get-DevVMFileCopyLayout {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Config)

    $files = @{}
    if ($Config.Raw.Contains('Files') -and $Config.Raw['Files']) { $files = $Config.Raw['Files'] }

    # Source/Destination are the single-directory shorthand, which is what the
    # quick settings CopyFrom and CopyTo expand to. Copy is the list form.
    $entries = New-Object System.Collections.Generic.List[object]
    if ($files.Contains('Source') -and $files['Source']) {
        $entries.Add(@{
            From  = $files['Source']
            To    = if ($files.Contains('Destination')) { $files['Destination'] } else { '' }
            Owner = if ($files.Contains('Owner')) { $files['Owner'] } else { '' }
            Mode  = if ($files.Contains('Mode')) { $files['Mode'] } else { '' }
        })
    }
    if ($files.Contains('Copy') -and $files['Copy']) {
        foreach ($entry in (ConvertTo-Array $files['Copy'])) {
            if (-not ($entry -is [System.Collections.IDictionary])) {
                throw 'Each entry in Files.Copy must be a hashtable, for example @{ From = "files/certs"; To = "/usr/local/share/ca-certificates" }.'
            }
            $entries.Add($entry)
        }
    }

    $layout = New-Object System.Collections.Generic.List[object]
    if ($entries.Count -eq 0) { return $layout.ToArray() }

    $primaryUser = [string]$Config.Guest['PrimaryUser']['Username']
    $index = 0
    $totalBytes = 0L

    foreach ($entry in $entries) {
        if (-not $entry.Contains('From') -or -not $entry['From']) {
            throw 'A Files.Copy entry is missing From.'
        }
        $from = [System.Environment]::ExpandEnvironmentVariables([string]$entry['From'])
        # A relative path is relative to the repo, so a committed files/ directory
        # works the same on every machine.
        if (-not [System.IO.Path]::IsPathRooted($from)) {
            $from = Join-Path $Config.RepoRoot $from
        }
        if (-not (Test-Path -LiteralPath $from)) {
            throw "Files.Copy source not found: $from"
        }
        if (-not (Test-Path -LiteralPath $from -PathType Container)) {
            throw "Files.Copy From must be a directory, not a file: $from. Point it at the directory holding the files."
        }
        $from = (Resolve-Path -LiteralPath $from).Path

        $to = ''
        if ($entry.Contains('To') -and $entry['To']) { $to = ([string]$entry['To']).Trim() }
        if (-not $to) { $to = '~' }
        $to = $to -replace '\\', '/'
        if ($to -ne '~' -and -not $to.StartsWith('~/') -and -not $to.StartsWith('/')) {
            throw "Files.Copy To must be an absolute guest path or start with '~', not '$to'."
        }
        $to = $to.TrimEnd('/')
        if (-not $to) { $to = '/' }

        $owner = $primaryUser
        if ($entry.Contains('Owner') -and $entry['Owner']) { $owner = ([string]$entry['Owner']).Trim() }
        if ($owner -notmatch '^[A-Za-z_][A-Za-z0-9_-]*(:[A-Za-z_][A-Za-z0-9_-]*)?$') {
            throw "Files.Copy Owner '$owner' is not a valid user or user:group."
        }

        $mode = ''
        if ($entry.Contains('Mode') -and $entry['Mode']) { $mode = ([string]$entry['Mode']).Trim() }
        if ($mode -and $mode -notmatch '^[0-7]{3,4}$') {
            throw "Files.Copy Mode '$mode' must be an octal permission such as 0644."
        }

        $items = New-Object System.Collections.Generic.List[object]
        foreach ($file in (Get-ChildItem -LiteralPath $from -Recurse -File | Sort-Object FullName)) {
            $relative = $file.FullName.Substring($from.Length).TrimStart('\', '/') -replace '\\', '/'
            $totalBytes += $file.Length
            # Windows has no execute bit, so shell scripts are the one case worth
            # inferring. Anything else can be set explicitly with Mode.
            $perm = if ($mode) { $mode }
                    elseif ($file.Extension -eq '.sh') { '0755' }
                    else { '0644' }
            $items.Add([pscustomobject]@{
                SourcePath   = $file.FullName
                RelativePath = $relative
                SizeBytes    = $file.Length
                Permissions  = $perm
            })
        }

        if ($items.Count -eq 0) {
            Write-Warn "Files.Copy source '$from' has no files in it; nothing to copy."
            continue
        }

        $layout.Add([pscustomobject]@{
            Index       = $index
            From        = $from
            To          = $to
            Owner       = $owner
            Mode        = $mode
            Files       = $items.ToArray()
            SizeBytes   = ($items | Measure-Object -Property SizeBytes -Sum).Sum
            # Where the seed drops them before 06-files.sh moves them into place.
            StageRoot   = "/var/lib/vm-bootstrap/files/$index"
        })
        $index++
    }

    if ($totalBytes -gt $script:FileCopyMaxBytes) {
        throw ("Files.Copy totals {0:N1} MB, over the {1:N0} MB limit. Everything here is embedded in the 256 MB seed disk and base64 inflates it by a third, so this is for config and certificates, not bulk data. Put large files on a share, or add them to a data disk after boot." -f `
            ($totalBytes / 1MB), ($script:FileCopyMaxBytes / 1MB))
    }
    if ($totalBytes -gt $script:FileCopyWarnBytes) {
        Write-Warn ("Files.Copy totals {0:N1} MB, which will slow the install noticeably: it is base64-encoded into the seed and decoded by the installer." -f ($totalBytes / 1MB))
    }

    return $layout.ToArray()
}

<#
    ------------------------------------------------- Red Hat subscription

    Only redhat-rhel-workstation needs this. Red Hat Enterprise Linux ships with
    no usable package repositories until subscription-manager has registered the
    machine against an account, which is the one thing that genuinely separates
    it from Rocky and Alma: those are the same software with public repos and
    need none of this.

    Two ways to register, and the difference matters for an unattended build:

      activation key + org id   made for automation, carries no password, can be
                                revoked on its own, and is what Red Hat tells you
                                to use for provisioning. Preferred.
      username + password       your actual Red Hat login. Works, but it is a
                                real credential going onto a seed disk.

    Neither is required. Without one the install still completes - everything on
    the DVD is installed - but the machine has no repositories, so bootstrap can
    add nothing beyond it: no EPEL, therefore no xrdp, therefore no Enhanced
    Session. Say that plainly at render time rather than letting someone find out
    forty minutes in.
#>
function Get-DevVMRedHatPlan {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Config)

    $rh = @{}
    if ($Config.PSObject.Properties.Name -contains 'RedHat' -and $Config.RedHat) { $rh = $Config.RedHat }

    $get = {
        param([string]$key)
        if ($rh.Contains($key) -and $null -ne $rh[$key]) { return ([string]$rh[$key]).Trim() }
        return ''
    }

    $activationKey = & $get 'ActivationKey'
    $orgId         = & $get 'OrganizationId'
    $username      = & $get 'Username'
    $password      = & $get 'Password'

    # A key file keeps the secret out of the config file itself, the same way
    # GitHub.Cli.TokenFile does.
    $keyFile = & $get 'ActivationKeyFile'
    if ($keyFile -and -not $activationKey) {
        if (-not (Test-Path -LiteralPath $keyFile)) {
            throw "RedHat.ActivationKeyFile points at a file that does not exist: $keyFile"
        }
        $activationKey = ([string](Get-Content -LiteralPath $keyFile -Raw)).Trim()
    }

    $autoAttach = $true
    if ($rh.Contains('AutoAttach')) { $autoAttach = [bool]$rh['AutoAttach'] }

    # Pin dnf to the minor release the ISO is, so 'dnf upgrade' during bootstrap
    # does not quietly move a machine that was asked for 9.7 onto 9.8. Someone
    # naming a minor release means it.
    $pinRelease = $true
    if ($rh.Contains('PinRelease')) { $pinRelease = [bool]$rh['PinRelease'] }

    $mode = 'none'
    if ($activationKey -and $orgId)      { $mode = 'activation-key' }
    elseif ($username -and $password)    { $mode = 'credentials' }

    # Half a credential is a typo, not a choice. Fail rather than silently
    # building an unregistered machine someone thinks is registered.
    if ($mode -eq 'none') {
        if ($activationKey -and -not $orgId) {
            throw 'RedHat.ActivationKey is set but RedHat.OrganizationId is not. An activation key only works with its organization id, shown as the Organization ID on console.redhat.com.'
        }
        if ($orgId -and -not $activationKey) {
            throw 'RedHat.OrganizationId is set but RedHat.ActivationKey is not.'
        }
        if ($username -and -not $password) {
            throw 'RedHat.Username is set but RedHat.Password is not. Prefer an activation key: it carries no password and can be revoked on its own.'
        }
        if ($password -and -not $username) {
            throw 'RedHat.Password is set but RedHat.Username is not.'
        }
    }

    return [pscustomobject]@{
        Mode          = $mode
        ActivationKey = $activationKey
        OrganizationId = $orgId
        Username      = $username
        Password      = $password
        AutoAttach    = $autoAttach
        PinRelease    = $pinRelease
        # The one secret worth keeping out of /etc/vm-bootstrap.env.
        Secret        = $(if ($mode -eq 'activation-key') { $activationKey } elseif ($mode -eq 'credentials') { $password } else { '' })
    }
}

<#
    ------------------------------------------------------------- post-start hooks

    Commands from config, rendered into one script that runs in the guest after
    everything else is provisioned.

    Ordering matters: the script runs as step 80, after the repos are cloned, so a
    hook can reasonably do something inside a checkout. It runs once by default;
    RunAtEveryBoot installs a systemd unit instead, so it also runs on every
    subsequent start.
#>
function Get-DevVMHookPlan {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Config)

    $hooks = @{}
    if ($Config.Raw.Contains('Hooks') -and $Config.Raw['Hooks']) { $hooks = $Config.Raw['Hooks'] }

    $commands = New-Object System.Collections.Generic.List[string]
    if ($hooks.Contains('PostStart') -and $hooks['PostStart']) {
        foreach ($line in (ConvertTo-Array $hooks['PostStart'])) {
            $text = [string]$line
            if (-not $text.Trim()) { continue }
            # A multi-line string in config is a block of commands, which is the
            # natural way to write anything longer than a one-liner in YAML.
            foreach ($split in ($text -split "`r?`n")) { $commands.Add($split) }
        }
    }

    $scriptFile = ''
    if ($hooks.Contains('PostStartFile') -and $hooks['PostStartFile']) {
        $scriptFile = [System.Environment]::ExpandEnvironmentVariables([string]$hooks['PostStartFile'])
        if (-not [System.IO.Path]::IsPathRooted($scriptFile)) {
            $scriptFile = Join-Path $Config.RepoRoot $scriptFile
        }
        if (-not (Test-Path -LiteralPath $scriptFile -PathType Leaf)) {
            throw "Hooks.PostStartFile not found: $scriptFile"
        }
        $scriptFile = (Resolve-Path -LiteralPath $scriptFile).Path
    }

    $runAs = 'user'
    if ($hooks.Contains('RunAs') -and $hooks['RunAs']) { $runAs = ([string]$hooks['RunAs']).Trim().ToLowerInvariant() }
    if ($runAs -notin @('user', 'root')) {
        throw "Hooks.RunAs must be 'user' or 'root', not '$runAs'."
    }

    $workingDir = ''
    if ($hooks.Contains('WorkingDirectory') -and $hooks['WorkingDirectory']) {
        $workingDir = ([string]$hooks['WorkingDirectory']).Trim() -replace '\\', '/'
        if ($workingDir -ne '~' -and -not $workingDir.StartsWith('~/') -and -not $workingDir.StartsWith('/')) {
            throw "Hooks.WorkingDirectory must be an absolute guest path or start with '~', not '$workingDir'."
        }
    }

    $everyBoot = $hooks.Contains('RunAtEveryBoot') -and $hooks['RunAtEveryBoot']

    $hasWork = ($commands.Count -gt 0 -or $scriptFile)
    if (-not $hasWork) {
        if ($everyBoot) {
            throw 'Hooks.RunAtEveryBoot is set but there are no hooks to run. Add commands to Hooks.PostStart, or point Hooks.PostStartFile at a script.'
        }
        return $null
    }

    return [pscustomobject]@{
        Commands         = $commands.ToArray()
        ScriptFile       = $scriptFile
        RunAs            = $runAs
        WorkingDirectory = $workingDir
        RunAtEveryBoot   = [bool]$everyBoot
    }
}

<#
    Normalise Repos.Items into a validated list, one object per repository.

    Entries may be a bare URL string or a hashtable, so the common case stays a
    one-liner. Everything is validated here rather than in the guest: a typo in a
    URL should fail during -RenderOnly, not twenty minutes into a first boot.
#>
function Get-DevVMRepoLayout {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Config)

    $layout = New-Object System.Collections.Generic.List[object]

    $repos = @{}
    if ($Config.Raw.Contains('Repos') -and $Config.Raw['Repos']) { $repos = $Config.Raw['Repos'] }

    $items = @()
    if ($repos.Contains('Items') -and $repos['Items']) { $items = @($repos['Items']) }
    if ($items.Count -eq 0) { return @() }

    $seenDirs = @{}

    foreach ($item in $items) {
        $url = ''
        $entry = @{}
        if ($item -is [System.Collections.IDictionary]) {
            $entry = $item
            if ($entry.Contains('Url')) { $url = ([string]$entry['Url']).Trim() }
        }
        else {
            $url = ([string]$item).Trim()
        }

        if (-not $url) {
            throw 'A Repos.Items entry has no Url.'
        }
        # Covers the forms git actually accepts as a remote. A bare path or a
        # half-pasted URL is almost always a mistake, so reject it here.
        if ($url -notmatch '^(https?://|ssh://|git://|file://|[A-Za-z0-9._-]+@[A-Za-z0-9._-]+:)') {
            throw "Repos entry '$url' does not look like a git URL. Expected https://, ssh://, git://, file://, or user@host:path."
        }

        # Default the target directory to the repository name.
        $directory = ''
        if ($entry.Contains('Directory') -and $entry['Directory']) {
            $directory = ([string]$entry['Directory']).Trim()
        }
        else {
            $leaf = ($url -replace '/+$', '') -replace '^.*[/:]', ''
            $directory = $leaf -replace '\.git$', ''
        }
        if (-not $directory) { throw "Could not work out a directory name for '$url'. Set Directory explicitly." }

        $directory = $directory -replace '\\', '/'
        if ($directory.StartsWith('/') -or $directory -match '(^|/)\.\.(/|$)') {
            throw "Repos entry '$url': Directory must be a relative path without '..' (got '$directory')."
        }
        if ($seenDirs.ContainsKey($directory)) {
            throw "Two Repos entries would both clone into '$directory'. Set Directory on one of them."
        }
        $seenDirs[$directory] = $true

        $branch = ''
        if ($entry.Contains('Branch') -and $entry['Branch']) { $branch = ([string]$entry['Branch']).Trim() }

        $depth = 0
        if ($entry.Contains('Depth') -and $entry['Depth']) { $depth = [int]$entry['Depth'] }
        if ($depth -lt 0) { throw "Repos entry '$url': Depth cannot be negative." }

        $recurse = $false
        if ($entry.Contains('Recurse') -and $entry['Recurse']) { $recurse = $true }

        $postClone = ''
        if ($entry.Contains('PostClone') -and $entry['PostClone']) { $postClone = [string]$entry['PostClone'] }

        $layout.Add([pscustomobject]@{
            Url       = $url
            Directory = $directory
            Branch    = $branch
            Depth     = $depth
            Recurse   = $recurse
            PostClone = $postClone
            # git@host: and ssh:// remotes need the key from 55-github.sh.
            NeedsSsh  = ($url -match '^(ssh://|[A-Za-z0-9._-]+@[A-Za-z0-9._-]+:)')
        })
    }

    # Emitted as individual objects; every caller wraps the call in @(...).
    return $layout.ToArray()
}

function Get-DevVMRepoRoot {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Config)

    $root = 'src'
    if ($Config.Raw.Contains('Repos') -and $Config.Raw['Repos'] -and
        $Config.Raw['Repos'].Contains('Root') -and $Config.Raw['Repos']['Root']) {
        $root = ([string]$Config.Raw['Repos']['Root']).Trim()
    }
    $root = ($root -replace '\\', '/').TrimEnd('/')
    if (-not $root) { $root = 'src' }
    if ($root -match '(^|/)\.\.(/|$)') {
        throw "Repos.Root must not contain '..' (got '$root')."
    }
    return $root
}

<#
    Drop a host's entry from this laptop's ~/.ssh/known_hosts.

    A rebuilt VM has a new host key under the same name, and ssh treats that as
    the signature of a man-in-the-middle rather than the routine event it is
    here:

        @@@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @@@
        Host key verification failed.

    StrictHostKeyChecking=accept-new does not help: it accepts keys it has never
    seen, and still refuses one that changed. So rebuilding a machine under a
    name you have used before leaves you locked out of your own VM until you
    know to run ssh-keygen -R by hand, and every script here that talks to the
    guest fails the same way.

    Called when a VM is created and when one is removed: both mean any key we
    remember for that name is now wrong. Deliberately quiet and never fatal - a
    missing known_hosts, or no ssh-keygen on PATH, is not a reason to fail a
    build.
#>
function Remove-KnownHostEntry {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$HostName)

    if (-not $HostName) { return $false }
    $keygen = Get-Command ssh-keygen.exe -ErrorAction SilentlyContinue
    if (-not $keygen) { return $false }

    $knownHosts = Join-Path $HOME '.ssh\known_hosts'
    if (-not (Test-Path -LiteralPath $knownHosts)) { return $false }

    # Only touch it when there is something to remove, so the common case stays
    # silent instead of announcing work it did not do.
    $before = @(Get-Content -LiteralPath $knownHosts -ErrorAction SilentlyContinue)
    if (-not ($before -match [regex]::Escape($HostName))) { return $false }

    # ssh-keygen writes to stderr on the way past; under $ErrorActionPreference
    # 'Stop' that is a terminating NativeCommandError in Windows PowerShell 5.1,
    # and '2>$null' does not prevent it.
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & $keygen.Source -R $HostName 2>$null | Out-Null }
    catch { return $false }
    finally { $ErrorActionPreference = $previous }

    return $true
}

# ------------------------------------------------------------------ ssh_keys

<#
    ssh_keys/ is a drop directory: put a key file in it and the build finds it,
    with no config to edit. Everything in there is git-ignored twice over.
#>
function Get-DevVMSshKeyDir {
    [CmdletBinding()] param([string]$RepoRoot)
    if (-not $RepoRoot) { $RepoRoot = Get-RepoRoot }
    return (Join-Path $RepoRoot 'ssh_keys')
}

<#
    Derive the public half of a private key with `ssh-keygen -y`.

    Returns the public key text, or '' when the key is passphrase-protected (or
    otherwise unreadable). $null means ssh-keygen itself is unavailable.

    Run through Start-Process rather than the call operator because Windows
    PowerShell 5.1 DROPS an empty-string argument: `& ssh-keygen -y -P '' -f key`
    reaches the exe as `-y -P -f key`, so -P swallows '-f' as the passphrase and
    ssh-keygen fails with "Too many arguments". Start-Process passes a raw command
    line, where the literal token "" is parsed back into an empty argument.
#>
function Get-SshPublicKeyFromPrivate {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Path)

    $keygen = Get-SshKeygenPath
    if (-not $keygen) { return $null }

    $outFile = [System.IO.Path]::GetTempFileName()
    $errFile = [System.IO.Path]::GetTempFileName()
    try {
        $proc = Start-Process -FilePath $keygen `
            -ArgumentList @('-y', '-P', '""', '-f', ('"' + $Path + '"')) `
            -NoNewWindow -Wait -PassThru `
            -RedirectStandardOutput $outFile -RedirectStandardError $errFile `
            -ErrorAction Stop

        if ($proc.ExitCode -ne 0) { return '' }

        $text = (Get-Content -LiteralPath $outFile -Raw -ErrorAction SilentlyContinue)
        if (-not $text) { return '' }
        return $text.Trim()
    }
    catch { return '' }
    finally {
        Remove-Item -LiteralPath $outFile, $errFile -Force -ErrorAction SilentlyContinue
    }
}

function Get-SshKeygenPath {
    [CmdletBinding()] param()

    $cmd = Get-Command ssh-keygen.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }

    $candidates = New-Object System.Collections.Generic.List[string]
    # Windows ships OpenSSH; Git for Windows brings its own.
    if ($env:SystemRoot) {
        $candidates.Add((Join-Path $env:SystemRoot 'System32\OpenSSH\ssh-keygen.exe'))
    }
    $git = Get-Command git.exe -ErrorAction SilentlyContinue
    if ($git) {
        $gitRoot = Split-Path -Parent (Split-Path -Parent $git.Source)
        $candidates.Add((Join-Path $gitRoot 'usr\bin\ssh-keygen.exe'))
    }
    if ($env:ProgramFiles) {
        $candidates.Add((Join-Path $env:ProgramFiles 'Git\usr\bin\ssh-keygen.exe'))
    }

    foreach ($c in $candidates) {
        if ($c -and (Test-Path -LiteralPath $c)) { return $c }
    }
    return $null
}

<#
    Classify whatever is sitting in ssh_keys/.

    Returns private keys (each paired with its public half, derived if missing),
    plus any standalone public keys destined for the guest's authorized_keys.
    Warnings are collected rather than thrown: a stray file in a drop directory
    should produce a clear message, not a failed build.
#>
function Get-DevVMSshKeyMaterial {
    [CmdletBinding()]
    param(
        [string]$RepoRoot,
        [string]$PreferName
    )

    $dir = Get-DevVMSshKeyDir -RepoRoot $RepoRoot

    # This function reports what is in the directory; it does not decide what is
    # fatal. Ambiguity is flagged rather than thrown, because whether it matters
    # depends on the caller's mode - two private keys are a problem only if we
    # actually have to pick one.
    $result = [pscustomobject]@{
        Dir              = $dir
        PrivateKeys      = @()
        Identity         = $null   # the private key to use as the GitHub identity
        AuthorizedKeys   = @()
        Warnings         = @()
        Ambiguous        = $false
        AmbiguityMessage = ''
    }
    if (-not (Test-Path -LiteralPath $dir)) { return $result }

    # Housekeeping files and anything that is plainly not key material.
    $skip = @('README.md', '.gitignore', '.gitkeep', 'known_hosts', 'config', '.DS_Store', 'Thumbs.db')

    $privates = New-Object System.Collections.Generic.List[object]
    $publicsByStem = @{}
    $standalonePublics = New-Object System.Collections.Generic.List[object]
    $authorizedLines = New-Object System.Collections.Generic.List[string]
    $warnings = New-Object System.Collections.Generic.List[string]

    $files = @(Get-ChildItem -LiteralPath $dir -File -ErrorAction SilentlyContinue | Sort-Object Name)

    foreach ($file in $files) {
        if ($skip -contains $file.Name) { continue }

        if ($file.Extension -eq '.ppk') {
            $warnings.Add("$($file.Name) is a PuTTY key, which OpenSSH cannot read. Convert it: PuTTYgen, then Conversions, then Export OpenSSH key.")
            continue
        }

        $firstLine = ''
        try {
            $firstLine = (Get-Content -LiteralPath $file.FullName -TotalCount 1 -ErrorAction Stop)
            if ($null -eq $firstLine) { $firstLine = '' }
        }
        catch {
            $warnings.Add("could not read $($file.Name): $($_.Exception.Message)")
            continue
        }

        if ($file.Name -eq 'authorized_keys') {
            foreach ($line in (Get-Content -LiteralPath $file.FullName)) {
                $trimmed = $line.Trim()
                if ($trimmed -and -not $trimmed.StartsWith('#')) { $authorizedLines.Add($trimmed) }
            }
            continue
        }

        if ($firstLine -match '^-----BEGIN .*PRIVATE KEY-----') {
            $privates.Add($file)
            continue
        }

        if ($firstLine -match '^(ssh-|ecdsa-|sk-)') {
            $stem = [System.IO.Path]::GetFileNameWithoutExtension($file.Name)
            $publicsByStem[$stem] = [pscustomobject]@{ Path = $file.FullName; Text = $firstLine.Trim() }
            $standalonePublics.Add([pscustomobject]@{ Stem = $stem; Path = $file.FullName; Text = $firstLine.Trim() })
            continue
        }

        $warnings.Add("ignoring $($file.Name) - it is neither an OpenSSH private key nor a public key.")
    }


    $keyObjects = New-Object System.Collections.Generic.List[object]
    foreach ($private in $privates) {
        $stem = $private.Name
        $publicText = ''
        $publicPath = $null
        $encrypted = $false

        if ($publicsByStem.ContainsKey($stem)) {
            $publicText = $publicsByStem[$stem].Text
            $publicPath = $publicsByStem[$stem].Path
        }
        else {
            # An empty passphrase fails on an encrypted key, which is exactly how
            # we detect one.
            $derived = Get-SshPublicKeyFromPrivate -Path $private.FullName
            if ($null -eq $derived) {
                $warnings.Add("no ssh-keygen found to derive a public key for $($private.Name). Drop $($private.Name).pub in as well.")
            }
            elseif ($derived) {
                $publicText = $derived
            }
            else {
                $encrypted = $true
                $warnings.Add("$($private.Name) looks passphrase-protected and has no $($private.Name).pub beside it. Drop the .pub file in too - otherwise git prompts for the passphrase and an unattended clone during first boot will fail.")
            }
        }

        $keyObjects.Add([pscustomobject]@{
            Name       = $private.Name
            Path       = $private.FullName
            PublicPath = $publicPath
            PublicText = $publicText
            Encrypted  = $encrypted
        })
    }

    # Which key is the GitHub identity?
    $identity = $null
    if ($keyObjects.Count -eq 1) {
        $identity = $keyObjects[0]
    }
    elseif ($keyObjects.Count -gt 1) {
        $names = ($keyObjects.ToArray() | ForEach-Object { $_.Name }) -join ', '

        if ($PreferName) {
            # Indexing [0] into an empty match throws under StrictMode, so match
            # into a variable and test the count.
            $matched = @($keyObjects.ToArray() | Where-Object { $_.Name -eq $PreferName })
            if ($matched.Count -eq 0) {
                $result.Ambiguous = $true
                $result.AmbiguityMessage = "GitHub.SshKey.Name is '$PreferName' but ssh_keys/ holds: $names"
            }
            else { $identity = $matched[0] }
        }
        else {
            # Fall back to the conventional name before giving up.
            $conventional = @($keyObjects.ToArray() | Where-Object { $_.Name -match '^id_(ed25519|rsa|ecdsa)$' })
            if ($conventional.Count -eq 0) {
                $result.Ambiguous = $true
                $result.AmbiguityMessage = "ssh_keys/ holds more than one private key ($names) and none is named id_ed25519, id_rsa, or id_ecdsa. Set GitHub.SshKey.Name to the one GitHub should use."
            }
            else { $identity = $conventional[0] }
        }
    }

    # Every public key in ssh_keys/ becomes an authorized key, paired or not.
    #
    # This used to skip a .pub whose private half was also present, on the theory
    # that such a pair is "the GitHub identity, not a login key". That reasoning
    # does not survive contact with how people actually use the directory: the
    # setup ssh_keys/README.md calls typical is
    #
    #     Copy-Item $HOME\.ssh\id_ed25519     .\ssh_keys\
    #     Copy-Item $HOME\.ssh\id_ed25519.pub .\ssh_keys\
    #
    # which is a pair, and the README promises it means "SSH from the host just
    # works". It did not. The build finished, the VM came up, and ssh answered
    # with Permission denied (publickey) for the very key sitting in the folder.
    #
    # Someone who puts their own public key here wants in. A key being useful to
    # git as well does not make it less theirs, and this is a single-user dev VM,
    # not a bastion. Anyone who wants the separation can leave the .pub out and
    # set Guest.PrimaryUser.AuthorizedKeys by hand.
    foreach ($pub in $standalonePublics.ToArray()) {
        $authorizedLines.Add($pub.Text)
    }

    # .ToArray() throughout, never @(): see ConvertTo-Array.
    $result.PrivateKeys = $keyObjects.ToArray()
    $result.Identity = $identity
    $result.AuthorizedKeys = @($authorizedLines.ToArray() | Select-Object -Unique)
    $result.Warnings = $warnings.ToArray()
    return $result
}

function Get-MachineNames {
    [CmdletBinding()] param([string]$RepoRoot)

    if (-not $RepoRoot) { $RepoRoot = Get-RepoRoot }

    $basePath = Resolve-ConfigLayerPath -RepoRoot $RepoRoot -BaseName 'vm.config'
    if (-not $basePath) { return @() }
    $user = Import-ConfigLayer -Path $basePath

    $localPath = Resolve-ConfigLayerPath -RepoRoot $RepoRoot -BaseName 'vm.config.local'
    if ($localPath) {
        $user = Merge-Config -Base $user -Override (Import-ConfigLayer -Path $localPath)
    }
    if ($user.Contains('Machines') -and $user['Machines']) {
        return @(@($user['Machines'].Keys) | Sort-Object)
    }
    return @()
}

# ------------------------------------------------------------------- password

<#
    openssl ships with Git for Windows, which every developer cloning this repo
    already has, so password hashing needs no extra dependency.
#>
function Get-OpenSslPath {
    [CmdletBinding()] param()

    $cmd = Get-Command openssl.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }

    $candidates = New-Object System.Collections.Generic.List[string]
    $git = Get-Command git.exe -ErrorAction SilentlyContinue
    if ($git) {
        # Two levels up from ...\Git\cmd\git.exe is the ...\Git root.
        $gitRoot = Split-Path -Parent (Split-Path -Parent $git.Source)
        $candidates.Add((Join-Path $gitRoot 'usr\bin\openssl.exe'))
        $candidates.Add((Join-Path $gitRoot 'mingw64\bin\openssl.exe'))
    }
    if ($env:ProgramFiles) {
        $candidates.Add((Join-Path $env:ProgramFiles 'Git\usr\bin\openssl.exe'))
    }
    if (${env:ProgramFiles(x86)}) {
        $candidates.Add((Join-Path ${env:ProgramFiles(x86)} 'Git\usr\bin\openssl.exe'))
    }
    if ($env:LOCALAPPDATA) {
        $candidates.Add((Join-Path $env:LOCALAPPDATA 'Programs\Git\usr\bin\openssl.exe'))
    }

    foreach ($c in $candidates) {
        if ($c -and (Test-Path -LiteralPath $c)) { return $c }
    }
    return $null
}

function New-Sha512Crypt {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Password)

    $openssl = Get-OpenSslPath
    if (-not $openssl) {
        $msg = "Cannot hash the account password: openssl.exe was not found.`n`n" +
               "Fix it either way:`n" +
               "  * Install Git for Windows (it bundles openssl), or`n" +
               "  * Generate a hash elsewhere and set Guest.PrimaryUser.PasswordHash`n" +
               "    in vm.config.local.yaml:   openssl passwd -6"
        throw $msg
    }

    # crypt(3) salts draw from this 64-character alphabet.
    $alphabet = './0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz'
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try {
        $bytes = New-Object byte[] 16
        $rng.GetBytes($bytes)
    }
    finally { $rng.Dispose() }
    $salt = -join (@($bytes) | ForEach-Object { $alphabet[$_ % 64] })

    # The password reaches openssl through a FILE, not a pipe and not a command
    # line argument.
    #
    # It used to be piped, with $OutputEncoding set to UTF-8 to stop non-ASCII
    # characters becoming '?'. That fixed one corruption and hid another:
    # Windows PowerShell 5.1 writes the encoding's BOM and a CRLF into the pipe,
    # so `'secret' | openssl passwd -6 -stdin` hands openssl
    #
    #     EF BB BF  s e c r e t  0D 0A
    #
    # and -stdin strips only the 0A. The account was built with the hash of
    # "<BOM>secret\r", which no one can type. Measured with od, and the wrong
    # hash reproduced exactly from those bytes on Linux.
    #
    # Nothing about that is visible: the build succeeds, the hash is a perfectly
    # well-formed $6$ string, and SSH keys keep working - so the machine looks
    # fine until someone tries the console, GDM, xrdp or sudo, and every one of
    # them says the password is wrong.
    #
    # A file is the only delivery that is exact. Passing the password as an
    # argument would work too, but it would sit in the process list for every
    # local user to read, which is the thing the pipe was chosen to avoid.
    $passwordFile = [System.IO.Path]::GetTempFileName()
    try {
        # openssl -in reads one password per line, so exactly one newline, and
        # no BOM: UTF8Encoding($false) is the encoder, and WriteAllBytes does not
        # add a preamble of its own the way the pipeline did.
        $utf8 = New-Object System.Text.UTF8Encoding($false)
        [System.IO.File]::WriteAllBytes($passwordFile, $utf8.GetBytes($Password + "`n"))

        # Read it back and compare. This is the check that would have caught the
        # pipe: it asserts the exact bytes openssl is about to see, rather than
        # trusting the layer in between.
        $written = [System.IO.File]::ReadAllBytes($passwordFile)
        $expected = $utf8.GetBytes($Password + "`n")
        if ($written.Length -ne $expected.Length) {
            throw "Could not stage the password for hashing: wrote $($expected.Length) bytes, read back $($written.Length)."
        }
        for ($i = 0; $i -lt $expected.Length; $i++) {
            if ($written[$i] -ne $expected[$i]) {
                throw "Could not stage the password for hashing: byte $i changed on the way to disk."
            }
        }

        $hash = ([string](& $openssl passwd -6 -salt $salt -in $passwordFile)).Trim()
    }
    finally {
        if (Test-Path -LiteralPath $passwordFile) {
            # Overwrite before unlinking, so the plaintext is not left in a
            # temp file's slack for the next person to find.
            try { [System.IO.File]::WriteAllBytes($passwordFile, (New-Object byte[] 64)) } catch { }
            Remove-Item -LiteralPath $passwordFile -Force -ErrorAction SilentlyContinue
        }
    }

    if ($hash -notmatch '^\$6\$') {
        throw "openssl did not return a SHA-512 crypt hash (got: '$hash'). Set Guest.PrimaryUser.PasswordHash manually."
    }
    # The salt openssl echoes back must be the one we asked for. A mismatch means
    # it rejected or rewrote the salt, and the hash would not be what we think.
    if ($hash -notmatch ('^\$6\$' + [regex]::Escape($salt) + '\$')) {
        throw "openssl returned a hash with a different salt than requested. Set Guest.PrimaryUser.PasswordHash manually."
    }
    return $hash
}

<#
    Return the crypt hash for a user block, hashing plain text on the fly when
    no PasswordHash was supplied. Returns $null for a passwordless account.
#>
function Resolve-PasswordHash {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$User)

    if ($User.Contains('PasswordHash') -and $User['PasswordHash']) {
        $h = [string]$User['PasswordHash']
        if ($h -notmatch '^\$[0-9a-z]+\$') {
            throw ("Guest user '{0}': PasswordHash does not look like a crypt hash. " -f $User['Username']) +
                  "Generate one with 'openssl passwd -6', or set Password instead and let the tooling hash it."
        }
        return $h
    }
    if ($User.Contains('Password') -and $User['Password']) {
        return (New-Sha512Crypt -Password ([string]$User['Password']))
    }
    return $null
}

<#
    Is <hostname>.local already answering on this network?

    Windows 10 and later resolve .local over mDNS, so this asks the OS resolver
    the same question a browser would. A hit means something already claims the
    name: another VM, a teammate's machine on the same switch, or a previous build
    of this one that is still running.

    Returns the address as a string, or $null. Never throws - being unable to
    resolve is the expected answer, and no network is not an error either.
#>
function Test-DevVMHostnameResolvable {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Hostname)

    $fqdn = $Hostname
    if ($fqdn -notmatch '\.') { $fqdn = "$Hostname.local" }

    try {
        $addresses = [System.Net.Dns]::GetHostAddresses($fqdn)
        foreach ($address in @($addresses)) {
            # Ignore IPv6 link-local, which tells us nothing useful here.
            if ($address.AddressFamily -eq 'InterNetwork') { return $address.IPAddressToString }
        }
        if (@($addresses).Count -gt 0) { return @($addresses)[0].IPAddressToString }
    }
    catch { }
    return $null
}

# ------------------------------------------------------------------ preflight

function Test-Elevated {
    [CmdletBinding()] param()
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Assert-Prerequisites {
    [CmdletBinding()] param([switch]$RequireHyperV)

    if (-not (Test-Elevated)) {
        throw "This script must run in an elevated PowerShell session - the Hyper-V cmdlets require Administrator."
    }
    if ($RequireHyperV) {
        if (-not (Get-Module -ListAvailable -Name Hyper-V)) {
            throw "The Hyper-V PowerShell module is not installed. Run: Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All -All"
        }
        Import-Module Hyper-V -ErrorAction Stop
        try { Get-VMHost -ErrorAction Stop | Out-Null }
        catch { throw "Hyper-V does not appear to be running on this host. Enable it (Windows Features, Hyper-V) and reboot." }
    }
}

<#
    Write text as UTF-8 with no BOM and LF line endings. cloud-init silently
    ignores a user-data file that begins with a BOM, which is a miserable
    failure to debug from a VM console, so this is not optional politeness.
#>
function Write-UnixText {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][AllowEmptyString()][string]$Content
    )
    $normalized = $Content -replace "`r`n", "`n"
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($Path, $normalized, $utf8NoBom)
}

# =============================================================================
#  The YAML reader
#
#  Lives here rather than in its own module because config is the only thing that
#  needs it, and a separate file meant every caller imported two modules to read
#  one file.
# =============================================================================


<#
    A small YAML reader, so vm.config.yaml and vm.config.local.yaml can be read
    without a dependency.

    Why hand-rolled? Windows PowerShell 5.1 has no YAML support, and the
    alternative is telling every developer to Install-Module powershell-yaml
    before they can build a VM - which defeats the point of a repo you clone and
    run. This parser has no dependencies and behaves identically on every laptop.

    The trade-off is that it reads a SUBSET of YAML, chosen to cover config files
    and nothing more. Supported:

        mappings          Key: value          (nested by indentation)
        sequences         - item              (at or below the key's indent)
        sequences of maps - Key: value
                            Other: value
        flow sequences    [a, b, c]  and  []
        empty mapping     {}
        strings           plain, 'single', "double" (with \n \t \" \\ escapes)
        numbers           42, -3, 1.5
        booleans          true/false, yes/no, on/off  (any case)
        null              null, ~, or an empty value
        comments          # to end of line, honouring quotes
        document marker   a leading ---

    NOT supported, and rejected with a clear error rather than misread: anchors
    and aliases (&x / *x), tags (!!str), block scalars (| and >), multiple
    documents, complex keys, and flow mappings ({a: 1}) other than empty {}.

    None of the unsupported constructs are needed to express a config, and each is
    a way to write something the next person cannot read. A nested mapping goes on
    its own indented lines.

    Keys land in ordinary PowerShell hashtables, which are case-insensitive, so
    'cpuCount', 'CpuCount', and 'cpucount' all reach the same setting.
#>

function Remove-YamlComment {
    [CmdletBinding()] param([Parameter(Mandatory)][AllowEmptyString()][string]$Line)

    $inSingle = $false
    $inDouble = $false

    for ($i = 0; $i -lt $Line.Length; $i++) {
        $ch = $Line[$i]

        if ($ch -eq "'" -and -not $inDouble) { $inSingle = -not $inSingle; continue }
        if ($ch -eq '"' -and -not $inSingle) {
            # A backslash-escaped quote does not close the string.
            if ($i -gt 0 -and $Line[$i - 1] -eq '\') { continue }
            $inDouble = -not $inDouble
            continue
        }
        if ($ch -eq '#' -and -not $inSingle -and -not $inDouble) {
            # Only a '#' at the start of the line or preceded by whitespace
            # begins a comment; 'a#b' is a plain scalar.
            if ($i -eq 0 -or $Line[$i - 1] -match '\s') {
                return $Line.Substring(0, $i)
            }
        }
    }
    return $Line
}

<#
    Split 'Key: value' into its two halves. Returns $null when the line is not a
    mapping entry, so the caller can tell a key from a sequence item or a scalar.
#>
function Split-YamlKey {
    [CmdletBinding()] param([Parameter(Mandatory)][AllowEmptyString()][string]$Text)

    $inSingle = $false
    $inDouble = $false

    for ($i = 0; $i -lt $Text.Length; $i++) {
        $ch = $Text[$i]

        if ($ch -eq "'" -and -not $inDouble) { $inSingle = -not $inSingle; continue }
        if ($ch -eq '"' -and -not $inSingle) {
            if ($i -gt 0 -and $Text[$i - 1] -eq '\') { continue }
            $inDouble = -not $inDouble
            continue
        }
        if ($inSingle -or $inDouble) { continue }

        # The separator is a colon followed by whitespace or end of line. That is
        # what keeps 'Path: C:\VMs' and a git@host:path URL parsing correctly.
        if ($ch -eq ':') {
            $isLast = ($i -eq $Text.Length - 1)
            if ($isLast -or $Text[$i + 1] -match '\s') {
                $key = $Text.Substring(0, $i).Trim()
                if (-not $key) { return $null }

                # A flow mapping reaches here as a KEY, not a value: in
                # '- {from: x, to: y}' the brace opens the key, so the rejection in
                # ConvertFrom-YamlScalar never sees it. Without this it parsed as a
                # key literally named '{from', and the failure surfaced later as a
                # baffling complaint about a missing field.
                if ($key.StartsWith('{')) {
                    throw "Flow mappings other than {} are not supported: $Text. Write the keys on their own indented lines."
                }

                $value = ''
                if (-not $isLast) { $value = $Text.Substring($i + 1).Trim() }
                return [pscustomobject]@{
                    Key   = (ConvertFrom-YamlKeyText $key)
                    Value = $value
                }
            }
        }
    }
    return $null
}

function ConvertFrom-YamlKeyText {
    [CmdletBinding()] param([Parameter(Mandatory)][string]$Text)

    $t = $Text.Trim()
    if ($t.Length -ge 2) {
        if ($t.StartsWith('"') -and $t.EndsWith('"')) { return (Expand-YamlEscapes $t.Substring(1, $t.Length - 2)) }
        if ($t.StartsWith("'") -and $t.EndsWith("'")) { return $t.Substring(1, $t.Length - 2).Replace("''", "'") }
    }
    return $t
}

function Expand-YamlEscapes {
    [CmdletBinding()] param([Parameter(Mandatory)][AllowEmptyString()][string]$Text)

    $sb = New-Object System.Text.StringBuilder
    for ($i = 0; $i -lt $Text.Length; $i++) {
        if ($Text[$i] -eq '\' -and $i -lt $Text.Length - 1) {
            $i++
            switch ($Text[$i]) {
                'n'  { [void]$sb.Append("`n") }
                't'  { [void]$sb.Append("`t") }
                'r'  { [void]$sb.Append("`r") }
                '0'  { [void]$sb.Append([char]0) }
                '"'  { [void]$sb.Append('"') }
                '\'  { [void]$sb.Append('\') }
                default { [void]$sb.Append('\'); [void]$sb.Append($Text[$i]) }
            }
        }
        else { [void]$sb.Append($Text[$i]) }
    }
    return $sb.ToString()
}

<#
    Split a flow sequence body on commas, honouring quotes.
#>
function Split-YamlFlow {
    [CmdletBinding()] param([Parameter(Mandatory)][AllowEmptyString()][string]$Body)

    $parts = New-Object System.Collections.Generic.List[string]
    $current = New-Object System.Text.StringBuilder
    $inSingle = $false
    $inDouble = $false

    for ($i = 0; $i -lt $Body.Length; $i++) {
        $ch = $Body[$i]

        if ($ch -eq "'" -and -not $inDouble) { $inSingle = -not $inSingle; [void]$current.Append($ch); continue }
        if ($ch -eq '"' -and -not $inSingle) { $inDouble = -not $inDouble; [void]$current.Append($ch); continue }

        if ($ch -eq ',' -and -not $inSingle -and -not $inDouble) {
            $parts.Add($current.ToString())
            $current = New-Object System.Text.StringBuilder
            continue
        }
        [void]$current.Append($ch)
    }
    if ($current.Length -gt 0) { $parts.Add($current.ToString()) }
    return $parts.ToArray()
}

function ConvertFrom-YamlScalar {
    [CmdletBinding()] param([Parameter(Mandatory)][AllowEmptyString()][string]$Raw, [int]$LineNumber = 0, [string]$Source = '<yaml>')

    $v = $Raw.Trim()
    if ($v -eq '') { return $null }

    # Unsupported YAML that must not be silently misread.
    #
    # These messages used to suggest a .psd1 config as the way round an anchor.
    # That was wrong: Import-PowerShellDataFile parses in restricted mode, which
    # rejects a variable reference outright, so a .psd1 cannot reuse a value
    # either. Point at what actually works instead.
    if ($v -match '^[&*]') {
        throw ("Anchors and aliases (& and *) are not supported (line $LineNumber of $Source). " +
               "You usually do not need them: a top-level setting already applies to every machine, " +
               "so write it once at the top and override it only in the machine that differs. " +
               "For a whole kind of machine, give a blueprint an Inherits. Otherwise write the value out.")
    }
    if ($v -match '^!!?') {
        throw "Explicit tags (! and !!) are not supported (line $LineNumber of $Source). Remove the tag and let the value speak for itself: quoted means string, bare digits mean a number."
    }
    if ($v -eq '|' -or $v -eq '>' -or $v -match '^[|>][-+]?\d*$') {
        throw "Block scalars (| and >) are not supported (line $LineNumber of $Source). Use a quoted string with \n, or a .psd1 config."
    }

    if ($v.Length -ge 2) {
        if ($v.StartsWith('"') -and $v.EndsWith('"')) {
            return (Expand-YamlEscapes $v.Substring(1, $v.Length - 2))
        }
        if ($v.StartsWith("'") -and $v.EndsWith("'")) {
            return $v.Substring(1, $v.Length - 2).Replace("''", "'")
        }
    }

    if ($v -eq '[]') { return , ([object[]]@()) }
    if ($v -eq '{}') { return @{} }

    if ($v.StartsWith('[') -and $v.EndsWith(']')) {
        $items = New-Object System.Collections.Generic.List[object]
        foreach ($part in (Split-YamlFlow $v.Substring(1, $v.Length - 2))) {
            if ($part.Trim() -eq '') { continue }
            $items.Add((ConvertFrom-YamlScalar -Raw $part -LineNumber $LineNumber -Source $Source))
        }
        return , ([object[]]$items.ToArray())
    }

    if ($v.StartsWith('{')) {
        throw "Flow mappings other than {} are not supported (line $LineNumber of $Source). Write the keys on their own indented lines."
    }

    if ($v -eq '~' -or $v -match '^(?i)null$') { return $null }
    if ($v -match '^(?i)(true|yes|on)$') { return $true }
    if ($v -match '^(?i)(false|no|off)$') { return $false }

    # Integers, but a leading zero stays a string: '007' is far more likely to be
    # an identifier than an octal number, and guessing either way surprises
    # somebody.
    if ($v -match '^[+-]?[1-9][0-9]*$' -or $v -eq '0') {
        $parsed = 0L
        if ([long]::TryParse($v, [ref]$parsed)) {
            if ($parsed -ge [int]::MinValue -and $parsed -le [int]::MaxValue) { return [int]$parsed }
            return $parsed
        }
    }
    if ($v -match '^[+-]?([0-9]+\.[0-9]*|\.[0-9]+)([eE][+-]?[0-9]+)?$') {
        $d = 0.0
        if ([double]::TryParse($v, [System.Globalization.NumberStyles]::Float,
                [System.Globalization.CultureInfo]::InvariantCulture, [ref]$d)) {
            return $d
        }
    }

    return $v
}

function Get-YamlLines {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][AllowEmptyString()][string]$Text,
        [string]$Source = '<yaml>'
    )

    $lines = New-Object System.Collections.Generic.List[object]
    $number = 0

    foreach ($raw in ($Text -split "`r?`n")) {
        $number++

        # YAML forbids tabs in indentation, and a tab-indented file that parses
        # "somehow" is worse than one that says what is wrong.
        if ($raw -match '^ *\t') {
            throw "Tab used for indentation on line $number of $Source. YAML requires spaces."
        }

        $stripped = Remove-YamlComment $raw
        if ($stripped -match '^\s*$') { continue }

        $trimmedStart = $stripped.TrimStart(' ')
        $indent = $stripped.Length - $trimmedStart.Length
        $text = $trimmedStart.TrimEnd()

        if ($text -eq '---') { continue }          # a single document marker
        if ($text -eq '...') { continue }
        if ($text -match '^%') { continue }        # directives

        $lines.Add([pscustomobject]@{ Indent = $indent; Text = $text; Line = $number })
    }

    # `, ([object[]]...)` and not a bare return: PowerShell unrolls a returned
    # collection, so a document with exactly ONE setting came back as a single
    # PSCustomObject and every $lines.Count in the parser threw under StrictMode.
    # A one-line vm.config.local.yaml is the most natural thing a person writes
    # first, so this was reachable on day one.
    return , ([object[]]$lines.ToArray())
}

function Test-YamlSequenceLine {
    [CmdletBinding()] param([Parameter(Mandatory)]$Line)
    return ($Line.Text -eq '-' -or $Line.Text -like '- *' -or $Line.Text -like "-`t*")
}

function Read-YamlNode {
    [CmdletBinding()]
    param($Lines, [ref]$Index, [int]$Indent, [string]$Source)

    if ($Index.Value -ge $Lines.Count) { return $null }
    if (Test-YamlSequenceLine $Lines[$Index.Value]) {
        # Read-YamlSequence already guards its array with `, (...)`, but passing that
        # straight back through `return (...)` undoes it twice over: the wrapper is
        # unrolled, and then a single-element inner array is unrolled too. So a
        # one-item sequence arrived as a bare scalar, and 'repos:' with exactly one
        # entry was silently dropped, because the caller type-tests for an array.
        # Re-wrap here, and normalise with @() so a scalar from any path becomes one.
        $sequence = Read-YamlSequence -Lines $Lines -Index $Index -Indent $Indent -Source $Source
        return , ([object[]]@($sequence))
    }
    return (Read-YamlMapping -Lines $Lines -Index $Index -Indent $Indent -Source $Source)
}

function Read-YamlMapping {
    [CmdletBinding()]
    param($Lines, [ref]$Index, [int]$Indent, [string]$Source)

    $map = @{}

    while ($Index.Value -lt $Lines.Count) {
        $line = $Lines[$Index.Value]
        if ($line.Indent -lt $Indent) { break }
        if (Test-YamlSequenceLine $line) { break }
        if ($line.Indent -gt $Indent) {
            throw "Unexpected indentation on line $($line.Line) of $Source - expected $Indent spaces, found $($line.Indent)."
        }

        $kv = Split-YamlKey $line.Text
        if (-not $kv) {
            throw "Expected 'key: value' on line $($line.Line) of $Source, found: $($line.Text)"
        }
        if ($map.Contains($kv.Key)) {
            throw "Duplicate key '$($kv.Key)' on line $($line.Line) of $Source."
        }

        $Index.Value++

        if ($kv.Value -ne '') {
            $map[$kv.Key] = ConvertFrom-YamlScalar -Raw $kv.Value -LineNumber $line.Line -Source $Source
            continue
        }

        # An empty value means the value is the block that follows: either
        # indented deeper, or a sequence at the same indent as the key.
        if ($Index.Value -lt $Lines.Count) {
            $next = $Lines[$Index.Value]
            if ($next.Indent -gt $Indent) {
                $map[$kv.Key] = Read-YamlNode -Lines $Lines -Index $Index -Indent $next.Indent -Source $Source
                continue
            }
            if ($next.Indent -eq $Indent -and (Test-YamlSequenceLine $next)) {
                $map[$kv.Key] = Read-YamlSequence -Lines $Lines -Index $Index -Indent $Indent -Source $Source
                continue
            }
        }
        $map[$kv.Key] = $null
    }

    return $map
}

function Read-YamlSequence {
    [CmdletBinding()]
    param($Lines, [ref]$Index, [int]$Indent, [string]$Source)

    $items = New-Object System.Collections.Generic.List[object]

    while ($Index.Value -lt $Lines.Count) {
        $line = $Lines[$Index.Value]
        if ($line.Indent -lt $Indent) { break }
        if (-not (Test-YamlSequenceLine $line)) { break }
        if ($line.Indent -gt $Indent) {
            throw "Unexpected indentation on line $($line.Line) of $Source - expected $Indent spaces, found $($line.Indent)."
        }

        $after = $line.Text.Substring(1)
        $rest = $after.TrimStart(" `t")

        if ($rest -ne '') {
            # The item's content starts on this line. Its effective indent is the
            # real column of that content, so both '- Key: v' and '-   Key: v'
            # line up with their continuation lines.
            $itemIndent = $line.Indent + 1 + ($after.Length - $rest.Length)
            $kv = Split-YamlKey $rest

            if ($kv) {
                # A mapping starting on the dash line. Rewrite this entry as an
                # ordinary mapping line so the mapping reader can consume it
                # together with the lines below it.
                $Lines[$Index.Value] = [pscustomobject]@{
                    Indent = $itemIndent
                    Text   = $rest
                    Line   = $line.Line
                }
                $items.Add((Read-YamlMapping -Lines $Lines -Index $Index -Indent $itemIndent -Source $Source))
                continue
            }

            if ($rest -eq '-' -or $rest -like '- *') {
                $Lines[$Index.Value] = [pscustomobject]@{
                    Indent = $itemIndent
                    Text   = $rest
                    Line   = $line.Line
                }
                $items.Add((Read-YamlSequence -Lines $Lines -Index $Index -Indent $itemIndent -Source $Source))
                continue
            }

            $items.Add((ConvertFrom-YamlScalar -Raw $rest -LineNumber $line.Line -Source $Source))
            $Index.Value++
            continue
        }

        # A bare '-': the item is the indented block underneath it.
        $Index.Value++
        if ($Index.Value -lt $Lines.Count -and $Lines[$Index.Value].Indent -gt $line.Indent) {
            $items.Add((Read-YamlNode -Lines $Lines -Index $Index -Indent $Lines[$Index.Value].Indent -Source $Source))
        }
        else {
            $items.Add($null)
        }
    }

    return , ([object[]]$items.ToArray())
}

<#
    Parse YAML text into hashtables, arrays, and scalars.
#>
function ConvertFrom-YamlText {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][AllowEmptyString()][string]$Text,
        [string]$Source = '<yaml>'
    )

    if ($Text -match '(?m)^\s*---\s*$[\s\S]*?^\s*---\s*$') {
        throw "$Source contains more than one YAML document. This reader handles a single document."
    }

    $lines = Get-YamlLines -Text $Text -Source $Source
    if ($lines.Count -eq 0) { return @{} }

    $index = 0
    $result = Read-YamlNode -Lines $lines -Index ([ref]$index) -Indent $lines[0].Indent -Source $Source

    if ($index -lt $lines.Count) {
        $line = $lines[$index]
        throw "Could not parse line $($line.Line) of $Source - check the indentation: $($line.Text)"
    }
    return $result
}

function Import-Yaml {
    [CmdletBinding()] param([Parameter(Mandatory)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path)) { throw "YAML file not found: $Path" }
    $text = Get-Content -LiteralPath $Path -Raw
    if ($null -eq $text) { $text = '' }
    return (ConvertFrom-YamlText -Text $text -Source (Split-Path -Leaf $Path))
}


Export-ModuleMember -Function Write-Step, Write-Detail, Write-Warn, Get-RepoRoot,
    Reset-BuildSteps, Start-BuildStep, Complete-BuildStep, Write-BuildReport, Get-DevVMTargets,
    Copy-Deep, Merge-Config, Import-Psd, Import-ConfigLayer, Resolve-ConfigLayerPath,
    Expand-ConfigAliases, Set-ConfigPath, Get-ConfigAliasTable, Resolve-Blueprint, Get-DevVMConfig,
    Get-MachineNames, Get-OpenSslPath, New-Sha512Crypt, Resolve-PasswordHash, Get-DevVMRedHatPlan,
    Test-Elevated, Assert-Prerequisites, Write-UnixText, ConvertTo-Array,
    Get-DevVMDiskLocations, Get-DevVMDataDiskLayout, Get-DevVMRepoLayout, Get-DevVMRepoRoot,
    Get-DevVMDatabasePlan, Get-DevVMDatabaseEngineNames,
    Get-DevVMFileCopyLayout, Get-DevVMHookPlan,
    Get-DevVMSshKeyDir, Get-DevVMSshKeyMaterial, Get-SshKeygenPath, Get-SshPublicKeyFromPrivate,
    Remove-KnownHostEntry,
    Test-DevVMHostnameResolvable,
    ConvertFrom-YamlText, Import-Yaml, ConvertFrom-YamlScalar,
    Remove-YamlComment, Split-YamlKey, Split-YamlFlow, Expand-YamlEscapes
