# Authenticode signing hook. Tauri calls this for every Windows binary it
# produces (the launcher .exe, the NSIS uninstaller and the setup .exe) via
# bundle.windows.signCommand in src-tauri/tauri.conf.json.
#
# The signing method is chosen by environment variables (CI secrets). With
# none set, the file is left unsigned and the build still succeeds, so local
# builds and forks work without credentials.
#
#   ZIRTOLA_SIGN_MODE = "azure"       Azure Artifact Signing (formerly Trusted Signing). Recommended.
#     AZURE_TENANT_ID, AZURE_CLIENT_ID, AZURE_CLIENT_SECRET   app registration with the
#                                     "Artifact Signing Certificate Profile Signer" role
#     AZURE_SIGNING_ENDPOINT          e.g. https://eus.codesigning.azure.net
#     AZURE_SIGNING_ACCOUNT           Artifact Signing account name
#     AZURE_SIGNING_PROFILE           certificate profile name
#
#   ZIRTOLA_SIGN_MODE = "thumbprint"  any certificate already in the Windows cert store
#                                     (e.g. an OV/EV cert whose key sits in a vendor's
#                                     KSP/cloud HSM driver on a self-hosted runner)
#     SIGN_CERT_THUMBPRINT            SHA-1 thumbprint
#     SIGN_TIMESTAMP_URL              RFC 3161 timestamp server from your CA
#
#   ZIRTOLA_SIGN_MODE = "none" (or unset): skip.
param([Parameter(Mandatory = $true, Position = 0)][string]$File)

$ErrorActionPreference = "Stop"
$mode = $env:ZIRTOLA_SIGN_MODE
if (-not $mode) { $mode = "none" }

function Find-SignTool {
    $cmd = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $kits = "${env:ProgramFiles(x86)}\Windows Kits\10\bin"
    $found = Get-ChildItem $kits -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match "\\x64\\" } | Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $found) { throw "signtool.exe not found (install the Windows SDK)" }
    return $found.FullName
}

switch ($mode) {
    "none" {
        Write-Host "[sign] ZIRTOLA_SIGN_MODE not set: leaving $([IO.Path]::GetFileName($File)) unsigned"
        exit 0
    }
    "azure" {
        foreach ($v in "AZURE_TENANT_ID", "AZURE_CLIENT_ID", "AZURE_CLIENT_SECRET", "AZURE_SIGNING_ENDPOINT", "AZURE_SIGNING_ACCOUNT", "AZURE_SIGNING_PROFILE") {
            if (-not [Environment]::GetEnvironmentVariable($v)) { throw "[sign] $v is not set" }
        }
        $cli = Get-Command artifact-signing-cli -ErrorAction SilentlyContinue
        if (-not $cli) { throw "[sign] artifact-signing-cli not found (cargo install artifact-signing-cli)" }
        & $cli.Source -e $env:AZURE_SIGNING_ENDPOINT -a $env:AZURE_SIGNING_ACCOUNT -c $env:AZURE_SIGNING_PROFILE -d "Zirtola Launcher" $File
        if ($LASTEXITCODE -ne 0) { throw "[sign] artifact-signing-cli failed ($LASTEXITCODE)" }
    }
    "thumbprint" {
        if (-not $env:SIGN_CERT_THUMBPRINT) { throw "[sign] SIGN_CERT_THUMBPRINT is not set" }
        $ts = $env:SIGN_TIMESTAMP_URL
        if (-not $ts) { $ts = "http://timestamp.digicert.com" }
        $signtool = Find-SignTool
        & $signtool sign /sha1 $env:SIGN_CERT_THUMBPRINT /fd sha256 /tr $ts /td sha256 /d "Zirtola Launcher" $File
        if ($LASTEXITCODE -ne 0) { throw "[sign] signtool failed ($LASTEXITCODE)" }
    }
    default { throw "[sign] unknown ZIRTOLA_SIGN_MODE '$mode'" }
}
Write-Host "[sign] signed $File"
