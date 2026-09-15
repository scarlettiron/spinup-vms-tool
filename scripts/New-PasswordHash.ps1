#requires -Version 5.1

[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Import-Module (Join-Path (Join-Path $PSScriptRoot 'lib') 'Common.psm1') -Force -DisableNameChecking

try {
    $openssl = Get-OpenSslPath
    if (-not $openssl) {
        throw 'openssl.exe was not found. Install Git for Windows (it bundles openssl) and try again.'
    }
    Write-Detail "using $openssl"

    $first = Read-Host -Prompt 'Password' -AsSecureString
    $second = Read-Host -Prompt 'Confirm ' -AsSecureString

    $toPlain = {
        param($secure)
        $ptr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
        try { [Runtime.InteropServices.Marshal]::PtrToStringBSTR($ptr) }
        finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($ptr) }
    }

    $a = & $toPlain $first
    $b = & $toPlain $second

    if (-not $a) { throw 'Empty password.' }
    if ($a -ne $b) { throw 'The two entries did not match.' }

    $hash = New-Sha512Crypt -Password $a

    Write-Host ''
    Write-Host '  Paste this into vm.config.local.yaml:' -ForegroundColor Cyan
    Write-Host ''
    Write-Host '      Guest = @{'
    Write-Host '          PrimaryUser = @{'
    Write-Host "              Password     = ''"
    Write-Host "              PasswordHash = '$hash'"
    Write-Host '          }'
    Write-Host '      }'
    Write-Host ''
}
catch {
    Write-Host "FAILED: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
finally {
    $a = $null
    $b = $null
    [GC]::Collect()
}
