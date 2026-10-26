<#
Start everything needed to try the full flow on this PC, with no real money and
nothing online: the website (local copy), a local database, a stand-in for
Stripe's test mode, the mock CDN, and the DEV launcher pointed at the local site.

  powershell -ExecutionPolicy Bypass -File scripts\try-it-locally.ps1

Then follow docs\TRY_IT_LOCALLY.md. Close the opened windows to stop.
Expects the website repo next to this one (..\retail-store); pass -Site to change.
#>
param(
    [string]$Site = (Join-Path $PSScriptRoot "..\..\retail-store"),
    [switch]$NoLauncher
)
$ErrorActionPreference = "Stop"
$L = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Site = (Resolve-Path $Site).Path
$env:Path += ";$env:USERPROFILE\.cargo\bin"

function Step($m) { Write-Host "`n== $m" -ForegroundColor Yellow }

Step "Launcher repo: npm packages and demo builds (first run takes a few minutes)"
Push-Location $L
if (-not (Test-Path node_modules)) { npm install }
if (-not (Test-Path "mock-server\.data\manifests\dirt-simulator")) { npm run mock:seed }
Pop-Location

Step "Website repo: npm packages"
Push-Location $Site
if (-not (Test-Path node_modules)) { npm install }
Pop-Location

Step "Writing $Site\.env.local (local test values only)"
$keys = Join-Path $L "mock-server\.data\keys"
$launch = (Get-Content "$keys\launch-key.json" -Raw | ConvertFrom-Json | ConvertTo-Json -Compress)
$mk = Get-Content "$keys\manifest-key.json" -Raw | ConvertFrom-Json
$pub = (@{ keys = @(@{ kid = $mk.kid; kty = "OKP"; crv = "Ed25519"; x = $mk.x; use = "manifest" }) } | ConvertTo-Json -Compress -Depth 5)
$rand = { -join ((48..57) + (65..90) + (97..122) | Get-Random -Count 40 | ForEach-Object { [char]$_ }) }
$vars = [ordered]@{
    DATABASE_URL                  = "postgresql://postgres:postgres@127.0.0.1:55432/postgres?sslmode=disable&connection_limit=1&pgbouncer=true"
    AUTH_SECRET                   = (& $rand)
    NEXT_PUBLIC_SITE_URL          = "http://localhost:3000"
    ADMIN_EMAIL                   = "admin@example.test"
    ADMIN_PASSWORD                = "local-admin-pass-123"
    ADMIN_USERNAME                = "Zirtola"
    STRIPE_SECRET_KEY             = "sk_test_local_fake_stripe_key"
    STRIPE_WEBHOOK_SECRET         = "whsec_local_test"
    STRIPE_API_BASE               = "http://127.0.0.1:12111"
    LAUNCHER_LAUNCH_SIGNING_KEY   = $launch
    LAUNCHER_MANIFEST_PUBLIC_KEYS = $pub
    LAUNCHER_CDN_URL_TEMPLATE     = "http://127.0.0.1:8787/cdn/{game}/chunks/{prefix}/{hash}"
    LAUNCHER_PUBLISH_TOKEN        = (& $rand)
}
$lines = foreach ($k in $vars.Keys) { "$k='$($vars[$k])'" }
[IO.File]::WriteAllText((Join-Path $Site ".env.local"), ($lines -join "`n") + "`n")
foreach ($k in $vars.Keys) { Set-Item "env:$k" $vars[$k] }

Step "Starting the local database"
Start-Process powershell -ArgumentList "-NoExit", "-Command", "`$host.UI.RawUI.WindowTitle='Zirtola: database'; cd '$Site'; npm run db:local"
Start-Sleep 6

Step "Creating tables and demo accounts"
Push-Location $Site
npx prisma migrate deploy
npm run db:seed
npm run db:seed:demo
"UPDATE ""Game"" SET ""priceCents"" = 1999, ""salesOpen"" = true, ""published"" = true, ""launcherVisible"" = true WHERE ""slug"" IN ('dirt-simulator', 'alaskan-bush-simulator');" | npx prisma db execute --stdin
Pop-Location

Step "Starting the mock CDN, the Stripe test-mode stand-in and the website"
Start-Process powershell -ArgumentList "-NoExit", "-Command", "`$host.UI.RawUI.WindowTitle='Zirtola: CDN'; cd '$L'; `$env:MOCK_OPEN_CDN='1'; npm run mock"
Start-Process powershell -ArgumentList "-NoExit", "-Command", "`$host.UI.RawUI.WindowTitle='Zirtola: test payments'; cd '$Site'; `$env:SITE='http://localhost:3000'; `$env:STRIPE_WEBHOOK_SECRET='whsec_local_test'; npm run stripe:fake"
Start-Process powershell -ArgumentList "-NoExit", "-Command", "`$host.UI.RawUI.WindowTitle='Zirtola: website'; cd '$Site'; npm run dev"
Write-Host "waiting for the website..."
for ($i = 0; $i -lt 90; $i++) {
    try { Invoke-RestMethod http://localhost:3000/api/launcher/v1/config -TimeoutSec 3 | Out-Null; break } catch { Start-Sleep 2 }
}

Step "Publishing two Dirt Digger demo builds (1.0.0 stable, 1.0.1 beta)"
foreach ($b in @(@{ v = "1.0.0"; branch = "live"; notes = "## Dirt Digger 1.0.0`n`n- First build on the launcher (demo)." }, @{ v = "1.0.1"; branch = "beta"; notes = "## Dirt Digger 1.0.1 (beta)`n`n- A small patch, to try beta updates (demo)." })) {
    $env_ = Get-Content (Join-Path $L "mock-server\.data\manifests\dirt-simulator\$($b.v).json") -Raw | ConvertFrom-Json
    $body = @{ signed_manifest = $env_; branch = $b.branch; notes = $b.notes; make_live = $true } | ConvertTo-Json -Depth 6
    try {
        Invoke-RestMethod -Method Post -Uri http://localhost:3000/api/launcher/v1/admin/builds -Headers @{ Authorization = "Bearer $($vars.LAUNCHER_PUBLISH_TOKEN)" } -ContentType "application/json" -Body $body | Out-Null
        Write-Host "published $($b.v) ($($b.branch))"
    } catch { Write-Host "($($b.v) already published)" }
}

if (-not $NoLauncher) {
    Step "Starting the DEV launcher (first build takes a few minutes)"
    Start-Process powershell -ArgumentList "-NoExit", "-Command", "`$host.UI.RawUI.WindowTitle='Zirtola: DEV launcher'; cd '$L'; `$env:Path += ';$env:USERPROFILE\.cargo\bin'; `$env:ZIRTOLA_API_BASE='http://localhost:3000/api/launcher'; npm run dev"
}

Write-Host @"

All started. Website: http://localhost:3000
Player account:  pixel_pioneer / password123
Admin account:   admin@example.test / local-admin-pass-123
Test card:       4242 4242 4242 4242, any future date, any CVC (nothing is charged)
Next: docs\TRY_IT_LOCALLY.md
"@ -ForegroundColor Green
