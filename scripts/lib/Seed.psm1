#requires -Version 5.1
Set-StrictMode -Version Latest

Import-Module (Join-Path $PSScriptRoot 'Common.psm1') -DisableNameChecking

$script:SecretsDir = '/root/vm-bootstrap-secrets'

function ConvertTo-YamlBool {
    param($Value)
    if ($Value) { return 'true' }
    return 'false'
}

function ConvertTo-ShellBool {
    param($Value)
    if ($Value) { return 'true' }
    return 'false'
}

function ConvertTo-ShellSingleQuoted {
    param([AllowEmptyString()][string]$Value)
    if ($null -eq $Value) { $Value = '' }
    return "'" + ($Value -replace "'", "'\''") + "'"
}

function ConvertTo-Base64Lines {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Path,
        [int]$Width = 76
    )
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    $b64 = [Convert]::ToBase64String($bytes)
    $lines = New-Object System.Collections.Generic.List[string]
    for ($i = 0; $i -lt $b64.Length; $i += $Width) {
        $len = [Math]::Min($Width, $b64.Length - $i)
        $lines.Add($b64.Substring($i, $len))
    }
    return $lines
}

function New-WriteFileEntry {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$SourcePath,
        [Parameter(Mandatory)][string]$TargetPath,
        [string]$Permissions = '0644',
        [string]$Owner = 'root:root',
        [int]$Indent = 6
    )

    $pad = ' ' * $Indent
    $inner = ' ' * ($Indent + 2)
    $body = ' ' * ($Indent + 4)

    $sb = New-Object System.Text.StringBuilder
    [void]$sb.AppendLine("$pad- path: $TargetPath")
    [void]$sb.AppendLine("${inner}owner: $Owner")
    [void]$sb.AppendLine("${inner}permissions: '$Permissions'")
    [void]$sb.AppendLine("${inner}encoding: b64")
    [void]$sb.AppendLine("${inner}content: |")
    foreach ($line in (ConvertTo-Base64Lines -Path $SourcePath)) {
        [void]$sb.AppendLine("$body$line")
    }
    return $sb.ToString().TrimEnd("`r", "`n")
}

function New-ShellWriteFileCommand {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$SourcePath,
        [Parameter(Mandatory)][string]$TargetPath,
        [string]$Permissions = '0644',
        [string]$Owner = 'root:root'
    )

    $sb = New-Object System.Text.StringBuilder
    [void]$sb.AppendLine("install -d -m 0755 '$([System.IO.Path]::GetDirectoryName($TargetPath) -replace '\\', '/')'")

    [void]$sb.AppendLine("base64 -d > '$TargetPath' <<'VMBOOTSTRAP_B64'")
    foreach ($line in (ConvertTo-Base64Lines -Path $SourcePath)) {
        [void]$sb.AppendLine($line)
    }
    [void]$sb.AppendLine('VMBOOTSTRAP_B64')
    [void]$sb.AppendLine("chown $Owner '$TargetPath'")
    [void]$sb.AppendLine("chmod $Permissions '$TargetPath'")
    return $sb.ToString().TrimEnd("`r", "`n")
}

function New-BootstrapEnv {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Config)

    $g = $Config.Guest
    $b = $Config.Bootstrap
    $vm = $Config.VM
    $git = $Config.Git
    $gh = $Config.GitHub
    $prov = $Config.Blueprint.Data['Provisioning']

    $bootstrapRoot = '/usr/local/lib/vm-bootstrap'
    if ($prov.Contains('BootstrapRoot') -and $prov['BootstrapRoot']) {
        $bootstrapRoot = [string]$prov['BootstrapRoot']
    }

    function Get-Or($table, [string]$key, $fallback) {
        if ($table.Contains($key) -and $null -ne $table[$key]) { return $table[$key] }
        return $fallback
    }

    $sshKey = @{}
    if ($gh.Contains('SshKey') -and $gh['SshKey']) { $sshKey = $gh['SshKey'] }
    $ghCli = @{}
    if ($gh.Contains('Cli') -and $gh['Cli']) { $ghCli = $gh['Cli'] }

    $keySource = Resolve-SshKeySource -Config $Config
    $keyType = [string](Get-Or $sshKey 'Type' 'ed25519')
    if ($keySource.KeyType) { $keyType = $keySource.KeyType }

    $extraPackages = @(Get-Or $b 'ExtraAptPackages' @()) -join ' '

    $vscodeSeen = @{}
    $vscodeList = New-Object System.Collections.Generic.List[string]
    foreach ($ext in (@(Get-Or $b 'VSCodeExtensions' @()) + @(Get-Or $b 'ExtraVSCodeExtensions' @()))) {
        $id = ([string]$ext).Trim()
        if (-not $id) { continue }
        if ($vscodeSeen.ContainsKey($id.ToLowerInvariant())) { continue }
        $vscodeSeen[$id.ToLowerInvariant()] = $true
        $vscodeList.Add($id)
    }
    $vscodeExtensions = ($vscodeList.ToArray()) -join ' '
    $peers = @($Config.Peers) -join ' '

    $dbPlan = Get-DevVMDatabasePlan -Config $Config
    $dbEngine = ''; $dbVersion = ''; $dbSource = ''
    $dbPort = ''; $dbListen = ''; $dbSELinuxType = ''; $dbDatabases = ''
    $dbClient = ConvertTo-ShellBool $false
    $dbServer = ConvertTo-ShellBool $false
    $adminerInstall = ConvertTo-ShellBool $false
    $adminerPort = ''; $adminerVersion = ''
    $adminerPhpApt = ''; $adminerPhpDnf = ''
    if ($dbPlan) {
        $dbEngine = $dbPlan.Engine
        $dbVersion = $dbPlan.Version
        $dbSource = $dbPlan.Source
        $dbPort = [string]$dbPlan.Port
        $dbListen = $dbPlan.ListenAddress
        $dbSELinuxType = $dbPlan.SELinuxType
        $dbDatabases = (@($dbPlan.Databases)) -join ' '
        $dbClient = ConvertTo-ShellBool $dbPlan.InstallClient
        $dbServer = ConvertTo-ShellBool $dbPlan.InstallServer

        $adminerInstall = ConvertTo-ShellBool $dbPlan.AdminerInstall
        $adminerPort = [string]$dbPlan.AdminerPort
        $adminerVersion = $dbPlan.AdminerVersion
        $adminerPhpApt = $dbPlan.PhpDriverApt
        $adminerPhpDnf = $dbPlan.PhpDriverDnf
    }

    $rhPlan = Get-DevVMRedHatPlan -Config $Config
    $rhsmMode = 'none'; $rhsmOrgId = ''; $rhsmUsername = ''
    $rhsmAutoAttach = ConvertTo-ShellBool $true
    $rhsmRelease = ''
    if ($rhPlan) {
        $rhsmMode = $rhPlan.Mode
        $rhsmOrgId = $rhPlan.OrganizationId
        $rhsmUsername = $rhPlan.Username
        $rhsmAutoAttach = ConvertTo-ShellBool $rhPlan.AutoAttach

        if ($rhPlan.PinRelease -and [string]$Config.Iso['Release'] -match '^\d+\.\d+$') {
            $rhsmRelease = [string]$Config.Iso['Release']
        }
    }

    $hookPlan = Get-DevVMHookPlan -Config $Config
    $hooksRunAs = ''; $hooksWorkingDir = ''; $hooksScriptPath = ''
    $hooksEveryBoot = ConvertTo-ShellBool $false
    if ($hookPlan) {
        $hooksRunAs = $hookPlan.RunAs
        $hooksWorkingDir = $hookPlan.WorkingDirectory
        $hooksEveryBoot = ConvertTo-ShellBool $hookPlan.RunAtEveryBoot
        $hooksScriptPath = "$bootstrapRoot/hooks/post-start.sh"
    }

    $vars = [ordered]@{
        VM_BOOTSTRAP_ROOT           = $bootstrapRoot
        VM_BLUEPRINT                = $Config.Blueprint.Name
        VM_HOSTNAME                 = [string](Get-Or $g 'Hostname' 'ubuntu-dev')
        VM_PRIMARY_USER             = [string]$g['PrimaryUser']['Username']
        VM_TIMEZONE                 = [string](Get-Or $g 'Timezone' '')
        VM_LOCALE                   = [string](Get-Or $g 'Locale' '')
        VM_KEYBOARD_LAYOUT          = [string](Get-Or $g 'KeyboardLayout' 'us')

        VM_SSH_INSTALL_SERVER       = ConvertTo-ShellBool (Get-Or $g['Ssh'] 'InstallServer' $true)
        VM_SSH_ALLOW_PASSWORD_AUTH  = ConvertTo-ShellBool (Get-Or $g['Ssh'] 'AllowPasswordAuth' $true)

        VM_ENABLE_MDNS              = ConvertTo-ShellBool (Get-Or $b 'EnableMdns' $true)
        VM_MDNS_PEERS               = $peers

        VM_ENHANCED_SESSION         = ConvertTo-ShellBool (Get-Or $vm 'EnhancedSession' $true)

        VM_INSTALL_DESKTOP          = ConvertTo-ShellBool (Get-Or $b 'InstallDesktop' $true)

        VM_DESKTOP_PACKAGE          = [string](Get-Or $b 'DesktopPackage' '')
        VM_DISPLAY_RESOLUTION       = [string](Get-Or $b 'DisplayResolution' '1920x1080')

        VM_INSTALL_DOCKER           = ConvertTo-ShellBool (Get-Or $b 'InstallDocker' $true)
        VM_INSTALL_DDEV             = ConvertTo-ShellBool (Get-Or $b 'InstallDdev' $false)
        VM_INSTALL_NODE             = ConvertTo-ShellBool (Get-Or $b 'InstallNode' $true)
        VM_NODE_VERSION             = [string](Get-Or $b 'NodeVersion' 'lts/*')
        VM_PYTHON_VERSION           = [string](Get-Or $b 'PythonVersion' '3.14')
        VM_NVM_VERSION              = [string](Get-Or $b 'NvmVersion' 'v0.40.1')
        VM_INSTALL_PYTHON           = ConvertTo-ShellBool (Get-Or $b 'InstallPython' $true)
        VM_INSTALL_VSCODE           = ConvertTo-ShellBool (Get-Or $b 'InstallVSCode' $true)
        VM_INSTALL_CHROME           = ConvertTo-ShellBool (Get-Or $b 'InstallChrome' $true)
        VM_INSTALL_POSTMAN          = ConvertTo-ShellBool (Get-Or $b 'InstallPostman' $false)
        VM_VSCODE_EXTENSIONS        = $vscodeExtensions
        VM_INSTALL_REMOTE_ACCESS    = ConvertTo-ShellBool (Get-Or $b 'InstallRemoteAccess' $true)

        VM_EXTRA_APT_PACKAGES       = $extraPackages
        VM_DOTFILES_REPO            = [string](Get-Or $b 'DotfilesRepo' '')
        VM_REPOS_ROOT               = Get-DevVMRepoRoot -Config $Config
        VM_REBOOT_WHEN_DONE         = ConvertTo-ShellBool (Get-Or $b 'RebootWhenDone' $true)

        VM_GIT_USER_NAME            = [string](Get-Or $git 'UserName' '')
        VM_GIT_USER_EMAIL           = [string](Get-Or $git 'UserEmail' '')
        VM_GIT_DEFAULT_BRANCH       = [string](Get-Or $git 'DefaultBranch' '')
        VM_GIT_EDITOR               = [string](Get-Or $git 'Editor' '')
        VM_GIT_PULL_REBASE          = ConvertTo-ShellBool (Get-Or $git 'PullRebase' $false)

        VM_DB_ENGINE                = $dbEngine
        VM_DB_VERSION               = $dbVersion
        VM_DB_SOURCE                = $dbSource
        VM_DB_PORT                  = $dbPort
        VM_DB_LISTEN_ADDRESS        = $dbListen
        VM_DB_INSTALL_CLIENT        = $dbClient
        VM_DB_INSTALL_SERVER        = $dbServer
        VM_DB_SELINUX_TYPE          = $dbSELinuxType
        VM_DB_DATABASES             = $dbDatabases

        VM_ADMINER_INSTALL          = $adminerInstall
        VM_ADMINER_PORT             = $adminerPort
        VM_ADMINER_VERSION          = $adminerVersion
        VM_ADMINER_PHP_DRIVER_APT   = $adminerPhpApt
        VM_ADMINER_PHP_DRIVER_DNF   = $adminerPhpDnf

        VM_RHSM_MODE                = $rhsmMode
        VM_RHSM_ORG_ID              = $rhsmOrgId
        VM_RHSM_USERNAME            = $rhsmUsername
        VM_RHSM_AUTO_ATTACH         = $rhsmAutoAttach
        VM_RHSM_RELEASE             = $rhsmRelease

        VM_HOOKS_RUN_AS             = $hooksRunAs
        VM_HOOKS_WORKING_DIR        = $hooksWorkingDir
        VM_HOOKS_EVERY_BOOT         = $hooksEveryBoot
        VM_HOOKS_SCRIPT             = $hooksScriptPath

        VM_SECRETS_DIR              = $script:SecretsDir
        VM_GITHUB_USERNAME          = [string](Get-Or $gh 'Username' '')
        VM_GITHUB_SSH_KEY_MODE      = $keySource.Mode
        VM_GITHUB_SSH_KEY_TYPE      = $keyType
        VM_GITHUB_SSH_KEY_COMMENT   = [string](Get-Or $sshKey 'Comment' '')
        VM_GITHUB_KNOWN_HOSTS       = ConvertTo-ShellBool (Get-Or $gh 'KnownHosts' $true)
        VM_GITHUB_INSTALL_CLI       = ConvertTo-ShellBool (Get-Or $ghCli 'Install' $false)
        VM_GITHUB_CLI_PROTOCOL      = [string](Get-Or $ghCli 'Protocol' 'ssh')
        VM_GITHUB_UPLOAD_SSH_KEY    = ConvertTo-ShellBool (Get-Or $gh 'UploadSshKey' $false)
        VM_GITHUB_SHRED_SECRETS     = ConvertTo-ShellBool (Get-Or $gh 'ShredSecretsAfterUse' $true)
    }

    $sb = New-Object System.Text.StringBuilder
    [void]$sb.AppendLine('# GENERATED by scripts/lib/Seed.psm1 - edit vm.config.yaml, not this file.')
    [void]$sb.AppendLine('# Sourced by /usr/local/sbin/vm-bootstrap with `set -a`.')
    foreach ($key in $vars.Keys) {
        [void]$sb.AppendLine(("{0}={1}" -f $key, (ConvertTo-ShellSingleQuoted ([string]$vars[$key]))))
    }
    return $sb.ToString()
}

function New-AdditionalUsersFile {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Config)

    $sb = New-Object System.Text.StringBuilder
    [void]$sb.AppendLine('# username<TAB>realname<TAB>hash<TAB>sudo<TAB>groups<TAB>base64(authorized_keys)')

    $extra = @()
    if ($Config.Guest.Contains('AdditionalUsers') -and $Config.Guest['AdditionalUsers']) {
        $extra = @($Config.Guest['AdditionalUsers'])
    }

    foreach ($user in $extra) {
        if (-not $user.Contains('Username') -or -not $user['Username']) {
            throw 'Every entry in Guest.AdditionalUsers needs a Username.'
        }
        $username = [string]$user['Username']

        $realName = $username
        if ($user.Contains('RealName') -and $user['RealName']) { $realName = [string]$user['RealName'] }

        $hash = Resolve-PasswordHash -User $user
        if (-not $hash) { $hash = '-' }

        $sudo = '0'
        if ($user.Contains('Sudo') -and $user['Sudo']) { $sudo = '1' }

        $groups = '-'
        if ($user.Contains('Groups') -and $user['Groups']) { $groups = (@($user['Groups']) -join ',') }

        $keysB64 = '-'
        if ($user.Contains('AuthorizedKeys') -and @($user['AuthorizedKeys']).Count -gt 0) {
            $joined = (@($user['AuthorizedKeys']) -join "`n") + "`n"
            $keysB64 = [Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes($joined))
        }

        [void]$sb.AppendLine(($username, $realName, $hash, $sudo, $groups, $keysB64) -join "`t")
    }
    return $sb.ToString()
}

function New-GitExtraFile {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Config)

    $sb = New-Object System.Text.StringBuilder
    [void]$sb.AppendLine('# section.key<TAB>value - applied with `git config --global`')

    $extra = @{}
    if ($Config.Git.Contains('Extra') -and $Config.Git['Extra']) { $extra = $Config.Git['Extra'] }

    foreach ($key in (@($extra.Keys) | Sort-Object)) {
        $value = [string]$extra[$key]
        if ($key -notmatch '^[A-Za-z0-9]+([.-][A-Za-z0-9_-]+)+$') {
            throw "Git.Extra key '$key' is not a git config key. Use 'section.key' form, for example 'push.autoSetupRemote'."
        }
        if ($value -match "`t|`n") {
            throw "Git.Extra['$key'] contains a tab or newline, which this file format cannot carry."
        }
        [void]$sb.AppendLine(($key, $value) -join "`t")
    }
    return $sb.ToString()
}

function New-FilesManifest {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Config)

    $sb = New-Object System.Text.StringBuilder
    [void]$sb.AppendLine('# stage<TAB>destination<TAB>owner<TAB>mode<TAB>filecount')
    [void]$sb.AppendLine('# mode is "-" when no explicit Mode was given: see the note below.')

    foreach ($entry in @(Get-DevVMFileCopyLayout -Config $Config)) {

        $mode = if ($entry.Mode) { $entry.Mode } else { '-' }
        [void]$sb.AppendLine(("{0}`t{1}`t{2}`t{3}`t{4}" -f `
            $entry.StageRoot, $entry.To, $entry.Owner, $mode, $entry.Files.Count))
    }
    return $sb.ToString()
}

function New-HookScript {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Config)

    $plan = Get-DevVMHookPlan -Config $Config
    if (-not $plan) { return '' }

    $sb = New-Object System.Text.StringBuilder
    [void]$sb.AppendLine('#!/usr/bin/env bash')
    [void]$sb.AppendLine('#')
    [void]$sb.AppendLine('# GENERATED by scripts/lib/Seed.psm1 from Hooks in vm.config.yaml.')
    [void]$sb.AppendLine('# Edit the config and rebuild, or re-run this file directly to test a change:')
    [void]$sb.AppendLine('#')
    [void]$sb.AppendLine('#     sudo vm-bootstrap --only 80-hooks --force')
    [void]$sb.AppendLine('#')

    [void]$sb.AppendLine('set -euo pipefail')
    [void]$sb.AppendLine('')

    if ($plan.Commands.Count -gt 0) {
        [void]$sb.AppendLine('# ---- Hooks.PostStart -------------------------------------------------')
        foreach ($command in $plan.Commands) {
            [void]$sb.AppendLine($command)
        }
        [void]$sb.AppendLine('')
    }

    if ($plan.ScriptFile) {
        $name = [System.IO.Path]::GetFileName($plan.ScriptFile)
        [void]$sb.AppendLine("# ---- Hooks.PostStartFile: $name ----")
        $lines = [System.IO.File]::ReadAllLines($plan.ScriptFile)
        for ($i = 0; $i -lt $lines.Count; $i++) {

            if ($i -eq 0 -and $lines[$i] -match '^#!') { continue }
            [void]$sb.AppendLine($lines[$i])
        }
    }

    return $sb.ToString()
}

function New-ReposFile {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Config)

    $sb = New-Object System.Text.StringBuilder
    [void]$sb.AppendLine('# url<TAB>directory<TAB>branch<TAB>depth<TAB>recurse<TAB>base64(post-clone command)')

    foreach ($repo in @(Get-DevVMRepoLayout -Config $Config)) {
        $recurse = '0'
        if ($repo.Recurse) { $recurse = '1' }

        $postB64 = '-'
        if ($repo.PostClone) {
            $postB64 = [Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes($repo.PostClone))
        }

        $branch = $repo.Branch
        if (-not $branch) { $branch = '-' }

        [void]$sb.AppendLine(($repo.Url, $repo.Directory, $branch, $repo.Depth, $recurse, $postB64) -join "`t")
    }
    return $sb.ToString()
}

function New-DataDiskFile {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Config)

    $sb = New-Object System.Text.StringBuilder
    [void]$sb.AppendLine('# label<TAB>scsi-lun<TAB>mountpoint<TAB>filesystem<TAB>sizeGB')
    foreach ($disk in (Get-DevVMDataDiskLayout -Config $Config)) {
        [void]$sb.AppendLine(($disk.Label, $disk.ControllerLocation, $disk.MountPoint,
                              $disk.Filesystem, $disk.SizeGB) -join "`t")
    }
    return $sb.ToString()
}

function ConvertTo-SshKeyType {
    [CmdletBinding()] param([AllowEmptyString()][string]$PublicText)

    if (-not $PublicText) { return '' }
    $algorithm = ($PublicText.Trim() -split '\s+')[0]
    switch -Wildcard ($algorithm) {
        'ssh-ed25519'      { return 'ed25519' }
        'sk-ssh-ed25519*'  { return 'ed25519_sk' }
        'ssh-rsa'          { return 'rsa' }
        'ecdsa-sha2-*'     { return 'ecdsa' }
        'sk-ecdsa-sha2-*'  { return 'ecdsa_sk' }
        default            { return '' }
    }
}

function Resolve-SshKeySource {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Config)

    $gh = $Config.GitHub
    $sshKey = @{}
    if ($gh -and $gh.Count -gt 0 -and $gh.Contains('SshKey') -and $gh['SshKey']) { $sshKey = $gh['SshKey'] }

    $mode = 'auto'
    if ($sshKey.Contains('Mode') -and $sshKey['Mode']) { $mode = ([string]$sshKey['Mode']).ToLowerInvariant() }
    if ($mode -notin @('auto', 'none', 'generate', 'copy')) {
        throw "GitHub.SshKey.Mode must be 'auto', 'none', 'generate', or 'copy' (got '$mode')."
    }

    $result = [pscustomobject]@{
        Mode           = $mode
        PrivateKeyPath = ''
        PublicKeyPath  = ''
        PublicText     = ''
        KeyType        = ''
        Source         = ''
        Material       = $null
    }
    $preferName = ''
    if ($sshKey.Contains('Name') -and $sshKey['Name']) { $preferName = [string]$sshKey['Name'] }

    $material = Get-DevVMSshKeyMaterial -RepoRoot $Config.RepoRoot -PreferName $preferName
    $result.Material = $material

    if ($mode -eq 'none') { $result.Source = 'disabled'; return $result }

    $explicit = ''
    if ($sshKey.Contains('PrivateKeyPath') -and $sshKey['PrivateKeyPath']) {
        $explicit = [System.Environment]::ExpandEnvironmentVariables([string]$sshKey['PrivateKeyPath'])
    }

    if ($explicit -and $mode -ne 'generate') {
        if (-not (Test-Path -LiteralPath $explicit)) {
            throw "GitHub.SshKey.PrivateKeyPath not found: $explicit"
        }
        $public = ''
        if ($sshKey.Contains('PublicKeyPath') -and $sshKey['PublicKeyPath']) {
            $public = [System.Environment]::ExpandEnvironmentVariables([string]$sshKey['PublicKeyPath'])
        }
        else { $public = "$explicit.pub" }

        $result.Mode = 'copy'
        $result.PrivateKeyPath = $explicit
        if (Test-Path -LiteralPath $public) {
            $result.PublicKeyPath = $public
            $result.PublicText = ((Get-Content -LiteralPath $public -TotalCount 1) | Out-String).Trim()
        }
        else {
            $derived = Get-SshPublicKeyFromPrivate -Path $explicit
            if ($derived) { $result.PublicText = $derived }
        }
        $result.KeyType = ConvertTo-SshKeyType $result.PublicText
        $result.Source = "GitHub.SshKey.PrivateKeyPath ($explicit)"
        return $result
    }

    if ($mode -eq 'generate') {
        $result.Mode = 'generate'
        $result.Source = 'generated in the guest'
        return $result
    }

    if ($material.Ambiguous) { throw $material.AmbiguityMessage }

    if ($material.Identity) {
        $result.Mode = 'copy'
        $result.PrivateKeyPath = $material.Identity.Path
        $result.PublicKeyPath = $(if ($material.Identity.PublicPath) { $material.Identity.PublicPath } else { '' })
        $result.PublicText = $material.Identity.PublicText
        $result.KeyType = ConvertTo-SshKeyType $material.Identity.PublicText
        $result.Source = "ssh_keys/$($material.Identity.Name)"
        return $result
    }

    if ($mode -eq 'copy') {
        throw "GitHub.SshKey.Mode is 'copy' but no private key was found. Drop one into ssh_keys/, set PrivateKeyPath, or use Mode = 'auto'."
    }

    $result.Mode = 'generate'
    $result.Source = 'generated in the guest (ssh_keys/ has no private key)'
    return $result
}

function New-SecretDescriptors {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]$Config,
        [Parameter(Mandatory)][string]$StageDir,

        $KeySource
    )

    $descriptors = New-Object System.Collections.Generic.List[object]

    if (-not (Test-Path -LiteralPath $StageDir)) {
        New-Item -ItemType Directory -Path $StageDir -Force | Out-Null
    }

    $dbPlan = Get-DevVMDatabasePlan -Config $Config
    if ($dbPlan -and $dbPlan.AdminPassword) {
        $stagedDbPassword = Join-Path $StageDir 'db-admin-password'
        Write-UnixText -Path $stagedDbPassword -Content $dbPlan.AdminPassword
        $descriptors.Add([pscustomobject]@{
            SourcePath  = $stagedDbPassword
            TargetPath  = "$script:SecretsDir/db-admin-password"
            Permissions = '0600'
            Secret      = $true
            Label       = "$($dbPlan.DisplayName) admin password"
        })
    }

    $rhPlan = Get-DevVMRedHatPlan -Config $Config
    if ($rhPlan -and $rhPlan.Secret) {
        $stagedRhsm = Join-Path $StageDir 'redhat-subscription-secret'
        Write-UnixText -Path $stagedRhsm -Content $rhPlan.Secret
        $label = $(if ($rhPlan.Mode -eq 'activation-key') { 'Red Hat activation key' } else { "Red Hat account password for $($rhPlan.Username)" })
        $descriptors.Add([pscustomobject]@{
            SourcePath  = $stagedRhsm
            TargetPath  = "$script:SecretsDir/redhat-subscription-secret"
            Permissions = '0600'
            Secret      = $true
            Label       = $label
        })
    }

    $gh = $Config.GitHub
    if (-not $gh -or $gh.Count -eq 0) { return $descriptors }

    if (-not $KeySource) { $KeySource = Resolve-SshKeySource -Config $Config }
    $mode = $KeySource.Mode

    if ($mode -eq 'copy') {
        $privatePath = $KeySource.PrivateKeyPath
        $publicPath = $KeySource.PublicKeyPath

        if (-not $publicPath -or -not (Test-Path -LiteralPath $publicPath)) {

            if ($KeySource.PublicText) {
                $publicPath = Join-Path $StageDir 'github-ssh-key.derived.pub'
                Write-UnixText -Path $publicPath -Content ($KeySource.PublicText + "`n")
            }
            else {

                Write-Warn "no public key available for $privatePath (passphrase-protected?). The key is still installed, but it cannot be uploaded to GitHub and git will prompt for the passphrase."
                $publicPath = ''
            }
        }

        $stagedPrivate = Join-Path $StageDir 'github-ssh-key'
        Copy-Item -LiteralPath $privatePath -Destination $stagedPrivate -Force

        $descriptors.Add([pscustomobject]@{
            SourcePath  = $stagedPrivate
            TargetPath  = "$script:SecretsDir/github-ssh-key"
            Permissions = '0600'
            Secret      = $true
            Label       = "private SSH key from $privatePath"
        })

        if ($publicPath) {
            $stagedPublic = Join-Path $StageDir 'github-ssh-key.pub'
            Copy-Item -LiteralPath $publicPath -Destination $stagedPublic -Force
            $descriptors.Add([pscustomobject]@{
                SourcePath  = $stagedPublic
                TargetPath  = "$script:SecretsDir/github-ssh-key.pub"
                Permissions = '0644'
                Secret      = $false
                Label       = 'public SSH key'
            })
        }
    }

    $ghCli = @{}
    if ($gh.Contains('Cli') -and $gh['Cli']) { $ghCli = $gh['Cli'] }

    $token = ''
    $tokenOrigin = ''
    if ($ghCli.Contains('TokenFile') -and $ghCli['TokenFile']) {
        $tokenFile = [System.Environment]::ExpandEnvironmentVariables([string]$ghCli['TokenFile'])
        if (-not (Test-Path -LiteralPath $tokenFile)) {
            throw "GitHub.Cli.TokenFile not found: $tokenFile"
        }
        $token = (Get-Content -LiteralPath $tokenFile -Raw).Trim()
        $tokenOrigin = $tokenFile
        if (-not $token) { throw "GitHub.Cli.TokenFile is empty: $tokenFile" }
    }
    elseif ($ghCli.Contains('Token') -and $ghCli['Token']) {
        $token = ([string]$ghCli['Token']).Trim()
        $tokenOrigin = 'GitHub.Cli.Token in config'
    }

    if ($token) {
        if ($token -match '\s') {
            throw 'The GitHub token contains whitespace. Check the file holds only the token.'
        }
        $stagedToken = Join-Path $StageDir 'github-token'
        Write-UnixText -Path $stagedToken -Content $token
        $descriptors.Add([pscustomobject]@{
            SourcePath  = $stagedToken
            TargetPath  = "$script:SecretsDir/github-token"
            Permissions = '0600'
            Secret      = $true
            Label       = "GitHub token from $tokenOrigin"
        })
    }

    return $descriptors
}

function New-SeedDisk {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$VhdPath,
        [Parameter(Mandatory)][string]$SourceDir,
        [string]$Label = 'CIDATA',
        [long]$SizeBytes = 256MB
    )

    Import-Module Hyper-V -ErrorAction Stop

    if (Test-Path -LiteralPath $VhdPath) {
        try { Dismount-VHD -Path $VhdPath -ErrorAction SilentlyContinue } catch { }
        Remove-Item -LiteralPath $VhdPath -Force
    }
    $parent = Split-Path -Parent $VhdPath
    if (-not (Test-Path -LiteralPath $parent)) { New-Item -ItemType Directory -Path $parent -Force | Out-Null }

    Write-Detail "creating the $Label seed disk: $VhdPath"
    New-VHD -Path $VhdPath -SizeBytes $SizeBytes -Dynamic | Out-Null

    $mounted = $null
    try {
        $mounted = Mount-VHD -Path $VhdPath -Passthru -ErrorAction Stop
        $diskNumber = $mounted.DiskNumber

        Initialize-Disk -Number $diskNumber -PartitionStyle MBR -Confirm:$false -ErrorAction Stop | Out-Null
        $partition = New-Partition -DiskNumber $diskNumber -UseMaximumSize -AssignDriveLetter -ErrorAction Stop

        $letter = $null
        foreach ($attempt in 1..20) {
            $partition = Get-Partition -DiskNumber $diskNumber -PartitionNumber $partition.PartitionNumber
            if ($partition.DriveLetter -and $partition.DriveLetter -ne [char]0) {
                $letter = [string]$partition.DriveLetter
                break
            }
            Start-Sleep -Milliseconds 250
        }
        if (-not $letter) { throw 'Windows did not assign a drive letter to the seed partition.' }

        Format-Volume -DriveLetter $letter -FileSystem FAT32 -NewFileSystemLabel $Label `
            -Confirm:$false -Force -ErrorAction Stop | Out-Null

        Write-Detail "copying seed payload to ${letter}:\"
        Copy-Item -Path (Join-Path $SourceDir '*') -Destination "${letter}:\" -Recurse -Force
    }
    finally {
        if ($mounted) {
            try { Dismount-VHD -Path $VhdPath -ErrorAction Stop } catch { Write-Warn "could not dismount $VhdPath : $($_.Exception.Message)" }
        }
    }

    return $VhdPath
}

function New-DevVMSeed {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]$Config,
        [switch]$SkipDisk
    )

    $vmName = [string]$Config.VM['Name']
    $buildDir = Join-Path (Join-Path $Config.RepoRoot 'build') $vmName
    $seedDir = Join-Path $buildDir 'seed'
    $bootstrapStage = Join-Path $seedDir 'bootstrap'

    if (Test-Path -LiteralPath $buildDir) {
        Get-ChildItem -LiteralPath $buildDir -Force | ForEach-Object {
            try { Remove-Item -LiteralPath $_.FullName -Recurse -Force -ErrorAction Stop }
            catch { Write-Warn "could not remove $($_.TargetObject): $($_.Exception.Message)" }
        }
    }
    New-Item -ItemType Directory -Path $seedDir -Force | Out-Null

    $prov = $Config.Blueprint.Data['Provisioning']
    $bootstrapRoot = '/usr/local/lib/vm-bootstrap'
    if ($prov.Contains('BootstrapRoot') -and $prov['BootstrapRoot']) {
        $bootstrapRoot = [string]$prov['BootstrapRoot']
    }

    $envPath = Join-Path $buildDir 'vm-bootstrap.env'
    Write-UnixText -Path $envPath -Content (New-BootstrapEnv -Config $Config)

    $usersPath = Join-Path $buildDir 'vm-bootstrap.users'
    Write-UnixText -Path $usersPath -Content (New-AdditionalUsersFile -Config $Config)

    $gitExtraPath = Join-Path $buildDir 'vm-bootstrap.gitconfig-extra'
    Write-UnixText -Path $gitExtraPath -Content (New-GitExtraFile -Config $Config)

    $disksPath = Join-Path $buildDir 'vm-bootstrap.disks'
    Write-UnixText -Path $disksPath -Content (New-DataDiskFile -Config $Config)

    $filesPath = Join-Path $buildDir 'vm-bootstrap.files'
    Write-UnixText -Path $filesPath -Content (New-FilesManifest -Config $Config)

    $reposPath = Join-Path $buildDir 'vm-bootstrap.repos'
    Write-UnixText -Path $reposPath -Content (New-ReposFile -Config $Config)

    $payload = New-Object System.Collections.Generic.List[object]
    function Add-Payload([string]$Source, [string]$Target, [string]$Permissions) {
        $payload.Add([pscustomobject]@{
            Source = $Source; Target = $Target; Permissions = $Permissions; Owner = 'root:root'
        })
    }

    Add-Payload $envPath       '/etc/vm-bootstrap.env'              '0600'
    Add-Payload $usersPath     '/etc/vm-bootstrap.users'            '0600'
    Add-Payload $gitExtraPath  '/etc/vm-bootstrap.gitconfig-extra'  '0644'
    Add-Payload $disksPath     '/etc/vm-bootstrap.disks'            '0644'
    Add-Payload $reposPath     '/etc/vm-bootstrap.repos'            '0644'
    Add-Payload $filesPath     '/etc/vm-bootstrap.files'            '0644'

    $copyLayout = @(Get-DevVMFileCopyLayout -Config $Config)
    $copiedCount = 0
    foreach ($entry in $copyLayout) {
        foreach ($file in $entry.Files) {
            Add-Payload $file.SourcePath ($entry.StageRoot + '/' + $file.RelativePath) $file.Permissions
            $copiedCount++
        }
    }
    if ($copiedCount -gt 0) {
        $totalMb = (($copyLayout | Measure-Object -Property SizeBytes -Sum).Sum) / 1MB
        Write-Detail ("copying {0} file(s) from {1} director(ies), {2:N1} MB" -f `
            $copiedCount, $copyLayout.Count, $totalMb)
    }

    $hookScript = New-HookScript -Config $Config
    if ($hookScript) {
        $hookPath = Join-Path $buildDir 'post-start.sh'
        Write-UnixText -Path $hookPath -Content $hookScript
        Add-Payload $hookPath ($bootstrapRoot.TrimEnd('/') + '/hooks/post-start.sh') '0755'
        $hookLines = @($hookScript -split "`n" | Where-Object { $_ -and $_ -notmatch '^\s*#' -and $_.Trim() }).Count
        Write-Detail "post-start hook script: $hookLines command line(s)"
    }

    $keySource = Resolve-SshKeySource -Config $Config
    $secrets = New-SecretDescriptors -Config $Config -KeySource $keySource `
        -StageDir (Join-Path $buildDir 'secrets')
    $embeddedSecret = $false
    foreach ($descriptor in $secrets) {
        Add-Payload $descriptor.SourcePath $descriptor.TargetPath $descriptor.Permissions
        if ($descriptor.Secret) {
            $embeddedSecret = $true
            Write-Warn "embedding a secret in the seed disk: $($descriptor.Label)"
        }
    }
    if ($embeddedSecret) {
        Write-Warn 'the seed disk holds it in plain text until bootstrap shreds it - see docs/github-access.md or docs/databases.md'
    }

    $bootstrapDirs = @($Config.Blueprint.Paths['BootstrapDirs'])
    $merged = [ordered]@{}
    foreach ($dir in $bootstrapDirs) {
        if (-not (Test-Path -LiteralPath $dir)) { throw "Bootstrap directory not found: $dir" }
        $prefix = (Resolve-Path -LiteralPath $dir).Path.TrimEnd('\')
        foreach ($file in (Get-ChildItem -LiteralPath $dir -Recurse -File | Sort-Object FullName)) {
            $relative = $file.FullName.Substring($prefix.Length).TrimStart('\')
            $merged[$relative] = $file.FullName
        }
    }

    foreach ($required in @('_runner.sh', '_vm-bootstrap.service')) {
        if (-not $merged.Contains($required)) {
            throw "Blueprint '$($Config.Blueprint.Name)' has no $required in any of its bootstrap directories."
        }
    }

    Add-Payload $merged['_runner.sh'] '/usr/local/sbin/vm-bootstrap' '0755'
    Add-Payload $merged['_vm-bootstrap.service'] '/etc/systemd/system/vm-bootstrap.service' '0644'

    $staged = New-Object System.Collections.Generic.List[string]
    foreach ($relative in @($merged.Keys)) {
        if ($relative -eq '_runner.sh' -or $relative -eq '_vm-bootstrap.service') { continue }

        $source = $merged[$relative]
        $target = ($bootstrapRoot.TrimEnd('/') + '/' + ($relative -replace '\\', '/'))

        $perm = '0644'
        if ([System.IO.Path]::GetExtension($source) -eq '.sh') { $perm = '0755' }

        Add-Payload $source $target $perm
        $staged.Add($relative)

        $stageTarget = Join-Path $bootstrapStage $relative
        $stageParent = Split-Path -Parent $stageTarget
        if (-not (Test-Path -LiteralPath $stageParent)) {
            New-Item -ItemType Directory -Path $stageParent -Force | Out-Null
        }
        Copy-Item -LiteralPath $source -Destination $stageTarget -Force
    }

    $primary = $Config.Guest['PrimaryUser']
    if (-not $primary.Contains('Username') -or -not $primary['Username']) {
        throw 'Guest.PrimaryUser.Username must be set.'
    }
    $passwordHash = Resolve-PasswordHash -User $primary
    if (-not $passwordHash) {
        throw 'Guest.PrimaryUser needs either Password or PasswordHash - the Ubuntu installer will not create a passwordless account.'
    }

    $keys = @()
    if ($primary.Contains('AuthorizedKeys') -and $primary['AuthorizedKeys']) {
        $keys = @(@($primary['AuthorizedKeys']) | Where-Object { $_ -and ([string]$_).Trim() })
    }

    if ($keySource.Material) {
        $material = $keySource.Material
        foreach ($warning in (ConvertTo-Array $material.Warnings)) { Write-Warn "ssh_keys: $warning" }

        $discovered = ConvertTo-Array $material.AuthorizedKeys
        if ($discovered.Count -gt 0) {
            Write-Detail "ssh_keys: $($discovered.Count) public key(s) added to the guest's authorized_keys"
            $keys = @($keys) + $discovered
        }
    }
    $keys = @($keys | Select-Object -Unique)

    $method = 'cloud-init-nocloud'
    if ($prov.Contains('Method') -and $prov['Method']) { $method = [string]$prov['Method'] }
    if ($method -notin @('cloud-init-nocloud', 'kickstart-oemdrv')) {
        throw "Blueprint '$($Config.Blueprint.Name)': unsupported Provisioning.Method '$method'. Use 'cloud-init-nocloud' or 'kickstart-oemdrv'."
    }
    $isKickstart = ($method -eq 'kickstart-oemdrv')

    $packages = @()
    if ($Config.Blueprint.Data.Contains('Packages') -and $Config.Blueprint.Data['Packages']) {
        $packages = @($Config.Blueprint.Data['Packages'])
    }

    if ($isKickstart) {

        $packageBlock = (@($packages) -join "`n")

        $keyLines = New-Object System.Collections.Generic.List[string]
        foreach ($key in $keys) {
            $keyLines.Add(("sshkey --username={0} `"{1}`"" -f $primary['Username'], ([string]$key).Trim()))
        }
        $keysBlock = ($keyLines -join "`n")

        $writeFiles = (@($payload.ToArray() | ForEach-Object {
            New-ShellWriteFileCommand -SourcePath $_.Source -TargetPath $_.Target `
                -Permissions $_.Permissions -Owner $_.Owner
        }) -join "`n`n")
    }
    else {
        $packageBlock = (@($packages | ForEach-Object { '    - ' + $_ }) -join "`n")
        if (-not $packageBlock) { $packageBlock = '    []' }

        $keysBlock = ''
        if ($keys.Count -gt 0) {
            $lines = New-Object System.Collections.Generic.List[string]
            $lines.Add('    authorized-keys:')
            foreach ($key in $keys) {
                $lines.Add('      - ' + ([string]$key).Trim())
            }
            $keysBlock = ($lines -join "`n")
        }

        $writeFiles = (@($payload.ToArray() | ForEach-Object {
            New-WriteFileEntry -SourcePath $_.Source -TargetPath $_.Target `
                -Permissions $_.Permissions -Owner $_.Owner
        }) -join "`n")
    }

    $sshCfg = $Config.Guest['Ssh']
    $sshInstall = $true
    if ($sshCfg.Contains('InstallServer')) { $sshInstall = [bool]$sshCfg['InstallServer'] }
    $sshAllowPw = $true
    if ($sshCfg.Contains('AllowPasswordAuth')) { $sshAllowPw = [bool]$sshCfg['AllowPasswordAuth'] }

    $sourceId = 'ubuntu-server'
    if ($prov.Contains('SourceId') -and $prov['SourceId']) { $sourceId = [string]$prov['SourceId'] }

    $shutdownMode = 'poweroff'
    if ($prov.Contains('PowerOffAfterInstall') -and -not $prov['PowerOffAfterInstall']) { $shutdownMode = 'reboot' }

    $realName = [string]$primary['Username']
    if ($primary.Contains('RealName') -and $primary['RealName']) { $realName = [string]$primary['RealName'] }

    $tokens = @{
        LOCALE               = [string]$Config.Guest['Locale']
        TIMEZONE             = [string]$Config.Guest['Timezone']
        KEYBOARD_LAYOUT      = [string]$Config.Guest['KeyboardLayout']
        SOURCE_ID            = $sourceId
        HOSTNAME             = [string]$Config.Guest['Hostname']
        REALNAME             = ($realName -replace '"', '\"')
        USERNAME             = [string]$primary['Username']
        PASSWORD_HASH        = $passwordHash
        SSH_INSTALL_SERVER   = ConvertTo-YamlBool $sshInstall
        SSH_ALLOW_PW         = ConvertTo-YamlBool $sshAllowPw
        SSH_AUTHORIZED_KEYS  = $keysBlock
        PACKAGES             = $packageBlock
        WRITE_FILES          = $writeFiles
        SHUTDOWN_MODE        = $shutdownMode
        INSTANCE_ID          = ('iid-{0}-{1}' -f $vmName, (Get-Date -Format 'yyyyMMddHHmmss'))
        SYSTEM_DISK          = $(if ($prov.Contains('SystemDisk') -and $prov['SystemDisk']) { [string]$prov['SystemDisk'] } else { 'sda' })

        INSTALL_SOURCE       = $(if ($prov.Contains('InstallSource')) {
                                    ([string]$prov['InstallSource']).Replace('{Release}', [string]$Config.Iso['Release'])
                                } else { '' })
    }

    $seedFileName = $(if ($prov.Contains('SeedFileName') -and $prov['SeedFileName']) { [string]$prov['SeedFileName'] } else { 'user-data' })

    $rendered = New-Object System.Collections.Generic.List[object]
    $rendered.Add([pscustomobject]@{
        Name = $seedFileName
        Text = (Get-Content -LiteralPath $Config.Blueprint.Paths['SeedTemplate'] -Raw)
    })
    if ($Config.Blueprint.Paths['MetaTemplate']) {
        $rendered.Add([pscustomobject]@{
            Name = 'meta-data'
            Text = (Get-Content -LiteralPath $Config.Blueprint.Paths['MetaTemplate'] -Raw)
        })
    }

    foreach ($file in $rendered) {
        $text = $file.Text
        foreach ($name in $tokens.Keys) {
            $text = $text.Replace(('{{' + $name + '}}'), [string]$tokens[$name])
        }
        if ($text -match '\{\{([A-Z_]+)\}\}') {
            throw "Unreplaced template token {{$($Matches[1])}} in $($file.Name) - the blueprint template expects a value New-DevVMSeed does not supply."
        }
        Write-UnixText -Path (Join-Path $seedDir $file.Name) -Content $text
    }

    Write-Detail "rendered $method seed: $(Join-Path $seedDir $seedFileName)"
    Write-Detail "embedded $($payload.Count) provisioning file(s)"

    $seedLabel = $(if ($prov.Contains('SeedLabel') -and $prov['SeedLabel']) { [string]$prov['SeedLabel'] } else { 'CIDATA' })

    $seedVhd = $null
    if (-not $SkipDisk) {
        $vmDir = Join-Path ([string]$Config.VM['Path']) $vmName
        $seedVhd = New-SeedDisk -VhdPath (Join-Path $vmDir "$vmName-seed.vhdx") `
            -SourceDir $seedDir -Label $seedLabel
    }

    return [pscustomobject]@{
        BuildDir     = $buildDir
        SeedDir      = $seedDir
        Method       = $method
        SeedLabel    = $seedLabel
        SeedPath     = (Join-Path $seedDir $seedFileName)
        SeedVhdPath  = $seedVhd
        Files        = @($staged)
    }
}

Export-ModuleMember -Function New-DevVMSeed, New-SeedDisk, New-BootstrapEnv,
    New-AdditionalUsersFile, New-GitExtraFile, New-DataDiskFile, New-ReposFile, New-SecretDescriptors,
    New-FilesManifest, New-HookScript,
    Resolve-SshKeySource, ConvertTo-SshKeyType,
    New-WriteFileEntry, New-ShellWriteFileCommand, ConvertTo-Base64Lines
