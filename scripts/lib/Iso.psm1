#requires -Version 5.1
Set-StrictMode -Version Latest

Import-Module (Join-Path $PSScriptRoot 'Common.psm1') -DisableNameChecking

function Initialize-Tls {
    try {
        [Net.ServicePointManager]::SecurityProtocol =
            [Net.SecurityProtocolType]::Tls12 -bor [Net.ServicePointManager]::SecurityProtocol
    }
    catch { }
}

function Get-WebText {
    [CmdletBinding()] param([Parameter(Mandatory)][string]$Uri)
    Initialize-Tls
    $content = $null
    try {

        $content = (Invoke-WebRequest -Uri $Uri -UseBasicParsing -ErrorAction Stop).Content
    }
    catch {
        throw "Could not fetch $Uri : $($_.Exception.Message)"
    }

    if ($content -is [byte[]]) {

        $text = (New-Object System.Text.UTF8Encoding($false)).GetString($content)
        return $text.TrimStart([char]0xFEFF)
    }
    return [string]$content
}

function Invoke-FileDownload {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Uri,
        [Parameter(Mandatory)][string]$Destination
    )

    Initialize-Tls
    $partial = "$Destination.part"
    if (Test-Path -LiteralPath $partial) { Remove-Item -LiteralPath $partial -Force }

    $bits = Get-Command Start-BitsTransfer -ErrorAction SilentlyContinue
    if ($bits) {
        try {
            Write-Detail 'downloading via BITS (resumable)'
            Start-BitsTransfer -Source $Uri -Destination $partial -Description 'Install media' -ErrorAction Stop
            Move-Item -LiteralPath $partial -Destination $Destination -Force
            return
        }
        catch {
            Write-Warn "BITS transfer failed ($($_.Exception.Message)); falling back to a direct download."
            if (Test-Path -LiteralPath $partial) { Remove-Item -LiteralPath $partial -Force }
        }
    }

    $client = New-Object System.Net.Http.HttpClient
    $client.Timeout = [TimeSpan]::FromHours(6)
    try {
        $response = $client.GetAsync($Uri, [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead).GetAwaiter().GetResult()
        if (-not $response.IsSuccessStatusCode) {
            throw "HTTP $([int]$response.StatusCode) $($response.ReasonPhrase)"
        }
        $total = 0L
        if ($response.Content.Headers.ContentLength) { $total = [long]$response.Content.Headers.ContentLength }

        $source = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
        $target = [System.IO.File]::Open($partial, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write)
        try {
            $buffer = New-Object byte[] (1MB)
            $read = 0
            $done = 0L
            $lastReport = [DateTime]::MinValue
            while (($read = $source.Read($buffer, 0, $buffer.Length)) -gt 0) {
                $target.Write($buffer, 0, $read)
                $done += $read
                if (([DateTime]::UtcNow - $lastReport).TotalMilliseconds -gt 500) {
                    $lastReport = [DateTime]::UtcNow
                    if ($total -gt 0) {
                        Write-Progress -Activity 'Downloading install media' `
                            -Status ("{0:N0} MB of {1:N0} MB" -f ($done / 1MB), ($total / 1MB)) `
                            -PercentComplete ([int](100 * $done / $total))
                    }
                    else {
                        Write-Progress -Activity 'Downloading install media' -Status ("{0:N0} MB" -f ($done / 1MB))
                    }
                }
            }
        }
        finally {
            $target.Dispose()
            $source.Dispose()
        }
        Write-Progress -Activity 'Downloading install media' -Completed
        Move-Item -LiteralPath $partial -Destination $Destination -Force
    }
    finally { $client.Dispose() }
}

function Test-FileHashMatch {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string]$Sha256
    )
    $actual = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
    return ($actual -eq $Sha256.Trim().ToUpperInvariant())
}

function Select-ChecksumEntry {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Content,
        [Parameter(Mandatory)][string]$NamePattern
    )

    $found = New-Object System.Collections.Generic.List[object]
    foreach ($line in ($Content -split "`r?`n")) {
        $trimmed = $line.Trim()
        if (-not $trimmed -or $trimmed.StartsWith('#')) { continue }

        $hash = ''
        $name = ''

        if ($trimmed -match '^([0-9a-fA-F]{64})\s+\*?(.+)$') {
            $hash = $Matches[1].ToUpperInvariant()
            $name = $Matches[2].Trim()
        }
        elseif ($trimmed -match '^SHA256\s*\(\s*(.+?)\s*\)\s*=\s*([0-9a-fA-F]{64})$') {
            $name = $Matches[1].Trim()
            $hash = $Matches[2].ToUpperInvariant()
        }
        else { continue }

        if ($name -like $NamePattern) {
            $found.Add([pscustomobject]@{ Sha256 = $hash; Name = $name })
        }
    }

    if ($found.Count -eq 0) {
        throw "No entry matching '$NamePattern' in the vendor checksum file. Check Iso.Release and the blueprint's Media.IsoNamePattern."
    }

    return ($found | Sort-Object -Property Name -Descending | Select-Object -First 1)
}

function Resolve-ChecksumFileName {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$BaseUrl,
        [Parameter(Mandatory)][string]$Pattern
    )

    if ($Pattern -notmatch '[*?]') { return $Pattern }

    Write-Detail "looking for '$Pattern' in the release directory"
    $html = Get-WebText -Uri $BaseUrl

    $candidates = New-Object System.Collections.Generic.List[string]
    foreach ($m in [regex]::Matches($html, 'href\s*=\s*["'']([^"'']+)["'']')) {
        $href = $m.Groups[1].Value

        $leaf = ($href -split '[/\\]')[-1]
        if ($leaf -and $leaf -like $Pattern) { $candidates.Add($leaf) }
    }

    $unique = @($candidates.ToArray() | Select-Object -Unique)
    if ($unique.Count -eq 0) {
        throw "No file matching '$Pattern' listed at $BaseUrl. Check Iso.Release, or pin Media.IsoUrl and Media.IsoSha256 instead."
    }

    return (@($unique | Sort-Object -Descending)[0])
}

function Resolve-DevVMIso {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Config)

    $iso = $Config.Iso
    $media = @{}
    if ($Config.Blueprint.Data.Contains('Media') -and $Config.Blueprint.Data['Media']) {
        $media = $Config.Blueprint.Data['Media']
    }

    $verify = $true
    if ($iso.Contains('VerifyChecksum')) { $verify = [bool]$iso['VerifyChecksum'] }

    if ($iso.Contains('Path') -and $iso['Path']) {
        $explicit = [string]$iso['Path']
        if (-not (Test-Path -LiteralPath $explicit)) {
            throw "Iso.Path points at a file that does not exist: $explicit"
        }
        Write-Detail "using the ISO named in config: $explicit"

        if ($media.Contains('RequiresManualDownload') -and $media['RequiresManualDownload'] -and
            $media.Contains('IsoNamePattern') -and $media['IsoNamePattern']) {
            $leaf = [System.IO.Path]::GetFileName($explicit)
            if ($leaf -notlike [string]$media['IsoNamePattern']) {
                Write-Warn "the ISO is named '$leaf', which does not match the expected '$($media['IsoNamePattern'])' for $($Config.Blueprint.Name)."
                Write-Warn '  This blueprint installs from the binary DVD. A boot ISO carries the installer but no packages and will not complete unattended.'
                Write-Warn '  If this really is the DVD under a different name, ignore this.'
            }
        }

        return (Resolve-Path -LiteralPath $explicit).Path
    }

    if ($media.Contains('RequiresManualDownload') -and $media['RequiresManualDownload']) {
        $page = ''
        if ($media.Contains('DownloadPage') -and $media['DownloadPage']) {
            $page = "`n  Download it from: $($media['DownloadPage'])"
        }
        throw ("Blueprint '$($Config.Blueprint.Name)' cannot download its own install media: the vendor requires an account, so there is no anonymous URL to fetch and no published checksum to verify.$page`n" +
               "  Download the binary DVD yourself, then point config at it:`n" +
               "      isoPath: C:\ISO\rhel-9.7-x86_64-dvd.iso`n" +
               "  If you would rather not deal with an account, redhat-rocky-workstation is binary-compatible with RHEL and downloads its own media.")
    }

    $dir = 'C:\ISO'
    if ($iso.Contains('Directory') -and $iso['Directory']) { $dir = [string]$iso['Directory'] }
    if (-not (Test-Path -LiteralPath $dir)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
    }

    $url = $null
    $sha = $null

    if ($media.Contains('IsoUrl') -and $media['IsoUrl']) {
        $url = [string]$media['IsoUrl']
        if ($media.Contains('IsoSha256') -and $media['IsoSha256']) { $sha = [string]$media['IsoSha256'] }
    }

    elseif ($media.Contains('IsoBaseUrl') -and $media['IsoBaseUrl']) {
        $release = ''
        if ($iso.Contains('Release')) { $release = [string]$iso['Release'] }
        $baseUrl = ([string]$media['IsoBaseUrl']).Replace('{Release}', $release)
        if (-not $baseUrl.EndsWith('/')) { $baseUrl += '/' }

        $checksumFile = 'SHA256SUMS'
        if ($media.Contains('ChecksumFile') -and $media['ChecksumFile']) { $checksumFile = [string]$media['ChecksumFile'] }
        $pattern = '*.iso'
        if ($media.Contains('IsoNamePattern') -and $media['IsoNamePattern']) { $pattern = [string]$media['IsoNamePattern'] }

        $checksumFile = Resolve-ChecksumFileName -BaseUrl $baseUrl -Pattern $checksumFile
        Write-Detail "resolving the current release from ${baseUrl}${checksumFile}"
        $sums = Get-WebText -Uri ($baseUrl + $checksumFile)
        $entry = Select-ChecksumEntry -Content $sums -NamePattern $pattern

        $url = $baseUrl + $entry.Name
        $sha = $entry.Sha256
        Write-Detail "release resolved: $($entry.Name)"
    }
    else {
        throw "Blueprint '$($Config.Blueprint.Name)' does not describe any install media. Set Media.IsoUrl or Media.IsoBaseUrl, or point Iso.Path at a local file."
    }

    $fileName = [System.IO.Path]::GetFileName(([Uri]$url).LocalPath)
    $target = Join-Path $dir $fileName

    if (Test-Path -LiteralPath $target) {
        if (-not $verify -or -not $sha) {
            Write-Detail "reusing $target (verification off or no published hash)"
            return $target
        }
        Write-Detail 'verifying the cached ISO'
        if (Test-FileHashMatch -Path $target -Sha256 $sha) {
            Write-Detail "cached ISO verified: $target"
            return $target
        }
        Write-Warn 'the cached ISO failed checksum verification; downloading it again.'
        Remove-Item -LiteralPath $target -Force
    }

    Write-Detail "downloading $url"
    Write-Detail "        to $target"
    Invoke-FileDownload -Uri $url -Destination $target

    if ($verify -and $sha) {
        Write-Detail 'verifying the download'
        if (-not (Test-FileHashMatch -Path $target -Sha256 $sha)) {
            Remove-Item -LiteralPath $target -Force
            throw "The downloaded ISO failed SHA-256 verification and has been deleted. Re-run to try again."
        }
        Write-Detail 'checksum ok'
    }
    elseif ($verify) {
        Write-Warn 'no published checksum was available, so the download could not be verified.'
    }

    return $target
}

Export-ModuleMember -Function Resolve-DevVMIso, Invoke-FileDownload, Test-FileHashMatch,
    Resolve-ChecksumFileName,
    Select-ChecksumEntry, Get-WebText
