# Integrating a game (Unreal Engine 5)

When the launcher starts a game it:

- asks the website for a **launch token** (5-minute EdDSA JWT for this account
  and this game),
- starts the executable from the manifest's `launch` section with the
  manifest's `args` plus `-zirtola`, working directory = the executable's folder,
- sets these environment variables for the game process:

| Variable | Value |
| --- | --- |
| `ZIRTOLA_SESSION_TOKEN` | the launch token |
| `ZIRTOLA_GAME_ID` | e.g. `dirt-simulator` |
| `ZIRTOLA_GAME_VERSION` | installed version, e.g. `1.2.0` |
| `ZIRTOLA_API_BASE` | e.g. `https://www.zirtola.com/api/launcher` |

The token goes in the environment, not the command line, so it does not show
up in process listings or crash reports.

## What the game should check

1. `ZIRTOLA_SESSION_TOKEN` exists.
2. Its signature verifies with the website's `launch` public key.
3. `aud == "zirtola-game:<your game id>"`, `game == <your game id>`, and
   `exp` is in the future (allow a couple of minutes of clock skew).

If the check fails, show "Please start the game from the Zirtola Launcher"
with a button that opens the launcher, or allow a grace period (for example,
remember the last good check for 14 days) so players are not locked out when
offline. How strict to be is a design choice; the token only proves "this
account owned the game a few minutes ago".

`examples/license-check` is a complete reference implementation in Rust (it is
also the stand-in "game" inside the mock builds).

## Option A: verify online (simplest in Unreal)

`POST {ZIRTOLA_API_BASE}/v1/launch-tokens/verify` with `{"token": "…"}` returns
`{"valid": true, "claims": {…}}`. Add `"HTTP"` and `"Json"` to your module's
`PublicDependencyModuleNames`.

```cpp
// ZirtolaLicense.h
#pragma once
#include "CoreMinimal.h"

DECLARE_DELEGATE_TwoParams(FOnZirtolaLicense, bool /*bValid*/, const FString& /*AccountName*/);

namespace ZirtolaLicense
{
    void Check(const FString& ExpectedGameId, FOnZirtolaLicense OnDone);
}
```

```cpp
// ZirtolaLicense.cpp
#include "ZirtolaLicense.h"
#include "HttpModule.h"
#include "Interfaces/IHttpRequest.h"
#include "Interfaces/IHttpResponse.h"
#include "Dom/JsonObject.h"
#include "Serialization/JsonReader.h"
#include "Serialization/JsonSerializer.h"

void ZirtolaLicense::Check(const FString& ExpectedGameId, FOnZirtolaLicense OnDone)
{
    const FString Token = FPlatformMisc::GetEnvironmentVariable(TEXT("ZIRTOLA_SESSION_TOKEN"));
    FString Api = FPlatformMisc::GetEnvironmentVariable(TEXT("ZIRTOLA_API_BASE"));
    if (Token.IsEmpty())
    {
        OnDone.ExecuteIfBound(false, FString());
        return;
    }
    if (Api.IsEmpty())
    {
        Api = TEXT("https://www.zirtola.com/api/launcher");
    }

    TSharedRef<IHttpRequest, ESPMode::ThreadSafe> Req = FHttpModule::Get().CreateRequest();
    Req->SetURL(Api / TEXT("v1/launch-tokens/verify"));
    Req->SetVerb(TEXT("POST"));
    Req->SetHeader(TEXT("Content-Type"), TEXT("application/json"));
    Req->SetContentAsString(FString::Printf(TEXT("{\"token\":\"%s\"}"), *Token));
    Req->OnProcessRequestComplete().BindLambda(
        [ExpectedGameId, OnDone](FHttpRequestPtr, FHttpResponsePtr Resp, bool bOk)
        {
            bool bValid = false;
            FString Name;
            TSharedPtr<FJsonObject> Json;
            if (bOk && Resp.IsValid() && Resp->GetResponseCode() == 200 &&
                FJsonSerializer::Deserialize(TJsonReaderFactory<>::Create(Resp->GetContentAsString()), Json) && Json.IsValid())
            {
                const TSharedPtr<FJsonObject>* Claims = nullptr;
                if (Json->GetBoolField(TEXT("valid")) && Json->TryGetObjectField(TEXT("claims"), Claims))
                {
                    bValid = (*Claims)->GetStringField(TEXT("game")) == ExpectedGameId;
                    (*Claims)->TryGetStringField(TEXT("name"), Name);
                }
            }
            OnDone.ExecuteIfBound(bValid, Name);
        });
    Req->ProcessRequest();
}
```

Call it early, e.g. from your `UGameInstance::Init()`:

```cpp
ZirtolaLicense::Check(TEXT("dirt-simulator"), FOnZirtolaLicense::CreateLambda(
    [this](bool bValid, const FString& Name)
    {
        if (!bValid) { /* show "start from the Zirtola Launcher" screen or start the grace period */ }
    }));
```

In editor and development builds, skip the check (`#if UE_BUILD_SHIPPING`).

## Option B: verify offline (no network needed)

Pin the website's `launch` public key (the `x` value from `GET /v1/keys` with
`"use": "launch"`, 32 bytes base64url) in the game and verify the JWT yourself:

- split the token into `header.payload.signature`;
- verify the Ed25519 signature of the bytes `header.payload` with the pinned key;
- base64url-decode the payload and check `aud`, `game`, `exp` as above.

Unreal has no Ed25519 built in. Two small, permissively licensed options:
[Monocypher](https://monocypher.org) with its optional `monocypher-ed25519`
files (`crypto_ed25519_check`), or libsodium (`crypto_sign_verify_detached`).
When the website rotates the launch key, ship the new public key in a patch
before the old one is retired (keys carry a `kid`, so a game can pin several).

## Crash reports

### Through the launcher (nothing to do in the game)

When a game the launcher started exits with a non-zero code or leaves a new
Unreal crash folder (`%LOCALAPPDATA%\<Project>\Saved\Crashes\UECC-Windows-*`,
or `<install>\<Project>\Saved\Crashes`), the launcher asks the player whether to
send a report (with an optional comment). It zips the log, `CrashContext.runtime-xml`
and the minidump (size-capped) and uploads them with the game version, build id,
device id and, if the player shares it, the hardware summary. At start-up it also
offers recent crash folders it has not asked about yet (for example a crash while
the launcher was closed). Players can turn the prompt off in Settings → Privacy.

`<Project>` is taken from the executable name (`DirtSimulator.exe` →
`DirtSimulator`, `DirtSimulator-Win64-Shipping.exe` → `DirtSimulator`) and the
build's top-level folder, so the usual UE layout works without configuration.

To try it without a real crash, start the launcher with `ZIRTOLA_TEST_CRASH=1`
in its environment: the demo game (examples/license-check) then writes an
Unreal-style crash folder and exits with code 3.

### Directly from the game (works without the launcher)

Each game has a **crash key** (Studio → Licensing → Launcher catalogue → the
game → "Create crash key"). It ships inside the game, so treat it as a spam
filter, not a secret; replacing it stops old builds from reporting.

**Option 1: Unreal's CrashReportClient.** Ship CrashReportClient with the game
(Project Settings → Packaging → "Include Crash Reporter") and point it at the
site. In the packaged game's `Engine/Programs/CrashReportClient/Config/DefaultEngine.ini`
(or your project's `Config/DefaultEngine.ini`, depending on engine version):

```ini
[CrashReportClient]
bAgreeToCrashUpload=false
DataRouterUrl="https://www.zirtola.com/api/crash/ue?key=<game crash key>"
```

CrashReportClient shows its own dialog and posts a compressed archive of the
crash folder. The site unpacks it best-effort (the archive layout varies between
engine versions) and always stores the original, so the dump is downloadable
either way. Verify with your engine version by forcing a crash (`debug crash`
console command in a test build) and checking Studio → Crash reports.

**Option 2: a small upload from the game.** On the next start after a crash,
zip the newest folder under `Saved/Crashes` and post it:

```
POST https://www.zirtola.com/api/crash/upload?key=<game crash key>
Content-Type: multipart/form-data
  metadata = {"game_version":"1.2.0","build_id":"…","comment":"","launch_token":"<ZIRTOLA_SESSION_TOKEN if started by the launcher>"}
  file     = crash.zip
```

`FHttpModule` can send this with a hand-built multipart body; keep the zip under
48 MB (skip the minidump if it is larger).
